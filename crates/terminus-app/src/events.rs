//! What the app does with a session's events: titles, the working directory, clipboard writes and
//! reads, the bell, notifications, colour scheme queries, and the program exiting.

use terminus_vt::terminal::Event;

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
/// program exited is closed.
pub fn pump(app: &mut App, pane: PaneId, os_dark: bool) -> Vec<Notice> {
    let Some(p) = app.panes.get(&pane) else { return Vec::new() };
    let events = p.session.take_events();
    let exited = p.session.exited();
    let mut notices = Vec::new();
    for e in events {
        apply(app, pane, e, os_dark, &mut notices);
    }
    if exited {
        app.close_pane(pane);
    } else {
        app.redraw_pane(pane);
    }
    notices
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
        Event::Prompt(terminus_vt::terminal::PromptEvent::CommandFinished(code)) => {
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
