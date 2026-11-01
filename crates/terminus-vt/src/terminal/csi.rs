//! CSI dispatch: one match from (private marker, intermediates, final byte) to the operation.

use super::edit::EraseRange;
use super::{CursorShape, Event, Terminal};
use crate::modes::{self, Mode, ModeKind};
use crate::parser::CsiSeq;
use crate::screen::Screen;

impl Terminal {
    /// Dispatches a CSI sequence.
    pub(super) fn csi_dispatch(&mut self, seq: &CsiSeq) {
        let p = &seq.params;
        let n = p.get_or(0, 1);
        match (seq.private, seq.intermediates(), seq.final_byte) {
            (0, [], b'@') => self.insert_blank_cells(n),
            (0, [b' '], b'@') => self.scroll_columns_left(self.screen().top, self.screen().bottom, n),
            (0, [], b'A') => self.cursor_up(n),
            (0, [b' '], b'A') => self.scroll_columns_right(self.screen().top, self.screen().bottom, n),
            (0, [], b'B') | (0, [], b'e') => self.cursor_down(n),
            (0, [], b'C') | (0, [], b'a') => self.cursor_forward(n),
            (0, [], b'D') => self.cursor_backward(n),
            (0, [], b'E') => {
                self.cursor_down(n);
                self.carriage_return();
            }
            (0, [], b'F') => {
                self.cursor_up(n);
                self.carriage_return();
            }
            (0, [], b'G') | (0, [], b'`') => self.goto_col(n),
            (0, [], b'H') | (0, [], b'f') => self.goto(p.get_or(0, 1), p.get_or(1, 1)),
            (0, [], b'I') => self.tab_forward(n),
            (0, [], b'J') if p.get(0) == 3 => self.clear_scrollback(),
            (0, [], b'J') => self.erase_display(erase_range(p.get(0)), false),
            (b'?', [], b'J') => self.erase_display(erase_range(p.get(0)), true),
            (0, [], b'K') => self.erase_line(erase_range(p.get(0)), false),
            (b'?', [], b'K') => self.erase_line(erase_range(p.get(0)), true),
            (0, [], b'L') => self.insert_lines(n),
            (0, [], b'M') => self.delete_lines(n),
            (0, [], b'P') => self.delete_cells(n),
            (0, [], b'S') => self.scroll_up_lines(n),
            (0, [], b'T') if p.len() <= 1 => self.scroll_down_lines(n),
            (0, [], b'X') => self.erase_chars(n),
            (0, [], b'Z') => self.tab_backward(n),
            (0, [], b'b') => self.repeat_last(n),
            (0, [], b'd') => self.goto_row(n),
            (0, [], b'g') => self.clear_tabs(p.get(0)),
            (0, [], b'h') => self.set_modes(seq, ModeKind::Ansi, true),
            (b'?', [], b'h') => self.set_modes(seq, ModeKind::Dec, true),
            (0, [], b'l') => self.set_modes(seq, ModeKind::Ansi, false),
            (b'?', [], b'l') => self.set_modes(seq, ModeKind::Dec, false),
            (0, [], b'm') => self.sgr(p),
            (b'>', [], b'm') => self.set_modify_keys(p),
            (b'>', [], b'n') => self.reset_modify_keys(p),
            (0, [], b'r') => self.set_top_bottom(p.get_or(0, 1), p.get(1)),
            (b'?', [], b'r') => self.restore_modes(seq),
            (b'?', [], b's') => self.save_modes(seq),
            (0, [], b's') if self.modes.get(Mode::LeftRightMargins) => self.set_left_right(p.get_or(0, 1), p.get(1)),
            (0, [], b's') => self.save_cursor(),
            (0, [], b'u') => self.restore_cursor(),
            (0, [], b't') => self.window_op(p),
            (0, [b' '], b'q') => self.set_cursor_shape(p.get(0)),
            (0, [b'!'], b'p') => self.soft_reset(),
            (0, [b'"'], b'q') => self.screen_mut().cursor.protected = p.get(0) == 1,
            (0, [b'"'], b'p') => {}
            (b'?', [], b'W') if p.get(0) == 5 => self.reset_tabs(),
            (0, [b'#'], b'{') | (0, [b'#'], b'p') => self.push_sgr(),
            (0, [b'#'], b'}') | (0, [b'#'], b'q') => self.pop_sgr(),
            (_, [], b'u') => self.kitty_keyboard(seq),
            (0, [b'\''], b'}') => self.scroll_columns_right(self.screen().top, self.screen().bottom, n),
            (0, [b'\''], b'~') => self.scroll_columns_left(self.screen().top, self.screen().bottom, n),
            (0, [b'$'], b'x') | (0, [b'$'], b'z') | (0, [b'$'], b'{') | (0, [b'$'], b'v') => self.rect_op(seq),
            _ => self.report_dispatch(seq),
        }
    }

    /// ED 3: clears the primary screen's scrollback.
    fn clear_scrollback(&mut self) {
        self.primary.pages.clear_history();
        let first = self.primary.pages.first_serial();
        self.graphics.prune_before(first);
    }

    /// REP: repeats the last printed character `n` times.
    fn repeat_last(&mut self, n: u16) {
        let Some(lp) = self.screen().last_print else { return };
        let c = {
            let s = self.screen_mut();
            let (page, i) = s.pages.active_mut(lp.row);
            page.cells(i)[lp.col as usize].char()
        };
        if let Some(c) = c {
            let n = n.min(self.cols().saturating_mul(self.rows()));
            for _ in 0..n {
                self.print_char(c);
            }
        }
    }

    /// SM, RM, DECSET and DECRST for every parameter.
    fn set_modes(&mut self, seq: &CsiSeq, kind: ModeKind, on: bool) {
        for i in 0..seq.params.len() {
            let number = seq.params.get(i);
            if let Some(def) = modes::lookup(kind, number) {
                self.set_one_mode(def.mode, on);
            }
        }
    }

    /// Sets one mode and does what changing it implies.
    pub(super) fn set_one_mode(&mut self, mode: Mode, on: bool) {
        match mode {
            Mode::AltScreenLegacy | Mode::AltScreen | Mode::AltScreenSaveCursor => self.switch_alt(mode, on),
            Mode::SaveCursor => {
                if on {
                    self.save_cursor()
                } else {
                    self.restore_cursor()
                }
            }
            Mode::MouseX10 | Mode::MouseNormal | Mode::MouseButtonEvent | Mode::MouseAnyEvent => {
                for m in [Mode::MouseX10, Mode::MouseNormal, Mode::MouseButtonEvent, Mode::MouseAnyEvent] {
                    self.modes.set(m, false);
                }
                self.set_mode_value(mode, on);
            }
            Mode::MouseUtf8 | Mode::MouseSgr | Mode::MouseUrxvt | Mode::MouseSgrPixels => {
                for m in [Mode::MouseUtf8, Mode::MouseSgr, Mode::MouseUrxvt, Mode::MouseSgrPixels] {
                    self.modes.set(m, false);
                }
                self.set_mode_value(mode, on);
            }
            Mode::Origin => {
                self.modes.set(mode, on);
                self.goto(1, 1);
            }
            Mode::Column132 => self.set_column_mode(on),
            Mode::LeftRightMargins => {
                self.modes.set(mode, on);
                let s = self.screen_mut();
                s.left = 0;
                s.right = s.cols() - 1;
            }
            Mode::SynchronizedOutput => {
                self.modes.set(mode, on);
                self.sync_pending = on;
                if on {
                    self.sync_generation += 1;
                }
                self.event(Event::ModeChanged(mode));
            }
            Mode::InBandResize => {
                self.modes.set(mode, on);
                if on {
                    self.report_size_in_band();
                }
            }
            Mode::ReverseVideo | Mode::CursorVisible => {
                self.set_mode_value(mode, on);
                self.screen_mut().pages.mark_all_dirty();
            }
            _ => self.set_mode_value(mode, on),
        }
    }

    /// DECCOLM: 80 or 132 columns, only when mode 40 allows it. Clears the screen unless
    /// DECNCSM is set.
    fn set_column_mode(&mut self, on: bool) {
        if !self.modes.get(Mode::Allow132) {
            return;
        }
        self.modes.set(Mode::Column132, on);
        let cols = if on { 132 } else { 80 };
        let rows = self.rows();
        self.event(Event::ResizeRequest { cols, rows });
        if !self.modes.get(Mode::NoClearOnColumnChange) {
            self.erase_display(EraseRange::All, false);
        }
        let s = self.screen_mut();
        s.reset_margins();
        self.goto(1, 1);
    }

    /// Switches to or from the alternate screen for modes 47, 1047 and 1049.
    fn switch_alt(&mut self, mode: Mode, on: bool) {
        if on == self.alt_active {
            self.modes.set(mode, on);
            return;
        }
        if mode == Mode::AltScreenSaveCursor && on {
            self.save_cursor();
        }
        if on {
            let (cols, rows) = (self.cols(), self.rows());
            let limit = self.config.scrollback;
            // The alternate screen is kept between uses so its own DECSC storage survives, as in
            // xterm; 1047 and 1049 clear its contents on entry, 47 does not.
            match self.alt.as_mut() {
                None => self.alt = Some(Screen::new(cols, rows, limit, false)),
                Some(alt) if mode != Mode::AltScreenLegacy => {
                    for r in 0..alt.rows() {
                        let (p, i) = alt.pages.active_mut(r);
                        p.clear_row(i);
                    }
                    alt.reset_margins();
                }
                Some(_) => {}
            }
            let (cursor, charsets) = (self.primary.cursor.clone(), self.primary.charsets);
            let alt = self.alt.as_mut().expect("created above");
            alt.cursor = cursor;
            alt.cursor.style_cache = None;
            alt.cursor.row = alt.cursor.row.min(alt.rows() - 1);
            alt.charsets = charsets;
            self.alt_active = true;
        } else {
            if mode != Mode::AltScreenLegacy {
                if let Some(alt) = self.alt.as_mut() {
                    let rows = alt.rows();
                    for r in 0..rows {
                        let (p, i) = alt.pages.active_mut(r);
                        p.clear_row(i);
                    }
                }
            }
            self.alt_active = false;
            self.primary.pages.mark_all_dirty();
            self.primary.cursor.style_cache = None;
        }
        for m in [Mode::AltScreenLegacy, Mode::AltScreen, Mode::AltScreenSaveCursor] {
            self.modes.set(m, false);
        }
        self.modes.set(mode, on);
        if mode == Mode::AltScreenSaveCursor && !on {
            self.restore_cursor();
        }
        self.event(Event::ModeChanged(mode));
    }

    /// XTSAVE: saves the listed DEC modes.
    fn save_modes(&mut self, seq: &CsiSeq) {
        for i in 0..seq.params.len() {
            if let Some(def) = modes::lookup(ModeKind::Dec, seq.params.get(i)) {
                self.modes.save(def.mode);
            }
        }
    }

    /// XTRESTORE: restores the listed DEC modes.
    fn restore_modes(&mut self, seq: &CsiSeq) {
        for i in 0..seq.params.len() {
            if let Some(def) = modes::lookup(ModeKind::Dec, seq.params.get(i)) {
                if let Some(v) = self.modes.saved(def.mode) {
                    self.set_one_mode(def.mode, v);
                }
            }
        }
    }

    /// DECSCUSR.
    fn set_cursor_shape(&mut self, ps: u16) {
        self.cursor_shape = match ps {
            0 => CursorShape::Default,
            1 => CursorShape::Block { blink: true },
            2 => CursorShape::Block { blink: false },
            3 => CursorShape::Underline { blink: true },
            4 => CursorShape::Underline { blink: false },
            5 => CursorShape::Bar { blink: true },
            6 => CursorShape::Bar { blink: false },
            _ => return,
        };
        self.event(Event::CursorShapeChanged);
    }

    /// XTMODKEYS: `CSI > 4 ; v m` sets modifyOtherKeys.
    fn set_modify_keys(&mut self, p: &crate::parser::Params) {
        if p.get(0) == 4 {
            self.modify_other_keys = p.get(1).min(2) as u8;
        }
    }

    /// `CSI > 4 n` resets modifyOtherKeys.
    fn reset_modify_keys(&mut self, p: &crate::parser::Params) {
        if p.is_empty() || p.get(0) == 4 {
            self.modify_other_keys = 0;
        }
    }

    /// XTPUSHSGR.
    fn push_sgr(&mut self) {
        if self.sgr_stack.len() == self.config.sgr_stack_depth {
            self.sgr_stack.remove(0);
        }
        let style = self.screen().cursor.style;
        self.sgr_stack.push(style);
    }

    /// XTPOPSGR.
    fn pop_sgr(&mut self) {
        if let Some(style) = self.sgr_stack.pop() {
            let s = self.screen_mut();
            s.cursor.style = style;
            s.cursor.style_cache = None;
        }
    }

    /// The kitty keyboard protocol stack: push `>`, pop `<`, set `=`, query `?`.
    fn kitty_keyboard(&mut self, seq: &CsiSeq) {
        let p = &seq.params;
        match seq.private {
            b'>' => self.screen_mut().kitty_keyboard.push(p.get(0) as u8),
            b'<' => self.screen_mut().kitty_keyboard.pop(p.get_or(0, 1) as usize),
            b'=' => self.screen_mut().kitty_keyboard.set(p.get(0) as u8, p.get_or(1, 1)),
            b'?' => {
                let flags = self.screen().kitty_keyboard.flags();
                self.reply(format!("\x1b[?{flags}u").as_bytes());
            }
            _ => return,
        }
        self.event(Event::ModeChanged(Mode::CursorKeys));
    }

    /// XTWINOPS: title stack, size reports and resize requests.
    fn window_op(&mut self, p: &crate::parser::Params) {
        match p.get(0) {
            8 => {
                let rows = p.get_or(1, self.rows());
                let cols = p.get_or(2, self.cols());
                self.event(Event::ResizeRequest { cols, rows });
            }
            14 => {
                let (w, h) = self.cell_pixel;
                let reply = format!("\x1b[4;{};{}t", h as u32 * self.rows() as u32, w as u32 * self.cols() as u32);
                self.reply(reply.as_bytes());
            }
            16 => {
                let (w, h) = self.cell_pixel;
                self.reply(format!("\x1b[6;{h};{w}t").as_bytes());
            }
            18 => {
                let reply = format!("\x1b[8;{};{}t", self.rows(), self.cols());
                self.reply(reply.as_bytes());
            }
            22 => {
                if self.title_stack.len() == self.config.title_stack_depth {
                    self.title_stack.remove(0);
                }
                self.title_stack.push((self.title.clone(), self.icon_name.clone()));
            }
            23 => {
                if let Some((title, icon)) = self.title_stack.pop() {
                    self.title = title.clone();
                    self.icon_name = icon;
                    self.event(Event::Title(title));
                }
            }
            _ => {}
        }
    }

    /// The rectangle operations: DECFRA, DECERA, DECSERA and DECCRA, with 1 based coordinates
    /// relative to the origin.
    fn rect_op(&mut self, seq: &CsiSeq) {
        let p = &seq.params;
        let base = if seq.final_byte == b'x' { 1 } else { 0 };
        let (rows, cols) = (self.rows(), self.cols());
        let origin = self.modes.get(Mode::Origin);
        let (oy, ox) = if origin { (self.screen().top, self.screen().left) } else { (0, 0) };
        let top = (p.get_or(base, 1) - 1 + oy).min(rows - 1);
        let left = (p.get_or(base + 1, 1) - 1 + ox).min(cols - 1);
        let bottom = (p.get_or(base + 2, rows) - 1 + oy).min(rows - 1);
        let right = (p.get_or(base + 3, cols) - 1 + ox).min(cols - 1);
        if top > bottom || left > right {
            return;
        }
        match seq.final_byte {
            b'x' => {
                let c = char::from_u32(p.get(0) as u32).filter(|c| (' '..='~').contains(c) || (*c as u32) >= 0xa0);
                if let Some(c) = c {
                    self.fill_rect(c, top, left, bottom, right);
                }
            }
            b'z' => self.erase_rect(top, left, bottom, right, false),
            b'{' => self.erase_rect(top, left, bottom, right, true),
            b'v' => {
                let dt = (p.get_or(5, 1) - 1 + oy).min(rows - 1);
                let dl = (p.get_or(6, 1) - 1 + ox).min(cols - 1);
                self.copy_rect(top, left, bottom, right, dt, dl);
            }
            _ => {}
        }
    }
}

/// The erase range a parameter selects.
fn erase_range(ps: u16) -> EraseRange {
    match ps {
        1 => EraseRange::ToStart,
        2 | 3 => EraseRange::All,
        _ => EraseRange::ToEnd,
    }
}
