//! Pixel goldens (`docs/TEST-SUITE.md` section 10, R2 and R3, and section 16 item 4).
//!
//! An image is compared with the accepted one under `corpus/render/<platform>/<name>.png` with a
//! tolerance: the largest per channel difference that counts as equal, and the fraction of pixels
//! allowed to exceed it (anti aliasing differs a little between machines). A failure writes
//! `<name>.new.png` and `<name>.diff.png` beside the accepted image (both gitignored) and reports
//! the row and column of the first differing cell. `TERMINUS_BLESS=1` accepts the new image.

use std::path::{Path, PathBuf};

use crate::Image;

/// How different two images may be and still match.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// A channel difference at or below this counts as equal.
    pub channel: u8,
    /// Fraction of pixels allowed to differ by more than `channel`.
    pub fraction: f32,
}

impl Tolerance {
    /// Exact match.
    pub const EXACT: Tolerance = Tolerance { channel: 0, fraction: 0.0 };
    /// CPU renderer against its own goldens: small anti aliasing differences between machines.
    pub const CPU: Tolerance = Tolerance { channel: 8, fraction: 0.002 };
    /// GPU renderer against the CPU image: wider, the rasterisers differ.
    pub const GPU: Tolerance = Tolerance { channel: 48, fraction: 0.03 };
}

/// The folder for this platform's accepted images.
pub fn platform_dir(corpus: &Path) -> PathBuf {
    corpus.join("render").join(std::env::consts::OS)
}

/// Compares two images; returns a description of the difference when they do not match.
pub fn compare(
    expected: &Image, actual: &Image, tol: Tolerance, cell: (u32, u32), padding: (u32, u32),
) -> Option<String> {
    if expected.width != actual.width || expected.height != actual.height {
        return Some(format!(
            "size {}x{} but expected {}x{}",
            actual.width, actual.height, expected.width, expected.height
        ));
    }
    let mut over = 0usize;
    let mut first = None;
    for (i, (a, e)) in actual.rgba.chunks_exact(4).zip(expected.rgba.chunks_exact(4)).enumerate() {
        let d = a.iter().zip(e).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if d > tol.channel {
            over += 1;
            first.get_or_insert(i);
        }
    }
    let total = (actual.width * actual.height) as usize;
    if over as f32 <= tol.fraction * total as f32 {
        return None;
    }
    let i = first.unwrap_or(0) as u32;
    let (x, y) = (i % actual.width, i / actual.width);
    let col = x.saturating_sub(padding.0) / cell.0.max(1);
    let row = y.saturating_sub(padding.1) / cell.1.max(1);
    Some(format!(
        "{over} of {total} pixels differ (allowed {:.0}); first at pixel {x},{y}, cell row {row} column {col}",
        tol.fraction * total as f32
    ))
}

/// Checks `actual` against the accepted image `name` in `dir`. Writes the new and difference
/// images on failure, or accepts `actual` when `TERMINUS_BLESS=1`. A missing accepted image is
/// written and the check fails, so a new case is reviewed before it is trusted.
pub fn check(
    dir: &Path, name: &str, actual: &Image, tol: Tolerance, cell: (u32, u32), padding: (u32, u32),
) -> Result<(), String> {
    let accepted = dir.join(format!("{name}.png"));
    let bless = std::env::var("TERMINUS_BLESS").is_ok_and(|v| v == "1");
    if bless {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        return write_png(&accepted, actual);
    }
    let Some(expected) = read_png(&accepted) else {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        write_png(&dir.join(format!("{name}.new.png")), actual)?;
        return Err(format!(
            "no accepted image {}; wrote {name}.new.png; review it and run with TERMINUS_BLESS=1",
            accepted.display()
        ));
    };
    match compare(&expected, actual, tol, cell, padding) {
        None => Ok(()),
        Some(why) => {
            write_png(&dir.join(format!("{name}.new.png")), actual)?;
            write_png(&dir.join(format!("{name}.diff.png")), &difference(&expected, actual))?;
            Err(format!("{name}: {why}"))
        }
    }
}

/// An image that is black where the two match and red where they differ, scaled by how much.
pub fn difference(a: &Image, b: &Image) -> Image {
    let mut rgba = Vec::with_capacity(a.rgba.len());
    for (x, y) in a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)) {
        let d = x.iter().zip(y).map(|(p, q)| p.abs_diff(*q)).max().unwrap_or(0);
        rgba.extend_from_slice(&[d.saturating_mul(4), 0, 0, 255]);
    }
    Image { width: a.width.min(b.width), height: a.height.min(b.height), rgba }
}

/// Writes an RGBA image as PNG.
pub fn write_png(path: &Path, img: &Image) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&img.rgba).map_err(|e| e.to_string())
}

/// Reads a PNG into RGBA, or None when it does not exist or is not RGBA8.
pub fn read_png(path: &Path) -> Option<Image> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(Image { width: info.width, height: info.height, rgba: buf })
}
