//! `terminus-gui`: the Terminus window (`docs/TDD.md` section 4.8).
//!
//! `terminus-gui [--cwd DIR] [--command CMD] [--socket NAME] [--headless]`. With `--headless` it
//! serves IPC with no windows (the E5 tests and scripts use it). When another instance already
//! serves the socket, the new window opens in that instance and this process exits.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use terminus_app::config::Config;

/// Parsed arguments.
#[derive(Default, Debug)]
struct Args {
    cwd: Option<String>,
    command: Option<String>,
    headless: bool,
}

/// Parses the arguments; `--socket` is applied to the environment so every part of the process
/// uses it.
fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--cwd" => a.cwd = Some(it.next().ok_or("--cwd needs a folder")?),
            "--command" | "-e" => a.command = Some(it.next().ok_or("--command needs a program")?),
            // SAFETY: set before any other thread starts.
            "--socket" => unsafe { std::env::set_var("TERMINUS_SOCKET", it.next().ok_or("--socket needs a name")?) },
            "--headless" => a.headless = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(a)
}

/// Entry point.
fn main() -> ExitCode {
    terminus_app::gui::mark_start();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("terminus-gui: {e}");
            return ExitCode::from(2);
        }
    };
    let path = Config::path();
    let config = Config::load(&path).map(|(c, _)| c).unwrap_or_default();
    let socket = terminus_protocol::socket_name();
    let result = if args.headless {
        terminus_app::headless::run(config, path, &socket)
    } else {
        terminus_app::gui::run(config, path, &socket, args.cwd, args.command)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("terminus-gui: {e}");
            ExitCode::FAILURE
        }
    }
}
