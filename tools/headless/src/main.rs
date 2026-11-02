//! `terminus-headless`: a terminal with no window.
//!
//! ```text
//! terminus-headless run [--cols N] [--rows N] [--record FILE] [--dump] [--script FILE --snaps DIR] -- PROGRAM [ARGS...]
//! terminus-headless replay FILE [--cols N] [--rows N]
//! ```
//!
//! `--script` drives the program with a script of waits, input and screen snapshots (see
//! `script.rs`); the snapshots are written to `--snaps`. The run fails when a wait times out.
//!
//! `run` spawns the program under a pseudoterminal of the given size, feeds everything it prints
//! into `terminus-vt`, and writes the terminal's replies (device attributes, cursor reports,
//! DECRQCRA checksums) back to it, so programs that query the terminal (esctest2, vttest, vim) work.
//! `--record` saves the program's output bytes; `--dump` prints the final screen. `replay` feeds a
//! recording whole, one byte at a time and in seeded random chunks, checks the three screens are
//! equal (docs/TEST-SUITE.md section 1) and prints the screen.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use terminus_pty::{Pty, PtySize, SpawnOptions};
use terminus_vt::{Event, Terminal, TerminalConfig, dump};

mod script;

/// Parsed command line options.
struct Options {
    cols: u16,
    rows: u16,
    record: Option<String>,
    dump: bool,
    script: Option<String>,
    snaps: Option<String>,
    rest: Vec<String>,
}

/// Reads `--cols`, `--rows`, `--record`, `--dump`, `--script`, `--snaps` and the words after `--`
/// (or the remaining ones).
fn parse_options(args: &[String]) -> Options {
    let mut o = Options { cols: 80, rows: 24, record: None, dump: false, script: None, snaps: None, rest: Vec::new() };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cols" => o.cols = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(80),
            "--rows" => o.rows = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(24),
            "--record" => o.record = args.get(i + 1).cloned(),
            "--script" => o.script = args.get(i + 1).cloned(),
            "--snaps" => o.snaps = args.get(i + 1).cloned(),
            "--dump" => {
                o.dump = true;
                i += 1;
                continue;
            }
            "--" => {
                o.rest = args[i + 1..].to_vec();
                break;
            }
            _ => {
                o.rest = args[i..].to_vec();
                break;
            }
        }
        i += 2;
    }
    o
}

/// Spawns the program and pumps bytes between it and the terminal until it exits. Returns the
/// program's exit code.
fn run(o: &Options) -> i32 {
    let Some((program, args)) = o.rest.split_first() else {
        eprintln!("usage: terminus-headless run [--cols N] [--rows N] [--record FILE] [--dump] -- PROGRAM [ARGS...]");
        return 2;
    };
    let size = PtySize { cols: o.cols, rows: o.rows, pixel_width: 0, pixel_height: 0 };
    let opts = SpawnOptions {
        program: program.clone(),
        args: args.to_vec(),
        cwd: None,
        env: vec![("TERM".into(), "xterm-256color".into())],
        size,
        login: false,
    };
    let mut pty = match Pty::spawn(&opts) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("terminus-headless: cannot start {program}: {e}");
            return 127;
        }
    };
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut reader = pty.take_reader().expect("reader");
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut writer = pty.take_writer().expect("writer");
    let config = TerminalConfig { allow_osc52_read: true, ..TerminalConfig::default() };
    let mut term = Terminal::new(o.cols, o.rows, config);
    term.set_apply_resize_requests(true);
    let mut recording = Vec::new();
    let mut runner = match load_script(o) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("terminus-headless: {e}");
            let _ = pty.kill();
            return 2;
        }
    };
    let code = pump(&mut pty, &rx, &mut *writer, &mut term, &mut recording, &mut runner);
    finish(o, &term, &recording);
    code
}

/// The script runner for `--script`, with its snapshot folder created.
fn load_script(o: &Options) -> Result<Option<script::Runner>, String> {
    let Some(path) = &o.script else { return Ok(None) };
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let steps = script::parse(&text).map_err(|e| format!("{path}: {e}"))?;
    let dir = std::path::PathBuf::from(o.snaps.clone().unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(Some(script::Runner::new(steps, dir)))
}

/// A snapshot for a script: the screen as text, for a person reviewing it, then every row in the
/// full cell dump format, so attributes and colours are compared as well.
fn snapshot(term: &Terminal) -> Vec<String> {
    let mut lines = dump::text_dump(term);
    lines.push("--- cells".into());
    lines.extend(dump::grid_dump(term));
    lines
}

/// Gives the script the new output and runs its next steps. On failure prints why and the screen.
fn drive_script(runner: &mut Option<script::Runner>, chunk: &[u8], term: &Terminal, writer: &mut dyn Write) -> bool {
    let Some(r) = runner else { return true };
    r.output(chunk);
    match r.drive(&|| snapshot(term), writer) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("terminus-headless: script failed at {e}; the screen:");
            for line in dump::text_dump(term) {
                eprintln!("|{line}");
            }
            false
        }
    }
}

/// Applies output, resize requests and replies until the child has exited and its output has drained.
fn pump(
    pty: &mut Pty, rx: &mpsc::Receiver<Vec<u8>>, writer: &mut dyn Write, term: &mut Terminal, recording: &mut Vec<u8>,
    runner: &mut Option<script::Runner>,
) -> i32 {
    let mut exited: Option<(i32, Instant)> = None;
    let mut last_size = (term.cols(), term.rows());
    let mut clipboard: Vec<u8> = Vec::new();
    loop {
        let chunk = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => {
                recording.extend_from_slice(&chunk);
                term.advance(&chunk);
                apply_host_events(pty, term, &mut last_size, &mut clipboard);
                let replies = term.take_replies();
                if !replies.is_empty() {
                    let _ = writer.write_all(&replies);
                    let _ = writer.flush();
                }
                chunk
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return script_code(runner, exited.map(|e| e.0).unwrap_or(0)),
            Err(mpsc::RecvTimeoutError::Timeout) => Vec::new(),
        };
        if !drive_script(runner, &chunk, term, writer) {
            let _ = pty.kill();
            return 1;
        }
        if exited.is_none()
            && let Ok(Some(status)) = pty.try_wait()
        {
            let code = match status {
                terminus_pty::ExitStatus::Code(c) => c,
                terminus_pty::ExitStatus::Signal(s) => 128 + s,
            };
            exited = Some((code, Instant::now()));
        }
        // After exit, output still in flight is read for half a second; ConPTY only ends the
        // stream when the pseudoconsole is closed.
        if let Some((code, at)) = exited
            && at.elapsed() > Duration::from_millis(500)
        {
            return script_code(runner, code);
        }
    }
}

/// The exit code once the program has ended: 1 when a script still had steps to run.
fn script_code(runner: &Option<script::Runner>, code: i32) -> i32 {
    if runner.as_ref().is_some_and(|r| !r.done()) {
        eprintln!("terminus-headless: the program ended before the script did");
        return 1;
    }
    code
}

/// Carries out what the terminal asked of its host. A resize request resizes the terminal when it
/// is not already that size, and the program's pseudoterminal is told the new size when it changed
/// since `last` so the program gets SIGWINCH. A clipboard write is kept in `clipboard` and a
/// clipboard read is answered from it, naming the selection `s0` as xterm does for the default
/// selection, which is what esctest2 expects.
fn apply_host_events(pty: &Pty, term: &mut Terminal, last: &mut (u16, u16), clipboard: &mut Vec<u8>) {
    for event in term.take_events() {
        match event {
            Event::ResizeRequest { cols, rows } if (cols, rows) != (term.cols(), term.rows()) => {
                term.resize(cols, rows);
            }
            Event::ClipboardWrite { data, .. } => *clipboard = data,
            Event::ClipboardRead { selection } => {
                let name = if selection == "c" { "s0" } else { selection.as_str() };
                term.reply_clipboard(name, clipboard);
            }
            _ => {}
        }
    }
    let now = (term.cols(), term.rows());
    if now != *last {
        *last = now;
        let size = PtySize { cols: now.0, rows: now.1, pixel_width: 0, pixel_height: 0 };
        let _ = pty.resize(size);
    }
}

/// Writes the recording and prints the screen when asked.
fn finish(o: &Options, term: &Terminal, recording: &[u8]) {
    if let Some(path) = &o.record
        && let Err(e) = std::fs::write(path, recording)
    {
        eprintln!("terminus-headless: cannot write {path}: {e}");
    }
    if o.dump {
        for line in dump::text_dump(term) {
            println!("{line}");
        }
    }
}

/// Replays a recording three ways and prints the screen; exits 1 when the ways disagree.
fn replay(o: &Options) -> i32 {
    let Some(path) = o.rest.first() else {
        eprintln!("usage: terminus-headless replay FILE [--cols N] [--rows N]");
        return 2;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("terminus-headless: cannot read {path}: {e}");
            return 2;
        }
    };
    let feed = |chunks: &mut dyn Iterator<Item = &[u8]>| {
        let mut t = Terminal::new(o.cols, o.rows, TerminalConfig::default());
        for c in chunks {
            t.advance(c);
        }
        dump::grid_dump(&t)
    };
    let whole = feed(&mut std::iter::once(bytes.as_slice()));
    let single = feed(&mut bytes.chunks(1));
    let random = feed(&mut random_chunks(&bytes, 0x5eed).into_iter());
    for (name, other) in [("one byte at a time", &single), ("random chunks", &random)] {
        if let Some(d) = dump::diff(&whole, other) {
            eprintln!("replay {name} differs from whole: {d}");
            return 1;
        }
    }
    let mut t = Terminal::new(o.cols, o.rows, TerminalConfig::default());
    t.advance(&bytes);
    for line in dump::text_dump(&t) {
        println!("{line}");
    }
    0
}

/// Splits `bytes` at pseudo random points from a fixed seed.
fn random_chunks(bytes: &[u8], seed: u64) -> Vec<&[u8]> {
    let mut x = seed | 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let end = (i + 1 + (x % 97) as usize).min(bytes.len());
        out.push(&bytes[i..end]);
        i = end;
    }
    out
}

/// Entry point.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("run") => run(&parse_options(&args[1..])),
        Some("replay") => {
            let mut o = parse_options(&args[1..]);
            if o.rest.is_empty() {
                o.rest = args[1..].iter().filter(|a| !a.starts_with("--")).take(1).cloned().collect();
            }
            replay(&o)
        }
        _ => {
            eprintln!("usage: terminus-headless run|replay ...");
            2
        }
    };
    std::process::exit(code);
}
