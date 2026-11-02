//! Test helper child: writes exact bytes or numbered lines to its console output.
//!
//! Usage: `helper raw <file> [vt]` writes the file's bytes to stdout (with `vt` it first turns on
//! virtual terminal processing); `helper numbers <count>` writes `LINE 0000001` style lines.

use std::fs::File;
use std::io::Write;
use std::mem::ManuallyDrop;

/// Entry point: dispatches on the first argument.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("raw") => write_raw(&args[2], args.get(3).is_some_and(|mode| mode == "vt")),
        Some("numbers") => write_numbers(args[2].parse().expect("count")),
        _ => eprintln!("usage: helper raw <file> [vt] | helper numbers <count>"),
    }
}

/// Opens the process's stdout handle as a file without taking ownership of it.
#[cfg(windows)]
fn stdout_file() -> ManuallyDrop<File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    // SAFETY: the handle belongs to the process and ManuallyDrop keeps it from being closed.
    ManuallyDrop::new(unsafe { File::from_raw_handle(std::io::stdout().as_raw_handle()) })
}

/// Opens the process's stdout descriptor as a file without taking ownership of it.
#[cfg(unix)]
fn stdout_file() -> ManuallyDrop<File> {
    use std::os::unix::io::FromRawFd;
    // SAFETY: descriptor 1 belongs to the process and ManuallyDrop keeps it from being closed.
    ManuallyDrop::new(unsafe { File::from_raw_fd(1) })
}

/// Turns on virtual terminal processing for the console output, as Windows Terminal hosted programs have it.
#[cfg(windows)]
fn enable_vt_processing() {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, SetConsoleMode,
    };
    let handle = HANDLE(std::io::stdout().as_raw_handle());
    let mut mode = CONSOLE_MODE(0);
    // SAFETY: the handle is the process's console output and `mode` is a valid out pointer.
    unsafe {
        if GetConsoleMode(handle, &mut mode).is_ok() {
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING).ok();
        }
    }
}

/// No console mode exists on Unix.
#[cfg(unix)]
fn enable_vt_processing() {}

/// Writes the bytes of `path` to stdout unchanged.
fn write_raw(path: &str, vt: bool) {
    if vt {
        enable_vt_processing();
    }
    let bytes = std::fs::read(path).expect("read input file");
    let mut out = stdout_file();
    out.write_all(&bytes).expect("write");
}

/// Writes `count` lines of the form `LINE 0000001` in large blocks.
fn write_numbers(count: u32) {
    let mut out = stdout_file();
    let mut block = String::new();
    for n in 1..=count {
        block.push_str(&format!("LINE {n:07}\n"));
        if block.len() >= 1 << 16 {
            out.write_all(block.as_bytes()).expect("write");
            block.clear();
        }
    }
    out.write_all(block.as_bytes()).expect("write");
}
