//! The frame builder: render state in, frame records out, independent of how they are drawn.
//!
//! Per row it merges background spans, groups cells into runs of one face and colour, shapes each
//! run (through the font grid's run cache) and places every glyph at its cell's position so text
//! stays on the grid, and collects decorations, sprite characters and the cursor.

use terminus_font::{FaceId, FontGrid, FontStyle, sprite};
use terminus_vt::style::{Rgb, Underline, attr};
use terminus_vt::terminal::CursorShape;

use crate::frame::{
    BgSpan, CursorFrame, CursorStyle, Decoration, DecorationKind, Frame, GlyphPos, GlyphRun, Rgba, RowFrame, SpriteCell,
};
use crate::state::{CellState, CellText, CellWidth, RenderState};

/// Settings the frame builder takes from the configuration.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    /// Padding around the grid in pixels (horizontal, vertical).
    pub padding: (u32, u32),
    /// Draw bold text in the bright version of the eight ANSI colours.
    pub bold_is_bright: bool,
    /// Minimum contrast ratio between text and its background (1.0 turns the adjustment off).
    pub minimum_contrast: f32,
    /// Build every row, not only the ones marked dirty.
    pub all_rows: bool,
    /// The cursor shape when the program has not set one.
    pub default_cursor: CursorShape,
}

impl Default for BuildOptions {
    /// Two pixels of padding, contrast at 1 (off), every row.
    fn default() -> Self {
        BuildOptions {
            padding: (2, 2),
            bold_is_bright: false,
            minimum_contrast: 1.0,
            all_rows: true,
            default_cursor: CursorShape::Block { blink: false },
        }
    }
}

/// The colours one cell is drawn in.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellColors {
    fg: Rgba,
    bg: Rgba,
    underline: Rgba,
}

/// Converts a terminal colour to an opaque RGBA.
fn rgba(c: Rgb) -> Rgba {
    Rgba::rgb(c.r, c.g, c.b)
}

/// Builds a frame from the render state.
pub fn build_frame(state: &RenderState, grid: &mut FontGrid, opts: &BuildOptions) -> Frame {
    let mut frame = frame_shell(state, grid, opts);
    for (r, line) in state.lines.iter().enumerate() {
        if line.dirty || opts.all_rows {
            frame.rows.push(build_row(state, r as u16, grid, opts));
        }
    }
    frame
}

/// A frame with its size, background and cursor, and no rows yet.
fn frame_shell(state: &RenderState, grid: &mut FontGrid, opts: &BuildOptions) -> Frame {
    let m = *grid.metrics();
    let (cw, ch) = (m.width, m.height);
    let background = rgba(if state.reverse_video { state.colors.foreground } else { state.colors.background });
    let mut frame = Frame {
        width: state.cols as u32 * cw + 2 * opts.padding.0,
        height: state.rows as u32 * ch + 2 * opts.padding.1,
        cell_width: cw,
        cell_height: ch,
        background,
        padding: opts.padding,
        ..Frame::default()
    };
    if let Some(c) = state.cursor.filter(|c| c.visible) {
        let (cursor, text) = build_cursor(state, c, grid, opts);
        frame.cursor = Some(cursor);
        frame.cursor_text = text;
    }
    frame
}

/// Builds frames with a cache of row frames keyed by row serial, so rows that only moved (a
/// scroll) are reused with their position changed instead of being shaped again. Every frame it
/// returns has every row, because a vello scene is drawn whole.
#[derive(Default)]
pub struct FrameBuilder {
    cache: std::collections::HashMap<u64, RowFrame>,
    rows_built: u64,
    rows_reused: u64,
}

impl FrameBuilder {
    /// An empty builder.
    pub fn new() -> FrameBuilder {
        FrameBuilder::default()
    }

    /// Rows built and rows reused since the builder was made.
    pub fn stats(&self) -> (u64, u64) {
        (self.rows_built, self.rows_reused)
    }

    /// Builds a full frame, reusing cached rows whose content did not change.
    pub fn build(&mut self, state: &RenderState, grid: &mut FontGrid, opts: &BuildOptions) -> Frame {
        let mut frame = frame_shell(state, grid, opts);
        let mut next = std::collections::HashMap::with_capacity(state.lines.len());
        for (r, line) in state.lines.iter().enumerate() {
            let y = (opts.padding.1 + r as u32 * frame.cell_height) as f32;
            let row = match self.cache.remove(&line.serial).filter(|_| !line.content_changed) {
                Some(cached) => {
                    self.rows_reused += 1;
                    moved(cached, r as u16, y)
                }
                None => {
                    self.rows_built += 1;
                    build_row(state, r as u16, grid, opts)
                }
            };
            next.insert(line.serial, row.clone());
            frame.rows.push(row);
        }
        self.cache = next;
        frame
    }
}

/// A cached row frame moved to row `r` at height `y`.
fn moved(mut row: RowFrame, r: u16, y: f32) -> RowFrame {
    let dy = y - row.y;
    row.row = r;
    row.y = y;
    if dy != 0.0 {
        row.runs.iter_mut().flat_map(|run| run.glyphs.iter_mut()).for_each(|g| g.y += dy);
        row.sprites.iter_mut().for_each(|s| s.y += dy);
        row.decorations.iter_mut().for_each(|d| d.y += dy);
    }
    row
}

/// The x of a column's left edge.
fn col_x(col: u16, cw: u32, opts: &BuildOptions) -> f32 {
    (opts.padding.0 + col as u32 * cw) as f32
}

/// Builds one row.
fn build_row(state: &RenderState, r: u16, grid: &mut FontGrid, opts: &BuildOptions) -> RowFrame {
    let m = *grid.metrics();
    let y = (opts.padding.1 + r as u32 * m.height) as f32;
    let line = &state.lines[r as usize];
    let colors: Vec<CellColors> =
        line.cells.iter().enumerate().map(|(col, cell)| cell_colors(state, cell, r, col as u16, opts)).collect();
    let window_bg = rgba(if state.reverse_video { state.colors.foreground } else { state.colors.background });
    let mut row = RowFrame { row: r, y, ..RowFrame::default() };
    row.backgrounds = background_spans(&colors, window_bg, m.width, opts);
    row.decorations = decorations(line.cells.as_slice(), &colors, y, grid, opts);
    text_runs(&mut row, line.cells.as_slice(), &colors, y, grid, opts);
    row
}

/// Resolves the foreground, background and underline colour of a cell, applying inverse, faint,
/// invisible, reverse video, the selection and the minimum contrast.
fn cell_colors(state: &RenderState, cell: &CellState, r: u16, col: u16, opts: &BuildOptions) -> CellColors {
    let c = &state.colors;
    let s = &cell.style;
    let (dfg, dbg) = if state.reverse_video { (c.background, c.foreground) } else { (c.foreground, c.background) };
    let mut fg_color = s.fg;
    if opts.bold_is_bright
        && s.has(attr::BOLD)
        && let terminus_vt::style::Color::Palette(i @ 0..=7) = fg_color
    {
        fg_color = terminus_vt::style::Color::Palette(i + 8);
    }
    let mut fg = c.resolve(fg_color, dfg);
    let mut bg = c.resolve(s.bg, dbg);
    if s.has(attr::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if s.has(attr::FAINT) {
        fg = mix(fg, bg, 0.5);
    }
    if state.selection.iter().any(|&(row, a, b)| row == r && col >= a && col <= b) {
        bg = c.selection_background;
        fg = c.selection_foreground.unwrap_or(fg);
    }
    if s.has(attr::INVISIBLE) {
        fg = bg;
    }
    fg = ensure_contrast(fg, bg, opts.minimum_contrast);
    let underline = match s.underline_color {
        terminus_vt::style::Color::Default => fg,
        other => c.resolve(other, fg),
    };
    CellColors { fg: rgba(fg), bg: rgba(bg), underline: rgba(underline) }
}

/// Mixes two colours, `t` of the way from `a` to `b`.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb::new(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b))
}

/// The WCAG relative luminance of a colour.
fn luminance(c: Rgb) -> f32 {
    let ch = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

/// The WCAG contrast ratio of two colours.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Moves `fg` toward white or black until it has at least `minimum` contrast against `bg`.
fn ensure_contrast(fg: Rgb, bg: Rgb, minimum: f32) -> Rgb {
    if minimum <= 1.0 || contrast(fg, bg) >= minimum {
        return fg;
    }
    let target = if luminance(bg) < 0.5 { Rgb::new(255, 255, 255) } else { Rgb::new(0, 0, 0) };
    let mut out = fg;
    for step in 1..=10 {
        out = mix(fg, target, step as f32 / 10.0);
        if contrast(out, bg) >= minimum {
            break;
        }
    }
    out
}

/// Merges adjacent cells with the same non window background into spans.
fn background_spans(colors: &[CellColors], window: Rgba, cw: u32, opts: &BuildOptions) -> Vec<BgSpan> {
    let mut spans: Vec<BgSpan> = Vec::new();
    for (col, c) in colors.iter().enumerate() {
        if c.bg == window {
            continue;
        }
        let x0 = col_x(col as u16, cw, opts);
        match spans.last_mut() {
            Some(last) if last.color == c.bg && (last.x1 - x0).abs() < 0.01 => last.x1 = x0 + cw as f32,
            _ => spans.push(BgSpan { x0, x1: x0 + cw as f32, color: c.bg }),
        }
    }
    spans
}

/// Underlines, strike through and overlines, merged across cells with the same kind and colour.
fn decorations(
    cells: &[CellState], colors: &[CellColors], y: f32, grid: &FontGrid, opts: &BuildOptions,
) -> Vec<Decoration> {
    let m = grid.metrics();
    let mut out: Vec<Decoration> = Vec::new();
    for (col, cell) in cells.iter().enumerate() {
        let x0 = col_x(col as u16, m.width, opts);
        let x1 = x0 + m.width as f32;
        let s = &cell.style;
        let mut push = |kind: DecorationKind, top: f32, thickness: f32, color: Rgba| match out.last_mut() {
            Some(d) if d.kind == kind && d.color == color && (d.x1 - x0).abs() < 0.01 && d.y == top => d.x1 = x1,
            _ => out.push(Decoration { kind, x0, x1, y: top, thickness, color }),
        };
        if let Some(kind) = underline_kind(s.underline) {
            push(kind, y + m.underline_position, m.underline_thickness, colors[col].underline);
        }
        if s.has(attr::STRIKE) {
            push(DecorationKind::Strike, y + m.strike_position, m.strike_thickness, colors[col].fg);
        }
        if s.has(attr::OVERLINE) {
            push(DecorationKind::Overline, y, m.underline_thickness, colors[col].fg);
        }
    }
    out
}

/// The decoration kind for an underline style.
fn underline_kind(u: Underline) -> Option<DecorationKind> {
    Some(match u {
        Underline::None => return None,
        Underline::Single => DecorationKind::Underline,
        Underline::Double => DecorationKind::DoubleUnderline,
        Underline::Curly => DecorationKind::CurlyUnderline,
        Underline::Dotted => DecorationKind::DottedUnderline,
        Underline::Dashed => DecorationKind::DashedUnderline,
    })
}

/// A run of cells being collected for shaping.
struct Pending {
    face: FaceId,
    color: Rgba,
    style: FontStyle,
    text: String,
    /// For each byte offset where a cell's text starts, that cell's column.
    starts: Vec<(u32, u16)>,
}

/// Groups the row's cells into runs, shapes them, and places the glyphs on the grid.
fn text_runs(
    row: &mut RowFrame, cells: &[CellState], colors: &[CellColors], y: f32, grid: &mut FontGrid, opts: &BuildOptions,
) {
    let m = *grid.metrics();
    let mut pending: Option<Pending> = None;
    for (col, cell) in cells.iter().enumerate() {
        let col = col as u16;
        let Some(first) = cell.text.first().filter(|_| cell.wide != CellWidth::Tail) else {
            flush(row, &mut pending, y, grid, opts);
            continue;
        };
        if first == ' ' && !matches!(cell.text, CellText::Cluster(_)) {
            flush(row, &mut pending, y, grid, opts);
            continue;
        }
        if sprite::is_sprite(first) {
            flush(row, &mut pending, y, grid, opts);
            row.sprites.push(SpriteCell { ch: first, x: col_x(col, m.width, opts), y, color: colors[col as usize].fg });
            continue;
        }
        let style = FontStyle::from_flags(cell.style.has(attr::BOLD), cell.style.has(attr::ITALIC));
        let face = grid.face_for(first, style);
        let color = colors[col as usize].fg;
        if pending.as_ref().is_some_and(|p| p.face != face || p.color != color || p.style != style) {
            flush(row, &mut pending, y, grid, opts);
        }
        let p = pending.get_or_insert_with(|| Pending { face, color, style, text: String::new(), starts: Vec::new() });
        p.starts.push((p.text.len() as u32, col));
        cell.text.push_to(&mut p.text);
    }
    flush(row, &mut pending, y, grid, opts);
}

/// Shapes a pending run and adds its glyphs to the row.
fn flush(row: &mut RowFrame, pending: &mut Option<Pending>, y: f32, grid: &mut FontGrid, opts: &BuildOptions) {
    let Some(p) = pending.take() else { return };
    let m = *grid.metrics();
    let shaped = grid.shape(p.face, &p.text);
    let face = grid.face(p.face);
    let wants_bold = matches!(p.style, FontStyle::Bold | FontStyle::BoldItalic);
    let wants_italic = matches!(p.style, FontStyle::Italic | FontStyle::BoldItalic);
    let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
    let mut last_cluster = u32::MAX;
    let mut pen = 0.0;
    for g in &shaped.glyphs {
        let idx = p.starts.partition_point(|&(off, _)| off <= g.cluster).saturating_sub(1);
        let col = p.starts[idx].1;
        // Each cluster starts at its cell; glyphs after the first in a cluster follow the shaper.
        if g.cluster != last_cluster {
            pen = 0.0;
            last_cluster = g.cluster;
        }
        glyphs.push(GlyphPos {
            id: g.id,
            x: col_x(col, m.width, opts) + pen + g.x_offset,
            y: y + m.baseline - g.y_offset,
        });
        pen += g.advance;
    }
    row.runs.push(GlyphRun {
        face: p.face,
        size: grid.size_px(),
        color: p.color,
        embolden: wants_bold && face.synthetic_bold,
        skew: wants_italic && face.synthetic_italic,
        glyphs,
    });
}

/// The cursor rectangle, and for a block cursor the glyph under it in the cursor's text colour.
fn build_cursor(
    state: &RenderState, c: crate::state::CursorState, grid: &mut FontGrid, opts: &BuildOptions,
) -> (CursorFrame, Option<GlyphRun>) {
    let m = *grid.metrics();
    let x = col_x(c.col, m.width, opts);
    let y = (opts.padding.1 + c.row as u32 * m.height) as f32;
    let width = m.width as f32 * if c.wide { 2.0 } else { 1.0 };
    let shape = if c.shape == CursorShape::Default { opts.default_cursor } else { c.shape };
    let style = match (state.focused, shape) {
        (false, _) => CursorStyle::Hollow,
        (true, CursorShape::Bar { .. }) => CursorStyle::Bar,
        (true, CursorShape::Underline { .. }) => CursorStyle::Underline,
        _ => CursorStyle::Block,
    };
    let color = rgba(state.colors.cursor);
    let cursor = CursorFrame { style, x, y, width, height: m.height as f32, color };
    if style != CursorStyle::Block {
        return (cursor, None);
    }
    let cell = state.lines.get(c.row as usize).and_then(|l| l.cells.get(c.col as usize));
    let text = cell.and_then(|cell| cursor_glyph(cell, x, y, grid, rgba(state.colors.background)));
    (cursor, text)
}

/// The glyph under a block cursor, drawn in the background colour so it shows through.
fn cursor_glyph(cell: &CellState, x: f32, y: f32, grid: &mut FontGrid, color: Rgba) -> Option<GlyphRun> {
    let first = cell.text.first().filter(|c| *c != ' ' && !sprite::is_sprite(*c))?;
    let style = FontStyle::from_flags(cell.style.has(attr::BOLD), cell.style.has(attr::ITALIC));
    let face = grid.face_for(first, style);
    let mut text = String::new();
    cell.text.push_to(&mut text);
    let shaped = grid.shape(face, &text);
    let baseline = grid.metrics().baseline;
    let mut pen = 0.0;
    let glyphs = shaped.glyphs.iter().map(|g| {
        let pos = GlyphPos { id: g.id, x: x + pen + g.x_offset, y: y + baseline - g.y_offset };
        pen += g.advance;
        pos
    });
    Some(GlyphRun { face, size: grid.size_px(), color, embolden: false, skew: false, glyphs: glyphs.collect() })
}
