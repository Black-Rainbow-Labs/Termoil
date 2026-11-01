//! Animation: frame data (`a=f`), animation control (`a=a`), composition (`a=c`) and the clock.

use super::pixels::{Rect, blend_rect, filled_canvas};
use super::reply::KErr;
use super::{AnimationState, Frame, GraphicsContext, GraphicsResult, GraphicsStore, Image, KittyCommand};
use std::sync::Arc;

/// The gap in milliseconds a new frame gets when none is given.
const DEFAULT_GAP_MS: i32 = 40;

/// Most frames one image may hold.
const MAX_FRAMES: usize = 1024;

/// Runs `edit` on the pixels of one frame, keeping `Image::rgba` in step when the first frame changes.
fn edit_frame(image: &mut Image, index: usize, edit: impl FnOnce(&mut Vec<u8>)) {
    if index == 0 {
        image.rgba = Arc::new(Vec::new());
    }
    edit(Arc::make_mut(&mut image.frames[index].rgba));
    if index == 0 {
        image.rgba = image.frames[0].rgba.clone();
    }
}

/// Validates that a rectangle of size `w` by `h` at (x, y) lies inside a `width` by `height` image.
fn check_inside(x: u32, y: u32, w: u32, h: u32, width: u32, height: u32) -> Result<(), KErr> {
    let fits = x as u64 + w as u64 <= width as u64 && y as u64 + h as u64 <= height as u64;
    if w == 0 || h == 0 || !fits {
        return Err(KErr::new("EINVAL", "Rectangle is outside the image"));
    }
    Ok(())
}

/// Turns a 1 based frame number from the command into an index, or ENOENT.
fn frame_index(number: u32, count: usize) -> Result<usize, KErr> {
    match number as usize {
        n if n >= 1 && n <= count => Ok(n - 1),
        _ => Err(KErr::new("ENOENT", "Frame not found")),
    }
}

impl GraphicsStore {
    /// Handles `a=f`: adds a frame to an image or edits one.
    pub(super) fn run_frame(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext) -> GraphicsResult {
        let outcome = self.add_frame(cmd, ctx);
        let ok = outcome.is_ok();
        let assigned = outcome.as_ref().ok().copied().unwrap_or(0);
        let mut result = self.finish(cmd, assigned, outcome.map(|_| ()), None);
        result.dirty = ok;
        result
    }

    /// Decodes the frame data and applies it. Returns the id of the image changed.
    fn add_frame(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext) -> Result<u32, KErr> {
        let index = self.find_target(cmd)?;
        let (id, width, height) = (self.images[index].id, self.images[index].width, self.images[index].height);
        let data = self.load_pixels(cmd, Some((width, height)), ctx.allow_file_transfer)?;
        let at = (cmd.uint(b'x'), cmd.uint(b'y'));
        check_inside(at.0, at.1, data.width, data.height, width, height)?;
        let edit = cmd.uint(b'r');
        let count = self.images[index].frames.len();
        let target = if edit > 0 { Some(frame_index(edit, count)?) } else { None };
        let base = if cmd.uint(b'c') > 0 { Some(frame_index(cmd.uint(b'c'), count)?) } else { None };
        if target.is_none() {
            self.reserve_frame(index, id)?;
        }
        let index = self.index_of_id(id).ok_or_else(|| KErr::new("ENOENT", "Image not found"))?;
        self.apply_frame(index, cmd, &data, target, base, at);
        Ok(id)
    }

    /// Makes room for one more frame of the image, evicting other images if needed.
    fn reserve_frame(&mut self, index: usize, id: u32) -> Result<(), KErr> {
        if self.images[index].frames.len() >= MAX_FRAMES {
            return Err(KErr::new("ENOSPC", "Too many frames"));
        }
        let frame_bytes = self.images[index].width as usize * self.images[index].height as usize * 4;
        self.make_room(frame_bytes, Some(id))
    }

    /// Paints the frame data onto the right canvas and stores the result as a new or edited frame.
    fn apply_frame(
        &mut self, index: usize, cmd: &KittyCommand, data: &super::pixels::Decoded, target: Option<usize>,
        base: Option<usize>, at: (u32, u32),
    ) {
        let (width, height) = (self.images[index].width, self.images[index].height);
        let replace = cmd.uint(b'X') == 1;
        let to = Rect { x: at.0, y: at.1, w: data.width, h: data.height };
        let from = Rect { x: 0, y: 0, w: data.width, h: data.height };
        let gap = cmd.int(b'z');
        if let Some(t) = target {
            edit_frame(&mut self.images[index], t, |px| {
                blend_rect(px, width, to, &data.rgba, data.width, from, replace)
            });
            if gap != 0 {
                self.images[index].frames[t].gap_ms = gap;
            }
        } else {
            let mut canvas = match base {
                Some(b) => self.images[index].frames[b].rgba.as_ref().clone(),
                None => filled_canvas(width, height, cmd.uint(b'Y')),
            };
            blend_rect(&mut canvas, width, to, &data.rgba, data.width, from, replace);
            let gap_ms = if gap == 0 { DEFAULT_GAP_MS } else { gap };
            self.used += canvas.len();
            self.images[index].frames.push(Frame { rgba: Arc::new(canvas), gap_ms });
        }
        self.touch();
    }

    /// Handles `a=a`: animation state, loops, the gap of a frame, and the current frame.
    pub(super) fn run_animation_control(&mut self, cmd: &KittyCommand) -> GraphicsResult {
        let outcome = self.control_animation(cmd);
        let assigned = self.find_target(cmd).map(|i| self.images[i].id).unwrap_or(0);
        let ok = outcome.is_ok();
        let mut result = self.finish(cmd, assigned, outcome, None);
        result.dirty = ok;
        result
    }

    /// Applies the animation control keys to the image.
    fn control_animation(&mut self, cmd: &KittyCommand) -> Result<(), KErr> {
        let index = self.find_target(cmd)?;
        let count = self.images[index].frames.len();
        let current = if cmd.uint(b'c') > 0 { Some(frame_index(cmd.uint(b'c'), count)?) } else { None };
        let gap_target = if cmd.uint(b'r') > 0 { Some(frame_index(cmd.uint(b'r'), count)?) } else { None };
        let image = &mut self.images[index];
        let gap = cmd.int(b'z');
        if let Some(t) = gap_target.filter(|_| gap != 0) {
            image.frames[t].gap_ms = gap;
        }
        match cmd.uint(b'v') {
            0 => {}
            1 => image.animation.loops = 0,
            n => image.animation.loops = n - 1,
        }
        if let Some(c) = current {
            image.current_frame = c;
            image.animation.next_change_ms = None;
        }
        set_state(image, cmd.uint(b's'));
        self.touch();
        Ok(())
    }

    /// Handles `a=c`: composes a rectangle of one frame onto another.
    pub(super) fn run_compose(&mut self, cmd: &KittyCommand) -> GraphicsResult {
        let outcome = self.compose_frames(cmd);
        let assigned = self.find_target(cmd).map(|i| self.images[i].id).unwrap_or(0);
        let ok = outcome.is_ok();
        let mut result = self.finish(cmd, assigned, outcome, None);
        result.dirty = ok;
        result
    }

    /// Validates and performs a composition.
    fn compose_frames(&mut self, cmd: &KittyCommand) -> Result<(), KErr> {
        let index = self.find_target(cmd)?;
        let (width, height) = (self.images[index].width, self.images[index].height);
        let count = self.images[index].frames.len();
        let src_frame = frame_index(cmd.uint(b'r'), count)?;
        let dst_frame = frame_index(cmd.uint(b'c'), count)?;
        let w = if cmd.uint(b'w') == 0 { width } else { cmd.uint(b'w') };
        let h = if cmd.uint(b'h') == 0 { height } else { cmd.uint(b'h') };
        let from = Rect { x: cmd.uint(b'X'), y: cmd.uint(b'Y'), w, h };
        let to = Rect { x: cmd.uint(b'x'), y: cmd.uint(b'y'), w, h };
        check_inside(from.x, from.y, w, h, width, height)?;
        check_inside(to.x, to.y, w, h, width, height)?;
        if src_frame == dst_frame && rects_overlap(&from, &to) {
            return Err(KErr::new("EINVAL", "Source and destination rectangles overlap"));
        }
        let source = self.images[index].frames[src_frame].rgba.clone();
        let replace = cmd.uint(b'C') == 1;
        edit_frame(&mut self.images[index], dst_frame, |px| blend_rect(px, width, to, &source, width, from, replace));
        self.touch();
        Ok(())
    }

    /// Steps running animations to `now_ms`. Returns true when a current frame changed.
    pub fn advance_animations(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for image in &mut self.images {
            changed |= step_animation(image, now_ms);
        }
        if changed {
            self.touch();
        }
        changed
    }
}

/// Whether two rectangles share any pixel.
fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Applies the `s` key: 1 stops (and resets the loop counter), 2 runs waiting for frames, 3 runs and loops.
fn set_state(image: &mut Image, state: u32) {
    let animation = &mut image.animation;
    match state {
        1 => {
            animation.state = AnimationState::Stopped;
            animation.loops_done = 0;
            animation.next_change_ms = None;
        }
        2 | 3 => {
            animation.state = if state == 2 { AnimationState::Loading } else { AnimationState::Running };
            animation.next_change_ms = None;
        }
        _ => {}
    }
}

/// Milliseconds a frame stays on screen: its gap, or 0 for a gapless frame.
fn delay_of(image: &Image, frame: usize) -> u64 {
    image.frames[frame].gap_ms.max(0) as u64
}

/// Advances one image's animation to `now_ms`, following gaps, loops and the loading state.
fn step_animation(image: &mut Image, now_ms: u64) -> bool {
    if image.animation.state == AnimationState::Stopped || image.frames.len() < 2 {
        return false;
    }
    let mut changed = false;
    for _ in 0..image.frames.len() * 2 + 2 {
        let due = match image.animation.next_change_ms {
            Some(t) => t,
            None => now_ms + delay_of(image, image.current_frame),
        };
        image.animation.next_change_ms = Some(due);
        if now_ms < due || !advance_one(image) {
            break;
        }
        changed = true;
        image.animation.next_change_ms = Some(due + delay_of(image, image.current_frame));
    }
    changed
}

/// Moves to the next frame, looping or stopping at the end. Returns false when it stays on the same frame.
fn advance_one(image: &mut Image) -> bool {
    let last = image.current_frame + 1 >= image.frames.len();
    if !last {
        image.current_frame += 1;
        return true;
    }
    if image.animation.state == AnimationState::Loading {
        return false;
    }
    image.animation.loops_done += 1;
    if image.animation.loops != 0 && image.animation.loops_done >= image.animation.loops {
        image.animation.state = AnimationState::Stopped;
        return false;
    }
    image.current_frame = 0;
    true
}
