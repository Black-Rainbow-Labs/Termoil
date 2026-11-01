//! Editing the grid: erase, insert and delete characters and lines, scroll, and cursor
//! positioning with origin mode.

use super::Terminal;
use super::print::fix_wide_boundaries;
use crate::cell::{Cell, row_flags};
use crate::modes::Mode;

/// Which part of a line or the screen an erase covers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum EraseRange {
    /// From the cursor to the end.
    ToEnd,
    /// From the start to the cursor.
    ToStart,
    /// Everything.
    All,
}

impl Terminal {
    /// The blank cell an erase writes: the pen's background colour (background colour erase).
    fn erase_cell(&mut self, row: u16) -> Cell {
        let style = self.screen().cursor.style.erase_style();
        let s = self.screen_mut();
        let (page, _) = s.pages.active_mut(row);
        Cell::blank(page.intern_style(&style))
    }

    /// Clears columns `from..to` of active row `row`. Selective erase keeps protected cells.
    pub(super) fn erase_cells(&mut self, row: u16, from: u16, to: u16, selective: bool) {
        let blank = self.erase_cell(row);
        let s = self.screen_mut();
        let (page, i) = s.pages.active_mut(row);
        let to = (to as usize).min(page.cols() as usize);
        let from = from as usize;
        if from >= to {
            page.drop_unused_style(blank.style_id());
            return;
        }
        fix_wide_boundaries(page, i, from, to);
        page.clear_cells(i, from, to, blank, selective);
        page.drop_unused_style(blank.style_id());
        if to == page.cols() as usize {
            page.row_mut(i).set(row_flags::WRAP, false);
        }
    }

    /// Gives rows `from..to` that a scroll just exposed the pen's background colour (background
    /// colour erase), inside the left and right margins when they are on.
    pub(super) fn fill_exposed(&mut self, from: u16, to: u16) {
        if self.screen().cursor.style.bg == crate::style::Color::Default {
            return;
        }
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let (left, right) = if lrmm { (self.screen().left, self.screen().right + 1) } else { (0, self.screen().cols()) };
        for r in from..to.min(self.screen().rows()) {
            self.erase_cells(r, left, right, false);
        }
    }

    /// EL and DECSEL.
    pub(super) fn erase_line(&mut self, range: EraseRange, selective: bool) {
        let (row, col, cols) = {
            let s = self.screen();
            (s.cursor.row, s.cursor.col, s.cols())
        };
        match range {
            EraseRange::ToEnd => self.erase_cells(row, col, cols, selective),
            EraseRange::ToStart => self.erase_cells(row, 0, col + 1, selective),
            EraseRange::All => self.erase_cells(row, 0, cols, selective),
        }
        self.screen_mut().cursor.pending_wrap = false;
    }

    /// ED and DECSED.
    pub(super) fn erase_display(&mut self, range: EraseRange, selective: bool) {
        let (row, rows) = (self.screen().cursor.row, self.screen().rows());
        match range {
            EraseRange::ToEnd => {
                self.erase_line(EraseRange::ToEnd, selective);
                for r in row + 1..rows {
                    self.erase_row(r, selective);
                }
            }
            EraseRange::ToStart => {
                for r in 0..row {
                    self.erase_row(r, selective);
                }
                self.erase_line(EraseRange::ToStart, selective);
            }
            EraseRange::All => {
                for r in 0..rows {
                    self.erase_row(r, selective);
                }
            }
        }
        self.screen_mut().cursor.pending_wrap = false;
        self.screen_mut().last_print = None;
    }

    /// Clears a whole active row, including its wrap and line size flags.
    fn erase_row(&mut self, row: u16, selective: bool) {
        let cols = self.screen().cols();
        self.erase_cells(row, 0, cols, selective);
        let s = self.screen_mut();
        let (page, i) = s.pages.active_mut(row);
        if !selective {
            let r = page.row_mut(i);
            r.flags &= !(row_flags::WRAP | row_flags::WRAP_CONTINUATION | row_flags::LINE_SIZE | row_flags::SEMANTIC_PROMPT);
        }
    }

    /// ECH: erases `n` characters from the cursor without moving it.
    pub(super) fn erase_chars(&mut self, n: u16) {
        let (row, col, right) = {
            let s = self.screen();
            (s.cursor.row, s.cursor.col, s.cols())
        };
        let end = (col as u32 + n.max(1) as u32).min(right as u32) as u16;
        self.erase_cells(row, col, end, false);
        self.screen_mut().cursor.pending_wrap = false;
    }

    /// ICH: inserts `n` blank cells at the cursor, pushing cells right within the margins.
    pub(super) fn insert_blank_cells(&mut self, n: u16) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let (row, col, right) = {
            let s = self.screen();
            let right = if lrmm && s.cursor.col >= s.left && s.cursor.col <= s.right { s.right } else { s.cols() - 1 };
            if lrmm && (s.cursor.col < s.left || s.cursor.col > s.right) {
                return;
            }
            (s.cursor.row, s.cursor.col, right)
        };
        self.shift_cells(row, col, right, n.max(1), true);
        self.screen_mut().cursor.pending_wrap = false;
    }

    /// DCH: deletes `n` cells at the cursor, pulling cells left within the margins.
    pub(super) fn delete_cells(&mut self, n: u16) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let (row, col, right) = {
            let s = self.screen();
            if lrmm && (s.cursor.col < s.left || s.cursor.col > s.right) {
                return;
            }
            let right = if lrmm { s.right } else { s.cols() - 1 };
            (s.cursor.row, s.cursor.col, right)
        };
        self.shift_cells(row, col, right, n.max(1), false);
        self.screen_mut().cursor.pending_wrap = false;
    }

    /// Moves cells of `row` between `col` and `right` (inclusive) right (insert) or left (delete)
    /// by `n`, filling the gap with erase cells.
    fn shift_cells(&mut self, row: u16, col: u16, right: u16, n: u16, insert: bool) {
        let blank = self.erase_cell(row);
        let s = self.screen_mut();
        let (page, i) = s.pages.active_mut(row);
        let (col, right) = (col as usize, right as usize);
        let span = right + 1 - col;
        let n = (n as usize).min(span);
        // A wide character cut by the left edge, the right edge, or the point where cells fall
        // off (insert) or are removed (delete) is cleared, so no half is moved alone.
        let cut = if insert { right + 1 - n } else { col + n };
        page.clear_split_wide(i, col, right + 1);
        page.clear_split_wide(i, cut, cut);
        let tmp = page.extract_row(i);
        let order: Vec<(usize, usize)> = match (insert, n < span) {
            (_, false) => Vec::new(),
            (true, true) => (col..=right - n).rev().map(|c| (c, c + n)).collect(),
            (false, true) => (col + n..=right).map(|c| (c, c - n)).collect(),
        };
        for (src, dst) in order {
            let cell = tmp.cells(0)[src];
            page.copy_cell_from(i, dst, &tmp, tmp.cell_index(0, src), cell);
        }
        let gap = if insert { col..col + n } else { right + 1 - n..right + 1 };
        page.clear_cells(i, gap.start, gap.end, blank, false);
        page.drop_unused_style(blank.style_id());
        let r = page.row_mut(i);
        r.set(row_flags::WRAP, false);
    }

    /// IL: inserts `n` blank lines at the cursor row inside the scrolling region.
    pub(super) fn insert_lines(&mut self, n: u16) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let s = self.screen_mut();
        if s.cursor.row < s.top || s.cursor.row > s.bottom {
            return;
        }
        if lrmm && (s.cursor.col < s.left || s.cursor.col > s.right) {
            return;
        }
        let margins = (lrmm && !(s.left == 0 && s.right == s.cols() - 1)).then_some((s.left, s.right + 1));
        let (row, bottom) = (s.cursor.row, s.bottom);
        s.pages.scroll_down(row, bottom, n.max(1), margins);
        s.cursor.col = if lrmm { s.left } else { 0 };
        s.cursor.pending_wrap = false;
        s.last_print = None;
        self.fill_exposed(row, (row + n.max(1)).min(bottom + 1));
    }

    /// DL: deletes `n` lines at the cursor row inside the scrolling region.
    pub(super) fn delete_lines(&mut self, n: u16) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let s = self.screen_mut();
        if s.cursor.row < s.top || s.cursor.row > s.bottom {
            return;
        }
        if lrmm && (s.cursor.col < s.left || s.cursor.col > s.right) {
            return;
        }
        let margins = (lrmm && !(s.left == 0 && s.right == s.cols() - 1)).then_some((s.left, s.right + 1));
        let (row, bottom) = (s.cursor.row, s.bottom);
        s.pages.scroll_up(row, bottom, n.max(1), margins);
        s.cursor.col = if lrmm { s.left } else { 0 };
        s.cursor.pending_wrap = false;
        s.last_print = None;
        let n = n.max(1).min(bottom + 1 - row);
        self.fill_exposed(bottom + 1 - n, bottom + 1);
    }

    /// SU: scrolls the region up `n` lines. On the primary screen with a full region the lines
    /// go into history.
    pub(super) fn scroll_up_lines(&mut self, n: u16) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let alt = self.alt_active;
        let s = self.screen_mut();
        let margins = (lrmm && !(s.left == 0 && s.right == s.cols() - 1)).then_some((s.left, s.right + 1));
        if margins.is_none() && s.top == 0 && !alt {
            let bottom = s.bottom;
            let rows = s.rows();
            for _ in 0..n.max(1).min(rows) {
                s.push_top_line_to_history(bottom);
            }
        } else {
            let (top, bottom) = (s.top, s.bottom);
            s.pages.scroll_up(top, bottom, n.max(1), margins);
        }
        s.cursor.style_cache = None;
        s.last_print = None;
        let (top, bottom) = (s.top, s.bottom);
        let n = n.max(1).min(bottom + 1 - top);
        self.fill_exposed(bottom + 1 - n, bottom + 1);
    }

    /// SD: scrolls the region down `n` lines.
    pub(super) fn scroll_down_lines(&mut self, n: u16) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let s = self.screen_mut();
        let margins = (lrmm && !(s.left == 0 && s.right == s.cols() - 1)).then_some((s.left, s.right + 1));
        let (top, bottom) = (s.top, s.bottom);
        s.pages.scroll_down(top, bottom, n.max(1), margins);
        s.last_print = None;
        self.fill_exposed(top, (top + n.max(1)).min(bottom + 1));
    }

    /// Scrolls columns of the region right by `n` (DECIC and DECBI at the margin).
    pub(super) fn scroll_columns_right(&mut self, top: u16, bottom: u16, n: u16) {
        let (left, right) = (self.screen().left, self.screen().right);
        for r in top..=bottom {
            self.shift_cells(r, left, right, n, true);
        }
    }

    /// Scrolls columns of the region left by `n` (DECDC and DECFI at the margin).
    pub(super) fn scroll_columns_left(&mut self, top: u16, bottom: u16, n: u16) {
        let (left, right) = (self.screen().left, self.screen().right);
        for r in top..=bottom {
            self.shift_cells(r, left, right, n, false);
        }
    }

    /// CUP and HVP: moves to a 1 based row and column, relative to the region in origin mode.
    pub(super) fn goto(&mut self, row: u16, col: u16) {
        let origin = self.modes.get(Mode::Origin);
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let s = self.screen_mut();
        let (top, bottom) = if origin { (s.top, s.bottom) } else { (0, s.rows() - 1) };
        let (left, right) = if origin && lrmm { (s.left, s.right) } else { (0, s.cols() - 1) };
        s.cursor.row = (top as u32 + row.saturating_sub(1) as u32).min(bottom as u32) as u16;
        s.cursor.col = (left as u32 + col.saturating_sub(1) as u32).min(right as u32) as u16;
        s.cursor.pending_wrap = false;
    }

    /// Moves to a 1 based row, keeping the column (VPA).
    pub(super) fn goto_row(&mut self, row: u16) {
        let col = self.cursor_col_in_origin();
        self.goto(row, col + 1);
    }

    /// Moves to a 1 based column, keeping the row (CHA, HPA).
    pub(super) fn goto_col(&mut self, col: u16) {
        let row = self.cursor_row_in_origin();
        self.goto(row + 1, col);
    }

    /// The cursor row relative to the origin.
    fn cursor_row_in_origin(&self) -> u16 {
        let s = self.screen();
        if self.modes.get(Mode::Origin) { s.cursor.row.saturating_sub(s.top) } else { s.cursor.row }
    }

    /// The cursor column relative to the origin.
    fn cursor_col_in_origin(&self) -> u16 {
        let s = self.screen();
        if self.modes.get(Mode::Origin) && self.modes.get(Mode::LeftRightMargins) { s.cursor.col.saturating_sub(s.left) } else { s.cursor.col }
    }

    /// CUU: up `n`, stopping at the top margin when inside the region.
    pub(super) fn cursor_up(&mut self, n: u16) {
        let s = self.screen_mut();
        let limit = if s.cursor.row >= s.top { s.top } else { 0 };
        s.cursor.row = s.cursor.row.saturating_sub(n.max(1)).max(limit);
        s.cursor.pending_wrap = false;
    }

    /// CUD: down `n`, stopping at the bottom margin when inside the region.
    pub(super) fn cursor_down(&mut self, n: u16) {
        let s = self.screen_mut();
        let limit = if s.cursor.row <= s.bottom { s.bottom } else { s.rows() - 1 };
        s.cursor.row = (s.cursor.row as u32 + n.max(1) as u32).min(limit as u32) as u16;
        s.cursor.pending_wrap = false;
    }

    /// CUF: right `n`, stopping at the right margin when inside it.
    pub(super) fn cursor_forward(&mut self, n: u16) {
        let s = self.screen_mut();
        let limit = if s.cursor.col <= s.right { s.right } else { s.cols() - 1 };
        s.cursor.col = (s.cursor.col as u32 + n.max(1) as u32).min(limit as u32) as u16;
        s.cursor.pending_wrap = false;
    }

    /// CUB: left `n`, stopping at the left margin when inside it. With reverse wrap on, moving
    /// past the left margin continues on the previous line.
    pub(super) fn cursor_backward(&mut self, n: u16) {
        let reverse = self.modes.get(Mode::ReverseWrap) && self.modes.get(Mode::Autowrap);
        let s = self.screen_mut();
        let mut n = n.max(1);
        let limit = if s.cursor.col >= s.left { s.left } else { 0 };
        if !reverse {
            s.cursor.col = s.cursor.col.saturating_sub(n).max(limit);
            s.cursor.pending_wrap = false;
            return;
        }
        // In reverse wrap mode a pending wrap uses up one step, as xterm does.
        if s.cursor.pending_wrap {
            n -= 1;
            s.cursor.pending_wrap = false;
        }
        for _ in 0..n {
            if s.cursor.col > limit {
                s.cursor.col -= 1;
            } else if s.cursor.row > s.top {
                s.cursor.row -= 1;
                s.cursor.col = s.right;
            } else {
                break;
            }
        }
    }

    /// DECSTBM: sets the top and bottom margins (1 based) and homes the cursor.
    pub(super) fn set_top_bottom(&mut self, top: u16, bottom: u16) {
        let rows = self.screen().rows();
        let top = top.max(1);
        let bottom = if bottom == 0 { rows } else { bottom.min(rows) };
        if top >= bottom {
            return;
        }
        let s = self.screen_mut();
        s.top = top - 1;
        s.bottom = bottom - 1;
        self.goto(1, 1);
    }

    /// DECSLRM: sets the left and right margins (1 based) when DECLRMM is on.
    pub(super) fn set_left_right(&mut self, left: u16, right: u16) {
        let cols = self.screen().cols();
        let left = left.max(1);
        let right = if right == 0 { cols } else { right.min(cols) };
        if left >= right {
            return;
        }
        let s = self.screen_mut();
        s.left = left - 1;
        s.right = right - 1;
        self.goto(1, 1);
    }

    /// DECFRA: fills a rectangle with a character.
    pub(super) fn fill_rect(&mut self, c: char, top: u16, left: u16, bottom: u16, right: u16) {
        for r in top..=bottom {
            let base = {
                let style = self.screen().cursor.style;
                let s = self.screen_mut();
                let (page, _) = s.pages.active_mut(r);
                Cell::new(c, page.intern_style(&style))
            };
            let s = self.screen_mut();
            let (page, i) = s.pages.active_mut(r);
            fix_wide_boundaries(page, i, left as usize, right as usize + 1);
            for col in left..=right {
                page.write_cell(i, col as usize, base);
            }
        }
    }

    /// DECERA and DECSERA: erases a rectangle (selective keeps protected cells).
    pub(super) fn erase_rect(&mut self, top: u16, left: u16, bottom: u16, right: u16, selective: bool) {
        for r in top..=bottom {
            self.erase_cells(r, left, right + 1, selective);
        }
    }

    /// DECCRA: copies a rectangle to another position on the same screen.
    pub(super) fn copy_rect(&mut self, top: u16, left: u16, bottom: u16, right: u16, dst_top: u16, dst_left: u16) {
        let s = self.screen_mut();
        let (rows, cols) = (s.rows(), s.cols());
        let rows_src: Vec<crate::page::Page> = (top..=bottom).map(|r| {
            let (p, i) = s.pages.active_mut(r);
            p.extract_row(i)
        }).collect();
        for (k, tmp) in rows_src.iter().enumerate() {
            let dr = dst_top + k as u16;
            if dr >= rows {
                break;
            }
            let (page, i) = s.pages.active_mut(dr);
            for c in left..=right {
                let dc = dst_left + (c - left);
                if dc >= cols {
                    break;
                }
                let cell = tmp.cells(0)[c as usize];
                page.copy_cell_from(i, dc as usize, tmp, tmp.cell_index(0, c as usize), cell);
            }
        }
    }
}
