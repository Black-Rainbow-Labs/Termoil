//! `termoil-gui`: the Termoil window (`docs/TDD.md` section 4.8).
//!
//! `termoil-gui [--cwd DIR] [--command CMD] [--socket NAME] [--headless] [--separate]`. `--separate`
//! starts an instance with its own socket instead of handing the window to the running one. With `--headless` it
//! serves IPC with no windows (the E5 tests and scripts use it). When another instance already
//! serves the socket, the new window opens in that instance and this process exits.
//!
//! `--bench-latency KEYS OUT` runs the P2 latency benchmark and writes its JSON result to `OUT`.
//! Its pane runs `termoil --echo-child`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use termoil_app::config::Config;
use termoil_app::gui::bench::LatencyBench;

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
    unsafe { std::env::set_var("TERMOIL_PRESENT", "immediate") };
    // The child must be a console program to read the pseudoconsole, so it is termoil.exe.
    let cli =
        std::env::current_exe().ok().map(|p| p.with_file_name(if cfg!(windows) { "termoil.exe" } else { "termoil" }));
    let cli = cli.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    args.command = Some(format!("\"{cli}\" --echo-child"));
    Some(LatencyBench::new(keys, out))
}

/// Entry point.
fn main() -> ExitCode {
    termoil_app::gui::mark_start();
    termoil_app::update::apply::take_restore_from_env();
    termoil_app::platform::set_app_id();
    termoil_app::crash::install();
    let mut args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("termoil-gui: {e}");
            return ExitCode::from(2);
        }
    };
    let socket = args
        .socket
        .clone()
        .or_else(|| args.separate.then(|| termoil_protocol::separate_socket_name(std::process::id())));
    if let Some(s) = socket {
        // SAFETY: set before any other thread starts.
        unsafe { std::env::set_var("TERMOIL_SOCKET", s) };
    }
    let _ = termoil_app::app::INSTANCE
        .set(termoil_app::app::InstanceMode { separate: args.separate, headless: args.headless });
    let path = Config::path();
    let config = Config::load(&path).map(|(c, _)| c).unwrap_or_default();
    let socket = termoil_protocol::socket_name();
    let bench = bench_setup(&mut args);
    let result = if args.headless {
        let first = args.separate.then(|| (args.cwd.clone().map(std::path::PathBuf::from), args.command.clone()));
        termoil_app::headless::run(config, path, &socket, first)
    } else {
        termoil_app::gui::run(config, path, &socket, args.cwd, args.command, bench)
    };
    // The closing telemetry request, which waits at most 600 ms and only when telemetry is on.
    termoil_app::telemetry::close(std::time::Duration::from_millis(600));
    // Replies to the request that ended the instance (quit) are written by the IPC threads; give
    // them a moment to reach their clients before the process ends.
    std::thread::sleep(std::time::Duration::from_millis(150));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("termoil-gui: {e}");
            ExitCode::FAILURE
        }
    }
}
