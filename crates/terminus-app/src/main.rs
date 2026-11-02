//! `terminus-gui`: the Terminus window (`docs/TDD.md` section 4.8).
//!
//! `terminus-gui [--cwd DIR] [--command CMD] [--socket NAME] [--headless] [--separate]`. `--separate`
//! starts an instance with its own socket instead of handing the window to the running one. With `--headless` it
//! serves IPC with no windows (the E5 tests and scripts use it). When another instance already
//! serves the socket, the new window opens in that instance and this process exits.
//!
//! `--bench-latency KEYS OUT` runs the P2 latency benchmark and writes its JSON result to `OUT`.
//! Its pane runs `terminus --echo-child`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use terminus_app::config::Config;
use terminus_app::gui::bench::LatencyBench;

/// Parsed arguments.
#[derive(Default, Debug)]
struct Args {
    cwd: Option<String>,
    command: Option<String>,
    headless: bool,
    separate: bool,
    socket: Option<String>,
    bench: Option<(usize, String)>,
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
            "--socket" => a.socket = Some(it.next().ok_or("--socket needs a name")?),
            "--separate" => a.separate = true,
            "--headless" => a.headless = true,
            "--bench-latency" => {
                let keys = it
                    .next()
                    .ok_or("--bench-latency needs a key count")?
                    .parse()
                    .map_err(|_| "--bench-latency needs a number")?;
                a.bench = Some((keys, it.next().ok_or("--bench-latency needs an output file")?));
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(a)
}

/// Sets up the latency benchmark: unpaced presents and the echo child as the pane's program.
fn bench_setup(args: &mut Args) -> Option<LatencyBench> {
    let (keys, out) = args.bench.take()?;
    // SAFETY: set before any other thread starts.
    unsafe { std::env::set_var("TERMINUS_PRESENT", "immediate") };
    // The child must be a console program to read the pseudoconsole, so it is terminus.exe.
    let cli =
        std::env::current_exe().ok().map(|p| p.with_file_name(if cfg!(windows) { "terminus.exe" } else { "terminus" }));
    let cli = cli.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    args.command = Some(format!("\"{cli}\" --echo-child"));
    Some(LatencyBench::new(keys, out))
}

/// Entry point.
fn main() -> ExitCode {
    terminus_app::gui::mark_start();
    terminus_app::platform::set_app_id();
    terminus_app::crash::install();
    let mut args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("terminus-gui: {e}");
            return ExitCode::from(2);
        }
    };
    let socket = args
        .socket
        .clone()
        .or_else(|| args.separate.then(|| terminus_protocol::separate_socket_name(std::process::id())));
    if let Some(s) = socket {
        // SAFETY: set before any other thread starts.
        unsafe { std::env::set_var("TERMINUS_SOCKET", s) };
    }
    let _ = terminus_app::app::INSTANCE
        .set(terminus_app::app::InstanceMode { separate: args.separate, headless: args.headless });
    let path = Config::path();
    let config = Config::load(&path).map(|(c, _)| c).unwrap_or_default();
    let socket = terminus_protocol::socket_name();
    let bench = bench_setup(&mut args);
    let result = if args.headless {
        let first = args.separate.then(|| (args.cwd.clone().map(std::path::PathBuf::from), args.command.clone()));
        terminus_app::headless::run(config, path, &socket, first)
    } else {
        terminus_app::gui::run(config, path, &socket, args.cwd, args.command, bench)
    };
    // Replies to the request that ended the instance (quit) are written by the IPC threads; give
    // them a moment to reach their clients before the process ends.
    std::thread::sleep(std::time::Duration::from_millis(150));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("terminus-gui: {e}");
            ExitCode::FAILURE
        }
    }
}
