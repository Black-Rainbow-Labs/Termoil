//! `termoil`: the command line.
//!
//! - `termoil +<action> [--param value ...]` sends one catalogue action to the running instance
//!   over the IPC socket and prints the result: text as text, anything else as JSON. `version`,
//!   `show-config` and `install-shell-integration` also work with no instance running.
//! - `termoil +mcp` is an MCP server on stdin and stdout whose tools are the catalogue's command
//!   line actions (the generated `docs/mcp-tools.json`), each forwarded to the running instance.
//! - `termoil ssh [ssh arguments]` copies the terminfo entry to the remote host, then runs ssh with
//!   `TERM=xterm-termoil` (see `ssh.rs`).
//! - `termoil` with no `+` starts the windowed app, `termoil-gui`, with the same arguments.

mod args;
mod mcp;
mod ssh;

use std::process::ExitCode;
use std::time::Duration;

use serde_json::{Map, Value};
use termoil_protocol::{Client, Response};

/// Entry point.
fn main() -> ExitCode {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().is_some_and(|a| a == "--socket") && argv.len() >= 2 {
        // SAFETY: set before any other thread starts; the client reads it to find the instance.
        unsafe { std::env::set_var("TERMOIL_SOCKET", &argv[1]) };
        argv.drain(..2);
    }
    match argv.first().map(String::as_str) {
        Some("+mcp") => mcp::serve(),
        Some("ssh") => ssh::run(&argv[1..]),
        // The update helper, started by install-update from a copy in the download folder.
        Some("--apply-update") => {
            if termoil_app::update::apply::run(&argv[1..]) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        // The P2 latency benchmark's child: a console program, which termoil-gui is not.
        Some("--echo-child") => {
            if termoil_app::gui::bench::echo_child().is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some("+help") | Some("--help") | Some("-h") => {
            print!("{}", args::usage());
            ExitCode::SUCCESS
        }
        Some(a) if a.starts_with('+') => run_command(&a[1..], &argv[1..]),
        _ => launch_gui(&argv),
    }
}

/// Runs one `+<action>` and prints its result.
fn run_command(action: &str, rest: &[String]) -> ExitCode {
    let args = match args::parse(action, rest) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("termoil: {e}");
            return ExitCode::from(2);
        }
    };
    match call(action, args) {
        Ok(resp) if resp.ok => {
            print_result(&resp.result);
            ExitCode::SUCCESS
        }
        Ok(resp) => {
            eprintln!("termoil: {}", resp.error.unwrap_or_default());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("termoil: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Sends an action to the running instance, or runs it here when it needs no instance and none is
/// running.
pub fn call(action: &str, args: Map<String, Value>) -> Result<Response, String> {
    if action == "install-shell-integration" {
        return Ok(Response::from_result(0, termoil_app::actions::install_shell_integration(&args)));
    }
    match Client::connect(Duration::from_secs(2)) {
        Ok(mut c) => c.call(action, args).map_err(|e| format!("the connection to Termoil failed: {e}")),
        Err(_) => offline(action, &args).ok_or_else(|| "Termoil is not running (start it with `termoil`)".to_string()),
    }
}

/// The actions answered without a running instance.
fn offline(action: &str, args: &Map<String, Value>) -> Option<Response> {
    if matches!(action, "check-for-updates" | "list-updates" | "install-update") {
        return Some(offline_update(action, args));
    }
    let result = match action {
        "version" => termoil_app::actions::version_json(),
        "show-config" if args.get("default").and_then(Value::as_bool) == Some(true) => {
            Value::String(termoil_app::config::Config::default_file_text())
        }
        "show-config" => {
            let path = termoil_app::config::Config::path();
            match termoil_app::config::Config::load(&path) {
                Ok((c, _)) => Value::String(c.effective_text()),
                Err(e) => {
                    return Some(Response::from_result(
                        0,
                        Err(termoil_protocol::ActionError::Failed(format!(
                            "{}: line {}: {}",
                            path.display(),
                            e.line,
                            e.message
                        ))),
                    ));
                }
            }
        }
        _ => return None,
    };
    Some(Response::from_result(0, Ok(result)))
}

/// Runs an update action in this process with the configuration file's settings, for when no Termoil is
/// running. There is no window to restart, so `install-update` hands the file to the helper and returns.
fn offline_update(action: &str, args: &Map<String, Value>) -> Response {
    let path = termoil_app::config::Config::path();
    let config = termoil_app::config::Config::load(&path).map(|(c, _)| c).unwrap_or_default();
    let wakers: termoil_app::app::WakerFactory = Box::new(|_| std::sync::Arc::new(|| {}));
    let mut app = termoil_app::App::new(config, path, Box::new(termoil_app::app::MemoryClipboard::default()), wakers);
    let call = termoil_protocol::ActionCall { action: action.into(), args: args.clone() };
    let result = match app.run_action(&call) {
        termoil_app::Outcome::Done(r) => r,
        termoil_app::Outcome::Later(job) => job(false),
        termoil_app::Outcome::Wait(..) => Ok(Value::Null),
    };
    Response::from_result(0, result)
}

/// Prints a result: a string as is, null as nothing, anything else as pretty JSON.
fn print_result(v: &Value) {
    match v {
        Value::Null => {}
        Value::String(s) => {
            print!("{s}");
            if !s.ends_with('\n') {
                println!();
            }
        }
        other => println!("{}", serde_json::to_string_pretty(other).unwrap_or_default()),
    }
}

/// Starts `termoil-gui` from the same folder with the same arguments and returns at once.
fn launch_gui(argv: &[String]) -> ExitCode {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { "termoil-gui.exe" } else { "termoil-gui" })));
    let Some(gui) = exe.filter(|p| p.is_file()) else {
        eprintln!("termoil: termoil-gui was not found next to this program");
        return ExitCode::FAILURE;
    };
    match std::process::Command::new(&gui).args(argv).spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("termoil: could not start {}: {e}", gui.display());
            ExitCode::FAILURE
        }
    }
}
