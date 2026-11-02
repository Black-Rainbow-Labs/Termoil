//! Printing text into the grid.
//!
//! Plain ASCII in the default character set takes a fast path that writes a whole run of cells
//! per row with one style lookup. Everything else goes through [`Terminal::print_char`], which
//! handles character sets, widths, grapheme clusters (mode 2027), legacy combining characters
//! when 2027 is off, wide characters at the margin, insert mode and hyperlinks.

use super::Terminal;
use super::segment::{PrintModes, write_segment};
use crate::cell::{Cell, Semantic, Wide, row_flags};
use crate::modes::Mode;
use crate::page::Page;
use crate::screen::{LastPrint, Screen};
use crate::unicode::{GraphemeBreaker, char_width, cluster_width};

/// The kitty graphics Unicode placeholder.
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

impl Terminal {
    /// Prints a run of text with no control characters in it. Insert mode and translated
    /// character sets go character by character; everything else goes through the segment
    /// writer one row at a time. `plain_ascii` says the parser already checked that the text is
    /// printable ASCII only, so the segment writer skips its own scan.
    pub(super) fn print(&mut self, text: &str, plain_ascii: bool) {
        if self.modes.get(Mode::Insert) || !self.screen().charsets.is_plain() {
            for c in text.chars() {
                self.print_char(c);
            }
            return;
        }
        let autowrap = self.modes.get(Mode::Autowrap);
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let cluster = self.modes.get(Mode::GraphemeCluster);
        let max_cluster = self.config.max_cluster_len;
        let mut rest = text;
        while !rest.is_empty() {
            let right = effective_right(self.screen(), lrmm);
            let m = PrintModes { autowrap, cluster, right, max_cluster, plain_ascii };
            let n = write_segment(self.screen_mut(), rest, m);
            rest = &rest[n..];
            if rest.is_empty() {
                break;
            }
            if self.screen().cursor.pending_wrap && autowrap {
                self.wrap_line();
            } else if n == 0 {
                // Nothing fitted and nothing will: drop one character rather than loop.
                let skip = rest.chars().next().map(char::len_utf8).unwrap_or(1);
                rest = &rest[skip..];
            }
        }
    }

    /// A blank cell carrying the cursor's style id (for the page holding `row`), protection and
    /// semantic content, to which a code point is added.
    fn pen_cell(&mut self, row: u16) -> Cell {
        let screen = self.screen_mut();
        let style = screen.cursor.style;
        let cache = screen.cursor.style_cache;
        let protected = screen.cursor.protected;
        let semantic = screen.cursor.semantic;
        let (page, _) = screen.pages.active_mut(row);
        let id = match cache {
            Some((uid, id)) if uid == page.uid() && page.style_id_valid(id, &style) => id,
            _ => {
                let id = page.intern_style(&style);
                let uid = page.uid();
                screen.cursor.style_cache = Some((uid, id));
                id
            }
        };
        let mut cell = Cell::blank(id);
        cell.set_protected(protected);
        if semantic != Semantic::Output {
            cell.set_semantic(semantic);
        }
        cell
    }

    /// Prints one character through the slow path.
    pub(super) fn print_char(&mut self, c: char) {
        if ('\u{80}'..='\u{9f}').contains(&c) {
            return;
        }
        let c = self.screen_mut().charsets.translate(c);
        let cluster_mode = self.modes.get(Mode::GraphemeCluster);
        let mut breaker = GraphemeBreaker::new(c);
        if let Some(lp) = self.joinable_last_print() {
            if cluster_mode {
                let mut br = lp.breaker();
                if !br.should_break(c) {
                    self.join_cluster(lp, c, br);
                    return;
                }
            } else if char_width(c) == 0 {
                self.join_cluster(lp, c, lp.breaker());
                return;
            }
            breaker = GraphemeBreaker::new(c);
        }
        let width = if cluster_mode { cluster_width(&[c]) } else { char_width(c).max(1) };
        if let Some((row, col)) = self.put_char(c, width.clamp(1, 2)) {
            let s = self.screen_mut();
            let after = (s.cursor.row, s.cursor.col, s.cursor.pending_wrap);
            s.last_print = Some(LastPrint { row, col, state: Some(breaker), ascii: 0, after });
        }
    }

    /// The last printed cell if the cursor has not moved since it was printed.
    fn joinable_last_print(&self) -> Option<LastPrint> {
        let s = self.screen();
        let lp = s.last_print?;
        (lp.after == (s.cursor.row, s.cursor.col, s.cursor.pending_wrap)).then_some(lp)
    }

    /// Adds `c` to the cluster in the last printed cell, widening or narrowing it when the
    /// cluster's width changes (an emoji presentation selector, for example).
    fn join_cluster(&mut self, lp: LastPrint, c: char, breaker: GraphemeBreaker) {
        let limit = self.config.max_cluster_len;
        let cluster_mode = self.modes.get(Mode::GraphemeCluster);
        let autowrap = self.modes.get(Mode::Autowrap);
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let screen = self.screen_mut();
        let (page, i) = screen.pages.active_mut(lp.row);
        if !page.append_grapheme(i, lp.col as usize, c, limit) {
            return;
        }
        if cluster_mode {
            let index = page.cell_index(i, lp.col as usize);
            let cell = page.cells(i)[lp.col as usize];
            let mut chars = vec![cell.char().unwrap_or(' ')];
            chars.extend_from_slice(page.grapheme(index));
            let width = cluster_width(&chars);
            let right = effective_right(screen, lrmm);
            adjust_cluster_width(screen, lp, width, right, autowrap);
        }
        let s = self.screen_mut();
        let after = (s.cursor.row, s.cursor.col, s.cursor.pending_wrap);
        s.last_print = Some(LastPrint { state: Some(breaker), after, ..lp });
    }

    /// Writes one character of the given width at the cursor and advances. Returns the row and
    /// column of the cell written, or None when it did not fit.
    fn put_char(&mut self, c: char, width: u8) -> Option<(u16, u16)> {
        let autowrap = self.modes.get(Mode::Autowrap);
        let insert = self.modes.get(Mode::Insert);
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        if self.screen().cursor.pending_wrap && autowrap {
            self.wrap_line();
        }
        let right = effective_right(self.screen(), lrmm);
        if width == 2 && self.screen().cursor.col >= right {
            if !autowrap || right == 0 {
                return None;
            }
            self.write_spacer_head();
            self.wrap_line();
        }
        if insert {
            let n = width as u16;
            self.insert_blank_cells(n);
        }
        let row = self.screen().cursor.row;
        let col = self.screen().cursor.col;
        let base = self.pen_cell(row);
        let link = self.screen().cursor.hyperlink;
        let screen = self.screen_mut();
        write_char_cell(screen, row, col, c, width, base, link);
        let end = col + width as u16;
        if end > right {
            screen.cursor.col = right;
            screen.cursor.pending_wrap = autowrap;
        } else {
            screen.cursor.col = end;
            screen.cursor.pending_wrap = false;
        }
        Some((row, col))
    }

    /// Writes a spacer head at the cursor when a wide character does not fit at the end of a row.
    fn write_spacer_head(&mut self) {
        let screen = self.screen_mut();
        let (row, col) = (screen.cursor.row, screen.cursor.col);
        if col + 1 != screen.cols() {
            return;
        }
        let (page, i) = screen.pages.active_mut(row);
        fix_wide_boundaries(page, i, col as usize, col as usize + 1);
        let mut head = Cell::BLANK;
        head.set_wide(Wide::SpacerHead);
        page.write_cell(i, col as usize, head);
    }

    /// Moves to the start of the next line because the last line is full, marking the wrap.
    pub(super) fn wrap_line(&mut self) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        {
            let screen = self.screen_mut();
            let row = screen.cursor.row;
            let (page, i) = screen.pages.active_mut(row);
            page.row_mut(i).set(row_flags::WRAP, true);
            let left = if lrmm && screen.cursor.col >= screen.left && screen.cursor.col <= screen.right {
                screen.left
            } else {
                0
            };
            screen.cursor.col = left;
            screen.cursor.pending_wrap = false;
        }
        self.index();
        let screen = self.screen_mut();
        let row = screen.cursor.row;
        let (page, i) = screen.pages.active_mut(row);
        page.row_mut(i).set(row_flags::WRAP_CONTINUATION, true);
    }
}

/// The column text stops at: the right margin when DECLRMM is on and the cursor is not right of
/// it, else the last column. A cursor left of the left margin walks into the margins as it
/// prints, so it wraps at the right margin too; deciding this per run or per character gives
/// the same result.
pub(super) fn effective_right(s: &Screen, lrmm: bool) -> u16 {
    if lrmm && s.cursor.col <= s.right { s.right } else { s.cols() - 1 }
}

/// Clears half of a wide character that writing columns `from..to` would leave behind.
pub(super) fn fix_wide_boundaries(page: &mut Page, i: usize, from: usize, to: usize) {
    let cols = page.cols() as usize;
    if from > 0 && page.cells(i)[from].wide() == Wide::SpacerTail {
        page.clear_cells(i, from - 1, from, Cell::BLANK, false);
    }
    if to < cols && to > 0 && page.cells(i)[to - 1].wide() == Wide::Wide && page.cells(i)[to].wide() == Wide::SpacerTail
    {
        page.clear_cells(i, to, to + 1, Cell::BLANK, false);
    }
    if to < cols && page.cells(i)[to.saturating_sub(1)].wide() == Wide::Wide && to > from {
        // The wide character being overwritten keeps no orphan tail.
        if page.cells(i)[to].wide() == Wide::SpacerTail {
            page.clear_cells(i, to, to + 1, Cell::BLANK, false);
        }
    }
}

/// Writes one character cell (and its spacer tail when wide) at (row, col).
fn write_char_cell(screen: &mut Screen, row: u16, col: u16, c: char, width: u8, base: Cell, link: Option<u32>) {
    let (page, i) = screen.pages.active_mut(row);
    let cols = page.cols() as usize;
    let col = col as usize;
    let to = (col + width as usize).min(cols);
    fix_wide_boundaries(page, i, col, to);
    let mut cell = base;
    cell.set_char(c);
    if width == 2 {
        cell.set_wide(Wide::Wide);
    }
    page.write_cell(i, col, cell);
    if width == 2 && col + 1 < cols {
        let mut tail = base;
        tail.set_wide(Wide::SpacerTail);
        page.write_cell(i, col + 1, tail);
    }
    if c == KITTY_PLACEHOLDER {
        page.row_mut(i).flags |= row_flags::KITTY_PLACEHOLDER;
    }
    if let Some(id) = link {
        for k in col..to {
            page.set_hyperlink(i, k, id);
        }
    }
}

/// Makes the last printed cell wide or narrow to match its cluster's new width.
fn adjust_cluster_width(screen: &mut Screen, lp: LastPrint, width: u8, right: u16, autowrap: bool) {
    let cols = screen.cols();
    let (page, i) = screen.pages.active_mut(lp.row);
    let cell = page.cells(i)[lp.col as usize];
    let col = lp.col as usize;
    if width == 2 && cell.wide() == Wide::Narrow {
        if lp.col + 1 > right || lp.col + 1 >= cols {
            return;
        }
        fix_wide_boundaries(page, i, col + 1, col + 2);
        let mut head = page.cells(i)[col];
        head.set_wide(Wide::Wide);
        page.cells_mut(i)[col] = head;
        let mut tail = Cell::blank(head.style_id());
        tail.set_protected(head.protected());
        tail.set_semantic(head.semantic());
        tail.set_wide(Wide::SpacerTail);
        page.write_cell(i, col + 1, tail);
        let end = lp.col + 2;
        if end > right {
            screen.cursor.col = right;
            screen.cursor.pending_wrap = autowrap;
        } else {
            screen.cursor.col = end;
            screen.cursor.pending_wrap = false;
        }
    } else if width == 1 && cell.wide() == Wide::Wide {
        let mut head = cell;
        head.set_wide(Wide::Narrow);
        page.cells_mut(i)[col] = head;
        if col + 1 < cols as usize {
            page.clear_cells(i, col + 1, col + 2, Cell::BLANK, false);
        }
        screen.cursor.col = lp.col + 1;
        screen.cursor.pending_wrap = false;
    }
}
