//! Reading a terminal for the host: the keyboard and mouse modes the encoders need, and the text
//! and JSON views `screen` and `scrollback` return.

use serde_json::{Value, json};
use terminus_input::{KeyboardModes, MouseEncoding, MouseModes, MouseTracking};
use terminus_vt::Terminal;
use terminus_vt::cell::Wide;
use terminus_vt::dump::color_name;
use terminus_vt::modes::Mode;
use terminus_vt::style::{Style, Underline, attr};

/// The modes that decide how keys are encoded.
pub fn keyboard_modes(term: &Terminal) -> KeyboardModes {
    KeyboardModes {
        app_cursor: term.mode(Mode::CursorKeys),
        app_keypad: term.mode(Mode::KeypadApplication),
        modify_other_keys: term.modify_other_keys(),
        kitty_flags: term.screen().kitty_keyboard.flags(),
        backarrow_is_bs: term.mode(Mode::BackarrowSendsBs),
        alt_sends_escape: true,
    }
}

/// The mouse tracking mode and encoding the program asked for.
pub fn mouse_modes(term: &Terminal) -> MouseModes {
    let tracking = if term.mode(Mode::MouseAnyEvent) {
        MouseTracking::AnyEvent
    } else if term.mode(Mode::MouseButtonEvent) {
        MouseTracking::ButtonEvent
    } else if term.mode(Mode::MouseNormal) {
        MouseTracking::Normal
    } else if term.mode(Mode::MouseX10) {
        MouseTracking::X10
    } else {
        MouseTracking::Off
    };
    let encoding = if term.mode(Mode::MouseSgrPixels) {
        MouseEncoding::SgrPixels
    } else if term.mode(Mode::MouseSgr) {
        MouseEncoding::Sgr
    } else if term.mode(Mode::MouseUrxvt) {
        MouseEncoding::Urxvt
    } else if term.mode(Mode::MouseUtf8) {
        MouseEncoding::Utf8
    } else {
        MouseEncoding::Default
    };
    MouseModes { tracking, encoding }
}

/// The visible rows as text, trailing blanks removed. `scroll` is how far the view is above the
/// bottom.
pub fn screen_text(term: &Terminal, scroll: usize) -> Vec<String> {
    let pages = &term.screen().pages;
    let top = pages.active_y(0).saturating_sub(scroll);
    (0..term.rows() as usize).map(|r| pages.row_text(top + r).trim_end().to_string()).collect()
}

/// The last `lines` rows of history and screen as text, oldest first, with trailing empty rows
/// below the cursor dropped.
pub fn scrollback_text(term: &Terminal, lines: usize) -> Vec<String> {
    let pages = &term.screen().pages;
    let total = pages.total_rows();
    let cursor_y = pages.active_y(term.screen().cursor.row);
    let mut out: Vec<String> =
        (0..total).skip(total.saturating_sub(lines)).map(|y| pages.row_text(y).trim_end().to_string()).collect();
    let keep = out.len().saturating_sub(total - 1 - cursor_y.min(total - 1));
    while out.len() > keep && out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// The visible screen as JSON: size, cursor, title and, per row, runs of text sharing one style.
pub fn screen_json(term: &Terminal, scroll: usize) -> Value {
    let pages = &term.screen().pages;
    let top = pages.active_y(0).saturating_sub(scroll);
    let rows: Vec<Value> = (0..term.rows() as usize).map(|r| pages.with_row(top + r, row_runs)).collect();
    let c = &term.screen().cursor;
    json!({
        "cols": term.cols(),
        "rows": term.rows(),
        "cursor": { "row": c.row, "col": c.col, "visible": term.mode(Mode::CursorVisible) },
        "title": term.title(),
        "alt_screen": term.is_alt_screen(),
        "lines": rows,
    })
}

/// One row as runs of equal style: `{"text": ..., "style": {...}}`, trailing blank default runs
/// dropped.
fn row_runs(page: &terminus_vt::page::Page, i: usize) -> Value {
    let cells = page.cells(i);
    let mut runs: Vec<(Style, String)> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        if matches!(cell.wide(), Wide::SpacerTail | Wide::SpacerHead) {
            continue;
        }
        let style = page.style(cell.style_id());
        let mut text = String::new();
        match cell.char() {
            Some(ch) => {
                text.push(ch);
                text.extend(page.grapheme(page.cell_index(i, col)));
            }
            None => text.push(' '),
        }
        match runs.last_mut() {
            Some((s, t)) if *s == style => t.push_str(&text),
            _ => runs.push((style, text)),
        }
    }
    if let Some((s, t)) = runs.last_mut()
        && s.bg == terminus_vt::style::Color::Default
    {
        let trimmed = t.trim_end().len();
        t.truncate(trimmed);
    }
    while runs.last().is_some_and(|(_, t)| t.is_empty()) {
        runs.pop();
    }
    Value::Array(runs.into_iter().map(|(s, t)| json!({ "text": t, "style": style_json(&s) })).collect())
}

/// The non default parts of a style as a JSON object.
fn style_json(s: &Style) -> Value {
    let mut m = serde_json::Map::new();
    if s.fg != terminus_vt::style::Color::Default {
        m.insert("fg".into(), json!(color_name(s.fg)));
    }
    if s.bg != terminus_vt::style::Color::Default {
        m.insert("bg".into(), json!(color_name(s.bg)));
    }
    if s.underline != Underline::None {
        m.insert("underline".into(), json!(format!("{:?}", s.underline).to_lowercase()));
    }
    for (bit, name) in [
        (attr::BOLD, "bold"),
        (attr::FAINT, "faint"),
        (attr::ITALIC, "italic"),
        (attr::BLINK, "blink"),
        (attr::INVERSE, "inverse"),
        (attr::INVISIBLE, "invisible"),
        (attr::STRIKE, "strike"),
        (attr::OVERLINE, "overline"),
    ] {
        if s.has(bit) {
            m.insert(name.into(), json!(true));
        }
    }
    Value::Object(m)
}
