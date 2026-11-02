//! Image storage with quota eviction, and placement creation including relative placements.

use super::reply::KErr;
use super::{Animation, Frame, GraphicsStore, KittyCommand};
use super::{GraphicsContext, Image, Placement};
use std::sync::Arc;

/// Longest chain of relative placements: a placement may have this many ancestors.
pub(super) const MAX_RELATIVE_DEPTH: usize = 8;

/// An image ready to be stored.
pub(super) struct NewImage {
    /// The client's image id, 0 if none.
    pub id: u32,
    /// The client's image number, 0 if none.
    pub number: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA pixels.
    pub rgba: Vec<u8>,
    /// Stays when its placements go away.
    pub persistent: bool,
    /// Usage hint `N=1`.
    pub transient: bool,
}

/// The placement keys of a command, with defaults filled in.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PlaceParams {
    pub placement_id: u32,
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
    pub x_offset: u32,
    pub y_offset: u32,
    pub cols: u32,
    pub rows: u32,
    pub no_move: bool,
    pub virtual_placement: bool,
    pub z: i32,
    pub parent_image: u32,
    pub parent_placement: u32,
    pub h_offset: i32,
    pub v_offset: i32,
    pub stretch: bool,
}

impl PlaceParams {
    /// Reads the placement keys (`p x y w h X Y c r C U z P Q H V`) from a command.
    pub fn from_command(cmd: &KittyCommand) -> PlaceParams {
        PlaceParams {
            placement_id: cmd.uint(b'p'),
            src_x: cmd.uint(b'x'),
            src_y: cmd.uint(b'y'),
            src_w: cmd.uint(b'w'),
            src_h: cmd.uint(b'h'),
            x_offset: cmd.uint(b'X'),
            y_offset: cmd.uint(b'Y'),
            cols: cmd.uint(b'c'),
            rows: cmd.uint(b'r'),
            no_move: cmd.uint(b'C') == 1,
            virtual_placement: cmd.uint(b'U') == 1,
            z: cmd.int(b'z'),
            parent_image: cmd.uint(b'P'),
            parent_placement: cmd.uint(b'Q'),
            h_offset: cmd.int(b'H'),
            v_offset: cmd.int(b'V'),
            stretch: false,
        }
    }
}

/// The result of creating a placement.
pub(super) struct Placed {
    /// The image placed.
    pub image_id: u32,
    /// The placement id that was stored (0 for an anonymous image).
    pub placement_id: u32,
    /// Cursor movement due, if any.
    pub move_cursor: Option<(u16, u16)>,
    /// Whether the placement is visible (not virtual).
    pub visible: bool,
}

/// Integer division rounding up.
fn ceil_div(a: u64, b: u64) -> u64 {
    a.div_ceil(b.max(1))
}

/// Clamps a cell count to what fits a `u16` and is at least 1.
fn cells(n: u64) -> u16 {
    n.clamp(1, u16::MAX as u64) as u16
}

/// Clips the requested source rectangle to the image. An empty rectangle is an error.
fn resolve_source(p: &PlaceParams, w: u32, h: u32) -> Result<(u32, u32, u32, u32), KErr> {
    if p.src_x >= w || p.src_y >= h {
        return Err(KErr::new("EINVAL", "Source rectangle is outside the image"));
    }
    let sw = if p.src_w == 0 { w - p.src_x } else { p.src_w.min(w - p.src_x) };
    let sh = if p.src_h == 0 { h - p.src_y } else { p.src_h.min(h - p.src_y) };
    Ok((p.src_x, p.src_y, sw, sh))
}

/// Works out how many cells a placement covers. With neither `c` nor `r` the source is shown at its own size
/// from the pixel offset; with one of them the other follows the aspect ratio; with both they are used as given.
pub(super) fn layout_cells(p: &PlaceParams, src: (u32, u32, u32, u32), cell_px: (u16, u16)) -> (u16, u16) {
    let (cw, ch) = (cell_px.0.max(1) as u64, cell_px.1.max(1) as u64);
    let (sw, sh) = (src.2.max(1) as u64, src.3.max(1) as u64);
    match (p.cols, p.rows) {
        (0, 0) => (cells(ceil_div(sw + p.x_offset as u64, cw)), cells(ceil_div(sh + p.y_offset as u64, ch))),
        (c, 0) => (cells(c as u64), cells(ceil_div(c as u64 * cw * sh, sw * ch))),
        (0, r) => (cells(ceil_div(r as u64 * ch * sw, sh * cw)), cells(r as u64)),
        (c, r) => (cells(c as u64), cells(r as u64)),
    }
}

impl GraphicsStore {
    /// Index of the image with this id.
    pub(super) fn index_of_id(&self, id: u32) -> Option<usize> {
        self.images.iter().position(|i| i.id == id)
    }

    /// Index of the newest image with this image number.
    pub(super) fn index_of_number(&self, number: u32) -> Option<usize> {
        self.images.iter().rposition(|i| i.number == number)
    }

    /// Finds the image a command names with `i` or `I`. Using both is an error.
    pub(super) fn find_target(&self, cmd: &KittyCommand) -> Result<usize, KErr> {
        let (id, number) = (cmd.uint(b'i'), cmd.uint(b'I'));
        if id != 0 && number != 0 {
            return Err(KErr::new("EINVAL", "Cannot specify both i and I"));
        }
        let found = if id != 0 { self.index_of_id(id) } else { self.index_of_number(number) };
        match (id | number, found) {
            (0, _) => Err(KErr::new("EINVAL", "An image id or number is required")),
            (_, None) => Err(KErr::new("ENOENT", "Image not found")),
            (_, Some(i)) => Ok(i),
        }
    }

    /// Removes an image, its placements, and the relative placements that depended on them.
    pub(super) fn remove_image(&mut self, id: u32) {
        if let Some(i) = self.index_of_id(id) {
            let image = self.images.remove(i);
            self.used = self.used.saturating_sub(image.bytes());
            self.placements.retain(|p| p.image_id != id);
            self.cascade_orphans();
            self.touch();
        }
    }

    /// Removes every placement the predicate selects, plus relative placements left without a parent.
    /// Returns the ids of the images that lost a placement.
    pub(super) fn remove_placements(&mut self, pred: &dyn Fn(&Placement) -> bool) -> Vec<u32> {
        let mut touched: Vec<u32> = self.placements.iter().filter(|p| pred(p)).map(|p| p.image_id).collect();
        if touched.is_empty() {
            return touched;
        }
        self.placements.retain(|p| !pred(p));
        touched.extend(self.cascade_orphans());
        touched.sort_unstable();
        touched.dedup();
        self.touch();
        touched
    }

    /// Index of the placement a (parent image, parent placement) pair names: an exact match when the
    /// placement id is non-zero, otherwise the newest placement of the image.
    pub(super) fn find_parent(&self, image: u32, placement: u32) -> Option<usize> {
        let hit = |p: &Placement| p.image_id == image && (placement == 0 || p.placement_id == placement);
        self.placements.iter().rposition(hit)
    }

    /// Deletes relative placements whose parent is gone, repeatedly, then deletes images that lost a relative
    /// placement this way and have no placement left ("a parent and its relatives are managed together").
    /// Returns the images that lost placements.
    fn cascade_orphans(&mut self) -> Vec<u32> {
        let mut affected = Vec::new();
        loop {
            let orphans: Vec<u64> = self
                .placements
                .iter()
                .filter(|p| p.parent.is_some_and(|(i, q)| self.find_parent(i, q).is_none()))
                .map(|p| p.uid)
                .collect();
            if orphans.is_empty() {
                break;
            }
            affected.extend(self.placements.iter().filter(|p| orphans.contains(&p.uid)).map(|p| p.image_id));
            self.placements.retain(|p| !orphans.contains(&p.uid));
        }
        affected.sort_unstable();
        affected.dedup();
        for id in affected.clone() {
            if !self.placements.iter().any(|p| p.image_id == id) {
                self.remove_image(id);
            }
        }
        affected
    }

    /// Evicts images, oldest first, until `need` more bytes fit the quota. Images without placements go before
    /// images with placements, and transient ones first among those. The image `keep` is never evicted.
    pub(super) fn make_room(&mut self, need: usize, keep: Option<u32>) -> Result<(), KErr> {
        if need > self.quota {
            return Err(KErr::new("ENOSPC", "Image is larger than the storage quota"));
        }
        while self.used + need > self.quota {
            let victim = self.pick_victim(keep).ok_or_else(|| KErr::new("ENOSPC", "Not enough storage space"))?;
            self.remove_image(victim);
        }
        Ok(())
    }

    /// Chooses the image to evict next, or `None` when only `keep` is left.
    fn pick_victim(&self, keep: Option<u32>) -> Option<u32> {
        let free = |i: &&Image| Some(i.id) != keep;
        let unplaced = |i: &&Image| !self.placements.iter().any(|p| p.image_id == i.id);
        let candidates = || self.images.iter().filter(free);
        let transient = candidates().find(|i| i.transient && unplaced(i));
        let plain = || candidates().find(unplaced);
        transient.or_else(plain).or_else(|| candidates().next()).map(|i| i.id)
    }

    /// Picks an unused id: ascending from 1 for image numbers, descending from the top for anonymous images.
    fn alloc_id(&mut self, anonymous: bool) -> u32 {
        loop {
            let id = if anonymous { self.next_anonymous_id } else { self.next_number_id };
            if anonymous {
                self.next_anonymous_id = id.checked_sub(1).filter(|n| *n != 0).unwrap_or(u32::MAX);
            } else {
                self.next_number_id = id.checked_add(1).unwrap_or(1);
            }
            if id != 0 && self.index_of_id(id).is_none() {
                return id;
            }
        }
    }

    /// Stores a new image, replacing any image with the same id (and deleting its placements), and evicting
    /// others to stay inside the quota. Returns the id the image has.
    pub(super) fn insert_image(&mut self, new: NewImage) -> Result<u32, KErr> {
        if new.id != 0 {
            self.remove_image(new.id);
        }
        self.make_room(new.rgba.len(), None)?;
        let anonymous = new.id == 0 && new.number == 0;
        let id = if new.id != 0 { new.id } else { self.alloc_id(anonymous) };
        let root = Arc::new(new.rgba);
        let frame = Frame { rgba: root.clone(), gap_ms: 0 };
        self.used += root.len();
        self.images.push(Image {
            id,
            number: new.number,
            width: new.width,
            height: new.height,
            rgba: root,
            frames: vec![frame],
            current_frame: 0,
            animation: Animation::default(),
            anonymous,
            transient: new.transient,
            persistent: new.persistent,
        });
        Ok(id)
    }

    /// Creates a placement of an image at the cursor (or relative to a parent) and says how the cursor moves.
    pub(super) fn place_image(
        &mut self, image_id: u32, params: &PlaceParams, ctx: &GraphicsContext,
    ) -> Result<Placed, KErr> {
        let image = self.image(image_id).ok_or_else(|| KErr::new("ENOENT", "Image not found"))?;
        let (width, height, anonymous) = (image.width, image.height, image.anonymous);
        let src = resolve_source(params, width, height)?;
        let (cols, rows) = layout_cells(params, src, ctx.cell_px);
        let placement_id = if anonymous { 0 } else { params.placement_id };
        let relative = params.parent_image != 0;
        if params.virtual_placement && relative {
            return Err(KErr::new("EINVAL", "A virtual placement cannot be relative"));
        }
        if relative {
            self.check_parent(image_id, placement_id, params)?;
        }
        if placement_id != 0 {
            self.placements.retain(|p| !(p.image_id == image_id && p.placement_id == placement_id));
        }
        let cell = (ctx.cell_px.0.max(1) as u32, ctx.cell_px.1.max(1) as u32);
        let uid = self.next_uid();
        self.placements.push(make_placement(image_id, placement_id, params, src, (cols, rows), cell, ctx, uid));
        self.refresh_relatives();
        let visible = !params.virtual_placement;
        if visible {
            self.touch();
        }
        let moves = visible && !relative && !params.no_move;
        Ok(Placed { image_id, placement_id, move_cursor: moves.then_some((cols, rows)), visible })
    }

    /// Checks the parent of a new relative placement exists and the chain has no cycle and is not too deep.
    fn check_parent(&self, image_id: u32, placement_id: u32, params: &PlaceParams) -> Result<(), KErr> {
        let mut current = self
            .find_parent(params.parent_image, params.parent_placement)
            .ok_or_else(|| KErr::new("ENOPARENT", "Parent placement not found"))?;
        let mut depth = 1;
        loop {
            let p = &self.placements[current];
            if placement_id != 0 && p.image_id == image_id && p.placement_id == placement_id {
                return Err(KErr::new("ECYCLE", "Relative placement would create a cycle"));
            }
            if depth > MAX_RELATIVE_DEPTH {
                return Err(KErr::new("ETOODEEP", "Chain of relative placements is too deep"));
            }
            match p.parent.and_then(|(i, q)| self.find_parent(i, q)) {
                Some(next) => current = next,
                None => return Ok(()),
            }
            depth += 1;
        }
    }

    /// Recomputes the position of every relative placement from its parent's position, parents first.
    pub(super) fn refresh_relatives(&mut self) {
        if !self.placements.iter().any(|p| p.parent.is_some()) {
            return;
        }
        for _ in 0..=MAX_RELATIVE_DEPTH + 1 {
            for i in 0..self.placements.len() {
                let Some((pi, pq)) = self.placements[i].parent else { continue };
                let Some(parent) = self.find_parent(pi, pq) else { continue };
                let (serial, col) = (self.placements[parent].serial as i64, self.placements[parent].col as i64);
                let (dx, dy) = self.placements[i].offset_from_parent;
                self.placements[i].serial = (serial + dy as i64).max(0) as u64;
                self.placements[i].col = (col + dx as i64).clamp(0, u16::MAX as i64) as u16;
            }
        }
    }
}

/// Builds the placement record.
fn make_placement(
    image_id: u32, placement_id: u32, p: &PlaceParams, src: (u32, u32, u32, u32), size: (u16, u16), cell: (u32, u32),
    ctx: &GraphicsContext, uid: u64,
) -> Placement {
    let relative = p.parent_image != 0;
    Placement {
        image_id,
        placement_id,
        serial: if p.virtual_placement { 0 } else { ctx.cursor_serial },
        col: if p.virtual_placement { 0 } else { ctx.cursor_col },
        z: p.z,
        x_offset: p.x_offset.min(cell.0 - 1),
        y_offset: p.y_offset.min(cell.1 - 1),
        src,
        cols: size.0,
        rows: size.1,
        virtual_placement: p.virtual_placement,
        parent: relative.then_some((p.parent_image, p.parent_placement)),
        offset_from_parent: if relative { (p.h_offset, p.v_offset) } else { (0, 0) },
        stretch: p.stretch,
        uid,
    }
}
