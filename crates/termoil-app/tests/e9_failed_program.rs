//! E9: a pane started with a command stays open when that command fails, so its error can be read.
//! Umbra 0.5.0 started with no model printed "no model set" and exited with code 2 after the first
//! line typed. Termoil closed the pane at once, and because it was the last pane Termoil quit, which
//! looked like a crash and left nothing to read. `shell.hold_on_failure` turns this off.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use termoil_app::app::{App, Effect, MemoryClipboard};
use termoil_app::config::Config;
use termoil_protocol::ActionCall;

/// An app whose shell is bash (cmd on Windows), with `extra` appended to the `[shell]` section.
fn new_app(extra: &str) -> App {
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    let text = format!("[shell]\nprogram = \"{shell}\"\nintegration = false\n{extra}\n[updates]\ncheck = \"off\"\n");
    let config: Config = toml::from_str(&text).expect("test config");
    let wakers: termoil_app::app::WakerFactory = Box::new(|_| Arc::new(|| {}));
    let path = std::env::temp_dir().join(format!("termoil-e9-{}.toml", std::process::id()));
    App::new(config, path, Box::new(MemoryClipboard::default()), wakers)
}

/// A command that prints a line and exits with `code`.
fn command_exiting(code: i32) -> String {
    if cfg!(windows) {
        format!("cmd.exe /c \"echo no model set & exit {code}\"")
    } else {
        format!("bash -c \"echo no model set; exit {code}\"")
    }
}

/// Opens a window running `command` and returns its pane id.
fn open(app: &mut App, command: &str) -> u64 {
    let v = termoil_app::actions::new_window(app, None, Some(command)).expect("window opens");
    v["pane"].as_u64().unwrap()
}

/// Pumps the pane until its program has exited and `done` says the app reacted, or 20 seconds pass.
fn pump_until(app: &mut App, pane: u64, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        termoil_app::events::pump(app, pane, true);
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let state = app.panes.get(&pane).map(|p| (p.session.exited(), p.session.exit_status(), p.held, p.command.clone()));
    panic!("pane {pane} did not reach the expected state; exited, status, held, command: {state:?}");
}

/// Runs an action and returns its answer.
fn action(app: &mut App, name: &str, args: Value) -> Result<Value, String> {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    match app.run_action(&ActionCall { action: name.into(), args }) {
        termoil_app::app::Outcome::Done(r) => r.map_err(|e| e.to_string()),
        _ => Err(format!("{name} did not finish at once")),
    }
}

/// Whether the pane is held: still in the app after its program exited.
fn held(app: &App, pane: u64) -> bool {
    app.panes.get(&pane).is_some_and(|p| p.held)
}

#[test]
fn e9_failed_command_is_held_with_its_output_and_exit_code() {
    let mut app = new_app("");
    let pane = open(&mut app, &command_exiting(2));
    pump_until(&mut app, pane, |a| held(a, pane));
    let screen = action(&mut app, "screen", json!({ "pane": pane })).unwrap();
    let text = screen.as_str().unwrap();
    assert!(text.contains("no model set"), "the program's own output stays:\n{text}");
    assert!(text.contains("exited with code 2"), "the exit code is shown:\n{text}");
    assert!(text.contains("Press any key to close"), "the pane says how to close it:\n{text}");
    assert!(!app.take_effects().contains(&Effect::LastWindowClosed), "the app does not quit");
    assert_eq!(app.model.windows.len(), 1);
}

#[test]
fn e9_a_key_closes_a_held_pane() {
    let mut app = new_app("");
    let pane = open(&mut app, &command_exiting(2));
    pump_until(&mut app, pane, |a| held(a, pane));
    let _ = app.take_effects();
    let answer = action(&mut app, "send-key", json!({ "pane": pane, "key": "enter" })).unwrap();
    assert_eq!(answer["closed"], json!(true), "{answer}");
    assert!(!app.panes.contains_key(&pane));
    assert!(app.take_effects().contains(&Effect::LastWindowClosed), "closing the last pane still quits");
}

#[test]
fn e9_successful_command_still_closes() {
    let mut app = new_app("");
    let pane = open(&mut app, &command_exiting(0));
    pump_until(&mut app, pane, |a| !a.panes.contains_key(&pane));
}

#[test]
fn e9_off_closes_a_failed_command() {
    let mut app = new_app("hold_on_failure = false\n");
    let pane = open(&mut app, &command_exiting(2));
    pump_until(&mut app, pane, |a| !a.panes.contains_key(&pane));
}

#[test]
fn e9_failed_command_reports_its_exit_code() {
    let mut app = new_app("hold_on_failure = false\n");
    let pane = open(&mut app, &command_exiting(7));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !app.panes[&pane].session.exited() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = app.panes[&pane].session.exit_status();
    assert_eq!(status, Some(termoil_pty::ExitStatus::Code(7)), "a command's own exit code is recorded, not login's");
}

#[test]
fn e9_a_shell_that_exits_with_a_failure_still_closes() {
    let mut app = new_app("");
    let pane = termoil_app::actions::new_window(&mut app, None, None).expect("window opens")["pane"].as_u64().unwrap();
    termoil_app::view::send_text(&mut app, pane, "exit 3\n").unwrap();
    pump_until(&mut app, pane, |a| !a.panes.contains_key(&pane));
}
