//! What the app does with a session's events: titles, the working directory, clipboard writes and
//! reads, the bell, notifications, colour scheme queries, and the program exiting.

use termoil_vt::terminal::Event;

use termoil_pty::ExitStatus;

use crate::app::{App, Effect};
use crate::model::PaneId;

/// Something the window layer shows for a pane event.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// The bell rang in a pane.
    Bell(PaneId),
    /// A desktop notification.
    Notification { title: String, body: String },
    /// A command finished in a pane, with its exit code and how long it ran.
    CommandFinished { pane: PaneId, code: Option<i32>, seconds: u64 },
    /// A pane's progress (OSC 9;4) changed.
    Progress(PaneId),
}

/// Applies the queued events of a pane. Returns the notices for the window layer. A pane whose
/// program exited is closed, or held open when its program failed (`program_exited`).
pub fn pump(app: &mut App, pane: PaneId, os_dark: bool) -> Vec<Notice> {
    let Some(p) = app.panes.get(&pane) else { return Vec::new() };
    let events = p.session.take_events();
    let exited = p.session.exited();
    let mut notices = Vec::new();
    for e in events {
        apply(app, pane, e, os_dark, &mut notices);
    }
    if exited {
        program_exited(app, pane);
    } else {
        app.redraw_pane(pane);
    }
    notices
}

/// Closes a pane whose program exited, or keeps it open with a line giving the exit code when the
/// pane's command failed and `shell.hold_on_failure` is on. A pane that is already held stays as it is until a key closes it.
fn program_exited(app: &mut App, pane: PaneId) {
    let Some(p) = app.panes.get(&pane) else { return };
    if p.held {
        app.redraw_pane(pane);
        return;
    }
    let status = p.session.exit_status();
    if !should_hold(app.config.shell.hold_on_failure, p.command.is_some(), status) {
        app.close_pane(pane);
        return;
    }
    let line = exit_line(&exited_program_name(p), status);
    if let Some(p) = app.panes.get_mut(&pane) {
        p.held = true;
        p.scroll = 0;
        p.session.lock().advance(line.as_bytes());
    }
    app.redraw_pane(pane);
}

/// Whether a pane whose program ended with `status` is kept open: it was started with a command,
/// `shell.hold_on_failure` is on, and the command failed with a non zero code or a signal. A shell
/// closes either way, because zsh and bash exit with the last command's code and on macOS a shell runs
/// under `login`, which reports 0.
/// @param hold_on_failure - `shell.hold_on_failure`
/// @param started_with_command - the pane was opened with a command instead of the configured shell
/// @param status - how the program ended
pub fn should_hold(hold_on_failure: bool, started_with_command: bool, status: Option<ExitStatus>) -> bool {
    let failed = matches!(status, Some(ExitStatus::Code(c)) if c != 0) || matches!(status, Some(ExitStatus::Signal(_)));
    hold_on_failure && started_with_command && failed
}

/// The name a held pane's line uses: the file name of the command's program, or the pane's title.
fn exited_program_name(p: &crate::app::Pane) -> String {
    match p.command.as_deref().and_then(|c| c.split_whitespace().next()) {
        Some(program) => crate::view::program_name(program.trim_matches('"')),
        None => p.title.clone(),
    }
}

/// The dim line written under a failed program's output.
/// @param name - the program's name
/// @param status - how it ended
pub fn exit_line(name: &str, status: Option<ExitStatus>) -> String {
    let how = match status {
        Some(ExitStatus::Signal(s)) => format!("was ended by signal {s}"),
        Some(ExitStatus::Code(c)) => format!("exited with code {c}"),
        None => "exited".to_string(),
    };
    format!("\x1b[0m\r\n\x1b[2m[{name} {how}. Press any key to close this pane.]\x1b[0m\x1b[?25l")
}

/// Applies one event.
fn apply(app: &mut App, pane: PaneId, e: Event, os_dark: bool, notices: &mut Vec<Notice>) {
    match e {
        Event::Title(t) => {
            if let Some(p) = app.panes.get_mut(&pane) {
                p.title = t;
            }
            if let Some(w) = app.window_of(pane) {
                app.effects.push(Effect::Redraw(w));
            }
        }
        Event::WorkingDirectory(d) => {
            if let Some(p) = app.panes.get_mut(&pane) {
                p.cwd = Some(d);
            }
        }
        Event::ClipboardWrite { data, .. } => {
            if let Ok(text) = String::from_utf8(data) {
                app.effects.push(Effect::SetClipboard(text));
            }
        }
        Event::ClipboardRead { selection } => {
            let text = app.clipboard.get().unwrap_or_default();
            if let Some(p) = app.panes.get(&pane) {
                p.session.lock().reply_clipboard(&selection, text.as_bytes());
                flush_replies(app, pane);
            }
        }
        Event::ColorSchemeQuery => {
            if let Some(p) = app.panes.get(&pane) {
                p.session.lock().reply_color_scheme(os_dark);
                flush_replies(app, pane);
            }
        }
        Event::Bell => notices.push(Notice::Bell(pane)),
        Event::Prompt(termoil_vt::terminal::PromptEvent::CommandFinished(code)) => {
            let seconds =
                app.panes.get(&pane).and_then(|p| p.session.prompt_state().last_duration).map_or(0, |d| d.as_secs());
            notices.push(Notice::CommandFinished { pane, code, seconds });
        }
        Event::Notification { title, body } => notices.push(Notice::Notification { title, body }),
        Event::Progress { state, value } => {
            if let Some(p) = app.panes.get_mut(&pane) {
                p.progress = (state != 0).then_some(crate::app::Progress { state, value });
            }
            notices.push(Notice::Progress(pane));
        }
        _ => {}
    }
}

/// Writes replies the terminal produced outside the parser thread (answers the host supplied).
fn flush_replies(app: &App, pane: PaneId) {
    if let Some(p) = app.panes.get(&pane) {
        let replies = p.session.lock().take_replies();
        if !replies.is_empty() {
            let _ = p.session.write(&replies);
        }
    }
}
