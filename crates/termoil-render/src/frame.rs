//! The frame: what a renderer draws, as plain data.
//!
//! The frame builder turns a [`crate::RenderState`] into a frame without knowing how it will be
//! drawn, so the vello GPU, vello CPU and (if ever built) atlas renderers differ only in how they
//! draw these records, and the builder can be tested as data (R1).

use std::sync::Arc;

use termoil_font::FaceId;

use crate::images::ImageData;

/// An RGBA colour, straight alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha.
    pub a: u8,
}

impl Rgba {
    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }
}

/// A horizontal span of cells filled with one background colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BgSpan {
    /// Left edge in pixels.
    pub x0: f32,
    /// Right edge in pixels.
    pub x1: f32,
    /// Colour.
    pub color: Rgba,
}

/// One positioned glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphPos {
    /// Glyph id in the run's face.
    pub id: u32,
    /// Pen x in pixels.
    pub x: f32,
    /// Baseline y in pixels.
    pub y: f32,
}

/// Glyphs of one face, size and colour.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    /// The face.
    pub face: FaceId,
    /// Font size in pixels.
    pub size: f32,
    /// Text colour (ignored for colour glyphs, which paint their own).
    pub color: Rgba,
    /// Draw bold by emboldening (the family has no bold face).
    pub embolden: bool,
    /// Draw italic by skewing (the family has no italic face).
    pub skew: bool,
    /// The glyphs.
    pub glyphs: Vec<GlyphPos>,
}

/// A character drawn as geometry (box drawing, blocks, braille, powerline).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteCell {
    /// The character.
    pub ch: char,
    /// Left edge of the cell.
    pub x: f32,
    /// Top edge of the cell.
    pub y: f32,
    /// Colour.
    pub color: Rgba,
}

/// The decorations a cell can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    /// Single underline.
    Underline,
    /// Double underline.
    DoubleUnderline,
    /// Curly underline.
    CurlyUnderline,
    /// Dotted underline.
    DottedUnderline,
    /// Dashed underline.
    DashedUnderline,
    /// Strike through.
    Strike,
    /// Overline.
    Overline,
}

/// A decoration line across a span of cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    /// What it is.
    pub kind: DecorationKind,
    /// Left edge.
    pub x0: f32,
    /// Right edge.
    pub x1: f32,
    /// Top of the line.
    pub y: f32,
    /// Thickness.
    pub thickness: f32,
    /// Colour.
    pub color: Rgba,
}

/// Everything drawn on one row.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RowFrame {
    /// Row index.
    pub row: u16,
    /// Top of the row in pixels.
    pub y: f32,
    /// Background spans that differ from the window background.
    pub backgrounds: Vec<BgSpan>,
    /// Text.
    pub runs: Vec<GlyphRun>,
    /// Sprite characters.
    pub sprites: Vec<SpriteCell>,
    /// Underlines, strike through and overlines.
    pub decorations: Vec<Decoration>,
}

/// How the cursor is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    /// A filled block; the glyph under it is drawn in `text_color`.
    Block,
    /// An outline, for an unfocused window.
    Hollow,
    /// A vertical bar at the left edge.
    Bar,
    /// A line at the bottom.
    Underline,
}

/// The cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorFrame {
    /// Style.
    pub style: CursorStyle,
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width (two cells on a wide character).
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Colour.
    pub color: Rgba,
}

/// A rectangle in pixels: left, top, right and bottom edges.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Area {
    /// Left edge.
    pub x0: f32,
    /// Top edge.
    pub y0: f32,
    /// Right edge.
    pub x1: f32,
    /// Bottom edge.
    pub y1: f32,
}

/// A filled shape drawn by the window's own controls (the tab bar), above the cell backgrounds and
/// below the text, in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A rectangle with rounded corners.
    RoundRect {
        /// Where it is.
        area: Area,
        /// Corner radius in pixels.
        radius: f32,
        /// Fill colour.
        color: Rgba,
    },
    /// A straight line with square ends.
    Line {
        /// Start point.
        from: (f32, f32),
        /// End point.
        to: (f32, f32),
        /// Thickness in pixels.
        width: f32,
        /// Colour.
        color: Rgba,
    },
}

/// One image to draw: a rectangle of a decoded image stretched over a rectangle of the frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRecord {
    /// The decoded pixels, shared with the renderer's cache (keyed by id and generation).
    pub image: Arc<ImageData>,
    /// Where it is drawn in frame pixels, already clipped to the grid and offset by the padding.
    pub dest: Area,
    /// The part of the image drawn, in image pixels. Its size relative to `dest` gives the scale.
    pub src: Area,
    /// The z index the program gave the placement.
    pub z: i32,
}

/// A whole frame.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Frame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Cell width in pixels.
    pub cell_width: u32,
    /// Cell height in pixels.
    pub cell_height: u32,
    /// Window background.
    pub background: Rgba,
    /// Rows in the order drawn. A frame built for damage holds only the rows that changed.
    pub rows: Vec<RowFrame>,
    /// The cursor, if shown.
    pub cursor: Option<CursorFrame>,
    /// Text drawn over the cursor block, in the cursor's text colour.
    pub cursor_text: Option<GlyphRun>,
    /// Padding around the grid in pixels.
    pub padding: (u32, u32),
    /// Images drawn above the window background and below the cell backgrounds (z below -1073741824).
    pub images_below: Vec<ImageRecord>,
    /// Images drawn above the cell backgrounds and below the text (negative z).
    pub images_mid: Vec<ImageRecord>,
    /// Images drawn above the text (z of zero or more), below the cursor.
    pub images_above: Vec<ImageRecord>,
    /// Id and generation of every image the terminal still holds, on screen or not. A renderer
    /// drops cached copies of images that no frame lists here.
    pub live_images: Vec<(u32, u64)>,
    /// Shapes drawn above the cell backgrounds and below the text: the tab bar's tabs and buttons.
    pub shapes: Vec<Shape>,
}

impl Frame {
    /// Number of glyphs in the frame (for stats and tests).
    pub fn glyph_count(&self) -> usize {
        self.rows.iter().flat_map(|r| &r.runs).map(|r| r.glyphs.len()).sum()
    }

    /// Whether the frame draws any image under the text, which changes how the rows are drawn.
    pub fn has_images_under_text(&self) -> bool {
        !self.images_below.is_empty() || !self.images_mid.is_empty() || !self.shapes.is_empty()
    }
}
