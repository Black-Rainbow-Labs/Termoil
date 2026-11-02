//! Deleting images and placements (`a=d`), every selector of the `d` key.
//!
//! A lowercase selector removes placements and keeps image data. An uppercase selector also frees an image
//! whose last placement it removed. Virtual placements (`U=1`) are only touched by the selectors `i`, `n`
//! and `r`; the others describe a position on screen, which a virtual placement does not have.

use super::{GraphicsContext, GraphicsResult, GraphicsStore, KittyCommand, Placement};

/// The last serial a placement covers.
fn last_serial(p: &Placement) -> u64 {
    p.serial + (p.rows as u64).max(1) - 1
}

/// Whether a real placement covers the cell (serial, column).
fn covers_cell(p: &Placement, serial: u64, col: i64) -> bool {
    let (c0, c1) = (p.col as i64, p.col as i64 + (p.cols as i64).max(1));
    !p.virtual_placement && p.serial <= serial && serial <= last_serial(p) && c0 <= col && col < c1
}

impl GraphicsStore {
    /// Handles `a=d`. Deletion never answers.
    pub(super) fn run_delete(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext) -> GraphicsResult {
        let before = self.generation;
        let selector = cmd.char_or(b'd', b'a');
        let free = selector.is_ascii_uppercase();
        match selector.to_ascii_lowercase() {
            b'i' | b'n' => self.delete_by_image(cmd, free),
            b'f' => self.delete_frames(cmd),
            b'r' => self.delete_id_range(cmd, free),
            b'a' | b'c' | b'p' | b'q' | b'x' | b'y' | b'z' => self.delete_by_position(cmd, ctx, selector, free),
            _ => {}
        }
        GraphicsResult { dirty: self.generation != before, ..GraphicsResult::default() }
    }

    /// Removes the placements the predicate selects, and with `free` also the images that lost their last one.
    fn delete_placements(&mut self, free: bool, pred: &dyn Fn(&Placement) -> bool) {
        let touched = self.remove_placements(pred);
        if !free {
            return;
        }
        for id in touched {
            if !self.placements.iter().any(|p| p.image_id == id) {
                self.remove_image(id);
            }
        }
    }

    /// `d=i` and `d=n`: the image with this id or newest with this number, or only its placement `p`.
    fn delete_by_image(&mut self, cmd: &KittyCommand, free: bool) {
        let target = if cmd.char_or(b'd', b'a').eq_ignore_ascii_case(&b'i') {
            self.index_of_id(cmd.uint(b'i'))
        } else {
            self.index_of_number(cmd.uint(b'I'))
        };
        let Some(index) = target.filter(|_| cmd.uint(b'i') != 0 || cmd.uint(b'I') != 0) else { return };
        let id = self.images[index].id;
        let placement = cmd.uint(b'p');
        if placement == 0 {
            self.remove_placements(&|p: &Placement| p.image_id == id);
            if free {
                self.remove_image(id);
            }
            return;
        }
        self.delete_placements(free, &|p: &Placement| p.image_id == id && p.placement_id == placement);
    }

    /// `d=r`: images whose id is in the range `x..=y`.
    fn delete_id_range(&mut self, cmd: &KittyCommand, free: bool) {
        let (lo, hi) = (cmd.uint(b'x'), cmd.uint(b'y'));
        if hi == 0 || lo > hi {
            return;
        }
        let ids: Vec<u32> = self.images.iter().map(|i| i.id).filter(|id| (lo..=hi).contains(id)).collect();
        for id in ids {
            self.remove_placements(&|p: &Placement| p.image_id == id);
            if free {
                self.remove_image(id);
            }
        }
    }

    /// The selectors that describe a place on screen: visible, cursor cell, cell, cell with z, column, row, z.
    fn delete_by_position(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext, selector: u8, free: bool) {
        let top = ctx.screen_top_serial;
        let bottom = top + (ctx.rows as u64).max(1) - 1;
        let col = (cmd.uint(b'x') as i64 - 1).max(0);
        let row_serial = top + (cmd.uint(b'y') as u64).saturating_sub(1);
        let z = cmd.int(b'z');
        let cursor = (ctx.cursor_serial, ctx.cursor_col as i64);
        let pred = move |p: &Placement| -> bool {
            if p.virtual_placement {
                return false;
            }
            match selector.to_ascii_lowercase() {
                b'a' => p.serial <= bottom && last_serial(p) >= top,
                b'c' => covers_cell(p, cursor.0, cursor.1),
                b'p' => covers_cell(p, row_serial, col),
                b'q' => covers_cell(p, row_serial, col) && p.z == z,
                b'x' => (p.col as i64) <= col && col < p.col as i64 + (p.cols as i64).max(1),
                b'y' => p.serial <= row_serial && row_serial <= last_serial(p),
                _ => p.z == z,
            }
        };
        self.delete_placements(free, &pred);
    }

    /// `d=f`: with `r` removes that frame (1 based), otherwise every frame but the first, of the image named by
    /// `i` or `I`, or of every image when neither is given.
    fn delete_frames(&mut self, cmd: &KittyCommand) {
        let named = cmd.uint(b'i') != 0 || cmd.uint(b'I') != 0;
        let targets: Vec<u32> = match (named, self.find_target(cmd)) {
            (true, Ok(i)) => vec![self.images[i].id],
            (true, Err(_)) => return,
            (false, _) => self.images.iter().map(|i| i.id).collect(),
        };
        let which = cmd.uint(b'r') as usize;
        for id in targets {
            self.drop_frames(id, which);
        }
    }

    /// Removes frame `which` (1 based) of an image, or all but the first when `which` is 0. The last frame
    /// is never removed.
    fn drop_frames(&mut self, id: u32, which: usize) {
        let Some(index) = self.index_of_id(id) else { return };
        let image = &mut self.images[index];
        let before = image.bytes();
        if which == 0 {
            image.frames.truncate(1);
        } else if which <= image.frames.len() && image.frames.len() > 1 {
            image.frames.remove(which - 1);
            image.rgba = image.frames[0].rgba.clone();
        }
        image.current_frame = image.current_frame.min(image.frames.len() - 1);
        image.animation.next_change_ms = None;
        let after = image.bytes();
        self.used = self.used - before + after;
        self.touch();
    }
}
