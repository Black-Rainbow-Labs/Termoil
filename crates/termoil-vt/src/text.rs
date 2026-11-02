//! Logical lines as text with a map back to grid positions, shared by selection, search and hints.
//!
//! A logical line is a run of rows joined by the wrap flag. Its text has one entry per cell that
//! shows something (spacer cells are skipped, a blank cell reads as a space, a cluster contributes
//! all its code points), and `positions[i]` gives the row index and column of the cell the byte at
//! `offsets[i]` came from.

use crate::cell::{Wide, row_flags};
use crate::pagelist::PageList;

/// One logical line.
#[derive(Clone, Debug, Default)]
pub struct LogicalLine {
    /// The line's text.
    pub text: String,
    /// Byte offset in `text` where each cell's text starts.
    pub offsets: Vec<usize>,
    /// Row index and column of each cell, parallel to `offsets`.
    pub positions: Vec<(usize, u16)>,
    /// First row index of the line.
    pub first_row: usize,
    /// Last row index of the line.
    pub last_row: usize,
}

impl LogicalLine {
    /// The cell at byte offset `byte` (the cell whose text contains it).
    pub fn position_at(&self, byte: usize) -> (usize, u16) {
        let i = self.offsets.partition_point(|&o| o <= byte).saturating_sub(1);
        self.positions.get(i).copied().unwrap_or((self.first_row, 0))
    }

    /// The byte offset where the cell at (row, col) starts, or the nearest cell before it.
    pub fn offset_of(&self, row: usize, col: u16) -> usize {
        let i = self.positions.partition_point(|&p| p <= (row, col));
        if i == 0 { 0 } else { self.offsets[i - 1] }
    }
}

/// The first row of the logical line containing row `y`.
pub fn line_start(pages: &PageList, y: usize) -> usize {
    let mut y = y;
    while y > 0 && pages.row_header(y - 1).has(row_flags::WRAP) {
        y -= 1;
    }
    y
}

/// The last row of the logical line containing row `y`.
pub fn line_end(pages: &PageList, y: usize) -> usize {
    let last = pages.total_rows() - 1;
    let mut y = y;
    while y < last && pages.row_header(y).has(row_flags::WRAP) {
        y += 1;
    }
    y
}

/// Builds the logical line that starts at row `first`.
pub fn logical_line(pages: &PageList, first: usize) -> LogicalLine {
    let last = line_end(pages, first);
    let mut line = LogicalLine { first_row: first, last_row: last, ..LogicalLine::default() };
    pages.for_rows(first..last + 1, |y, page, i| {
        let row = *page.row(i);
        for (col, cell) in page.cells(i).iter().enumerate() {
            if cell.is_spacer() || (cell.wide() == Wide::SpacerHead) {
                continue;
            }
            line.offsets.push(line.text.len());
            line.positions.push((y, col as u16));
            match cell.char() {
                None => line.text.push(' '),
                Some(c) => {
                    line.text.push(c);
                    if cell.is_cluster() {
                        line.text.extend(page.grapheme(row.offset + col as u32));
                    }
                }
            }
        }
    });
    line
}

/// Calls `f` with every logical line that starts in `range` (row indexes), in order. Stops when
/// `f` returns false.
pub fn for_lines(pages: &PageList, range: core::ops::Range<usize>, mut f: impl FnMut(&LogicalLine) -> bool) {
    let mut y = line_start(pages, range.start);
    while y < range.end && y < pages.total_rows() {
        let line = logical_line(pages, y);
        if !f(&line) {
            return;
        }
        y = line.last_row + 1;
    }
}
