//! A session: one child process on a pseudoterminal, its `Terminal`, and the reader and parser
//! threads between them (`docs/TDD.md` section 4.2).
//!
//! The reader thread reads the PTY into 64 KiB chunks and sends them over a bounded channel. The
//! parser thread applies them to the terminal under its mutex in slices of at most
//! `SessionOptions::lock_slice` bytes, writes the terminal's replies back to the PTY, queues the
//! terminal's events for the UI and calls the waker. The UI thread and the IPC threads only ever
//! hold the terminal lock briefly to copy state out.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded};
use terminus_pty::{ExitStatus, Pty, PtySize, SpawnOptions};
use terminus_vt::terminal::{Event, PromptEvent};
use terminus_vt::{Terminal, TerminalConfig};

/// Called from the parser thread when the UI has something to look at.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How to start a session.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// The program and its environment.
    pub spawn: SpawnOptions,
    /// Terminal settings.
    pub terminal: TerminalConfig,
    /// Bytes read from the PTY in one read.
    pub read_buffer: usize,
    /// Chunks queued between the reader and the parser.
    pub channel_depth: usize,
    /// Bytes applied to the terminal per lock.
    pub lock_slice: usize,
    /// How often the exit watcher checks the child.
    pub exit_poll: Duration,
}

impl SessionOptions {
    /// Options for the user's shell at a size, with the TDD's thread settings.
    pub fn shell(cols: u16, rows: u16) -> SessionOptions {
        let size = PtySize { cols, rows, pixel_width: 0, pixel_height: 0 };
        SessionOptions {
            spawn: SpawnOptions::default_shell(size),
            terminal: TerminalConfig::default(),
            read_buffer: 64 << 10,
            channel_depth: 16,
            lock_slice: 64 << 10,
            exit_poll: Duration::from_millis(100),
        }
    }
}

/// Where the shell is, from its OSC 133 marks. `wait-prompt` reads this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PromptState {
    /// A command is running (C seen, D not yet).
    pub running: bool,
    /// The value of `enter_count` when the shell last showed a prompt or finished a command.
    pub settled_at: u64,
    /// Lines of input sent (writes containing Enter).
    pub enter_count: u64,
    /// The exit code of the last command that finished.
    pub last_exit: Option<i32>,
    /// Commands finished so far.
    pub commands: u64,
    /// The shell has sent any OSC 133 mark (shell integration is active).
    pub integrated: bool,
}

/// State the threads share.
struct Shared {
    events: Mutex<Vec<Event>>,
    prompt: Mutex<PromptState>,
    prompt_changed: Condvar,
    exited: AtomicBool,
    exit: Mutex<Option<ExitStatus>>,
    bytes_read: AtomicU64,
}

/// A running session.
pub struct Session {
    term: Arc<Mutex<Terminal>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pty: Arc<Mutex<Pty>>,
    shared: Arc<Shared>,
    pid: u32,
    cwd: Option<PathBuf>,
}

/// Locks a mutex, recovering the data if a thread panicked while holding it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Session {
    /// Starts the program and the reader and parser threads. `waker` runs on the parser thread
    /// whenever the terminal changed or an event is queued.
    pub fn spawn(opts: SessionOptions, waker: Waker) -> std::io::Result<Session> {
        let mut pty = Pty::spawn(&opts.spawn)?;
        let reader = pty.take_reader().ok_or_else(|| std::io::Error::other("pty reader taken"))?;
        let writer = pty.take_writer().ok_or_else(|| std::io::Error::other("pty writer taken"))?;
        let size = opts.spawn.size;
        let term = Arc::new(Mutex::new(Terminal::new(size.cols, size.rows, opts.terminal.clone())));
        let shared = Arc::new(Shared {
            events: Mutex::new(Vec::new()),
            prompt: Mutex::new(PromptState::default()),
            prompt_changed: Condvar::new(),
            exited: AtomicBool::new(false),
            exit: Mutex::new(None),
            bytes_read: AtomicU64::new(0),
        });
        let session = Session {
            term,
            writer: Arc::new(Mutex::new(writer)),
            pid: pty.pid(),
            pty: Arc::new(Mutex::new(pty)),
            shared,
            cwd: opts.spawn.cwd.clone(),
        };
        let (tx, rx) = bounded::<Vec<u8>>(opts.channel_depth.max(1));
        spawn_reader(reader, tx, opts.read_buffer.max(1024), session.shared.clone())?;
        spawn_parser(
            rx,
            session.term.clone(),
            session.writer.clone(),
            session.pty.clone(),
            session.shared.clone(),
            opts.lock_slice.max(1024),
            waker.clone(),
        )?;
        spawn_exit_watcher(session.pty.clone(), session.shared.clone(), opts.exit_poll, waker)?;
        Ok(session)
    }

    /// The terminal, for the UI and the IPC handlers to lock briefly.
    pub fn terminal(&self) -> &Arc<Mutex<Terminal>> {
        &self.term
    }

    /// Locks the terminal.
    pub fn lock(&self) -> MutexGuard<'_, Terminal> {
        lock(&self.term)
    }

    /// Writes bytes to the program. A write containing Enter counts as a line of input for
    /// `wait-prompt`.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        if bytes.iter().any(|&b| b == b'\r' || b == b'\n') {
            lock(&self.shared.prompt).enter_count += 1;
        }
        let mut w = lock(&self.writer);
        w.write_all(bytes)?;
        w.flush()
    }

    /// Resizes the PTY and the terminal.
    pub fn resize(&self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) {
        let size = PtySize {
            cols,
            rows,
            pixel_width: cols.saturating_mul(cell_width),
            pixel_height: rows.saturating_mul(cell_height),
        };
        {
            let mut t = self.lock();
            t.set_cell_pixels(cell_width, cell_height);
            if t.cols() != cols || t.rows() != rows {
                t.resize(cols, rows);
            }
        }
        let _ = lock(&self.pty).resize(size);
    }

    /// Takes the events queued since the last call.
    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut *lock(&self.shared.events))
    }

    /// The program has exited and its output has been read to the end.
    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    /// How the program ended, once it has.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        *lock(&self.shared.exit)
    }

    /// The child's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The directory the session started in.
    pub fn start_cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    /// Bytes read from the PTY so far.
    pub fn bytes_read(&self) -> u64 {
        self.shared.bytes_read.load(Ordering::Relaxed)
    }

    /// The prompt state now.
    pub fn prompt_state(&self) -> PromptState {
        *lock(&self.shared.prompt)
    }

    /// A handle that waits for the shell to settle at a prompt, usable from another thread.
    pub fn prompt_waiter(&self) -> PromptWaiter {
        PromptWaiter { shared: self.shared.clone() }
    }

    /// Ends the program and its process tree.
    pub fn kill(&self) {
        let _ = lock(&self.pty).kill();
    }
}

/// Waits for a session's shell to finish the command that was last entered.
pub struct PromptWaiter {
    shared: Arc<Shared>,
}

impl PromptWaiter {
    /// Blocks until no command is running and the shell has shown a prompt since the last line of
    /// input was sent, then returns the last exit code. Errors on timeout, on exit, or when the
    /// shell never sent an OSC 133 mark within the time allowed.
    pub fn wait(&self, timeout: Duration) -> Result<Option<i32>, String> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.shared.prompt);
        loop {
            if !state.running && state.settled_at >= state.enter_count && state.integrated {
                return Ok(state.last_exit);
            }
            if self.shared.exited.load(Ordering::Acquire) {
                return Err("the program exited".into());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(if state.integrated {
                    "timed out".into()
                } else {
                    "timed out; the shell sent no OSC 133 prompt marks (is shell integration installed?)".into()
                });
            }
            let wait = (deadline - now).min(Duration::from_millis(250));
            state = self
                .shared
                .prompt_changed
                .wait_timeout(state, wait)
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

/// Starts the thread that reads the PTY.
fn spawn_reader(
    mut reader: Box<dyn Read + Send>, tx: Sender<Vec<u8>>, size: usize, shared: Arc<Shared>,
) -> std::io::Result<()> {
    thread::Builder::new().name("terminus pty reader".into()).spawn(move || {
        let mut buf = vec![0u8; size];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    shared.bytes_read.fetch_add(n as u64, Ordering::Relaxed);
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    })?;
    Ok(())
}

/// Starts the thread that applies PTY output to the terminal.
fn spawn_parser(
    rx: Receiver<Vec<u8>>, term: Arc<Mutex<Terminal>>, writer: Arc<Mutex<Box<dyn Write + Send>>>, pty: Arc<Mutex<Pty>>,
    shared: Arc<Shared>, slice: usize, waker: Waker,
) -> std::io::Result<()> {
    thread::Builder::new().name("terminus parser".into()).spawn(move || {
        while let Ok(first) = rx.recv() {
            apply_chunk(&first, &term, &writer, &shared, slice);
            while let Ok(more) = rx.try_recv() {
                apply_chunk(&more, &term, &writer, &shared, slice);
            }
            waker();
        }
        let status = lock(&pty).try_wait().ok().flatten();
        mark_exited(&shared, status, &waker);
    })?;
    Ok(())
}

/// Starts the thread that notices the child exiting. On Windows the output pipe stays open until
/// the pseudoconsole closes, so the reader alone cannot tell; this polls the process instead and,
/// after a short grace period for the last output, marks the session exited.
fn spawn_exit_watcher(pty: Arc<Mutex<Pty>>, shared: Arc<Shared>, poll: Duration, waker: Waker) -> std::io::Result<()> {
    thread::Builder::new().name("terminus exit watcher".into()).spawn(move || {
        loop {
            if shared.exited.load(Ordering::Acquire) {
                return;
            }
            let status = lock(&pty).try_wait().ok().flatten();
            if let Some(status) = status {
                thread::sleep(poll);
                mark_exited(&shared, Some(status), &waker);
                return;
            }
            thread::sleep(poll);
        }
    })?;
    Ok(())
}

/// Records the exit once and wakes the UI and any `wait-prompt` callers.
fn mark_exited(shared: &Shared, status: Option<ExitStatus>, waker: &Waker) {
    if shared.exited.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut exit = lock(&shared.exit);
    if exit.is_none() {
        *exit = status;
    }
    drop(exit);
    shared.prompt_changed.notify_all();
    waker();
}

/// Applies one chunk in slices, answering replies and queueing events after each slice.
fn apply_chunk(
    chunk: &[u8], term: &Mutex<Terminal>, writer: &Mutex<Box<dyn Write + Send>>, shared: &Shared, slice: usize,
) {
    for part in chunk.chunks(slice) {
        let (replies, events) = {
            let mut t = lock(term);
            t.advance(part);
            (t.take_replies(), t.take_events())
        };
        if !replies.is_empty() {
            let mut w = lock(writer);
            let _ = w.write_all(&replies);
            let _ = w.flush();
        }
        if !events.is_empty() {
            track_prompts(&events, shared);
            lock(&shared.events).extend(events);
        }
    }
}

/// Updates the prompt state from OSC 133 events and wakes `wait-prompt` callers.
fn track_prompts(events: &[Event], shared: &Shared) {
    let mut changed = false;
    let mut p = lock(&shared.prompt);
    for e in events {
        let Event::Prompt(pe) = e else { continue };
        p.integrated = true;
        changed = true;
        match pe {
            PromptEvent::OutputStart => p.running = true,
            PromptEvent::CommandFinished(code) => {
                p.running = false;
                p.last_exit = *code;
                p.commands += 1;
                p.settled_at = p.enter_count;
            }
            PromptEvent::PromptStart | PromptEvent::InputStart => {
                p.running = false;
                p.settled_at = p.enter_count;
            }
        }
    }
    drop(p);
    if changed {
        shared.prompt_changed.notify_all();
    }
}
