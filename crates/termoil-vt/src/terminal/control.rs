//! C0 controls, escape sequences, and the cursor movement and reset operations they share with
//! CSI.

use super::{CursorShape, Event, Terminal};
use crate::charset::Charset;
use crate::modes::Mode;
use crate::screen::{SavedCursor, Screen, default_tabs};
use crate::style::default_palette;

impl Terminal {
    /// Executes a C0 control.
    pub(super) fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => self.event(Event::Bell),
            0x08 => self.backspace(),
            0x09 => self.tab_forward(1),
            0x0a..=0x0c => {
                self.index();
                if self.modes.get(Mode::LinefeedNewline) {
                    self.carriage_return();
                }
            }
            0x0d => self.carriage_return(),
            0x0e => self.screen_mut().charsets.gl = 1,
            0x0f => self.screen_mut().charsets.gl = 0,
            0x05 => {}
            _ => {}
        }
    }

    /// Dispatches an escape sequence.
    pub(super) fn esc_dispatch(&mut self, intermediates: &[u8], final_byte: u8) {
        match (intermediates, final_byte) {
            ([], b'7') => self.save_cursor(),
            ([], b'8') => self.restore_cursor(),
            ([], b'D') => self.index(),
            ([], b'E') => {
                self.index();
                self.carriage_return();
            }
            ([], b'H') => self.set_tab_stop(),
            ([], b'M') => self.reverse_index(),
            ([], b'N') => self.screen_mut().charsets.single_shift = Some(2),
            ([], b'O') => self.screen_mut().charsets.single_shift = Some(3),
            ([], b'c') => self.full_reset(),
            ([], b'Z') => self.reply_da1(),
            ([], b'V') => self.set_iso_protection(true),
            ([], b'W') => self.set_iso_protection(false),
            ([], b'=') => self.set_mode_value(Mode::KeypadApplication, true),
            ([], b'>') => self.set_mode_value(Mode::KeypadApplication, false),
            ([], b'n') => self.screen_mut().charsets.gl = 2,
            ([], b'o') => self.screen_mut().charsets.gl = 3,
            ([], b'~') => self.screen_mut().charsets.gr = 1,
            ([], b'}') => self.screen_mut().charsets.gr = 2,
            ([], b'|') => self.screen_mut().charsets.gr = 3,
            ([], b'6') => self.back_index(),
            ([], b'9') => self.forward_index(),
            ([b'#'], b'8') => self.alignment_test(),
            ([b'#'], b'3') => self.set_line_size(crate::cell::row_flags::DOUBLE_HEIGHT_TOP),
            ([b'#'], b'4') => self.set_line_size(crate::cell::row_flags::DOUBLE_HEIGHT_BOTTOM),
            ([b'#'], b'5') => self.set_line_size(0),
            ([b'#'], b'6') => self.set_line_size(crate::cell::row_flags::DOUBLE_WIDTH),
            ([g @ (b'(' | b')' | b'*' | b'+')], f) => self.designate(*g - b'(', f),
            ([g @ (b'-' | b'.' | b'/')], f) => self.designate(*g - b',', f),
            ([b' '], b'F') => self.c1_8bit = false,
            ([b' '], b'G') => self.c1_8bit = true,
            _ => {}
        }
    }

    /// SPA and EPA: starts or ends an ISO protected area. ED, EL and ECH skip protected cells
    /// from now on, and DECSED and DECSEL stop skipping them.
    fn set_iso_protection(&mut self, on: bool) {
        self.protect_iso = true;
        self.screen_mut().cursor.protected = on;
    }

    /// DECSCA: Ps 1 protects new cells from selective erase; 0 and 2 stop protecting them.
    pub(super) fn set_dec_protection(&mut self, on: bool) {
        self.protect_iso = false;
        self.screen_mut().cursor.protected = on;
    }

    /// SCS: designates a character set into G0 to G3.
    fn designate(&mut self, g: u8, final_byte: u8) {
        if let Some(set) = Charset::from_final(final_byte) {
            self.screen_mut().charsets.g[g as usize & 3] = set;
        }
    }

    /// BS: one column left, the same as CUB 1 (reverse wrap included).
    pub(super) fn backspace(&mut self) {
        self.cursor_backward(1);
    }

    /// CR: to the left margin (or column 0 when the cursor is left of it).
    pub(super) fn carriage_return(&mut self) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let s = self.screen_mut();
        s.cursor.col = if lrmm && s.cursor.col >= s.left { s.left } else { 0 };
        s.cursor.pending_wrap = false;
    }

    /// IND and LF: down one row, scrolling the region when at its bottom.
    pub(super) fn index(&mut self) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let alt = self.alt_active;
        let s = self.screen_mut();
        s.cursor.pending_wrap = false;
        if s.cursor.row == s.bottom {
            if lrmm && (s.cursor.col < s.left || s.cursor.col > s.right) {
                return;
            }
            let margins = lrmm && !(s.left == 0 && s.right == s.cols() - 1);
            if !margins && s.top == 0 && !alt {
                let bottom = s.bottom;
                s.push_top_line_to_history(bottom);
            } else {
                let s = self.screen_mut();
                let m = margins.then_some((s.left, s.right + 1));
                s.pages.scroll_up(s.top, s.bottom, 1, m);
            }
            let bottom = self.screen().bottom;
            self.fill_exposed(bottom, bottom + 1);
            self.on_scroll();
        } else if s.cursor.row + 1 < s.rows() {
            s.cursor.row += 1;
        }
    }

    /// RI: up one row, scrolling the region down when at its top.
    pub(super) fn reverse_index(&mut self) {
        let lrmm = self.modes.get(Mode::LeftRightMargins);
        let s = self.screen_mut();
        s.cursor.pending_wrap = false;
        if s.cursor.row == s.top {
            if lrmm && (s.cursor.col < s.left || s.cursor.col > s.right) {
                return;
            }
            let margins = lrmm && !(s.left == 0 && s.right == s.cols() - 1);
            let m = margins.then_some((s.left, s.right + 1));
            s.pages.scroll_down(s.top, s.bottom, 1, m);
            let top = s.top;
            self.fill_exposed(top, top + 1);
        } else if s.cursor.row > 0 {
            s.cursor.row -= 1;
        }
    }

    /// DECBI: left one column, scrolling the margins right at the left margin.
    fn back_index(&mut self) {
        let s = self.screen_mut();
        if s.cursor.col == s.left {
            let (top, bottom) = (s.top, s.bottom);
            self.scroll_columns_right(top, bottom, 1);
        } else if s.cursor.col > 0 {
            s.cursor.col -= 1;
        }
    }

    /// DECFI: right one column, scrolling the margins left at the right margin.
    fn forward_index(&mut self) {
        let s = self.screen_mut();
        if s.cursor.col == s.right {
            let (top, bottom) = (s.top, s.bottom);
            self.scroll_columns_left(top, bottom, 1);
        } else if s.cursor.col + 1 < s.cols() {
            s.cursor.col += 1;
        }
    }

    /// Invalidates what depends on rows staying where they were after a scroll.
    fn on_scroll(&mut self) {
        let s = self.screen_mut();
        if let Some(lp) = s.last_print.as_mut() {
            if lp.row == 0 {
                s.last_print = None;
            } else {
                lp.row -= 1;
            }
        }
    }

    /// HT: forward `n` tab stops, stopping at the right margin. A tab that does not move the
    /// cursor leaves a pending wrap alone. With MoreFix (mode 41) a pending wrap is carried out
    /// first, so the tab starts on the next line.
    pub(super) fn tab_forward(&mut self, n: u16) {
        if self.screen().cursor.pending_wrap && self.modes.get(Mode::MoreFix) && self.modes.get(Mode::Autowrap) {
            self.wrap_line();
        }
        let s = self.screen_mut();
        let start = s.cursor.col;
        let right = if s.cursor.col <= s.right { s.right } else { s.cols() - 1 };
        for _ in 0..n.max(1) {
            let mut col = s.cursor.col;
            while col < right {
                col += 1;
                if s.tabs.get(col as usize).copied().unwrap_or(false) {
                    break;
                }
            }
            s.cursor.col = col;
        }
        if s.cursor.col != start {
            s.cursor.pending_wrap = false;
        }
    }

    /// CBT: back `n` tab stops, stopping at the left edge of the screen (the left margin is ignored).
    pub(super) fn tab_backward(&mut self, n: u16) {
        let s = self.screen_mut();
        for _ in 0..n.max(1) {
            let mut col = s.cursor.col;
            while col > 0 {
                col -= 1;
                if s.tabs.get(col as usize).copied().unwrap_or(false) {
                    break;
                }
            }
            s.cursor.col = col;
        }
        s.cursor.pending_wrap = false;
    }

    /// HTS: a tab stop at the cursor column.
    fn set_tab_stop(&mut self) {
        let s = self.screen_mut();
        let col = s.cursor.col as usize;
        if let Some(t) = s.tabs.get_mut(col) {
            *t = true;
        }
    }

    /// DECSC: saves the cursor, pen, origin mode and character sets.
    pub(super) fn save_cursor(&mut self) {
        let origin = self.modes.get(Mode::Origin);
        let s = self.screen_mut();
        s.saved = Some(SavedCursor {
            row: s.cursor.row,
            col: s.cursor.col,
            pending_wrap: s.cursor.pending_wrap,
            style: s.cursor.style,
            protected: s.cursor.protected,
            origin,
            charsets: s.charsets,
        });
    }

    /// DECRC: restores what DECSC saved, or homes the cursor with default settings.
    pub(super) fn restore_cursor(&mut self) {
        let saved = self.screen().saved.clone().unwrap_or_default();
        self.modes.set(Mode::Origin, saved.origin);
        let s = self.screen_mut();
        s.cursor.row = saved.row.min(s.rows() - 1);
        s.cursor.col = saved.col.min(s.cols() - 1);
        s.cursor.pending_wrap = saved.pending_wrap;
        s.cursor.style = saved.style;
        s.cursor.style_cache = None;
        s.cursor.protected = saved.protected;
        s.charsets = saved.charsets;
    }

    /// DECALN: fills the screen with `E` and resets the margins.
    fn alignment_test(&mut self) {
        let s = self.screen_mut();
        s.reset_margins();
        s.cursor.row = 0;
        s.cursor.col = 0;
        s.cursor.pending_wrap = false;
        let (rows, cols) = (s.rows(), s.cols());
        for r in 0..rows {
            let (page, i) = s.pages.active_mut(r);
            page.clear_row(i);
            for c in 0..cols as usize {
                page.write_cell(i, c, crate::cell::Cell::new('E', 0));
            }
        }
    }

    /// DECDHL and DECDWL: marks the cursor row's line size.
    fn set_line_size(&mut self, flag: u16) {
        use crate::cell::row_flags::LINE_SIZE;
        let s = self.screen_mut();
        let row = s.cursor.row;
        let (page, i) = s.pages.active_mut(row);
        let r = page.row_mut(i);
        r.flags = (r.flags & !LINE_SIZE) | flag | crate::cell::row_flags::DIRTY;
        if flag != 0 {
            let half = s.cols() / 2;
            if s.cursor.col >= half {
                s.cursor.col = half.saturating_sub(1);
            }
        }
    }

    /// Sets a mode by name and tells the host when it matters to input.
    pub(super) fn set_mode_value(&mut self, mode: Mode, on: bool) {
        if self.modes.set(mode, on) {
            self.event(Event::ModeChanged(mode));
        }
    }

    /// RIS: everything back to its initial state, keeping the size and the scrollback setting.
    pub(super) fn full_reset(&mut self) {
        let (cols, rows) = (self.cols(), self.rows());
        let was_132 = self.modes.get(Mode::Column132);
        let limit = self.config.scrollback;
        self.primary = Screen::new(cols, rows, limit, true);
        self.alt = None;
        self.alt_active = false;
        self.modes.reset();
        self.modes.set(Mode::GraphemeCluster, self.config.grapheme_clustering);
        self.palette = default_palette();
        self.special_colors = [None; 5];
        self.extra_colors = [None; 5];
        self.notification_pending.clear();
        self.colors = super::DynamicColors::default();
        self.title_stack.clear();
        self.title_modes = 0;
        self.window.restore = None;
        self.sgr_stack.clear();
        self.cursor_shape = CursorShape::Default;
        self.sync_pending = false;
        self.graphics.clear();
        self.event(Event::ColorsChanged);
        self.event(Event::CursorShapeChanged);
        self.protect_iso = false;
        self.vt_level = 5;
        self.c1_8bit = false;
        if was_132 {
            self.request_resize(80, rows);
        }
    }

    /// DECSTR: soft reset. Modes, pen, margins and saved cursor go back to defaults; the screen
    /// contents and the cursor position stay.
    pub(super) fn soft_reset(&mut self) {
        for mode in [
            Mode::Insert,
            Mode::Origin,
            Mode::CursorKeys,
            Mode::KeypadApplication,
            Mode::LeftRightMargins,
            Mode::ReverseWrap,
            Mode::ReverseWrapExtended,
        ] {
            self.modes.set(mode, false);
        }
        self.modes.set(Mode::Autowrap, true);
        self.modes.set(Mode::CursorVisible, true);
        self.protect_iso = false;
        let s = self.screen_mut();
        s.reset_margins();
        s.cursor.style = Default::default();
        s.cursor.style_cache = None;
        s.cursor.protected = false;
        s.cursor.pending_wrap = false;
        s.charsets = Default::default();
        s.saved = None;
        s.kitty_keyboard.reset();
        self.sgr_stack.clear();
    }

    /// TBC: clears the tab stop at the cursor (0) or every tab stop (3).
    pub(super) fn clear_tabs(&mut self, which: u16) {
        let s = self.screen_mut();
        match which {
            0 => {
                let col = s.cursor.col as usize;
                if let Some(t) = s.tabs.get_mut(col) {
                    *t = false;
                }
            }
            3 | 5 => s.tabs.iter_mut().for_each(|t| *t = false),
            _ => {}
        }
    }

    /// DECST8C: tab stops back to every eight columns.
    pub(super) fn reset_tabs(&mut self) {
        let s = self.screen_mut();
        s.tabs = default_tabs(s.cols());
    }
}
