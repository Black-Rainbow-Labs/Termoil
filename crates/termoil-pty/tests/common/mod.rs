//! Shared test support: a session that reads the pty on a thread into a buffer and waits on markers.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use termoil_pty::{ConptyChoice, ConptyConfig, Pty, PtySize, SpawnOptions};

/// The overall deadline for every wait on child output.
pub const DEADLINE: Duration = Duration::from_secs(30);

/// Output captured from the child and the condition variable that signals new bytes.
type Shared = Arc<(Mutex<Vec<u8>>, Condvar)>;

/// A child on a pty with its output collected by a reader thread.
pub struct Session {
    pub pty: Pty,
    pub writer: Box<dyn Write + Send>,
    shared: Shared,
    pub reader_thread: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    /// Spawns `opts` and starts the reader thread that appends output to the shared buffer.
    pub fn start(opts: &SpawnOptions, config: &ConptyConfig) -> Session {
        let mut pty = Pty::spawn_with_config(opts, config).expect("spawn");
        let mut reader = pty.take_reader().expect("reader");
        let writer = pty.take_writer().expect("writer");
        let shared: Shared = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let for_thread = Arc::clone(&shared);
        let reader_thread = std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 * 1024];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let (lock, cvar) = &*for_thread;
                lock.lock().unwrap().extend_from_slice(&chunk[..n]);
                cvar.notify_all();
            }
        });
        Session { pty, writer, shared, reader_thread: Some(reader_thread) }
    }

    /// Waits until `predicate` is true of the collected bytes or the deadline passes; returns whether it became true.
    pub fn wait_until(&self, predicate: impl Fn(&[u8]) -> bool) -> bool {
        let (lock, cvar) = &*self.shared;
        let end = Instant::now() + DEADLINE;
        let mut guard = lock.lock().unwrap();
        while !predicate(&guard) {
            let now = Instant::now();
            if now >= end {
                return false;
            }
            guard = cvar.wait_timeout(guard, end - now).unwrap().0;
        }
        true
    }

    /// Waits until the byte string `marker` appears in the output.
    pub fn wait_for(&self, marker: &str) -> bool {
        self.wait_until(|bytes| find(bytes, marker.as_bytes()).is_some())
    }

    /// A copy of everything received so far.
    pub fn bytes(&self) -> Vec<u8> {
        self.shared.0.lock().unwrap().clone()
    }

    /// Everything received so far with terminal escape sequences removed.
    pub fn plain_text(&self) -> String {
        strip_escapes(&self.bytes())
    }
}

/// Position of the first occurrence of `needle` in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Removes CSI, OSC, DCS, APC and two byte escape sequences, leaving printable text and line breaks.
pub fn strip_escapes(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i = skip_escape(bytes, i);
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Returns the index just past the escape sequence that starts at `start`.
fn skip_escape(bytes: &[u8], start: usize) -> usize {
    let Some(&kind) = bytes.get(start + 1) else { return bytes.len() };
    let mut i = start + 2;
    match kind {
        b'[' => {
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            i + 1
        }
        b']' | b'P' | b'_' | b'^' | b'X' => {
            while i < bytes.len() && bytes[i] != 0x07 && !(bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\')) {
                i += 1;
            }
            if bytes.get(i) == Some(&0x07) { i + 1 } else { i + 2 }
        }
        b'(' | b')' | b'#' | b'%' => i + 1,
        _ => i,
    }
}

/// Renders bytes with escapes visible, for logs and documentation.
pub fn show_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            0x1b => out.push_str("ESC "),
            0x07 => out.push_str("BEL "),
            b'\r' => out.push_str("CR "),
            b'\n' => out.push_str("LF\n"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

/// The default size used by tests.
pub fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols, rows, pixel_width: 0, pixel_height: 0 }
}

/// Options that run `program args` with the Termoil environment and the given size.
pub fn command(program: &str, args: &[&str], size: PtySize) -> SpawnOptions {
    let mut opts = SpawnOptions::default_shell(size);
    opts.program = program.to_string();
    opts.args = args.iter().map(|a| a.to_string()).collect();
    opts.login = false;
    opts
}

/// The directory where `cargo test` puts example binaries.
pub fn examples_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().join("examples")
}

/// Path of the built `helper` example binary.
pub fn helper_path() -> String {
    let name = if cfg!(windows) { "helper.exe" } else { "helper" };
    examples_dir().join(name).to_string_lossy().into_owned()
}

/// The directory that `tools/fetch-conpty.ps1` fills.
pub fn bundled_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../installer/windows/conpty")
}

/// True when the bundled conpty.dll and OpenConsole.exe have been fetched.
pub fn bundled_present() -> bool {
    bundled_dir().join("conpty.dll").is_file() && bundled_dir().join("OpenConsole.exe").is_file()
}

/// Every pseudoconsole configuration available here: the inbox one, and the bundled one when fetched.
pub fn configs() -> Vec<(&'static str, ConptyConfig)> {
    let mut all = vec![("inbox", ConptyConfig { choice: ConptyChoice::Inbox, flags: None, ..ConptyConfig::default() })];
    if bundled_present() {
        all.push((
            "bundled",
            ConptyConfig {
                choice: ConptyChoice::Bundled(bundled_dir().canonicalize().unwrap()),
                flags: None,
                ..ConptyConfig::default()
            },
        ));
    } else {
        println!("bundled conpty not fetched (run tools/fetch-conpty.ps1); testing the inbox ConPTY only");
    }
    all
}

/// A fresh scratch directory under the test target directory.
pub fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
