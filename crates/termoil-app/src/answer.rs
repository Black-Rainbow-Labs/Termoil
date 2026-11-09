//! What a person is told after running an action from the window (`docs/decisions/window-actions.md`).
//!
//! `App::run_action` answers an `Outcome` that the IPC server and the CLI read themselves. A person who
//! ran the action from the palette or a key has nothing reading it, so `App::run_for_person` turns the
//! outcome into an [`Answer`] here and the window shows it. Every catalogue row is in [`ANSWERS`] or in
//! [`ACTS`], and the A5 test fails naming a row that is in neither.

use std::sync::Arc;

use serde_json::Value;
use termoil_protocol::ActionError;

use crate::model::WinId;

/// The actions whose result is information a person reads. Run from the window, their result is shown.
pub const ANSWERS: &[&str] = &[
    "screen",
    "scrollback",
    "wait-prompt",
    "list",
    "run-action",
    "show-config",
    "list-themes",
    "install-shell-integration",
    "version",
    "check-for-updates",
    "list-updates",
    "install-update",
    "skip-update",
    "telemetry-status",
    "save-session",
];

/// The actions whose result is a change the person can already see. Run from the window, they show
/// nothing unless they fail and were run from the palette.
pub const ACTS: &[&str] = &[
    "new-window",
    "new-tab",
    "split",
    "split-right",
    "split-down",
    "close",
    "focus",
    "focus-left",
    "focus-right",
    "focus-up",
    "focus-down",
    "close-tab",
    "next-tab",
    "move-tab-to-window",
    "previous-tab",
    "zoom-pane",
    "resize-split",
    "show-hints",
    "send-text",
    "send-key",
    "resize",
    "set-title",
    "reload-config",
    "set-theme",
    "choose-theme",
    "open-settings",
    "open-config",
    "activate-key-table",
    "deactivate-key-table",
    "deactivate-all-key-tables",
    "copy",
    "paste",
    "select-all",
    "clear-selection",
    "scroll-page-up",
    "scroll-page-down",
    "scroll-line-up",
    "scroll-line-down",
    "scroll-to-top",
    "scroll-to-bottom",
    "clear-scrollback",
    "previous-prompt",
    "next-prompt",
    "select-last-output",
    "search",
    "search-next",
    "search-previous",
    "increase-font-size",
    "decrease-font-size",
    "reset-font-size",
    "toggle-fullscreen",
    "command-palette",
    "quick-terminal",
    "reset-terminal",
    "quit",
];

/// Where a person ran an action from. A failure of an action in [`ACTS`] is shown only for the
/// palette: a key pressed where it has nothing to do (copy with nothing selected) stays quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// The command palette.
    Palette,
    /// A keybinding, a mouse click, or the window's own controls.
    Binding,
}

/// What the window shows a person about an action they ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The window the action was run in; the focused window when it has closed.
    pub window: Option<WinId>,
    /// The catalogue name of the action.
    pub action: String,
    /// The first line, drawn bold.
    pub title: String,
    /// The lines under the title.
    pub lines: Vec<String>,
    /// The action failed.
    pub failed: bool,
    /// The action is still running; its answer replaces this one when it arrives.
    pub pending: bool,
}

/// Where a job started from the window sends its answer; set by the window runtime.
pub type Sink = Arc<dyn Fn(Answer) + Send + Sync>;

/// Says whether an action's result is shown to a person who ran it from the window.
/// @param action - the catalogue name
pub fn shows_result(action: &str) -> bool {
    ANSWERS.contains(&action)
}

/// The answer shown while a job runs, before its result arrives.
/// @param window - the window the action was run in
/// @param action - the catalogue name
pub fn waiting(window: Option<WinId>, action: &str) -> Answer {
    let title = match action {
        "check-for-updates" => "Checking for updates".to_string(),
        "list-updates" => "Reading the release channels".to_string(),
        "install-update" => "Downloading the update".to_string(),
        "wait-prompt" => "Waiting for the command to finish".to_string(),
        other => format!("Running {other}"),
    };
    Answer { window, action: action.into(), title, lines: Vec::new(), failed: false, pending: true }
}

/// Writes an action's result as the title and lines a person reads.
/// @param window - the window the action was run in
/// @param action - the catalogue name
/// @param result - what `run_action` (or its job) answered
pub fn describe(window: Option<WinId>, action: &str, result: &Result<Value, ActionError>) -> Answer {
    let (title, lines, failed) = match result {
        Err(e) => (format!("{action} failed"), split_lines(&e.to_string()), true),
        Ok(v) => {
            let (title, lines) = describe_value(action, v);
            (title, lines, false)
        }
    };
    Answer { window, action: action.into(), title, lines, failed, pending: false }
}

/// The title and lines for a successful result.
fn describe_value(action: &str, v: &Value) -> (String, Vec<String>) {
    match action {
        "check-for-updates" => check_text(v),
        "list-updates" => list_updates_text(v),
        "install-update" => install_text(v),
        "skip-update" => {
            let version = v["skip"].as_str().unwrap_or("?");
            (format!("Termoil {version} will not be announced"), vec![format!("updates.skip is now {version}.")])
        }
        "version" => (
            format!("Termoil {}", v["version"].as_str().unwrap_or("?")),
            vec![format!("Built {}", v["build_date"].as_str().unwrap_or("?"))],
        ),
        "wait-prompt" => match v["exit_code"].as_i64() {
            Some(code) => ("The command finished".into(), vec![format!("Exit code {code}.")]),
            None => ("The command finished".into(), Vec::new()),
        },
        _ => (action.to_string(), generic_lines(v)),
    }
}

/// check-for-updates: the sentences the automatic check uses, and the release notes.
fn check_text(v: &Value) -> (String, Vec<String>) {
    let current = v["current"].as_str().unwrap_or("?");
    let latest = v["latest"].as_str().unwrap_or("?");
    let channel = v["channel"].as_str().unwrap_or("?");
    let newer = v["newer"].as_bool() == Some(true);
    let (title, body) = crate::update::actions::sentence(current, latest, channel, newer);
    let mut lines = vec![body];
    if let Some(notes) = v["notes"].as_str().filter(|n| !n.is_empty()) {
        lines.push(format!("Release notes: {notes}"));
    }
    (title, lines)
}

/// list-updates: each channel that was asked, its versions, and which one is running.
fn list_updates_text(v: &Value) -> (String, Vec<String>) {
    let current = v["current"].as_str().unwrap_or("?");
    let mut lines = Vec::new();
    for channel in ["stable", "nightly"] {
        let Some(entry) = v.get(channel) else { continue };
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("{channel}:"));
        match entry.as_array() {
            Some(list) if list.is_empty() => lines.push("  no versions".into()),
            Some(list) => lines.extend(list.iter().map(version_line)),
            None => lines.push(format!("  could not be read: {}", entry["error"].as_str().unwrap_or("unknown error"))),
        }
    }
    (format!("Termoil versions (you have {current})"), lines)
}

/// One version of list-updates: the version, its date, and `running` beside the one running.
fn version_line(r: &Value) -> String {
    let running = if r["running"].as_bool() == Some(true) { "  running" } else { "" };
    format!("  {:<12}{}{running}", r["version"].as_str().unwrap_or("?"), r["date"].as_str().unwrap_or(""))
}

/// install-update: what happens next, or the file it downloaded.
fn install_text(v: &Value) -> (String, Vec<String>) {
    let version = v["version"].as_str().unwrap_or("?");
    if v["restarting"].as_bool() == Some(true) || v["installing"].as_bool() == Some(true) {
        return (
            format!("Termoil {version} is installing"),
            vec!["Termoil closes, installs the new version and opens the same windows again.".into()],
        );
    }
    let file = v["file"].as_str().unwrap_or("?");
    (format!("Termoil {version} was downloaded"), vec![format!("Its size and SHA-256 match the release: {file}")])
}

/// Any other answer: a string as its lines, null as nothing, anything else pretty printed.
fn generic_lines(v: &Value) -> Vec<String> {
    match v {
        Value::Null => vec!["Done.".into()],
        Value::String(s) => split_lines(s),
        other => split_lines(&serde_json::to_string_pretty(other).unwrap_or_default()),
    }
}

/// Splits text into lines, dropping a trailing empty line.
fn split_lines(s: &str) -> Vec<String> {
    let mut lines: Vec<String> = s.lines().map(|l| l.replace('\t', "    ")).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn check_for_updates_says_newer_or_up_to_date() {
        let newer =
            json!({ "current": "1.5.6", "latest": "1.6.0", "channel": "stable", "newer": true, "notes": "https://n" });
        let a = describe(Some(1), "check-for-updates", &Ok(newer));
        assert_eq!(a.title, "Termoil 1.6.0 is available");
        assert!(a.lines[0].starts_with("You have 1.5.6."));
        assert_eq!(a.lines[1], "Release notes: https://n");
        let same = json!({ "current": "1.5.6", "latest": "1.5.6", "channel": "nightly", "newer": false });
        let a = describe(None, "check-for-updates", &Ok(same));
        assert_eq!(
            (a.title.as_str(), a.lines[0].as_str()),
            ("Termoil is up to date", "1.5.6 is the newest version on the nightly channel.")
        );
    }

    #[test]
    fn list_updates_lists_channels_and_marks_the_running_one() {
        let v = json!({ "current": "1.5.6",
            "stable": [{ "version": "1.5.6", "date": "2026-11-07", "running": true }],
            "nightly": { "error": "curl: (7) refused" } });
        let a = describe(None, "list-updates", &Ok(v));
        assert_eq!(a.title, "Termoil versions (you have 1.5.6)");
        assert_eq!(
            a.lines,
            vec![
                "stable:",
                "  1.5.6       2026-11-07  running",
                "",
                "nightly:",
                "  could not be read: curl: (7) refused"
            ]
        );
    }

    #[test]
    fn install_skip_version_errors_and_others() {
        let a = describe(
            None,
            "install-update",
            &Ok(json!({ "version": "1.6.0", "file": "C:/t/x.exe", "restart": false })),
        );
        assert_eq!(a.title, "Termoil 1.6.0 was downloaded");
        assert!(a.lines[0].ends_with("C:/t/x.exe"));
        let a = describe(None, "install-update", &Ok(json!({ "version": "1.6.0", "restarting": true })));
        assert_eq!(a.title, "Termoil 1.6.0 is installing");
        assert_eq!(
            describe(None, "skip-update", &Ok(json!({ "skip": "1.6.0" }))).title,
            "Termoil 1.6.0 will not be announced"
        );
        assert_eq!(
            describe(None, "version", &Ok(json!({ "version": "1.5.6", "build_date": "d" }))).lines,
            vec!["Built d"]
        );
        let e = describe(None, "skip-update", &Err(ActionError::BadArgs("no check has found a version yet".into())));
        assert!(e.failed && e.title == "skip-update failed" && e.lines[0].contains("no check has found"));
        assert_eq!(describe(None, "show-config", &Ok(json!("a = 1\n\nb = 2\n"))).lines, vec!["a = 1", "", "b = 2"]);
        assert_eq!(describe(None, "list", &Ok(json!({ "k": 1 }))).lines, vec!["{", "  \"k\": 1", "}"]);
        assert!(waiting(None, "check-for-updates").pending);
    }
}
