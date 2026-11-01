//! Actions on what a pane shows and receives: input, the screen and scrollback as text, selection
//! and the clipboard, scrolling, prompt jumps and search.

use serde_json::{Map, Value, json};
use terminus_protocol::ActionError;
use terminus_vt::search::{compile, find_next};
use terminus_vt::selection::{Selection, SelectionMode};
use terminus_vt::terminal::MarkKind;

use crate::actions::{arg_bool, arg_str};
use crate::app::{App, Effect, Outcome, PaneSearch};
use crate::inspect;
use crate::keymap::{chord_to_event, parse_chord};
use crate::model::PaneId;

/// Types text into a pane. `\n` and `\r\n` become Enter (`\r`).
pub fn send_text(app: &mut App, pane: PaneId, text: &str) -> Result<Value, ActionError> {
    let bytes = text.replace("\r\n", "\r").replace('\n', "\r");
    let p = app.panes.get_mut(&pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    p.scroll = 0;
    p.session.write(bytes.as_bytes()).map_err(|e| ActionError::Failed(format!("write failed: {e}")))?;
    Ok(json!({ "bytes": bytes.len() }))
}

/// Presses a key in a pane, encoded for the program's keyboard mode.
pub fn send_key(app: &mut App, pane: PaneId, key: &str) -> Result<Value, ActionError> {
    let chord = parse_chord(key).map_err(ActionError::BadArgs)?;
    let ev =
        chord_to_event(&chord).ok_or_else(|| ActionError::BadArgs(format!("'{key}' is not a key that can be sent")))?;
    let p = app.panes.get_mut(&pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let modes = inspect::keyboard_modes(&p.session.lock());
    let mut out = Vec::new();
    terminus_input::encode_key(&ev, &modes, &mut out);
    p.scroll = 0;
    p.session.write(&out).map_err(|e| ActionError::Failed(format!("write failed: {e}")))?;
    Ok(json!({ "bytes": out.len() }))
}

/// The visible screen of a pane as text (one string per row) or JSON cells.
pub fn screen(app: &App, pane: PaneId, as_json: bool) -> Value {
    let p = &app.panes[&pane];
    let t = p.session.lock();
    if as_json {
        inspect::screen_json(&t, p.scroll)
    } else {
        Value::String(inspect::screen_text(&t, p.scroll).join("\n"))
    }
}

/// The last `lines` rows of scrollback and screen as text.
pub fn scrollback(app: &App, pane: PaneId, lines: usize) -> Value {
    let t = app.panes[&pane].session.lock();
    Value::String(inspect::scrollback_text(&t, lines).join("\n"))
}

/// Resets a pane's terminal (RIS), keeping the program running.
pub fn reset_terminal(app: &mut App, pane: PaneId) -> Value {
    if let Some(p) = app.panes.get_mut(&pane) {
        clear_selection(p);
        p.session.lock().advance(b"\x1bc");
        p.scroll = 0;
    }
    app.redraw_pane(pane);
    Value::Null
}

/// Every window, tab and pane with ids, titles, sizes and working directories.
pub fn list(app: &App) -> Value {
    let focused = app.model.focused_pane();
    let windows: Vec<Value> = app
        .model
        .windows
        .iter()
        .map(|w| {
            let tabs: Vec<Value> = w.tabs.iter().enumerate().map(|(ti, t)| json!({ "id": t.id, "active": ti == w.active, "title": tab_title(app, t), "panes": t.root.panes().iter().map(|p| pane_json(app, *p, focused)).collect::<Vec<_>>() })).collect();
            json!({ "id": w.id, "focused": app.model.focused == Some(w.id), "tabs": tabs })
        })
        .collect();
    json!({ "windows": windows })
}

/// A tab's title: the one set by `set-title`, or its focused pane's.
pub fn tab_title(app: &App, t: &crate::model::Tab) -> String {
    t.title.clone().or_else(|| app.panes.get(&t.focus).map(|p| p.title.clone())).unwrap_or_default()
}

/// One pane in the `list` output.
fn pane_json(app: &App, id: PaneId, focused: Option<PaneId>) -> Value {
    let Some(p) = app.panes.get(&id) else { return json!({ "id": id }) };
    let (cols, rows) = {
        let t = p.session.lock();
        (t.cols(), t.rows())
    };
    let prompt = p.session.prompt_state();
    json!({
        "id": id,
        "focused": focused == Some(id),
        "title": p.title,
        "cwd": p.cwd,
        "cols": cols,
        "rows": rows,
        "pid": p.session.pid(),
        "exited": p.session.exited(),
        "running": prompt.running,
        "last_exit_code": prompt.last_exit,
    })
}

/// copy, paste, select-all and clear-selection.
pub fn clipboard_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    let pane = match app.arg_pane(args) {
        Ok(p) => p,
        Err(e) => return Outcome::err(e),
    };
    let r = match name {
        "copy" => copy(app, pane),
        "paste" => paste(app, pane),
        "select-all" => select_all(app, pane),
        _ => {
            if let Some(p) = app.panes.get_mut(&pane) {
                clear_selection(p);
            }
            Ok(Value::Null)
        }
    };
    app.redraw_pane(pane);
    Outcome::Done(r)
}

/// Releases a pane's selection.
pub fn clear_selection(p: &mut crate::app::Pane) {
    if let Some(sel) = p.selection.take() {
        sel.release(&mut p.session.lock().screen_mut().pages);
    }
}

/// The selected text of a pane.
pub fn selected_text(app: &App, pane: PaneId) -> Option<String> {
    let p = app.panes.get(&pane)?;
    let sel = p.selection.as_ref()?;
    let text = sel.text(&p.session.lock().screen().pages, &app.config.clipboard.word_separators);
    (!text.is_empty()).then_some(text)
}

/// Copies the selection to the clipboard.
fn copy(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    match selected_text(app, pane) {
        Some(text) => {
            app.effects.push(Effect::SetClipboard(text.clone()));
            Ok(json!({ "copied": text.chars().count() }))
        }
        None => Ok(json!({ "copied": 0 })),
    }
}

/// Pastes the clipboard, bracketed when the program asked for it.
fn paste(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let Some(text) = app.clipboard.get() else { return Ok(json!({ "pasted": 0 })) };
    paste_text(app, pane, &text)
}

/// Writes pasted text to a pane: newlines become CR, bracketed paste wraps it, and the escape that
/// would end a bracketed paste early is removed.
pub fn paste_text(app: &mut App, pane: PaneId, text: &str) -> Result<Value, ActionError> {
    let p = app.panes.get_mut(&pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let bracketed = p.session.lock().mode(terminus_vt::modes::Mode::BracketedPaste);
    let mut out = Vec::new();
    terminus_input::encode_paste(text, bracketed, &mut out);
    p.scroll = 0;
    p.session.write(&out).map_err(|e| ActionError::Failed(format!("write failed: {e}")))?;
    Ok(json!({ "pasted": text.chars().count() }))
}

/// Selects the whole scrollback and screen.
fn select_all(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let p = app.panes.get_mut(&pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    clear_selection(p);
    let mut t = p.session.lock();
    let pages = &mut t.screen_mut().pages;
    let last = pages.total_rows().saturating_sub(1);
    let cols = pages.cols();
    let sel = Selection::new(pages, SelectionMode::Char, 0, 0);
    sel.update(pages, last, cols.saturating_sub(1));
    drop(t);
    p.selection = Some(sel);
    Ok(Value::Null)
}

/// The scrolling actions, clear-scrollback, prompt jumps and select-last-output.
pub fn scroll_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    let pane = match app.arg_pane(args) {
        Ok(p) => p,
        Err(e) => return Outcome::err(e),
    };
    let r = match name {
        "clear-scrollback" => clear_scrollback(app, pane),
        "previous-prompt" | "next-prompt" => jump_prompt(app, pane, name == "next-prompt"),
        "select-last-output" => select_last_output(app, pane),
        _ => {
            let (rows, history) = view_size(app, pane);
            let p = app.panes.get_mut(&pane).expect("pane checked");
            let page = rows.saturating_sub(1).max(1);
            p.scroll = match name {
                "scroll-page-up" => p.scroll + page,
                "scroll-page-down" => p.scroll.saturating_sub(page),
                "scroll-line-up" => p.scroll + 1,
                "scroll-line-down" => p.scroll.saturating_sub(1),
                "scroll-to-top" => history,
                _ => 0,
            }
            .min(history);
            Ok(json!({ "scroll": p.scroll }))
        }
    };
    app.redraw_pane(pane);
    Outcome::Done(r)
}

/// The pane's rows and the number of history rows above them.
pub fn view_size(app: &App, pane: PaneId) -> (usize, usize) {
    let t = app.panes[&pane].session.lock();
    (t.rows() as usize, t.screen().pages.history_rows())
}

/// Scrolls a pane by `delta` rows (positive is up into history), clamped.
pub fn scroll_by(app: &mut App, pane: PaneId, delta: isize) {
    let (_, history) = view_size(app, pane);
    if let Some(p) = app.panes.get_mut(&pane) {
        p.scroll = (p.scroll as isize + delta).clamp(0, history as isize) as usize;
    }
    app.redraw_pane(pane);
}

/// Erases the history of a pane.
fn clear_scrollback(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let p = app.panes.get_mut(&pane).expect("pane checked");
    clear_selection(p);
    p.session.lock().screen_mut().pages.clear_history();
    p.scroll = 0;
    Ok(Value::Null)
}

/// The rows (absolute) of the prompt marks of a pane, oldest first.
fn prompt_rows(app: &App, pane: PaneId) -> Vec<usize> {
    let t = app.panes[&pane].session.lock();
    let pages = &t.primary().pages;
    let mut rows: Vec<usize> = t
        .marks()
        .iter()
        .filter(|m| m.kind == MarkKind::PromptStart)
        .filter_map(|m| pages.pin_get(m.pin))
        .map(|(y, _)| y)
        .collect();
    rows.dedup();
    rows
}

/// Scrolls so the previous or next prompt is at the top of the view.
fn jump_prompt(app: &mut App, pane: PaneId, forward: bool) -> Result<Value, ActionError> {
    let rows = prompt_rows(app, pane);
    let (_, history) = view_size(app, pane);
    let p = app.panes.get_mut(&pane).expect("pane checked");
    let top = history - p.scroll.min(history);
    let target =
        if forward { rows.iter().copied().find(|&y| y > top) } else { rows.iter().copied().rev().find(|&y| y < top) };
    match target {
        Some(y) => p.scroll = history.saturating_sub(y),
        None if forward => p.scroll = 0,
        None => {}
    }
    Ok(json!({ "scroll": p.scroll }))
}

/// Selects the output of the last finished command (between the last two prompt marks).
fn select_last_output(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let p = app.panes.get_mut(&pane).expect("pane checked");
    clear_selection(p);
    let mut t = p.session.lock();
    let pages = &t.screen().pages;
    let cursor_y = pages.active_y(t.screen().cursor.row);
    let Some(row) = cursor_y.checked_sub(1) else { return Ok(Value::Null) };
    let pages = &mut t.screen_mut().pages;
    let sel = Selection::new(pages, SelectionMode::Semantic, row, 0);
    let text = sel.text(pages, "");
    drop(t);
    p.selection = Some(sel);
    Ok(json!({ "selected": text.chars().count() }))
}

/// search, search-next and search-previous.
pub fn search_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    let pane = match app.arg_pane(args) {
        Ok(p) => p,
        Err(e) => return Outcome::err(e),
    };
    if name == "search" {
        match arg_str(args, "pattern") {
            None => {
                if let Some(w) = app.window_of(pane) {
                    app.effects.push(Effect::ShowSearch(w));
                }
                return Outcome::ok(Value::Null);
            }
            Some(pat) => {
                let s = PaneSearch {
                    pattern: pat.to_string(),
                    case_sensitive: arg_bool(args, "case-sensitive"),
                    current: None,
                };
                app.panes.get_mut(&pane).expect("pane checked").search = Some(s);
            }
        }
    }
    let r = search_step(app, pane, name != "search-next");
    app.redraw_pane(pane);
    Outcome::Done(r)
}

/// Moves to the next match above (`backward`) or below the current one, selecting it and scrolling
/// it into view. A new search starts from the bottom and goes up, the way terminals search.
fn search_step(app: &mut App, pane: PaneId, backward: bool) -> Result<Value, ActionError> {
    let p = app.panes.get_mut(&pane).expect("pane checked");
    let Some(search) = p.search.clone() else {
        return Err(ActionError::Failed("no search is active; run search with --pattern first".into()));
    };
    let re = compile(&search.pattern, search.case_sensitive)
        .map_err(|e| ActionError::BadArgs(format!("bad pattern: {e}")))?;
    clear_selection(p);
    let mut t = p.session.lock();
    let pages = &mut t.screen_mut().pages;
    let from = search.current.map(|m| if backward { m.start } else { m.end }).unwrap_or((pages.total_rows(), 0));
    let Some(m) = find_next(pages, &re, from, !backward, true) else {
        drop(t);
        p.search.as_mut().expect("search set").current = None;
        return Ok(json!({ "found": false }));
    };
    let sel = Selection::new(pages, SelectionMode::Char, m.start.0, m.start.1);
    sel.update(pages, m.end.0, m.end.1);
    let (history, rows) = (pages.history_rows(), pages.rows() as usize);
    drop(t);
    p.selection = Some(sel);
    p.search.as_mut().expect("search set").current = Some(m);
    if m.start.0 < history - p.scroll.min(history) || m.start.0 >= history - p.scroll.min(history) + rows {
        p.scroll = history.saturating_sub(m.start.0.saturating_sub(rows / 2)).min(history);
    }
    Ok(json!({ "found": true, "row": m.start.0, "col": m.start.1 }))
}

/// reload-config: reads the file again; a bad file keeps the configuration in effect.
pub fn reload_config(app: &mut App) -> Result<Value, ActionError> {
    match crate::config::Config::load(&app.config_path) {
        Ok((config, warnings)) => {
            app.config = config;
            app.config_error = None;
            app.effects.push(Effect::ConfigChanged);
            Ok(
                json!({ "warnings": warnings.iter().map(|w| format!("line {}: {}", w.line, w.message)).collect::<Vec<_>>() }),
            )
        }
        Err(e) => {
            let msg = format!("{}: line {}, column {}: {}", app.config_path.display(), e.line, e.column, e.message);
            app.config_error = Some(msg.clone());
            app.effects.push(Effect::ConfigChanged);
            Err(ActionError::Failed(msg))
        }
    }
}

/// open-config: writes the documented default file when there is none, then opens it.
pub fn open_config(app: &mut App) -> Result<Value, ActionError> {
    let path = app.config_path.clone();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ActionError::Failed(e.to_string()))?;
        }
        std::fs::write(&path, crate::config::Config::default_file_text())
            .map_err(|e| ActionError::Failed(e.to_string()))?;
    }
    app.effects.push(Effect::OpenPath(path.clone()));
    Ok(json!({ "path": path }))
}
