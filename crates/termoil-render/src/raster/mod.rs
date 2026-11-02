//! `RasterRenderer`: draws frames straight into a 0RGB framebuffer on the CPU, with no GPU device
//! and no glyph atlas, and redraws only the rows that changed since the last frame (task-2481,
//! `docs/decisions/efficiency.md`).
//!
//! The GPU renderer costs 300 MB or more of driver memory on the machines measured, and
//! `vello_cpu` with its glyph atlas 64 MiB more for the atlas page. This renderer keeps a coverage
//! mask per glyph (`glyphs`), fills rectangles and paths itself (`fill`), and draws through the same
//! [`Canvas`] calls as the vello renderers, so the frame builder and the drawing order are shared.
//!
//! Damage: the renderer remembers each layer's rows, cursor and geometry. A row equal to the one
//! drawn at the same place last time is skipped; the rest are redrawn clipped to their own band.
//! Anything it does not track row by row (a different set of layers, images, shapes, a translucent
//! colour, a resize) redraws the whole window.

mod fill;
mod glyphs;
mod scroll;

use std::time::Instant;

use termoil_font::FontGrid;
use vello_common::kurbo::{Affine, BezPath, Rect};
use vello_common::peniko::FontData;

use crate::draw::{Canvas, Layer, draw_cursor, draw_row, draw_run, hint_enabled};
use crate::frame::{CursorFrame, Frame, GlyphRun, ImageRecord, Rgba, RowFrame};
use crate::{FrameStats, Image, RenderError, Renderer};

use fill::Target;
use glyphs::{GlyphCache, Pixels};

/// Glyphs the cache keeps before it starts again; about a kilobyte each at 13 points.
pub const DEFAULT_GLYPH_CAPACITY: usize = 4096;

/// What was drawn for one layer, to compare the next frame against.
#[derive(Clone, Debug, PartialEq)]
struct Memo {
    x: f32,
    y: f32,
    width: u32,
    height: u32,
    background: Rgba,
    rows: Vec<RowFrame>,
    cursor: Option<CursorFrame>,
    cursor_text: Option<GlyphRun>,
}

impl Memo {
    /// The memo of a layer as drawn.
    fn of(l: &Layer<'_>) -> Memo {
        let f = l.frame;
        Memo {
            x: l.x,
            y: l.y,
            width: f.width,
            height: f.height,
            background: f.background,
            rows: f.rows.clone(),
            cursor: f.cursor,
            cursor_text: f.cursor_text.clone(),
        }
    }

    /// The same place and size, the same colour, and nothing drawn that rows do not track.
    fn same_geometry(&self, l: &Layer<'_>) -> bool {
        let f = l.frame;
        self.x == l.x
            && self.y == l.y
            && self.width == f.width
            && self.height == f.height
            && self.background == f.background
    }
}

/// The CPU renderer. It draws into a framebuffer the caller passes in (the window's own buffer,
/// so the pixels are not held twice), or into one of its own through the [`Renderer`] trait.
pub struct RasterRenderer {
    /// The framebuffer the [`Renderer`] calls draw into; empty until one is made.
    own: Vec<u32>,
    width: u32,
    height: u32,
    glyphs: GlyphCache,
    hint: bool,
    memos: Vec<Memo>,
    background: Option<Rgba>,
    /// Horizontal bands (top, bottom) changed by the last frame, in window pixels.
    damage: Vec<(u32, u32)>,
}

impl RasterRenderer {
    /// A renderer for a `width` by `height` window that caches up to `glyph_capacity` glyphs.
    pub fn new(width: u32, height: u32, glyph_capacity: usize) -> RasterRenderer {
        let (width, height) = (width.max(1), height.max(1));
        RasterRenderer {
            own: Vec::new(),
            width,
            height,
            glyphs: GlyphCache::new(glyph_capacity),
            hint: hint_enabled(),
            memos: Vec::new(),
            background: None,
            damage: Vec::new(),
        }
    }

    /// The pixels the [`Renderer`] calls drew, as 0RGB words, row major.
    pub fn pixels(&self) -> &[u32] {
        &self.own
    }

    /// The window size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The bands the last frame changed, top and bottom in pixels; the whole window after a full redraw.
    pub fn damage(&self) -> &[(u32, u32)] {
        &self.damage
    }

    /// Frees the framebuffer the [`Renderer`] calls drew into, for a renderer that will draw into
    /// a buffer its caller owns from now on.
    pub fn free_framebuffer(&mut self) {
        self.own = Vec::new();
        self.memos.clear();
    }

    /// Glyphs in the cache (for tests).
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.len()
    }

    /// Draws the layers into `px`, a `width` by `height` framebuffer of 0RGB words (see `size`).
    /// `valid` says `px` still holds the last frame this renderer drew; when it does not, the frame
    /// is drawn whole. Otherwise only the rows that changed are drawn; `damage` says which bands.
    pub fn render_into(
        &mut self, px: &mut [u32], valid: bool, background: Rgba, layers: &[Layer<'_>], grid: &FontGrid,
    ) -> FrameStats {
        let start = Instant::now();
        assert!(px.len() >= self.width as usize * self.height as usize, "the framebuffer is smaller than the renderer");
        if !valid {
            self.memos.clear();
        }
        if self.needs_full(background, layers) {
            self.draw_full(px, background, layers, grid);
        } else {
            self.draw_damage(px, layers, grid);
        }
        self.memos = layers.iter().map(Memo::of).collect();
        self.background = Some(background);
        let glyphs = layers.iter().map(|l| l.frame.glyph_count()).sum();
        let total = start.elapsed();
        FrameStats { encode: total, total, glyphs, presented: true }
    }

    /// A canvas over the whole of `px`.
    fn canvas<'a>(&'a mut self, px: &'a mut [u32], grid: &'a FontGrid) -> RasterCanvas<'a> {
        let (w, h) = (self.width as usize, self.height as usize);
        RasterCanvas {
            target: Target { px, width: w, height: h, clip: (0, 0, w as i32, h as i32) },
            tf: Affine::IDENTITY,
            paint: Rgba::default(),
            glyphs: &mut self.glyphs,
            grid,
            hint: self.hint,
        }
    }

    /// Whether this frame must be drawn whole: the layers or their geometry changed, or a layer has
    /// something the row comparison does not track.
    fn needs_full(&self, background: Rgba, layers: &[Layer<'_>]) -> bool {
        self.background != Some(background)
            || self.memos.len() != layers.len()
            || layers.iter().zip(&self.memos).any(|(l, m)| !m.same_geometry(l) || untracked(l.frame))
    }

    /// Draws everything.
    fn draw_full(&mut self, px: &mut [u32], background: Rgba, layers: &[Layer<'_>], grid: &FontGrid) {
        let (w, h) = (self.width, self.height);
        let mut c = self.canvas(px, grid);
        crate::draw::draw_layers(&mut c, (w, h), opaque(background), layers, grid);
        self.damage = vec![(0, h)];
    }

    /// Redraws the rows of each layer that differ from what was drawn there, and the cursor. When
    /// the rows scrolled, their pixels are moved first and compared at their new places.
    fn draw_damage(&mut self, px: &mut [u32], layers: &[Layer<'_>], grid: &FontGrid) {
        let mut memos = std::mem::take(&mut self.memos);
        let mut damage = Vec::new();
        for (l, m) in layers.iter().zip(&mut memos) {
            let mut bands = Vec::new();
            if let Some(moved) = self.scroll_layer(px, l, m) {
                damage.push(moved);
                // The old cursor moved with its row: redraw where it went.
                bands.extend(m.cursor.map(|c| (c.y.floor() as i32, (c.y + c.height).ceil() as i32)));
            }
            bands.extend(changed_bands(l.frame, m));
            bands.sort_unstable();
            bands.dedup();
            let mut c = self.canvas(px, grid);
            for &(y0, y1) in &bands {
                redraw_band(&mut c, l, (y0, y1), grid);
                let top = (l.y.round() + y0 as f32).max(0.0) as u32;
                damage.push((top, top + (y1 - y0).max(0) as u32));
            }
        }
        self.damage = damage;
    }

    /// When a layer's rows scrolled up since `m`, moves their pixels up and changes `m` to describe
    /// the moved rows (and the cursor, which moved with them). Returns the window band changed.
    fn scroll_layer(&mut self, px: &mut [u32], l: &Layer<'_>, m: &mut Memo) -> Option<(u32, u32)> {
        let f = l.frame;
        let k = scroll::scrolled_rows(f, &m.rows);
        if k == 0 {
            return None;
        }
        let (ox, oy) = (l.x.round().max(0.0) as usize, l.y.round().max(0.0) as usize);
        let (ch, n) = (f.cell_height as usize, f.rows.len());
        let top = oy + f.rows[0].y.max(0.0) as usize;
        let x1 = (ox + f.width as usize).min(self.width as usize);
        let lines = ((n - k) * ch).min((self.height as usize).saturating_sub(top + k * ch));
        scroll::move_up(px, self.width as usize, (ox, x1), top + k * ch, top, lines);
        let dy = (k * ch) as f32;
        // Row i now shows what row i + k showed; the last k rows show nothing tracked.
        m.rows.drain(..k);
        m.rows.iter_mut().zip(&f.rows).for_each(|(r, now)| *r = now.clone());
        m.rows.truncate(n - k);
        m.cursor = m.cursor.map(|c| CursorFrame { y: c.y - dy, ..c });
        Some((top as u32, (top + n * ch) as u32))
    }
}

/// The colour with full alpha: the framebuffer has none.
fn opaque(c: Rgba) -> Rgba {
    Rgba { a: 255, ..c }
}

/// A layer the row comparison cannot redraw piecewise: images, shapes or a translucent colour.
fn untracked(f: &Frame) -> bool {
    f.background.a != 255 || !f.images_above.is_empty() || f.has_images_under_text()
}

/// The vertical bands of a layer (frame pixels, top and bottom) that must be redrawn: rows that
/// differ from the memo, and the rows under the old and the new cursor when the cursor changed.
fn changed_bands(f: &Frame, m: &Memo) -> Vec<(i32, i32)> {
    let ch = f.cell_height as i32;
    let mut bands: Vec<(i32, i32)> = Vec::new();
    for (i, row) in f.rows.iter().enumerate() {
        if m.rows.get(i) != Some(row) {
            bands.push((row.y as i32, row.y as i32 + ch));
        }
    }
    if m.cursor != f.cursor || m.cursor_text != f.cursor_text {
        for cur in [m.cursor, f.cursor].into_iter().flatten() {
            bands.push((cur.y.floor() as i32, (cur.y + cur.height).ceil() as i32));
        }
    }
    bands.sort_unstable();
    bands.dedup();
    bands
}

/// Redraws the band `y0..y1` of a layer: the rows that touch it and the cursor, clipped to it.
fn redraw_band(c: &mut RasterCanvas<'_>, l: &Layer<'_>, (y0, y1): (i32, i32), grid: &FontGrid) {
    let f = l.frame;
    let (ox, oy) = (l.x.round() as i32, l.y.round() as i32);
    c.target.clip = (ox, oy + y0, ox + f.width as i32, oy + y1);
    c.tf = Affine::translate((f64::from(ox), f64::from(oy)));
    let ch = f.cell_height as f32;
    for row in f.rows.iter().filter(|r| (r.y as i32) < y1 && (r.y + ch) as i32 > y0) {
        draw_row(c, row, f, grid);
    }
    if let Some(cursor) = f.cursor {
        draw_cursor(c, &cursor, f);
        if let Some(text) = &f.cursor_text {
            draw_run(c, text, grid);
        }
    }
    let (w, h) = (c.target.width as i32, c.target.height as i32);
    c.target.clip = (0, 0, w, h);
    c.tf = Affine::IDENTITY;
}

impl Renderer for RasterRenderer {
    /// Changes the size drawn; the next frame is drawn whole.
    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) != (self.width, self.height) {
            (self.width, self.height) = (w, h);
            self.memos.clear();
        }
    }

    /// Draws the layers into the renderer's own framebuffer: the rows that changed, or everything
    /// when the layers changed.
    fn render_layers(
        &mut self, background: Rgba, layers: &[Layer<'_>], grid: &FontGrid,
    ) -> Result<FrameStats, RenderError> {
        let mut own = std::mem::take(&mut self.own);
        let size = self.width as usize * self.height as usize;
        let valid = own.len() == size;
        if !valid {
            own = vec![0; size];
        }
        let stats = self.render_into(&mut own, valid, background, layers, grid);
        self.own = own;
        Ok(stats)
    }

    /// The framebuffer as RGBA8.
    fn read_pixels(&mut self) -> Result<Image, RenderError> {
        let rgba = self.own.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]).collect();
        Ok(Image { width: self.width, height: self.height, rgba })
    }

    /// The renderer's name.
    fn name(&self) -> String {
        "raster".to_string()
    }

    /// Turns glyph hinting on or off; glyphs already cached keep their hinting under their own key.
    fn set_hinting(&mut self, on: bool) {
        if self.hint != on {
            self.hint = on;
            self.memos.clear();
        }
    }
}

/// The canvas the shared drawing code draws through.
struct RasterCanvas<'a> {
    target: Target<'a>,
    tf: Affine,
    paint: Rgba,
    glyphs: &'a mut GlyphCache,
    grid: &'a FontGrid,
    hint: bool,
}

impl Canvas for RasterCanvas<'_> {
    /// Sets the paint colour.
    fn paint(&mut self, c: Rgba) {
        self.paint = c;
    }

    /// Fills a rectangle: directly when the transform only moves it, as a path otherwise.
    fn rect(&mut self, r: Rect) {
        let [a, b, c, d, e, f] = self.tf.as_coeffs();
        if a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 {
            self.target.fill_rect(Rect::new(r.x0 + e, r.y0 + f, r.x1 + e, r.y1 + f), self.paint);
        } else {
            self.path(&vello_common::kurbo::Shape::to_path(&r, 0.1));
        }
    }

    /// Fills a path.
    fn path(&mut self, p: &BezPath) {
        let moved = self.tf * p.clone();
        self.target.fill_path(&moved, self.paint);
    }

    /// Draws each glyph from the cache.
    fn glyphs(&mut self, font: &FontData, coords: &[i16], run: &GlyphRun) {
        let [_, _, _, _, ox, oy] = self.tf.as_coeffs();
        let color = self.grid.face(run.face).color;
        for g in &run.glyphs {
            let at = crate::frame::GlyphPos { x: g.x + ox as f32, y: g.y + oy as f32, ..*g };
            let (mask, x, y) = self.glyphs.place(font, coords, run, &at, self.hint, color);
            let (left, top) = (x + mask.left, y + mask.top);
            match &mask.pixels {
                Pixels::Coverage(m) => self.target.fill_mask(m, mask.width, mask.height, left, top, self.paint),
                Pixels::Color(px) => self.target.draw_premultiplied(px, mask.width, mask.height, left, top),
            }
        }
    }

    /// Sets the transform.
    fn transform(&mut self, t: Affine) {
        self.tf = t;
    }

    /// Draws an image stretched over its rectangle, nearest pixel.
    fn image(&mut self, rec: &ImageRecord) {
        draw_image(&mut self.target, self.tf, rec);
    }

    /// Nothing is cached per image: images are read from the record each frame.
    fn retain_images(&mut self, _live: &[(u32, u64)]) {}
}

/// Draws `rec`'s source rectangle over its destination (moved by `tf`), sampling the nearest pixel.
fn draw_image(t: &mut Target<'_>, tf: Affine, rec: &ImageRecord) {
    let [_, _, _, _, ox, oy] = tf.as_coeffs();
    let img = &rec.image;
    let (iw, ih) = (img.width as usize, img.height as usize);
    if iw == 0 || ih == 0 || img.rgba.len() < iw * ih * 4 {
        return;
    }
    let (dx0, dy0) = ((rec.dest.x0 as f64 + ox).round() as i32, (rec.dest.y0 as f64 + oy).round() as i32);
    let (dx1, dy1) = ((rec.dest.x1 as f64 + ox).round() as i32, (rec.dest.y1 as f64 + oy).round() as i32);
    let (dw, dh) = ((dx1 - dx0).max(1) as f32, (dy1 - dy0).max(1) as f32);
    let (sw, sh) = (rec.src.x1 - rec.src.x0, rec.src.y1 - rec.src.y0);
    let (cx0, cy0, cx1, cy1) = t.clip;
    for y in dy0.max(cy0).max(0)..dy1.min(cy1).min(t.height as i32) {
        let sy = (rec.src.y0 + (y - dy0) as f32 / dh * sh) as usize;
        for x in dx0.max(cx0).max(0)..dx1.min(cx1).min(t.width as i32) {
            let sx = (rec.src.x0 + (x - dx0) as f32 / dw * sw) as usize;
            let i = (sy.min(ih - 1) * iw + sx.min(iw - 1)) * 4;
            let (r, g, b, a) = (img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]);
            let p = &mut t.px[y as usize * t.width + x as usize];
            *p = fill::blend(*p, fill::word(Rgba::rgb(r, g, b)), u32::from(a));
        }
    }
}
