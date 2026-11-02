//! Cell metrics: the cell size in whole pixels and where the baseline, underline and strike
//! through go. Cell advances are whole pixels so one cached glyph per style serves every cell.

use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

use crate::face::Face;

/// The geometry of one cell, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    /// Cell width, the advance of the primary font rounded to a whole pixel.
    pub width: u32,
    /// Cell height: ascent plus descent plus line gap, times the line height setting, rounded up.
    pub height: u32,
    /// Distance from the top of the cell to the baseline.
    pub baseline: f32,
    /// Distance from the top of the cell to the top of the underline.
    pub underline_position: f32,
    /// Underline thickness, at least one pixel.
    pub underline_thickness: f32,
    /// Distance from the top of the cell to the top of the strike through line.
    pub strike_position: f32,
    /// Strike through thickness, at least one pixel.
    pub strike_thickness: f32,
    /// Thickness of a light box drawing line, at least one pixel.
    pub line_thickness: u32,
}

/// Measures the cell for the primary face at `size_px`.
pub fn measure(face: &Face, size_px: f32, line_height: f32) -> CellMetrics {
    let Some(font) = face.font_ref() else { return fallback_metrics(size_px) };
    let size = Size::new(size_px);
    let m = font.metrics(size, LocationRef::default());
    let advance = cell_advance(&font, size).unwrap_or(size_px * 0.6);
    let natural = m.ascent - m.descent + m.leading;
    let height = (natural * line_height).ceil().max(1.0);
    // The extra space from line_height goes half above and half below the text.
    let baseline = ((height - natural) / 2.0 + m.ascent + m.leading / 2.0).round();
    let ul = m.underline.map(|d| (d.offset, d.thickness)).unwrap_or((-size_px * 0.1, size_px * 0.05));
    let st = m.strikeout.map(|d| (d.offset, d.thickness)).unwrap_or((size_px * 0.3, size_px * 0.05));
    let thickness = |t: f32| t.round().max(1.0);
    CellMetrics {
        width: advance.round().max(1.0) as u32,
        height: height as u32,
        baseline,
        underline_position: (baseline - ul.0).round(),
        underline_thickness: thickness(ul.1),
        strike_position: (baseline - st.0 - st.1 / 2.0).round(),
        strike_thickness: thickness(st.1),
        line_thickness: thickness(size_px / 14.0) as u32,
    }
}

/// The advance of `0` (or `M`) at `size`, which is every cell's width in a monospace font.
fn cell_advance(font: &FontRef<'_>, size: Size) -> Option<f32> {
    let glyphs = font.glyph_metrics(size, LocationRef::default());
    let charmap = font.charmap();
    ['0', 'M', 'x'].iter().find_map(|&c| charmap.map(c).and_then(|g| glyphs.advance_width(g)))
}

/// Proportions used when the font cannot be parsed, so a grid still has a sane cell.
fn fallback_metrics(size_px: f32) -> CellMetrics {
    let height = (size_px * 1.2).ceil();
    let baseline = (size_px * 0.95).round();
    CellMetrics {
        width: (size_px * 0.6).round().max(1.0) as u32,
        height: height as u32,
        baseline,
        underline_position: baseline + 1.0,
        underline_thickness: 1.0,
        strike_position: (baseline - size_px * 0.3).round(),
        strike_thickness: 1.0,
        line_thickness: 1,
    }
}
