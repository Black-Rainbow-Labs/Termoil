//! The latency echo child (`termoil --echo-child`) on Unix: it puts its terminal in raw mode, so a
//! typed character is echoed once by the child with no line editing and no echo from the line
//! discipline, and Ctrl+C ends it.
#![cfg(unix)]

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use termoil_pty::{ExitStatus, Pty, PtySize, SpawnOptions};

/// Reads from the pty on a thread and sends each chunk on a channel.
fn reader_channel(mut pty: Pty) -> (Pty, Receiver<Vec<u8>>, Box<dyn Write + Send>) {
    let mut reader = pty.take_reader().expect("reader");
    let writer = pty.take_writer().expect("writer");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    (pty, rx, writer)
}

/// Collects output until it contains `needle` or five seconds pass; returns everything read.
fn read_until(rx: &Receiver<Vec<u8>>, seen: &mut Vec<u8>, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !String::from_utf8_lossy(seen).contains(needle) && Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            seen.extend_from_slice(&chunk);
        }
    }
    String::from_utf8_lossy(seen).into_owned()
}

#[test]
fn echo_child_runs_in_raw_mode() {
    let opts = SpawnOptions {
        program: env!("CARGO_BIN_EXE_termoil").to_string(),
        args: vec!["--echo-child".to_string()],
        cwd: None,
        env: Vec::new(),
        size: PtySize { cols: 80, rows: 24, pixel_width: 0, pixel_height: 0 },
        login: false,
    };
    let (mut pty, rx, mut writer) = reader_channel(Pty::spawn(&opts).expect("spawn termoil --echo-child"));
    let mut seen = Vec::new();
    let prompt = read_until(&rx, &mut seen, "latency echo\r\n");
    assert!(prompt.contains("latency echo\r\n"), "prompt line: {prompt:?}");
    seen.clear();
    writer.write_all(b"abc").unwrap();
    writer.flush().unwrap();
    let echoed = read_until(&rx, &mut seen, "abc");
    assert_eq!(echoed, "abc", "one echo per character, sent without waiting for a newline");
    writer.write_all(&[3]).unwrap();
    assert_eq!(pty.wait().expect("wait"), ExitStatus::Code(0), "Ctrl+C ends the echo child normally");
}
