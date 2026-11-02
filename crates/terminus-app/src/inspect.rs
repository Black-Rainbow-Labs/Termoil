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

/// The first row and one past the last row of the last `lines` rows of history and screen in `screen`.
/// The rows end at the cursor's row or the last row with text, whichever is lower, so the empty
/// rows below the prompt are not counted.
fn scrollback_span(screen: &terminus_vt::screen::Screen, lines: usize) -> (usize, usize) {
    let pages = &screen.pages;
    let total = pages.total_rows();
    let cursor_y = pages.active_y(screen.cursor.row).min(total.saturating_sub(1));
    let last_text = (cursor_y..total).rev().find(|&y| !pages.row_text(y).trim_end().is_empty()).unwrap_or(cursor_y);
    let end = last_text.max(cursor_y) + 1;
    (end.saturating_sub(lines), end)
}

/// The last `lines` rows of history and screen as text, oldest first. The rows end at the cursor's
/// row or the last row with text, whichever is lower, so the empty rows below the prompt are not
/// counted.
pub fn scrollback_text(term: &Terminal, lines: usize) -> Vec<String> {
    let screen = term.screen();
    let (start, end) = scrollback_span(screen, lines);
    (start..end).map(|y| screen.pages.row_text(y).trim_end().to_string()).collect()
}

/// The last `lines` rows of the primary screen's history and screen as text with SGR sequences that
/// reproduce each cell's colours and attributes, rows joined by CR LF. Used to save a pane for
/// session restore. The alternate screen is not included.
pub fn scrollback_ansi(term: &Terminal, lines: usize) -> String {
    let screen = term.primary();
    let (start, end) = scrollback_span(screen, lines);
    let mut out = String::new();
    screen.pages.for_rows(start..end, |y, page, r| {
        if y > start {
            out.push_str("\r\n");
        }
        row_ansi(page, r, &mut out);
    });
    out
}

/// Appends one row as text with SGR sequences, dropping trailing blanks that have the default
/// background, and ends with a reset when any style was written.
fn row_ansi(page: &terminus_vt::page::Page, i: usize, out: &mut String) {
    let cells = page.cells(i);
    let mut items: Vec<(Style, String)> = Vec::with_capacity(cells.len());
    for (col, cell) in cells.iter().enumerate() {
        if matches!(cell.wide(), Wide::SpacerTail | Wide::SpacerHead) {
            continue;
        }
        let mut text = String::new();
        match cell.char() {
            Some(ch) => {
                text.push(ch);
                text.extend(page.grapheme(page.cell_index(i, col)));
            }
            None => text.push(' '),
        }
        items.push((page.style(cell.style_id()), text));
    }
    while items.last().is_some_and(|(s, t)| s.bg == terminus_vt::style::Color::Default && t.trim().is_empty()) {
        items.pop();
    }
    let mut current = Style::default();
    for (style, text) in &items {
        if *style != current {
            out.push_str(&sgr_for(style));
            current = *style;
        }
        out.push_str(text);
    }
    if !current.is_default() {
        out.push_str("\x1b[0m");
    }
}

/// The SGR sequence that sets exactly `s`, starting from the default style.
fn sgr_for(s: &Style) -> String {
    use terminus_vt::style::Color;
    let mut p: Vec<String> = vec!["0".into()];
    for (bit, code) in [
        (attr::BOLD, "1"),
        (attr::FAINT, "2"),
        (attr::ITALIC, "3"),
        (attr::BLINK, "5"),
        (attr::INVERSE, "7"),
        (attr::INVISIBLE, "8"),
        (attr::STRIKE, "9"),
        (attr::OVERLINE, "53"),
    ] {
        if s.has(bit) {
            p.push(code.into());
        }
    }
    match s.underline {
        Underline::None => {}
        Underline::Single => p.push("4".into()),
        Underline::Double => p.push("4:2".into()),
        Underline::Curly => p.push("4:3".into()),
        Underline::Dotted => p.push("4:4".into()),
        Underline::Dashed => p.push("4:5".into()),
    }
    for (color, base) in [(s.fg, 38), (s.bg, 48), (s.underline_color, 58)] {
        match color {
            Color::Default => {}
            Color::Palette(n) => p.push(format!("{base};5;{n}")),
            Color::Rgb(r, g, b) => p.push(format!("{base};2;{r};{g};{b}")),
        }
    }
    format!("\x1b[{}m", p.join(";"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use terminus_vt::TerminalConfig;

    /// The last lines end at the cursor, not at the blank bottom of the screen.
    #[test]
    fn scrollback_ends_at_the_cursor() {
        let mut t = Terminal::new(20, 10, TerminalConfig::default());
        t.advance(b"one\r\ntwo\r\nthree\r\n> ");
        assert_eq!(scrollback_text(&t, 2), vec!["three", ">"]);
        assert_eq!(scrollback_text(&t, 100), vec!["one", "two", "three", ">"]);
        t.advance(b"\x1b[8;1Hlow\x1b[2;1H");
        assert_eq!(scrollback_text(&t, 1), vec!["low"]);
    }

    /// Saved text with colours feeds back into a terminal as the same colours and text.
    #[test]
    fn ansi_text_round_trips_styles() {
        let mut t = Terminal::new(30, 5, TerminalConfig::default());
        t.advance(b"\x1b[1;31mred bold\x1b[0m plain\r\n\x1b[38;2;1;2;3;48;5;200mtrue\x1b[0m\r\n\x1b[4:3mcurly\x1b[0m");
        let saved = scrollback_ansi(&t, 100);
        let mut u = Terminal::new(30, 5, TerminalConfig::default());
        u.advance(saved.as_bytes());
        assert_eq!(scrollback_ansi(&u, 100), saved);
        assert_eq!(scrollback_text(&u, 100), vec!["red bold plain", "true", "curly"]);
        assert!(saved.contains("38;2;1;2;3"), "{saved:?}");
        assert!(saved.contains("1;38;5;1"), "{saved:?}");
    }
}
