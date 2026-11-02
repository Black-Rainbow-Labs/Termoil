//! Session restore (`docs/PRD.md` section 5.4): an app with two windows, tabs and nested splits is
//! saved, loaded into a new app, and compared. The panes run real shells.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use termoil_app::app::{App, MemoryClipboard};
use termoil_app::config::Config;
use termoil_app::model::PaneId;
use termoil_app::restore::{SavedNode, SessionFile, build_file, load_file, restore_from, save_session};
use termoil_protocol::ActionCall;

/// An app whose panes run a fast shell, with session restore on.
fn new_app() -> App {
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    let text = format!("[shell]\nprogram = \"{shell}\"\nintegration = false\n\n[session]\nrestore = true\n");
    let config: Config = toml::from_str(&text).expect("test config");
    let wakers: termoil_app::app::WakerFactory = Box::new(|_| Arc::new(|| {}));
    App::new(config, PathBuf::from("unused.toml"), Box::new(MemoryClipboard::default()), wakers)
}

/// A fresh folder for one test.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termoil-restore-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs an action and returns its JSON result.
fn act(app: &mut App, name: &str, args: Value) -> Value {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    match app.run_action(&ActionCall { action: name.into(), args }) {
        termoil_app::Outcome::Done(r) => r.unwrap_or_else(|e| panic!("{name} failed: {e}")),
        termoil_app::Outcome::Wait(..) => Value::Null,
    }
}

/// The pane's output lines now.
fn lines(app: &App, pane: PaneId) -> Vec<String> {
    termoil_app::inspect::scrollback_text(&app.panes[&pane].session.lock(), 500)
}

/// Waits until a line of the pane's output is exactly `mark`.
fn wait_line(app: &App, pane: PaneId, mark: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !lines(app, pane).iter().any(|l| l == mark) {
        assert!(Instant::now() < deadline, "pane {pane} never printed {mark}; it shows {:?}", lines(app, pane));
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The shape of a saved tree: axes, ratios, working directories and commands, without the text.
fn skeleton(node: &SavedNode) -> String {
    match node {
        SavedNode::Pane { cwd, command, .. } => format!("P({cwd:?},{command:?})"),
        SavedNode::Split { axis, ratio, first, second } => {
            format!("S({axis},{ratio:.3},{},{})", skeleton(first), skeleton(second))
        }
    }
}

/// Everything about a file except the saved output.
fn structure(file: &SessionFile) -> Vec<String> {
    let mut out = vec![format!("focused={}", file.focused)];
    for (wi, w) in file.windows.iter().enumerate() {
        out.push(format!("window {wi} active={}", w.active));
        for t in &w.tabs {
            out.push(format!("tab {:?} focus={} zoomed={:?} {}", t.title, t.focus, t.zoomed, skeleton(&t.root)));
        }
    }
    out
}

/// The text of the first pane of a window's first tab.
fn first_text(file: &SessionFile, window: usize, tab: usize) -> String {
    let mut node = &file.windows[window].tabs[tab].root;
    loop {
        match node {
            SavedNode::Pane { text, .. } => return text.clone(),
            SavedNode::Split { first, .. } => node = first,
        }
    }
}

/// Builds two windows: the first has a tab with a vertical split whose right half is split again,
/// and a named second tab; the second window has one tab. Returns the panes that print marks.
fn build_layout(app: &mut App, a: &Path, b: &Path) -> (PaneId, PaneId, PaneId) {
    let first = act(app, "new-window", json!({ "cwd": a.to_str().unwrap() }));
    let p1 = first["pane"].as_u64().unwrap();
    let p2 = act(app, "split", json!({ "pane": p1, "direction": "right" }))["pane"].as_u64().unwrap();
    act(app, "split", json!({ "pane": p2, "direction": "down", "cwd": b.to_str().unwrap() }));
    act(app, "resize-split", json!({ "pane": p1, "direction": "right", "percent": 20 }));
    act(app, "focus", json!({ "pane": p1 }));
    let tab = act(app, "new-tab", json!({ "cwd": b.to_str().unwrap(), "name": "second tab" }));
    let p4 = tab["pane"].as_u64().unwrap();
    act(app, "focus", json!({ "pane": p1 }));
    let w2 = act(app, "new-window", json!({ "cwd": b.to_str().unwrap() }));
    (p1, p4, w2["pane"].as_u64().unwrap())
}

#[test]
fn restore_round_trip_keeps_structure_cwds_and_output() {
    let dir = scratch("round");
    let (a, b) = (dir.join("alpha"), dir.join("beta"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut app = new_app();
    let (p1, p4, p5) = build_layout(&mut app, &a, &b);
    for (pane, mark) in [(p1, "MARK_ONE"), (p4, "MARK_FOUR"), (p5, "MARK_FIVE")] {
        act(&mut app, "send-text", json!({ "pane": pane, "text": format!("echo {mark}\r") }));
        wait_line(&app, pane, mark);
    }
    let file_path = dir.join("session.json");
    let (path, windows, panes) = save_session(&app, Some(file_path.clone()), None).unwrap();
    assert_eq!((path, windows, panes), (file_path.clone(), 2, 5));
    let saved = load_file(&file_path).unwrap();
    assert!(first_text(&saved, 0, 0).contains("MARK_ONE"));
    assert!(!dir.join("session.json.tmp").exists(), "the temporary file was left behind");
    for id in app.panes.keys().copied().collect::<Vec<_>>() {
        app.close_pane(id);
    }
    let mut restored = new_app();
    assert!(restore_from(&mut restored, &file_path));
    assert!(!file_path.exists(), "the session file was not removed after restoring");
    assert_eq!(restored.panes.len(), 5);
    assert_eq!(structure(&saved), structure(&build_file(&restored, None)));
    let first_pane = restored.model.windows[0].tabs[0].root.panes()[0];
    wait_line(&restored, first_pane, "MARK_ONE");
    let text = lines(&restored, first_pane);
    let mark = text.iter().position(|l| l == "MARK_ONE").unwrap();
    let sep = text.iter().position(|l| l.contains("-- restored --")).expect("the separator line");
    assert!(mark < sep, "the old output must come before the separator: {text:?}");
    let second_tab_pane = restored.model.windows[0].tabs[1].root.panes()[0];
    wait_line(&restored, second_tab_pane, "MARK_FOUR");
    let last_window_pane = restored.model.windows[1].tabs[0].root.panes()[0];
    wait_line(&restored, last_window_pane, "MARK_FIVE");
    assert_eq!(restored.model.windows[0].tabs[1].title.as_deref(), Some("second tab"));
}

#[test]
fn a_restored_pane_shows_a_new_prompt_below_the_old_output() {
    let dir = scratch("prompt");
    let mut app = new_app();
    let pane = act(&mut app, "new-window", json!({ "cwd": dir.to_str().unwrap() }))["pane"].as_u64().unwrap();
    act(&mut app, "send-text", json!({ "pane": pane, "text": "echo BEFORE_QUIT\r" }));
    wait_line(&app, pane, "BEFORE_QUIT");
    let file_path = dir.join("session.json");
    save_session(&app, Some(file_path.clone()), None).unwrap();
    let mut restored = new_app();
    assert!(restore_from(&mut restored, &file_path));
    let p = restored.model.windows[0].tabs[0].root.panes()[0];
    act(&mut restored, "send-text", json!({ "pane": p, "text": "echo AFTER_RESTORE\r" }));
    wait_line(&restored, p, "AFTER_RESTORE");
    let text = lines(&restored, p);
    let order = |s: &str| text.iter().position(|l| l == s).unwrap();
    assert!(order("BEFORE_QUIT") < order("AFTER_RESTORE"), "{text:?}");
}

#[test]
fn a_bad_file_starts_normally_and_is_set_aside() {
    let dir = scratch("bad");
    for (name, body) in [
        ("garbage", "this is not json".to_string()),
        ("empty", json!({ "version": 1, "focused": 0, "windows": [] }).to_string()),
        ("future", json!({ "version": 99, "focused": 0, "windows": [] }).to_string()),
    ] {
        let path = dir.join(format!("{name}.json"));
        std::fs::write(&path, body).unwrap();
        let mut app = new_app();
        assert!(!restore_from(&mut app, &path), "{name} must not restore");
        assert!(app.model.windows.is_empty() && app.panes.is_empty());
        assert!(app.take_effects().is_empty());
        assert!(!path.exists(), "{name} was left in place");
        assert!(dir.join(format!("{name}.json.bad")).exists(), "{name} was not set aside");
    }
    assert!(!restore_from(&mut new_app(), &dir.join("missing.json")));
}

#[test]
fn a_file_with_an_unusable_directory_still_restores_in_the_default_one() {
    let dir = scratch("nodir");
    let node = json!({ "type": "pane", "cwd": dir.join("gone").to_str().unwrap(), "text": "old line" });
    let tab = json!({ "focus": 7, "root": node });
    let body = json!({ "version": 1, "focused": 3, "windows": [{ "active": 9, "tabs": [tab] }] });
    let path = dir.join("session.json");
    std::fs::write(&path, body.to_string()).unwrap();
    let mut app = new_app();
    assert!(restore_from(&mut app, &path));
    assert_eq!(app.panes.len(), 1);
    assert_eq!(app.model.windows[0].active, 0);
}

#[test]
fn session_restore_is_off_by_default() {
    assert!(!Config::default().session.restore);
    assert_eq!(Config::default().session.scrollback_lines, 2000);
}
