//! Drawing a frame onto a vello scene. `vello_cpu`'s `RenderContext` and `vello_gpu`'s `Scene`
//! have the same drawing calls, so both renderers share this code through the [`Canvas`] trait.

use terminus_font::{FontGrid, sprite};
use vello_common::kurbo::{Affine, BezPath, Rect, Shape, Vec2};
use vello_common::peniko::FontData;

use crate::frame::{CursorStyle, Decoration, DecorationKind, Frame, GlyphRun, Rgba, RowFrame, SpriteCell};

/// The drawing calls a renderer provides.
pub trait Canvas {
    /// Sets the colour for the following fills.
    fn paint(&mut self, c: Rgba);
    /// Fills a rectangle.
    fn rect(&mut self, r: Rect);
    /// Fills a path with the non zero rule.
    fn path(&mut self, p: &BezPath);
    /// Draws glyphs of one font at one size in the current colour.
    fn glyphs(&mut self, font: &FontData, run: &GlyphRun);
}

/// Draws the whole frame: background, then each row, then the cursor.
pub fn draw_frame<C: Canvas>(c: &mut C, frame: &Frame, grid: &FontGrid) {
    c.paint(frame.background);
    c.rect(Rect::new(0.0, 0.0, frame.width as f64, frame.height as f64));
    for row in &frame.rows {
        draw_row(c, row, frame, grid);
    }
    if let Some(cursor) = frame.cursor {
        draw_cursor(c, &cursor, frame);
        if let Some(text) = &frame.cursor_text {
            draw_run(c, text, grid);
        }
    }
}

/// Draws one row: its background (the window colour, then spans), text, sprites, decorations.
pub fn draw_row<C: Canvas>(c: &mut C, row: &RowFrame, frame: &Frame, grid: &FontGrid) {
    c.paint(frame.background);
    c.rect(Rect::new(0.0, row.y as f64, frame.width as f64, (row.y + frame.cell_height as f32) as f64));
    for span in &row.backgrounds {
        c.paint(span.color);
        c.rect(Rect::new(span.x0 as f64, row.y as f64, span.x1 as f64, (row.y + frame.cell_height as f32) as f64));
    }
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
fn draw_run<C: Canvas>(c: &mut C, run: &GlyphRun, grid: &FontGrid) {
    let face = grid.face(run.face);
    let font = FontData::new(face.blob.clone(), face.index);
    c.paint(run.color);
    c.glyphs(&font, run);
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

/// A wavy band from `x0` to `x1`, one wave per four thicknesses, as a filled outline.
fn curly(x0: f64, x1: f64, y: f64, t: f64) -> BezPath {
    let amp = t.max(1.0);
    let period = (4.0 * amp).max(4.0);
    let mut top = BezPath::new();
    top.move_to((x0, y));
    let mut x = x0;
    let mut up = true;
    while x < x1 {
        let next = (x + period / 2.0).min(x1);
        let dy = if up { -amp } else { amp };
        top.quad_to(((x + next) / 2.0, y + dy * 2.0), (next, y));
        x = next;
        up = !up;
    }
    // Offset the same curve down by the thickness to close a band.
    let mut band = top.clone();
    let bottom: Vec<_> = (Affine::translate((0.0, t)) * top).elements().iter().rev().copied().collect();
    band.line_to((x1, y + t));
    for el in bottom.iter().skip(1) {
        if let Some(p) = el.end_point() {
            band.line_to(p);
        }
    }
    band.close_path();
    band.bounding_box();
    band
}

/// Draws the cursor.
fn draw_cursor<C: Canvas>(c: &mut C, cur: &crate::frame::CursorFrame, frame: &Frame) {
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

/// The glyph transform for synthetic italic: a 12 degree slant.
pub fn skew_transform(run: &GlyphRun) -> Affine {
    if run.skew { Affine::skew(-0.21, 0.0) } else { Affine::IDENTITY }
}

/// Whether glyphs are hinted. On by default; `TERMINUS_HINT=0` turns it off for measurements.
pub fn hint_enabled() -> bool {
    std::env::var("TERMINUS_HINT").map(|v| v != "0").unwrap_or(true)
}
