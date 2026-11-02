//! iTerm2 inline images (OSC 1337 `File=`): PNG only.
//!
//! Format: `name=<base64>;size=<bytes>;width=<n>;height=<n>;preserveAspectRatio=<0|1>;inline=<0|1>:<base64 file>`.
//! `width` and `height` are `N` (cells), `Npx`, `N%` (of the terminal size) or `auto`.

use super::pixels::{decode_base64, decode_png};
use super::place::{NewImage, PlaceParams};
use super::{GraphicsContext, GraphicsResult, GraphicsStore};

/// A width or height request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dim {
    Auto,
    Cells(u32),
    Pixels(u32),
    Percent(u32),
}

/// The parsed arguments before the `:`.
#[derive(Debug)]
struct Args {
    width: Dim,
    height: Dim,
    preserve: bool,
    inline: bool,
}

/// Parses one dimension value.
fn parse_dim(text: &str) -> Dim {
    let number = |s: &str| s.trim().parse::<u32>().ok();
    if let Some(n) = text.strip_suffix("px").and_then(number) {
        return Dim::Pixels(n);
    }
    if let Some(n) = text.strip_suffix('%').and_then(number) {
        return Dim::Percent(n);
    }
    number(text).map(Dim::Cells).unwrap_or(Dim::Auto)
}

/// Parses the `key=value;...` list.
fn parse_args(text: &str) -> Args {
    let mut args = Args { width: Dim::Auto, height: Dim::Auto, preserve: true, inline: false };
    for part in text.split(';') {
        let Some((key, value)) = part.split_once('=') else { continue };
        match key {
            "width" => args.width = parse_dim(value),
            "height" => args.height = parse_dim(value),
            "preserveAspectRatio" => args.preserve = value != "0",
            "inline" => args.inline = value == "1",
            _ => {}
        }
    }
    args
}

/// Converts a dimension to pixels, or `None` for auto. `cell` and `whole` are the cell and terminal sizes.
fn to_pixels(dim: Dim, cell: u64, whole: u64) -> Option<u64> {
    match dim {
        Dim::Auto => None,
        Dim::Cells(n) => Some(n as u64 * cell),
        Dim::Pixels(n) => Some(n as u64),
        Dim::Percent(p) => Some(p as u64 * whole / 100),
    }
}

/// Works out the size in pixels the image is shown at.
fn target_size(args: &Args, image: (u32, u32), ctx: &GraphicsContext) -> (u64, u64) {
    let (cw, ch) = (ctx.cell_px.0.max(1) as u64, ctx.cell_px.1.max(1) as u64);
    let (iw, ih) = (image.0.max(1) as u64, image.1.max(1) as u64);
    let tw = to_pixels(args.width, cw, ctx.cols as u64 * cw);
    let th = to_pixels(args.height, ch, ctx.rows as u64 * ch);
    match (tw, th) {
        (None, None) => (iw, ih),
        (Some(w), None) => (w.max(1), (w * ih / iw).max(1)),
        (None, Some(h)) => ((h * iw / ih).max(1), h.max(1)),
        (Some(w), Some(h)) if args.preserve => {
            let scale_w = w * ih <= h * iw;
            if scale_w { (w.max(1), (w * ih / iw).max(1)) } else { ((h * iw / ih).max(1), h.max(1)) }
        }
        (Some(w), Some(h)) => (w.max(1), h.max(1)),
    }
}

impl GraphicsStore {
    /// Handles an iTerm2 inline image: the OSC 1337 payload after `File=`. The image is stored and placed at the
    /// cursor like a kitty `a=T`, and the cursor moves right by its columns and down by its rows. Only PNG
    /// files are decoded; any other format is dropped and counted.
    pub fn iterm2_file(&mut self, payload: &[u8], ctx: &GraphicsContext) -> GraphicsResult {
        let Some(colon) = payload.iter().position(|&b| b == b':') else {
            self.count_dropped();
            return GraphicsResult::default();
        };
        let args = parse_args(&String::from_utf8_lossy(&payload[..colon]));
        if !args.inline {
            return GraphicsResult::default();
        }
        let decoded = decode_base64(&payload[colon + 1..]).and_then(|bytes| decode_png(&bytes, &self.decode_limits()));
        let Ok(image) = decoded else {
            self.count_dropped();
            return GraphicsResult::default();
        };
        self.place_iterm_image(image, &args, ctx)
    }

    /// Stores a decoded iTerm2 image and places it using the requested size.
    fn place_iterm_image(
        &mut self, image: super::pixels::Decoded, args: &Args, ctx: &GraphicsContext,
    ) -> GraphicsResult {
        let (w, h) = target_size(args, (image.width, image.height), ctx);
        let (cw, ch) = (ctx.cell_px.0.max(1) as u64, ctx.cell_px.1.max(1) as u64);
        let new = NewImage {
            id: 0,
            number: 0,
            width: image.width,
            height: image.height,
            rgba: image.rgba,
            persistent: false,
            transient: false,
        };
        let Ok(id) = self.insert_image(new) else {
            self.count_dropped();
            return GraphicsResult::default();
        };
        let params = PlaceParams {
            cols: w.div_ceil(cw).min(u16::MAX as u64) as u32,
            rows: h.div_ceil(ch).min(u16::MAX as u64) as u32,
            stretch: !args.preserve,
            ..PlaceParams::default()
        };
        match self.place_image(id, &params, ctx) {
            Ok(placed) => GraphicsResult { move_cursor: placed.move_cursor, dirty: true, ..GraphicsResult::default() },
            Err(_) => GraphicsResult::default(),
        }
    }
}
