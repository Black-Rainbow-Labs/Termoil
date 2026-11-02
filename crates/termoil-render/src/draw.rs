//! Drawing a frame onto a vello scene. `vello_cpu`'s `RenderContext` and `vello_gpu`'s `Scene`
//! have the same drawing calls, so both renderers share this code through the [`Canvas`] trait.

use termoil_font::{FontGrid, sprite};
use vello_common::kurbo::{Affine, BezPath, Rect, Shape, Vec2};
use vello_common::peniko::FontData;

use crate::frame::{
    CursorStyle, Decoration, DecorationKind, Frame, GlyphRun, ImageRecord, Rgba, RowFrame, Shape as FrameShape,
    SpriteCell,
};

/// The drawing calls a renderer provides.
pub trait Canvas {
    /// Sets the colour for the following fills.
    fn paint(&mut self, c: Rgba);
    /// Fills a rectangle.
    fn rect(&mut self, r: Rect);
    /// Fills a path with the non zero rule.
    fn path(&mut self, p: &BezPath);
    /// Draws glyphs of one font at one size in the current colour.
    /// `coords` are the face's variation coordinates as raw F2Dot14 bits (empty for the default instance).
    fn glyphs(&mut self, font: &FontData, coords: &[i16], run: &GlyphRun);
    /// Sets the transform applied to everything drawn after it.
    fn transform(&mut self, t: Affine);
    /// Draws `rec.src` of the image stretched over `rec.dest`. A renderer keeps the decoded image
    /// in a cache keyed by the image's id and generation.
    fn image(&mut self, rec: &ImageRecord);
    /// Tells the renderer which images (id, generation) are still alive; it drops the others.
    fn retain_images(&mut self, live: &[(u32, u64)]);
}

/// One frame placed in the window at a pixel offset: a pane, the tab bar, an overlay.
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    /// Left edge in window pixels.
    pub x: f32,
    /// Top edge in window pixels.
    pub y: f32,
    /// The frame.
    pub frame: &'a Frame,
}

/// Fills the window with `background`, then draws each layer at its offset, in order.
pub fn draw_layers<C: Canvas>(c: &mut C, size: (u32, u32), background: Rgba, layers: &[Layer<'_>], grid: &FontGrid) {
    c.transform(Affine::IDENTITY);
    c.paint(background);
    c.rect(Rect::new(0.0, 0.0, size.0 as f64, size.1 as f64));
    let live: Vec<(u32, u64)> = layers.iter().flat_map(|l| l.frame.live_images.iter().copied()).collect();
    c.retain_images(&live);
    for l in layers {
        c.transform(Affine::translate((l.x.round() as f64, l.y.round() as f64)));
        draw_frame(c, l.frame, grid);
    }
    c.transform(Affine::IDENTITY);
}

/// Draws the whole frame. Without images under the text: each row (its background, then text),
/// then images above the text, then the cursor. With images under the text the layers are drawn in
/// the order the kitty graphics protocol gives: images below the cell backgrounds, the cell
/// backgrounds, images between backgrounds and text, the text, images above the text, the cursor.
pub fn draw_frame<C: Canvas>(c: &mut C, frame: &Frame, grid: &FontGrid) {
    // A translucent window colour is drawn once, by `draw_layers`; drawing it again here would
    // make it more opaque.
    if frame.background.a == 255 {
        c.paint(frame.background);
        c.rect(Rect::new(0.0, 0.0, frame.width as f64, frame.height as f64));
    }
    if frame.has_images_under_text() {
        frame.images_below.iter().for_each(|i| c.image(i));
        frame.rows.iter().for_each(|row| draw_row_spans(c, row, frame));
        frame.images_mid.iter().for_each(|i| c.image(i));
        frame.shapes.iter().for_each(|s| draw_shape(c, s));
        frame.rows.iter().for_each(|row| draw_row_text(c, row, frame, grid));
    } else {
        for row in &frame.rows {
            draw_row(c, row, frame, grid);
        }
    }
    frame.images_above.iter().for_each(|i| c.image(i));
    if let Some(cursor) = frame.cursor {
        draw_cursor(c, &cursor, frame);
        if let Some(text) = &frame.cursor_text {
            draw_run(c, text, grid);
        }
    }
}

/// Fills one of the frame's shapes.
fn draw_shape<C: Canvas>(c: &mut C, shape: &FrameShape) {
    match *shape {
        FrameShape::RoundRect { area, radius, color } => {
            let r = Rect::new(area.x0 as f64, area.y0 as f64, area.x1 as f64, area.y1 as f64);
            c.paint(color);
            c.path(&r.to_rounded_rect(radius as f64).to_path(0.1));
        }
        FrameShape::Line { from, to, width, color } => {
            c.paint(color);
            c.path(&line_outline(from, to, width));
        }
    }
}

/// The outline of a line `width` pixels thick from `from` to `to`, with square ends.
fn line_outline(from: (f32, f32), to: (f32, f32), width: f32) -> BezPath {
    let (dx, dy) = ((to.0 - from.0) as f64, (to.1 - from.1) as f64);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let half = width as f64 / 2.0;
    let (ux, uy) = (dx / len * half, dy / len * half);
    let (nx, ny) = (-uy, ux);
    let (x0, y0, x1, y1) = (from.0 as f64 - ux, from.1 as f64 - uy, to.0 as f64 + ux, to.1 as f64 + uy);
    let mut p = BezPath::new();
    p.move_to((x0 + nx, y0 + ny));
    p.line_to((x1 + nx, y1 + ny));
    p.line_to((x1 - nx, y1 - ny));
    p.line_to((x0 - nx, y0 - ny));
    p.close_path();
    p
}

/// Draws one row: its background (the window colour, then spans), text, sprites, decorations.
pub fn draw_row<C: Canvas>(c: &mut C, row: &RowFrame, frame: &Frame, grid: &FontGrid) {
    if frame.background.a == 255 {
        c.paint(frame.background);
        c.rect(Rect::new(0.0, row.y as f64, frame.width as f64, (row.y + frame.cell_height as f32) as f64));
    }
    draw_row_spans(c, row, frame);
    draw_row_text(c, row, frame, grid);
}

/// Draws the background spans of a row that differ from the window colour.
fn draw_row_spans<C: Canvas>(c: &mut C, row: &RowFrame, frame: &Frame) {
    for span in &row.backgrounds {
        c.paint(span.color);
        c.rect(Rect::new(span.x0 as f64, row.y as f64, span.x1 as f64, (row.y + frame.cell_height as f32) as f64));
    }
}

/// Draws a row's text, sprite characters and decorations.
fn draw_row_text<C: Canvas>(c: &mut C, row: &RowFrame, frame: &Frame, grid: &FontGrid) {
    for run in &row.runs {
        draw_run(c, run, grid);
    }
    for s in &row.sprites {
        draw_sprite(c, s, frame, grid);
    }
    for d in &row.decorations {
        draw_decoration(c, d);
    }
}

/// Draws a glyph run.
pub(crate) fn draw_run<C: Canvas>(c: &mut C, run: &GlyphRun, grid: &FontGrid) {
    let face = grid.face(run.face);
    let font = FontData::new(face.blob.clone(), face.index);
    c.paint(run.color);
    c.glyphs(&font, &face.raw_coords, run);
}

/// Draws a sprite character from its geometry.
fn draw_sprite<C: Canvas>(c: &mut C, s: &SpriteCell, frame: &Frame, grid: &FontGrid) {
    let light = grid.metrics().line_thickness;
    let Some(shapes) = sprite::sprite(s.ch, frame.cell_width, frame.cell_height, light) else { return };
    let at = Affine::translate((s.x as f64, s.y as f64));
    for shape in shapes {
        match shape {
            sprite::SpriteShape::Rect(r) => {
                c.paint(s.color);
                c.rect(r + Vec2::new(s.x as f64, s.y as f64));
            }
            sprite::SpriteShape::Path(p) => {
                c.paint(s.color);
                c.path(&(at * p));
            }
            sprite::SpriteShape::Shade { rect, alpha } => {
                c.paint(Rgba { a: (alpha * 255.0).round() as u8, ..s.color });
                c.rect(rect + Vec2::new(s.x as f64, s.y as f64));
            }
        }
    }
}

/// Draws an underline, strike through or overline.
fn draw_decoration<C: Canvas>(c: &mut C, d: &Decoration) {
    c.paint(d.color);
    let (x0, x1, y, t) = (d.x0 as f64, d.x1 as f64, d.y as f64, d.thickness as f64);
    match d.kind {
        DecorationKind::Underline | DecorationKind::Strike | DecorationKind::Overline => {
            c.rect(Rect::new(x0, y, x1, y + t))
        }
        DecorationKind::DoubleUnderline => {
            c.rect(Rect::new(x0, y, x1, y + t));
            c.rect(Rect::new(x0, y + 2.0 * t, x1, y + 3.0 * t));
        }
        DecorationKind::DottedUnderline => dashes(c, x0, x1, y, t, t, t),
        DecorationKind::DashedUnderline => dashes(c, x0, x1, y, t, 3.0 * t, 2.0 * t),
        DecorationKind::CurlyUnderline => c.path(&curly(x0, x1, y, t)),
    }
}

/// Rectangles of `on` pixels separated by `off` pixels along a line.
fn dashes<C: Canvas>(c: &mut C, x0: f64, x1: f64, y: f64, t: f64, on: f64, off: f64) {
    let mut x = x0;
    while x < x1 {
        c.rect(Rect::new(x, y, (x + on).min(x1), y + t));
        x += on + off;
    }
}

/// A wavy band from `x0` to `x1` as a filled outline, its lowest point where a straight underline at
/// `y` would end. A wave is about a cell wide (eight thicknesses, at least six pixels) and swings one
/// and a half thicknesses. Before task-2468 a wave was four thicknesses with a one thickness swing, so
/// the peaks sat a pixel apart and it could not be told from a dotted line, and its lower edge joined
/// only the ends of each half wave, which drew a row of humps on a flat line.
fn curly(x0: f64, x1: f64, y: f64, t: f64) -> BezPath {
    let amp = (1.5 * t).max(1.5);
    let period = (8.0 * t).max(6.0);
    let mid = y - amp;
    let mut halves = Vec::new();
    let (mut x, mut up) = (x0, true);
    while x < x1 {
        let next = (x + period / 2.0).min(x1);
        let control = ((x + next) / 2.0, mid + if up { -2.0 * amp } else { 2.0 * amp });
        halves.push((x, control, next));
        x = next;
        up = !up;
    }
    let mut band = BezPath::new();
    band.move_to((x0, mid));
    for &(_, c, end) in &halves {
        band.quad_to(c, (end, mid));
    }
    band.line_to((x1, mid + t));
    for &(start, c, _) in halves.iter().rev() {
        band.quad_to((c.0, c.1 + t), (start, mid + t));
    }
    band.close_path();
    band
}

/// Draws the cursor.
pub(crate) fn draw_cursor<C: Canvas>(c: &mut C, cur: &crate::frame::CursorFrame, frame: &Frame) {
    c.paint(cur.color);
    let (x, y, w, h) = (cur.x as f64, cur.y as f64, cur.width as f64, cur.height as f64);
    let t = (frame.cell_width as f64 / 8.0).round().max(1.0);
    match cur.style {
        CursorStyle::Block => c.rect(Rect::new(x, y, x + w, y + h)),
        CursorStyle::Bar => c.rect(Rect::new(x, y, x + t.max(2.0), y + h)),
        CursorStyle::Underline => c.rect(Rect::new(x, y + h - t.max(2.0), x + w, y + h)),
        CursorStyle::Hollow => {
            c.rect(Rect::new(x, y, x + w, y + t));
            c.rect(Rect::new(x, y + h - t, x + w, y + h));
            c.rect(Rect::new(x, y, x + t, y + h));
            c.rect(Rect::new(x + w - t, y, x + w, y + h));
        }
    }
}

/// The rectangle an image record fills.
pub fn image_rect(rec: &ImageRecord) -> Rect {
    Rect::new(rec.dest.x0 as f64, rec.dest.y0 as f64, rec.dest.x1 as f64, rec.dest.y1 as f64)
}

/// The transform that maps image pixels onto the frame so that `rec.src` covers `rec.dest`.
pub fn image_paint_transform(rec: &ImageRecord) -> Affine {
    let (dw, dh) = ((rec.dest.x1 - rec.dest.x0) as f64, (rec.dest.y1 - rec.dest.y0) as f64);
    let (sw, sh) = ((rec.src.x1 - rec.src.x0) as f64, (rec.src.y1 - rec.src.y0) as f64);
    let scale = Affine::scale_non_uniform(dw / sw.max(1e-6), dh / sh.max(1e-6));
    let to_dest = Affine::translate((rec.dest.x0 as f64, rec.dest.y0 as f64));
    to_dest * scale * Affine::translate((-rec.src.x0 as f64, -rec.src.y0 as f64))
}

/// The glyph transform for synthetic italic: a 12 degree slant.
pub fn skew_transform(run: &GlyphRun) -> Affine {
    if run.skew { Affine::skew(-0.21, 0.0) } else { Affine::IDENTITY }
}

/// Whether glyphs are hinted. On by default; `TERMOIL_HINT=0` turns it off for measurements.
pub fn hint_enabled() -> bool {
    std::env::var("TERMOIL_HINT").map(|v| v != "0").unwrap_or(true)
}
