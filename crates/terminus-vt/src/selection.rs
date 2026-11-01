//! Selection: a pair of pins and a mode, expanded to a range and copied as text or ANSI.
//!
//! Character selection covers the cells between the two ends; word selection grows each end to
//! the word boundaries (the separators are configurable); line selection takes whole logical
//! lines; block selection takes a rectangle; semantic selection takes the output of one command,
//! bounded by the OSC 133 marks around it.

use crate::cell::{Semantic, row_flags};
use crate::page::Page;
use crate::pagelist::{PageList, PinId};
use crate::style::{Color, Style, Underline, attr};
use crate::text::{line_end, line_start};

/// Characters that end a word for word selection, the default set.
pub const DEFAULT_WORD_SEPARATORS: &str = " \t'\"()[]{}<>|,;`";

/// How a selection grows from its ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionMode {
    /// Cell by cell.
    Char,
    /// Whole words.
    Word,
    /// Whole logical lines.
    Line,
    /// A rectangle.
    Block,
    /// The output of one command.
    Semantic,
}

/// A selection on one page list. The owner must call [`Selection::release`] to drop its pins.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    anchor: PinId,
    head: PinId,
    mode: SelectionMode,
}

/// A selected range after expansion: inclusive ends as (row index, column), plus whether it is a
/// rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectionRange {
    /// First cell.
    pub start: (usize, u16),
    /// Last cell, inclusive.
    pub end: (usize, u16),
    /// A rectangle between the columns of `start` and `end`.
    pub block: bool,
}

impl SelectionRange {
    /// True when the cell at (row, col) is selected.
    pub fn contains(&self, y: usize, col: u16) -> bool {
        if self.block {
            let (l, r) = (self.start.1.min(self.end.1), self.start.1.max(self.end.1));
            return y >= self.start.0 && y <= self.end.0 && col >= l && col <= r;
        }
        (y, col) >= self.start && (y, col) <= self.end
    }
}

impl Selection {
    /// Starts a selection at row `y`, column `col`.
    pub fn new(pages: &mut PageList, mode: SelectionMode, y: usize, col: u16) -> Selection {
        Selection { anchor: pages.pin_create(y, col), head: pages.pin_create(y, col), mode }
    }

    /// Moves the moving end.
    pub fn update(&self, pages: &mut PageList, y: usize, col: u16) {
        pages.pin_set(self.head, y, col);
    }

    /// The mode.
    pub fn mode(&self) -> SelectionMode {
        self.mode
    }

    /// Drops the pins.
    pub fn release(self, pages: &mut PageList) {
        pages.pin_remove(self.anchor);
        pages.pin_remove(self.head);
    }

    /// The expanded range, or None when an end was pruned away.
    pub fn range(&self, pages: &PageList, separators: &str) -> Option<SelectionRange> {
        let a = pages.pin_get(self.anchor)?;
        let h = pages.pin_get(self.head)?;
        let (start, end) = if (a.0, a.1) <= (h.0, h.1) { (a, h) } else { (h, a) };
        let cols = pages.cols();
        Some(match self.mode {
            SelectionMode::Char => SelectionRange { start, end, block: false },
            SelectionMode::Block => SelectionRange { start: (start.0, a.1.min(h.1)), end: (end.0, a.1.max(h.1)), block: true },
            SelectionMode::Line => SelectionRange { start: (line_start(pages, start.0), 0), end: (line_end(pages, end.0), cols - 1), block: false },
            SelectionMode::Word => SelectionRange { start: word_start(pages, start, separators), end: word_end(pages, end, separators), block: false },
            SelectionMode::Semantic => semantic_range(pages, start.0)?,
        })
    }

    /// The selected text: trailing blanks of each row removed, wrapped rows joined, rows of a
    /// block selection separated by newlines.
    pub fn text(&self, pages: &PageList, separators: &str) -> String {
        match self.range(pages, separators) {
            Some(r) => range_text(pages, r),
            None => String::new(),
        }
    }

    /// The selected text with SGR sequences reproducing its styles.
    pub fn ansi(&self, pages: &PageList, separators: &str) -> String {
        match self.range(pages, separators) {
            Some(r) => range_ansi(pages, r),
            None => String::new(),
        }
    }
}

/// The text of a range.
pub fn range_text(pages: &PageList, r: SelectionRange) -> String {
    let mut out = String::new();
    let mut line = String::new();
    pages.for_rows(r.start.0..r.end.0 + 1, |y, page, i| {
        let (from, to) = row_span(r, y, page.cols());
        for col in from..=to {
            push_cell_text(page, i, col as usize, &mut line);
        }
        let wrapped = page.row(i).has(row_flags::WRAP) && !r.block && to == page.cols() - 1;
        if wrapped && y != r.end.0 {
            return;
        }
        // A logical line ends here: its trailing blanks go, then a newline unless it is the last.
        out.push_str(line.trim_end_matches(' '));
        line.clear();
        if y != r.end.0 {
            out.push('\n');
        }
    });
    out
}

/// The columns of row `y` a range covers.
fn row_span(r: SelectionRange, y: usize, cols: u16) -> (u16, u16) {
    if r.block {
        return (r.start.1.min(r.end.1), r.start.1.max(r.end.1).min(cols - 1));
    }
    let from = if y == r.start.0 { r.start.1 } else { 0 };
    let to = if y == r.end.0 { r.end.1.min(cols - 1) } else { cols - 1 };
    (from, to)
}

/// Appends one cell's text (nothing for spacers, a space for blanks).
fn push_cell_text(page: &Page, i: usize, col: usize, out: &mut String) {
    let cell = page.cells(i)[col];
    if cell.is_spacer() {
        return;
    }
    match cell.char() {
        None => out.push(' '),
        Some(c) => {
            out.push(c);
            if cell.is_cluster() {
                out.extend(page.grapheme(page.cell_index(i, col)));
            }
        }
    }
}

/// The text of a range with SGR sequences before each change of style and a reset at the end.
pub fn range_ansi(pages: &PageList, r: SelectionRange) -> String {
    let mut out = String::new();
    let mut current = Style::default();
    pages.for_rows(r.start.0..r.end.0 + 1, |y, page, i| {
        let (from, to) = row_span(r, y, page.cols());
        let last = (from..=to).rev().find(|&c| !page.cells(i)[c as usize].is_blank() || page.cells(i)[c as usize].style_id() != 0).unwrap_or(from);
        for col in from..=last {
            let style = page.style(page.cells(i)[col as usize].style_id());
            if style != current {
                out.push_str(&sgr_for(&style));
                current = style;
            }
            push_cell_text(page, i, col as usize, &mut out);
        }
        let wrapped = page.row(i).has(row_flags::WRAP) && !r.block;
        if y != r.end.0 && !wrapped {
            out.push('\n');
        }
    });
    if current != Style::default() {
        out.push_str("\x1b[0m");
    }
    out
}

/// An SGR sequence that sets exactly `style`.
fn sgr_for(style: &Style) -> String {
    let mut p = vec!["0".to_string()];
    let flags = [(attr::BOLD, "1"), (attr::FAINT, "2"), (attr::ITALIC, "3"), (attr::BLINK, "5"), (attr::INVERSE, "7"), (attr::INVISIBLE, "8"), (attr::STRIKE, "9")];
    p.extend(flags.iter().filter(|(b, _)| style.has(*b)).map(|(_, c)| c.to_string()));
    if style.underline != Underline::None {
        p.push(format!("4:{}", style.underline as u8));
    }
    for (c, base) in [(style.fg, 38), (style.bg, 48)] {
        match c {
            Color::Default => {}
            Color::Palette(i) => p.push(format!("{base};5;{i}")),
            Color::Rgb(r, g, b) => p.push(format!("{base};2;{r};{g};{b}")),
        }
    }
    format!("\x1b[{}m", p.join(";"))
}

/// True when the character at (y, col) separates words.
fn is_separator(pages: &PageList, y: usize, col: u16, separators: &str) -> bool {
    pages.with_row(y, |page, i| {
        let cell = page.cells(i)[col as usize];
        match cell.char() {
            None => true,
            Some(c) => separators.contains(c),
        }
    })
}

/// Moves left from `p` to the first cell of its word, across wrapped rows.
fn word_start(pages: &PageList, p: (usize, u16), separators: &str) -> (usize, u16) {
    if is_separator(pages, p.0, p.1, separators) {
        return p;
    }
    let (mut y, mut col) = p;
    loop {
        let (py, pc) = if col > 0 {
            (y, col - 1)
        } else if y > 0 && pages.row_header(y - 1).has(row_flags::WRAP) {
            (y - 1, pages.cols() - 1)
        } else {
            return (y, col);
        };
        if is_separator(pages, py, pc, separators) && !pages.with_row(py, |p, i| p.cells(i)[pc as usize].is_spacer()) {
            return (y, col);
        }
        (y, col) = (py, pc);
    }
}

/// Moves right from `p` to the last cell of its word, across wrapped rows.
fn word_end(pages: &PageList, p: (usize, u16), separators: &str) -> (usize, u16) {
    if is_separator(pages, p.0, p.1, separators) && !pages.with_row(p.0, |pg, i| pg.cells(i)[p.1 as usize].is_spacer()) {
        return p;
    }
    let (mut y, mut col) = p;
    let last_col = pages.cols() - 1;
    loop {
        let (ny, nc) = if col < last_col {
            (y, col + 1)
        } else if pages.row_header(y).has(row_flags::WRAP) && y + 1 < pages.total_rows() {
            (y + 1, 0)
        } else {
            return (y, col);
        };
        let spacer = pages.with_row(ny, |pg, i| pg.cells(i)[nc as usize].is_spacer());
        if !spacer && is_separator(pages, ny, nc, separators) {
            return (y, col);
        }
        (y, col) = (ny, nc);
    }
}

/// The rows of command output around row `y`: from the first output row after the prompt above
/// to the row before the next prompt.
fn semantic_range(pages: &PageList, y: usize) -> Option<SelectionRange> {
    let total = pages.total_rows();
    let mut top = y;
    while top > 0 && !pages.row_header(top).has(row_flags::SEMANTIC_PROMPT) {
        top -= 1;
    }
    let mut start = top;
    while start < total && row_has_semantic(pages, start, &[Semantic::Prompt, Semantic::Input]) {
        start += 1;
    }
    let mut end = start;
    while end + 1 < total && !pages.row_header(end + 1).has(row_flags::SEMANTIC_PROMPT) {
        end += 1;
    }
    while end > start && pages.row_text(end).trim().is_empty() {
        end -= 1;
    }
    (start < total && start <= end).then(|| SelectionRange { start: (start, 0), end: (end, pages.cols() - 1), block: false })
}

/// True when any cell of row `y` has one of the semantic kinds.
fn row_has_semantic(pages: &PageList, y: usize, kinds: &[Semantic]) -> bool {
    pages.with_row(y, |p, i| p.cells(i).iter().any(|c| kinds.contains(&c.semantic())))
}
