//! DCS and APC: status string requests, terminfo queries, tmux passthrough, sixel and kitty
//! graphics, and the glue between the graphics store and the cursor.

use super::{Event, MarkKind, Terminal};
use crate::graphics::{GraphicsContext, GraphicsResult};
use crate::parser::{CsiSeq, Parser};

impl Terminal {
    /// Dispatches a DCS sequence.
    pub(super) fn dcs_dispatch(&mut self, seq: &CsiSeq, data: &[u8]) {
        match (seq.private, seq.intermediates(), seq.final_byte) {
            (0, [b'$'], b'q') => self.decrqss(data),
            (0, [b'+'], b'q') => self.xtgettcap(data),
            (0, [], b'q') => {
                let params: Vec<u16> = seq.params.iter().map(|g| g[0]).collect();
                let ctx = self.graphics_context();
                let result = self.graphics.sixel(&params, data, &ctx);
                self.apply_graphics_result(result);
            }
            (0, [], b't') if data.starts_with(b"mux;") => self.tmux_passthrough(&data[4..]),
            _ => {}
        }
    }

    /// Applies a sequence tmux wrapped for the outer terminal, as if it had arrived directly.
    fn tmux_passthrough(&mut self, inner: &[u8]) {
        self.tmux_passthrough.push(inner.to_vec());
        let mut parser = Parser::new(self.config.parser_limits);
        parser.advance(inner, self);
    }

    /// Dispatches an APC payload; only kitty graphics (`G`) is recognised.
    pub(super) fn apc_dispatch(&mut self, data: &[u8]) {
        if let Some(payload) = data.strip_prefix(b"G") {
            let ctx = self.graphics_context();
            let result = self.graphics.kitty_command(payload, &ctx);
            self.apply_graphics_result(result);
        }
    }

    /// What the graphics store needs to know about the cursor and the screen.
    pub(super) fn graphics_context(&self) -> GraphicsContext {
        let s = self.screen();
        let pages = &s.pages;
        GraphicsContext {
            cursor_serial: pages.serial_of(pages.active_y(s.cursor.row)),
            cursor_col: s.cursor.col,
            screen_top_serial: pages.serial_of(pages.active_y(0)),
            first_serial: pages.first_serial(),
            cols: s.cols(),
            rows: s.rows(),
            cell_px: self.cell_pixel,
            allow_file_transfer: true,
            shared_memory: self.config.shared_memory,
        }
    }

    /// Sends the store's reply and moves the cursor past a placed image.
    pub(super) fn apply_graphics_result(&mut self, result: GraphicsResult) {
        if let Some(reply) = result.reply {
            self.reply(&reply);
        }
        if result.cursor_to_column_zero {
            self.screen_mut().cursor.col = 0;
            self.screen_mut().cursor.pending_wrap = false;
        }
        if let Some((cols, rows)) = result.move_cursor {
            // Sixel leaves the cursor on the line after the image. A kitty or iTerm2 image leaves
            // it on the image's last row, to the right of the image, which is what kitty itself
            // and Ghostty do (the specification's wording says "down by the number of rows").
            let down = if result.cursor_to_column_zero { rows } else { rows.saturating_sub(1) };
            for _ in 0..down {
                self.index();
            }
            let s = self.screen_mut();
            s.cursor.col = (s.cursor.col as u32 + cols as u32).min(s.cols() as u32 - 1) as u16;
        }
        if result.dirty {
            self.mark_all_dirty();
        }
    }

    /// Records an OSC 133 mark as a pin on the primary screen's cursor row.
    pub(super) fn add_mark(&mut self, kind: MarkKind, exit_code: Option<i32>) {
        if self.alt_active {
            return;
        }
        let row = self.primary.cursor.row;
        let col = self.primary.cursor.col;
        let y = self.primary.pages.active_y(row);
        let pin = self.primary.pages.pin_create(y, col);
        self.marks.push(super::Mark { pin, kind, exit_code });
        if self.marks.len() > self.config.max_marks {
            self.drop_stale_marks();
        }
    }

    /// Removes marks whose rows were pruned, and the oldest ones beyond the limit.
    fn drop_stale_marks(&mut self) {
        let pages = &mut self.primary.pages;
        let mut kept = Vec::with_capacity(self.marks.len());
        for m in self.marks.drain(..) {
            if pages.pin_get(m.pin).is_some() {
                kept.push(m);
            } else {
                pages.pin_remove(m.pin);
            }
        }
        while kept.len() > self.config.max_marks {
            let m = kept.remove(0);
            pages.pin_remove(m.pin);
        }
        self.marks = kept;
        let _ = Event::Dropped;
    }
}
