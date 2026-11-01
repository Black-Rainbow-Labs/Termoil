//! Reflow: rewrapping every line when the column count changes.
//!
//! Rows joined by the `wrap` flag form one logical line. Each logical line is copied cell by cell
//! into new pages at the new width, wrapping where the new width ends. A wide character that
//! would start in the last column leaves a spacer head and moves to the next row. The cursor and
//! every pin are mapped through the offset of their cell inside its logical line, so a selection
//! or a prompt mark stays on the same character.
//!
//! Blank rows below the cursor are not reflowed; they are added back afterwards, as many as there
//! were, unless that would push text into history that fitted before.

use crate::cell::{Cell, Wide, row_flags};
use crate::page::{Page, standard_rows};
use crate::pagelist::{PageList, Pin, ReflowCursor};

/// A position to map: a row serial and a column.
#[derive(Clone, Copy)]
struct Tracked {
    serial: u64,
    col: u16,
}

/// Collects new rows into standard pages at the new width.
struct Builder {
    cols: u16,
    pages: Vec<Page>,
    total: usize,
}

impl Builder {
    /// A builder for rows `cols` wide.
    fn new(cols: u16) -> Builder {
        Builder { cols, pages: Vec::new(), total: 0 }
    }

    /// Appends a blank row and returns its row index.
    fn push_row(&mut self) -> usize {
        if self.pages.last().is_none_or(|p| p.is_full()) {
            self.pages.push(Page::new(self.cols, standard_rows(self.cols)));
        }
        self.pages.last_mut().expect("ensured above").push_row();
        self.total += 1;
        self.total - 1
    }

    /// The last page and its last row.
    fn last(&mut self) -> (&mut Page, usize) {
        let p = self.pages.last_mut().expect("a row was pushed");
        let r = p.len() - 1;
        (p, r)
    }
}

/// One source cell of a logical line, with where it came from.
struct SourceCell {
    cell: Cell,
    page: usize,
    index: u32,
}

/// Reflows `list` to `cols` columns. Returns the new cursor position.
pub fn reflow(list: &mut PageList, cols: u16, cursor: ReflowCursor) -> ReflowCursor {
    let old_rows = list.rows();
    let cursor_y = list.active_y(cursor.row);
    let end = content_end(list, cursor_y);
    let first_serial = list.first_serial();
    let snapshot = list.snapshot_rows();
    let mut tracked: Vec<Tracked> = vec![Tracked { serial: first_serial + cursor_y as u64, col: cursor.col }];
    let pin_list: Vec<Pin> = list.pins().map(|(_, p)| p).collect();
    tracked.extend(pin_list.iter().map(|p| Tracked { serial: p.serial, col: p.col }));
    let mut mapped: Vec<Option<(usize, u16)>> = vec![None; tracked.len()];
    let mut builder = Builder::new(cols);
    let rows = flatten(&snapshot, end);
    let mut i = 0;
    while i < rows.len() {
        let line_end = line_end(&snapshot, &rows, i);
        write_line(&snapshot, &rows[i..line_end], &tracked, &mut mapped, &mut builder);
        i = line_end;
    }
    let cursor_new_y = mapped[0].map(|m| m.0).unwrap_or(builder.total.saturating_sub(1));
    let after = (old_rows as usize - 1 - cursor.row as usize).min(old_rows as usize);
    let old_total = list.total_rows();
    let content_rows = builder.total.max(cursor_new_y + 1);
    let trailing = after.saturating_sub((content_rows + after).saturating_sub(old_total.max(old_rows as usize)));
    while builder.total < content_rows + trailing {
        builder.push_row();
    }
    finish(list, cols, builder, first_serial, &mapped, cursor_new_y, mapped[0].map(|m| m.1).unwrap_or(0))
}

/// Installs the built pages and maps the pins and cursor into the new list.
fn finish(list: &mut PageList, cols: u16, builder: Builder, first_serial: u64, mapped: &[Option<(usize, u16)>], cursor_y: usize, cursor_col: u16) -> ReflowCursor {
    let total = builder.total;
    list.replace_from_reflow(cols, builder.pages, first_serial, total);
    let mut k = 0;
    list.map_pins(|_| {
        k += 1;
        mapped.get(k).copied().flatten().map(|(y, col)| Pin { serial: first_serial + y as u64, col })
    });
    let active_start = list.total_rows() - list.rows() as usize;
    let row = cursor_y.saturating_sub(active_start).min(list.rows() as usize - 1) as u16;
    ReflowCursor { row, col: cursor_col.min(cols - 1) }
}

/// The row index after the last row worth reflowing: the cursor row or the last non blank row
/// of the active area, whichever is lower on the screen.
fn content_end(list: &PageList, cursor_y: usize) -> usize {
    let mut end = cursor_y + 1;
    for y in (cursor_y + 1..list.total_rows()).rev() {
        let blank = list.with_row(y, |p, r| p.cells(r).iter().all(|c| *c == Cell::BLANK));
        if !blank {
            end = y + 1;
            break;
        }
    }
    end
}

/// Every row up to `end` as (snapshot page index, row in page, serial).
fn flatten(snapshot: &[(u64, Page)], end: usize) -> Vec<(usize, usize, u64)> {
    let mut out = Vec::with_capacity(end);
    'outer: for (pi, (first, page)) in snapshot.iter().enumerate() {
        for r in 0..page.len() {
            if out.len() >= end {
                break 'outer;
            }
            out.push((pi, r, first + r as u64));
        }
    }
    out
}

/// The index after the last row of the logical line starting at `start`.
fn line_end(snapshot: &[(u64, Page)], rows: &[(usize, usize, u64)], start: usize) -> usize {
    let mut i = start;
    while i < rows.len() {
        let (pi, r, _) = rows[i];
        i += 1;
        if !snapshot[pi].1.row(r).has(row_flags::WRAP) {
            break;
        }
    }
    i
}

/// Writes one logical line into the builder and maps the tracked positions that fall in it.
fn write_line(snapshot: &[(u64, Page)], line: &[(usize, usize, u64)], tracked: &[Tracked], mapped: &mut [Option<(usize, u16)>], b: &mut Builder) {
    let (cells, offsets) = collect_line(snapshot, line);
    let first_flags = snapshot[line[0].0].1.row(line[0].1).flags;
    let start_y = b.push_row();
    {
        let (p, r) = b.last();
        p.row_mut(r).flags |= first_flags & (row_flags::SEMANTIC_PROMPT | row_flags::PROMPT_CONTINUATION);
    }
    let mut positions: Vec<(usize, u16)> = Vec::with_capacity(cells.len() + 1);
    let mut x: u16 = 0;
    let mut y = start_y;
    let mut skip_tail = false;
    for sc in &cells {
        if sc.cell.wide() == Wide::SpacerTail && std::mem::take(&mut skip_tail) {
            positions.push((y, x.saturating_sub(1)));
            continue;
        }
        if sc.cell.wide() == Wide::SpacerTail {
            positions.push((y, x));
            let (p, r) = b.last();
            p.copy_cell_from(r, x as usize, &snapshot[sc.page].1, sc.index, sc.cell);
            x += 1;
            continue;
        }
        // A one column grid cannot hold a wide character; it is kept as a narrow cell.
        let width = if sc.cell.wide() == Wide::Wide && b.cols > 1 { 2 } else { 1 };
        if x + width > b.cols {
            if width == 2 && x < b.cols && b.cols > 1 {
                let (p, r) = b.last();
                p.write_cell(r, x as usize, spacer_head());
            }
            (y, x) = wrap_row(b);
        }
        positions.push((y, x));
        let (p, r) = b.last();
        let page = &snapshot[sc.page].1;
        let mut cell = sc.cell;
        if width == 1 && cell.wide() == Wide::Wide {
            cell.set_wide(Wide::Narrow);
            skip_tail = true;
        }
        p.copy_cell_from(r, x as usize, page, sc.index, cell);
        x += 1;
    }
    map_tracked(line, &offsets, &positions, tracked, mapped, b.cols, (y, x));
}

/// Ends the current row with the wrap flag and starts a continuation row.
fn wrap_row(b: &mut Builder) -> (usize, u16) {
    {
        let (p, r) = b.last();
        p.row_mut(r).set(row_flags::WRAP, true);
    }
    let y = b.push_row();
    let (p, r) = b.last();
    p.row_mut(r).set(row_flags::WRAP_CONTINUATION, true);
    (y, 0)
}

/// A spacer head cell.
fn spacer_head() -> Cell {
    let mut c = Cell::BLANK;
    c.set_wide(Wide::SpacerHead);
    c
}

/// The cells of a logical line without spacer heads and without trailing blanks, and the offset
/// in that list where each source row starts.
fn collect_line(snapshot: &[(u64, Page)], line: &[(usize, usize, u64)]) -> (Vec<SourceCell>, Vec<usize>) {
    let mut cells = Vec::new();
    let mut offsets = Vec::with_capacity(line.len());
    for &(pi, r, _) in line {
        offsets.push(cells.len());
        let page = &snapshot[pi].1;
        let row_cells = page.cells(r);
        let mut n = row_cells.len();
        // A spacer head only means something at the end of a wrapped row; it is never copied.
        if row_cells.last().is_some_and(|c| c.wide() == Wide::SpacerHead) {
            n -= 1;
        }
        for (col, cell) in row_cells[..n].iter().enumerate() {
            cells.push(SourceCell { cell: *cell, page: pi, index: page.cell_index(r, col) });
        }
    }
    while cells.last().is_some_and(|c| c.cell == Cell::BLANK) {
        cells.pop();
    }
    (cells, offsets)
}

/// Maps every tracked position on this line's rows to its new row and column.
fn map_tracked(line: &[(usize, usize, u64)], offsets: &[usize], positions: &[(usize, u16)], tracked: &[Tracked], mapped: &mut [Option<(usize, u16)>], cols: u16, end: (usize, u16)) {
    for (k, t) in tracked.iter().enumerate() {
        let Some(row_idx) = line.iter().position(|&(_, _, s)| s == t.serial) else { continue };
        let offset = offsets[row_idx] + t.col as usize;
        mapped[k] = Some(match positions.get(offset) {
            Some(&p) => p,
            None => {
                // A position past the end of the text continues as if blank cells followed it.
                let linear = end.1 as usize + (offset - positions.len());
                (end.0 + linear / cols as usize, (linear % cols as usize) as u16)
            }
        });
    }
}
