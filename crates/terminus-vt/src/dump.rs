//! The two grid dump formats the test suite compares, and the diff that reports the row and
//! column of the first difference (`docs/TEST-SUITE.md` section 1).
//!
//! The text dump is one string per row with trailing blanks removed. The full cell dump writes
//! every attribute of a cell in a fixed order: character, width, foreground, background,
//! underline colour, underline style, attribute flags, hyperlink, protection and semantic
//! content, plus the row's wrap flag.

use crate::cell::{Cell, Semantic, Wide, row_flags};
use crate::page::Page;
use crate::screen::Screen;
use crate::style::{Color, Style, Underline, attr};
use crate::terminal::Terminal;

/// The text of every active row of the shown screen, trailing blanks removed.
pub fn text_dump(term: &Terminal) -> Vec<String> {
    screen_text(term.screen())
}

/// The text of every active row of `screen`, trailing blanks removed.
pub fn screen_text(screen: &Screen) -> Vec<String> {
    let pages = &screen.pages;
    let start = pages.active_y(0);
    (0..screen.rows() as usize).map(|r| pages.row_text(start + r).trim_end().to_string()).collect()
}

/// The text of every history row of the primary screen, oldest first, trailing blanks removed.
pub fn history_text(term: &Terminal) -> Vec<String> {
    let pages = &term.primary().pages;
    (0..pages.history_rows()).map(|y| pages.row_text(y).trim_end().to_string()).collect()
}

/// One cell in the full dump format.
pub fn cell_string(page: &Page, row: usize, col: usize) -> String {
    let cell = page.cells(row)[col];
    let style = page.style(cell.style_id());
    let index = page.cell_index(row, col);
    let mut text = String::new();
    if let Some(c) = cell.char() {
        text.push(c);
        text.extend(page.grapheme(index));
    }
    let mut out = format!("{:?} w={}", text, wide_name(cell));
    push_style(&mut out, &style);
    if let Some(link) = cell.has_hyperlink().then(|| page.hyperlink(index)).flatten() {
        out.push_str(&format!(" link={link}"));
    }
    if cell.protected() {
        out.push_str(" protected");
    }
    match cell.semantic() {
        Semantic::Output => {}
        Semantic::Input => out.push_str(" sem=input"),
        Semantic::Prompt => out.push_str(" sem=prompt"),
    }
    out
}

/// Appends the non default parts of a style.
fn push_style(out: &mut String, style: &Style) {
    if style.fg != Color::Default {
        out.push_str(&format!(" fg={}", color_name(style.fg)));
    }
    if style.bg != Color::Default {
        out.push_str(&format!(" bg={}", color_name(style.bg)));
    }
    if style.underline != Underline::None {
        out.push_str(&format!(" ul={:?}", style.underline).to_lowercase());
    }
    if style.underline_color != Color::Default {
        out.push_str(&format!(" ulc={}", color_name(style.underline_color)));
    }
    let names = [
        (attr::BOLD, "bold"),
        (attr::FAINT, "faint"),
        (attr::ITALIC, "italic"),
        (attr::BLINK, "blink"),
        (attr::INVERSE, "inverse"),
        (attr::INVISIBLE, "invisible"),
        (attr::STRIKE, "strike"),
        (attr::OVERLINE, "overline"),
    ];
    for (bit, name) in names {
        if style.has(bit) {
            out.push(' ');
            out.push_str(name);
        }
    }
}

/// The dump name of a width property.
fn wide_name(cell: Cell) -> &'static str {
    match cell.wide() {
        Wide::Narrow => "1",
        Wide::Wide => "2",
        Wide::SpacerTail => "tail",
        Wide::SpacerHead => "head",
    }
}

/// The dump name of a colour.
pub fn color_name(c: Color) -> String {
    match c {
        Color::Default => "default".to_string(),
        Color::Palette(i) => format!("p{i}"),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// One active row of the shown screen in the full cell dump format: the row flags, then every
/// cell up to the last one that is not a blank default cell.
pub fn row_dump(term: &Terminal, row: u16) -> String {
    let pages = &term.screen().pages;
    let y = pages.active_y(row);
    pages.with_row(y, |page, i| {
        let header = page.row(i);
        let mut out = String::new();
        if header.has(row_flags::WRAP) {
            out.push_str("[wrap]");
        }
        let cells = page.cells(i);
        let last = cells.iter().rposition(|c| *c != Cell::BLANK).map(|p| p + 1).unwrap_or(0);
        for col in 0..last {
            if !out.is_empty() {
                out.push_str(" | ");
            }
            out.push_str(&cell_string(page, i, col));
        }
        out
    })
}

/// The full cell dump of every active row.
pub fn grid_dump(term: &Terminal) -> Vec<String> {
    (0..term.rows()).map(|r| row_dump(term, r)).collect()
}

/// Compares two dumps and describes the first difference with its row and column, or None
/// when they are equal.
pub fn diff(expected: &[String], actual: &[String]) -> Option<String> {
    let rows = expected.len().max(actual.len());
    for r in 0..rows {
        let e = expected.get(r).map(String::as_str).unwrap_or("<missing>");
        let a = actual.get(r).map(String::as_str).unwrap_or("<missing>");
        if e != a {
            let col =
                e.chars().zip(a.chars()).position(|(x, y)| x != y).unwrap_or(e.chars().count().min(a.chars().count()));
            return Some(format!("row {r}, column {col}:\n  expected: {e:?}\n  actual:   {a:?}"));
        }
    }
    None
}
