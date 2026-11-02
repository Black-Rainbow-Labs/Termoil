//! Images in the render state and the frame: which graphics store placements are on screen, where
//! they sit in pixels, and the records the frame builder turns them into.
//!
//! The store keeps placements by row serial and cell. [`collect_images`] resolves the ones that touch
//! the viewport (and the cells of Unicode placeholders) to pixel rectangles relative to the grid
//! origin. [`image_records`] clips them to the grid, scales them to the font's cell size and sorts
//! them into the three z groups. Pixels are shared through an `Arc`; an image keeps its id and
//! generation while its pixels are unchanged, so a renderer uploads it once.

use std::collections::HashMap;
use std::sync::Arc;

use terminus_vt::Terminal;
use terminus_vt::graphics::{GraphicsStore, Image, Placement};
use terminus_vt::style::Color;

use crate::frame::{Area, ImageRecord};
use crate::placeholder::{PLACEHOLDER, diacritic_number};
use crate::state::{CellState, CellText, RowState};

/// z indexes below this draw under cells that have a non default background.
pub const Z_BELOW_BACKGROUND: i32 = i32::MIN / 2;

/// The decoded pixels of one image as the renderer needs them.
#[derive(Debug)]
pub struct ImageData {
    /// The store's image id.
    pub id: u32,
    /// Changes whenever the pixels shown for this id change (a new image under the same id, or the
    /// next animation frame). Renderers cache by (id, generation).
    pub generation: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Straight alpha RGBA8, `width * height * 4` bytes, shared with the store.
    pub rgba: Arc<Vec<u8>>,
}

impl PartialEq for ImageData {
    /// Two copies are equal when they have the same id and generation, which means the same pixels.
    fn eq(&self, other: &ImageData) -> bool {
        self.id == other.id && self.generation == other.generation
    }
}

/// A rectangle in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    /// Left edge.
    pub x: i64,
    /// Top edge.
    pub y: i64,
    /// Width.
    pub w: i64,
    /// Height.
    pub h: i64,
}

/// One placement on screen, in pixels of the terminal's cell size relative to the grid origin.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePlacement {
    /// The image.
    pub image: Arc<ImageData>,
    /// Where the whole image is drawn. It can extend above, below or beside the viewport.
    pub dest: PxRect,
    /// The part of the image shown (x, y, width, height in image pixels).
    pub src: (u32, u32, u32, u32),
    /// The z index.
    pub z: i32,
    /// Only this part of `dest` is drawn: the cells of a run of Unicode placeholders.
    pub clip: Option<PxRect>,
}

/// Remembers the generation given to each image so unchanged images keep their key.
#[derive(Clone, Debug, Default)]
pub struct ImageTracker {
    seen: HashMap<u32, Arc<ImageData>>,
    next_generation: u64,
}

impl PartialEq for ImageTracker {
    /// The tracker is bookkeeping and never makes two render states differ.
    fn eq(&self, _: &ImageTracker) -> bool {
        true
    }
}

impl ImageTracker {
    /// The shared pixel data for a store image, reusing the earlier copy while its pixels are the same.
    fn data_for(&mut self, image: &Image) -> Arc<ImageData> {
        let rgba = image.current_rgba();
        if let Some(old) = self.seen.get(&image.id)
            && Arc::ptr_eq(&old.rgba, rgba)
            && old.width == image.width
            && old.height == image.height
        {
            return old.clone();
        }
        self.next_generation += 1;
        let data = Arc::new(ImageData {
            id: image.id,
            generation: self.next_generation,
            width: image.width,
            height: image.height,
            rgba: rgba.clone(),
        });
        self.seen.insert(image.id, data.clone());
        data
    }

    /// Forgets images the store no longer holds.
    fn retain_live(&mut self, live: &[(u32, u64)]) {
        self.seen.retain(|id, _| live.iter().any(|(l, _)| l == id));
    }
}

/// What [`collect_images`] found.
#[derive(Debug, Default)]
pub struct CollectedImages {
    /// Placements that touch the viewport, in drawing order.
    pub placements: Vec<ImagePlacement>,
    /// Id and generation of every image in the store.
    pub live: Vec<(u32, u64)>,
}

/// Integer division rounding up.
fn ceil_div(a: i64, b: i64) -> i64 {
    (a + b - 1) / b.max(1)
}

/// The size in pixels an image is drawn at for a placement. An image whose cell counts are exactly
/// what its source size needs is drawn at its own size. Otherwise it is scaled to fit inside the
/// cells with its aspect ratio kept, or stretched to fill them when the placement says so.
pub fn drawn_size(p: &Placement, cell: (i64, i64)) -> (i64, i64) {
    let (sw, sh) = (p.src.2.max(1) as i64, p.src.3.max(1) as i64);
    let (xo, yo) = (p.x_offset as i64, p.y_offset as i64);
    let natural = (ceil_div(sw + xo, cell.0), ceil_div(sh + yo, cell.1));
    if (p.cols as i64, p.rows as i64) == natural && !p.stretch {
        return (sw, sh);
    }
    let bw = (p.cols as i64 * cell.0 - xo).max(1);
    let bh = (p.rows as i64 * cell.1 - yo).max(1);
    if p.stretch {
        return (bw, bh);
    }
    let scale = (bw as f64 / sw as f64).min(bh as f64 / sh as f64);
    (((sw as f64 * scale).round() as i64).max(1), ((sh as f64 * scale).round() as i64).max(1))
}

/// Finds the placements that touch the viewport whose top row has serial `top`, and the cells of
/// Unicode placeholders, as pixel rectangles relative to the grid origin.
pub fn collect_images(tracker: &mut ImageTracker, term: &Terminal, top: u64, lines: &[RowState]) -> CollectedImages {
    let store = term.graphics();
    let mut out = CollectedImages::default();
    for image in store.images() {
        let data = tracker.data_for(image);
        out.live.push((data.id, data.generation));
    }
    tracker.retain_live(&out.live);
    if out.live.is_empty() {
        return out;
    }
    let (cw, ch) = term.cell_pixels();
    let cell = (cw as i64, ch as i64);
    let last = top + lines.len().saturating_sub(1) as u64;
    for p in store.placements_in_rows(top, last) {
        let Some(data) = tracker.seen.get(&p.image_id).cloned() else { continue };
        let (w, h) = drawn_size(p, cell);
        let x = p.col as i64 * cell.0 + p.x_offset as i64;
        let y = (p.serial as i64 - top as i64) * cell.1 + p.y_offset as i64;
        out.placements.push(ImagePlacement {
            image: data,
            dest: PxRect { x, y, w, h },
            src: p.src,
            z: p.z,
            clip: None,
        });
    }
    if store.placements().any(|p| p.virtual_placement) {
        placeholder_placements(tracker, store, lines, cell, &mut out.placements);
    }
    out.placements.sort_by_key(|p| (p.z, p.image.id));
    out
}

/// The image id or placement id a colour encodes: the palette index, or the 24 bit value.
fn color_id(c: Color) -> u32 {
    match c {
        Color::Default => 0,
        Color::Palette(i) => i as u32,
        Color::Rgb(r, g, b) => (r as u32) << 16 | (g as u32) << 8 | b as u32,
    }
}

/// A decoded placeholder cell: the colours it carries and the numbers its diacritics give.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PlaceholderCell {
    fg: Color,
    underline: Color,
    row: Option<u32>,
    col: Option<u32>,
    high: Option<u32>,
}

/// Decodes a placeholder cell, or None for any other cell.
fn decode_placeholder(cell: &CellState) -> Option<PlaceholderCell> {
    let (fg, underline) = (cell.style.fg, cell.style.underline_color);
    let CellText::Cluster(text) = &cell.text else {
        let plain = cell.text.first() == Some(PLACEHOLDER);
        return plain.then_some(PlaceholderCell { fg, underline, row: None, col: None, high: None });
    };
    let mut chars = text.chars();
    if chars.next()? != PLACEHOLDER {
        return None;
    }
    let mut marks = chars.map_while(diacritic_number);
    let (row, col, high) = (marks.next(), marks.next(), marks.next());
    Some(PlaceholderCell { fg, underline, row, col, high })
}

/// A placeholder cell with the numbers filled in from the cell on its left where it left them out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedCell {
    image_id: u32,
    placement_id: u32,
    row: u32,
    col: u32,
}

/// Fills in the omitted diacritics of `cell` from `prev`, the cell to its left, which counts only
/// when it has the same colours. No diacritics: same row, next column. Only a row: next column when
/// the row matches the left cell, else column 0. A missing third mark keeps the left cell's id byte.
fn resolve_cell(cell: &PlaceholderCell, prev: Option<&(PlaceholderCell, ResolvedCell)>) -> ResolvedCell {
    let left = prev.filter(|(c, _)| c.fg == cell.fg && c.underline == cell.underline).map(|(_, r)| *r);
    let row = cell.row.or(left.map(|l| l.row)).unwrap_or(0);
    let col = match (cell.row, cell.col, left) {
        (_, Some(c), _) => c,
        (None, None, Some(l)) => l.col + 1,
        (Some(r), None, Some(l)) if l.row == r => l.col + 1,
        _ => 0,
    };
    let low = color_id(cell.fg) & 0x00ff_ffff;
    let high = cell.high.or(left.map(|l| l.image_id >> 24)).unwrap_or(0);
    ResolvedCell { image_id: low | (high & 0xff) << 24, placement_id: color_id(cell.underline), row, col }
}

/// A run of placeholder cells on one grid row: the first cell resolved, its grid column and the count.
type Run = (ResolvedCell, usize, usize);

/// Adds the images that Unicode placeholder cells in the viewport name. A run of neighbouring cells
/// that show neighbouring cells of one image becomes one placement clipped to those cells.
fn placeholder_placements(
    tracker: &mut ImageTracker, store: &GraphicsStore, lines: &[RowState], cell: (i64, i64),
    out: &mut Vec<ImagePlacement>,
) {
    for (gr, line) in lines.iter().enumerate() {
        let mut prev: Option<(PlaceholderCell, ResolvedCell)> = None;
        let mut run: Option<Run> = None;
        for (gc, c) in line.cells.iter().enumerate() {
            let decoded = decode_placeholder(c);
            let resolved = decoded.as_ref().map(|d| resolve_cell(d, prev.as_ref()));
            prev = decoded.zip(resolved);
            run = match (run, resolved) {
                (Some((r, first, n)), Some(c)) if extends(&r, n, &c) => Some((r, first, n + 1)),
                (open, next) => {
                    if let Some(r) = open {
                        push_run(tracker, store, r, gr, cell, out);
                    }
                    next.map(|c| (c, gc, 1))
                }
            };
        }
        if let Some(r) = run {
            push_run(tracker, store, r, gr, cell, out);
        }
    }
}

/// Whether `next` is the cell right after the `n`th cell of the run that started with `first`.
fn extends(first: &ResolvedCell, n: usize, next: &ResolvedCell) -> bool {
    first.image_id == next.image_id
        && first.placement_id == next.placement_id
        && first.row == next.row
        && first.col + n as u32 == next.col
}

/// Adds one run of placeholder cells on grid row `gr`.
fn push_run(
    tracker: &mut ImageTracker, store: &GraphicsStore, run: Run, gr: usize, cell: (i64, i64),
    out: &mut Vec<ImagePlacement>,
) {
    let (first, gc, n) = run;
    let Some(p) = store.virtual_placement(first.image_id, first.placement_id) else { return };
    let Some(image) = store.image(first.image_id) else { return };
    let data = tracker.data_for(image);
    let (w, h) = drawn_size(p, cell);
    let origin_x = (gc as i64 - first.col as i64) * cell.0;
    let origin_y = (gr as i64 - first.row as i64) * cell.1;
    let dest = PxRect { x: origin_x + p.x_offset as i64, y: origin_y + p.y_offset as i64, w, h };
    let clip = PxRect { x: gc as i64 * cell.0, y: gr as i64 * cell.1, w: n as i64 * cell.0, h: cell.1 };
    out.push(ImagePlacement { image: data, dest, src: p.src, z: p.z, clip: Some(clip) });
}

/// The three groups of image records a frame holds.
#[derive(Debug, Default)]
pub struct ImageGroups {
    /// z below [`Z_BELOW_BACKGROUND`].
    pub below: Vec<ImageRecord>,
    /// Negative z down to [`Z_BELOW_BACKGROUND`].
    pub mid: Vec<ImageRecord>,
    /// Zero and above.
    pub above: Vec<ImageRecord>,
}

/// The sizes the records are built for: the terminal's cell size the placements were laid out with,
/// the font's cell size, the grid size in cells and the padding in pixels.
#[derive(Clone, Copy, Debug)]
pub struct RecordGeometry {
    /// Cell size the state's pixel rectangles use.
    pub state_cell: (u16, u16),
    /// Cell size of the font the frame is drawn in.
    pub font_cell: (u32, u32),
    /// Columns and rows.
    pub grid: (u16, u16),
    /// Padding around the grid.
    pub padding: (u32, u32),
}

/// Turns the state's placements into frame records: scaled from the terminal's cell size to the
/// font's, clipped to the grid (and a placeholder run's cells), with the source rectangle cropped to
/// match, offset by the padding, and sorted into the z groups.
pub fn image_records(placements: &[ImagePlacement], g: RecordGeometry) -> ImageGroups {
    let mut groups = ImageGroups::default();
    let sx = g.font_cell.0 as f64 / g.state_cell.0.max(1) as f64;
    let sy = g.font_cell.1 as f64 / g.state_cell.1.max(1) as f64;
    let grid =
        PxRect { x: 0, y: 0, w: g.grid.0 as i64 * g.font_cell.0 as i64, h: g.grid.1 as i64 * g.font_cell.1 as i64 };
    for p in placements {
        let dest = scale_rect(p.dest, sx, sy);
        let mut visible = intersect(dest, grid);
        if let Some(clip) = p.clip {
            visible = visible.and_then(|v| intersect(v, scale_rect(clip, sx, sy)));
        }
        let Some(v) = visible else { continue };
        let rec =
            ImageRecord { image: p.image.clone(), dest: area(v, g.padding), src: crop_src(p.src, dest, v), z: p.z };
        match p.z {
            z if z < Z_BELOW_BACKGROUND => groups.below.push(rec),
            z if z < 0 => groups.mid.push(rec),
            _ => groups.above.push(rec),
        }
    }
    groups
}

/// A rectangle scaled by `sx` and `sy` with its edges rounded to whole pixels; never empty.
fn scale_rect(r: PxRect, sx: f64, sy: f64) -> PxRect {
    let x0 = (r.x as f64 * sx).round() as i64;
    let y0 = (r.y as f64 * sy).round() as i64;
    let x1 = ((r.x + r.w) as f64 * sx).round() as i64;
    let y1 = ((r.y + r.h) as f64 * sy).round() as i64;
    PxRect { x: x0, y: y0, w: (x1 - x0).max(1), h: (y1 - y0).max(1) }
}

/// The overlap of two rectangles, or None when they do not overlap.
fn intersect(a: PxRect, b: PxRect) -> Option<PxRect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    (x1 > x0 && y1 > y0).then_some(PxRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// A pixel rectangle as frame pixels, offset by the padding.
fn area(r: PxRect, padding: (u32, u32)) -> Area {
    let (px, py) = (padding.0 as f32, padding.1 as f32);
    Area { x0: r.x as f32 + px, y0: r.y as f32 + py, x1: (r.x + r.w) as f32 + px, y1: (r.y + r.h) as f32 + py }
}

/// The part of the source rectangle that shows through when only `visible` of `dest` is drawn.
fn crop_src(src: (u32, u32, u32, u32), dest: PxRect, visible: PxRect) -> Area {
    let (sx, sy, sw, sh) = (src.0 as f32, src.1 as f32, src.2 as f32, src.3 as f32);
    let (dw, dh) = (dest.w as f32, dest.h as f32);
    let fx = |v: i64| sx + (v - dest.x) as f32 / dw * sw;
    let fy = |v: i64| sy + (v - dest.y) as f32 / dh * sh;
    Area { x0: fx(visible.x), y0: fy(visible.y), x1: fx(visible.x + visible.w), y1: fy(visible.y + visible.h) }
}
