//! W4: the pseudoconsole's output pipe holds more than one small chunk, so a reader that pauses to let output collect
//! gets it in large reads (task-2485). With the system default pipe of about 4 KiB, the app's 1 ms pause after each
//! short read held a kitty graphics stream to about 4 MB/s.
#![cfg(windows)]

mod common;

use common::*;
use std::io::Read;
use std::time::Duration;
use termoil_pty::{ConptyConfig, DEFAULT_OUTPUT_PIPE_BYTES, Pty};

/// Streams `bytes` of output through `type` and returns the largest single read, pausing 1 ms after each read the
/// way the app's reader does while output streams.
fn largest_read(config: &ConptyConfig, file: &std::path::Path, end_marker: &str) -> usize {
    let opts = command(
        "cmd.exe",
        &["/d", "/c", &format!("type {} & echo {end_marker}", file.to_string_lossy().replace('/', "\\"))],
        size(200, 50),
    );
    let mut pty = Pty::spawn_with_config(&opts, config).expect("spawn");
    let mut reader = pty.take_reader().expect("reader");
    let mut buf = vec![0u8; 64 * 1024];
    let (mut seen, mut largest) = (Vec::new(), 0usize);
    while find(&seen, end_marker.as_bytes()).is_none() {
        let n = reader.read(&mut buf).expect("read");
        assert!(n > 0, "the pty closed before the end marker");
        largest = largest.max(n);
        seen.extend_from_slice(&buf[..n]);
        std::thread::sleep(Duration::from_millis(1));
    }
    largest
}

/// A file of printable lines, `count` of them, 199 characters each.
fn write_lines(name: &str, count: usize) -> std::path::PathBuf {
    let dir = scratch_dir(name);
    let path = dir.join("lines.txt");
    let line: String = (0..199).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    std::fs::write(&path, format!("{line}\r\n").repeat(count)).expect("write lines");
    path
}

#[test]
fn default_output_pipe_is_one_mebibyte() {
    assert_eq!(ConptyConfig::default().output_pipe_bytes, DEFAULT_OUTPUT_PIPE_BYTES);
    assert_eq!(DEFAULT_OUTPUT_PIPE_BYTES, 1 << 20);
}

#[test]
fn paused_reader_gets_large_reads() {
    let file = write_lines("w4-output-pipe", 20_000);
    for (name, config) in configs() {
        let largest = largest_read(&config, &file, "W4-DONE");
        assert!(largest > 16 * 1024, "{name}: the largest read was {largest} bytes; the output pipe is too small");
    }
}
