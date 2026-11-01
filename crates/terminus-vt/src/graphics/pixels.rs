//! Turning transmitted bytes into RGBA pixels: base64, zlib, raw RGB and RGBA, PNG, and rectangle blending.

use super::reply::KErr;
use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use std::io::Read;

/// Largest single payload accepted, encoded or decoded (65 MiB).
pub(crate) const MAX_PAYLOAD: usize = 65 * 1024 * 1024;

/// A decoded image: straight alpha RGBA8, row major.
#[derive(Debug)]
pub(crate) struct Decoded {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// What the control data says about the transmitted bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PixelSpec {
    /// `f`: 24, 32 or 100.
    pub format: u32,
    /// `s`: width for raw data.
    pub width: u32,
    /// `v`: height for raw data.
    pub height: u32,
    /// `o=z`: the bytes are zlib compressed.
    pub compressed: bool,
}

/// Limits applied while decoding.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodeLimits {
    /// Most pixels one image may have.
    pub max_pixels: u64,
    /// Most bytes of decoded pixels the store can ever hold.
    pub quota: usize,
}

/// A base64 engine that accepts input with or without padding, as clients differ on this.
fn lenient_engine() -> GeneralPurpose {
    GeneralPurpose::new(&STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent))
}

/// Decodes base64 text, ignoring ASCII whitespace, and refuses results over `MAX_PAYLOAD` bytes.
pub(crate) fn decode_base64(text: &[u8]) -> Result<Vec<u8>, KErr> {
    if text.len() / 4 * 3 > MAX_PAYLOAD + 3 {
        return Err(KErr::new("EFBIG", "Payload is too large"));
    }
    let clean: Vec<u8> = text.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    let bytes = lenient_engine().decode(&clean).map_err(|_| KErr::new("EINVAL", "Invalid base64 payload"))?;
    if bytes.len() > MAX_PAYLOAD {
        return Err(KErr::new("EFBIG", "Payload is too large"));
    }
    Ok(bytes)
}

/// Decompresses zlib data, reading at most `limit` bytes of output. More output than that is an error.
fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, KErr> {
    let mut out = Vec::new();
    let mut decoder = flate2::read::ZlibDecoder::new(data).take(limit as u64 + 1);
    decoder.read_to_end(&mut out).map_err(|_| KErr::new("EINVAL", "Failed to inflate zlib data"))?;
    if out.len() > limit {
        return Err(KErr::new("EFBIG", "Decompressed data is too large"));
    }
    Ok(out)
}

/// Decodes transmitted bytes (already base64 decoded or read from a file) according to `spec`.
pub(crate) fn decode_pixels(spec: &PixelSpec, data: Vec<u8>, limits: &DecodeLimits) -> Result<Decoded, KErr> {
    if !matches!(spec.format, 24 | 32 | 100) {
        return Err(KErr::new("EINVAL", format!("Unsupported format {}", spec.format)));
    }
    if spec.format == 100 {
        let data = if spec.compressed { inflate(&data, MAX_PAYLOAD)? } else { data };
        return decode_png(&data, limits);
    }
    let bpp = if spec.format == 24 { 3usize } else { 4 };
    let expected = check_raw_size(spec, bpp, limits)?;
    let data = if spec.compressed { inflate(&data, expected)? } else { data };
    if data.len() < expected {
        return Err(KErr::new("ENODATA", format!("Insufficient image data: got {} expected {}", data.len(), expected)));
    }
    if data.len() > expected {
        return Err(KErr::new("EINVAL", format!("Too much image data: got {} expected {}", data.len(), expected)));
    }
    Ok(Decoded { width: spec.width, height: spec.height, rgba: to_rgba(&data, bpp) })
}

/// Validates the dimensions of raw data and returns how many bytes the data must have.
fn check_raw_size(spec: &PixelSpec, bpp: usize, limits: &DecodeLimits) -> Result<usize, KErr> {
    if spec.width == 0 || spec.height == 0 {
        return Err(KErr::new("EINVAL", "Image width and height are required for raw data"));
    }
    check_dimensions(spec.width, spec.height, limits)?;
    Ok(spec.width as usize * spec.height as usize * bpp)
}

/// Rejects an image with too many pixels (EFBIG) or one that can never fit the quota (ENOSPC).
pub(crate) fn check_dimensions(width: u32, height: u32, limits: &DecodeLimits) -> Result<(), KErr> {
    let pixels = width as u64 * height as u64;
    if pixels > limits.max_pixels {
        return Err(KErr::new("EFBIG", format!("Image of {}x{} pixels is too large", width, height)));
    }
    if pixels * 4 > limits.quota as u64 {
        return Err(KErr::new("ENOSPC", "Image is larger than the storage quota"));
    }
    Ok(())
}

/// Converts RGB (3) or RGBA (4) bytes to RGBA.
fn to_rgba(data: &[u8], bpp: usize) -> Vec<u8> {
    if bpp == 4 {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len() / 3 * 4);
    for px in data.chunks_exact(3) {
        out.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    out
}

/// Decodes a PNG to RGBA8, refusing images over the pixel limit before allocating the pixels.
pub(crate) fn decode_png(data: &[u8], limits: &DecodeLimits) -> Result<Decoded, KErr> {
    let bad = |_| KErr::new("EINVAL", "Failed to decode PNG data");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits { bytes: limits.quota.max(1 << 20) });
    let mut reader = decoder.read_info().map_err(bad)?;
    let (width, height) = (reader.info().width, reader.info().height);
    check_dimensions(width, height, limits)?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(frame.buffer_size());
    let (color, _) = reader.output_color_type();
    Ok(Decoded { width, height, rgba: png_to_rgba(&buf, color) })
}

/// Expands decoded PNG rows (8 bit samples) of the given colour type to RGBA.
fn png_to_rgba(buf: &[u8], color: png::ColorType) -> Vec<u8> {
    match color {
        png::ColorType::Rgba => buf.to_vec(),
        png::ColorType::Rgb => to_rgba(buf, 3),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
    }
}

/// Where a rectangle sits inside an RGBA canvas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    /// Left edge in pixels.
    pub x: u32,
    /// Top edge in pixels.
    pub y: u32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

/// Copies or alpha blends a rectangle of `src` (row width `src_w`) onto `dst` (row width `dst_w`).
/// With `replace` the destination pixels are overwritten, otherwise the source is blended over them.
pub(crate) fn blend_rect(dst: &mut [u8], dst_w: u32, to: Rect, src: &[u8], src_w: u32, from: Rect, replace: bool) {
    for row in 0..to.h {
        let d = ((to.y + row) as usize * dst_w as usize + to.x as usize) * 4;
        let s = ((from.y + row) as usize * src_w as usize + from.x as usize) * 4;
        let n = to.w as usize * 4;
        if replace {
            dst[d..d + n].copy_from_slice(&src[s..s + n]);
            continue;
        }
        for px in 0..to.w as usize {
            let (di, si) = (d + px * 4, s + px * 4);
            let blended = blend_pixel(
                [dst[di], dst[di + 1], dst[di + 2], dst[di + 3]],
                [src[si], src[si + 1], src[si + 2], src[si + 3]],
            );
            dst[di..di + 4].copy_from_slice(&blended);
        }
    }
}

/// Alpha blends one straight alpha RGBA pixel `src` over `dst`.
pub(crate) fn blend_pixel(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let (sa, da) = (src[3] as u32, dst[3] as u32);
    if sa == 255 || da == 0 {
        return src;
    }
    if sa == 0 {
        return dst;
    }
    let out_a = sa * 255 + da * (255 - sa);
    let mut out = [0u8; 4];
    for c in 0..3 {
        let num = src[c] as u32 * sa * 255 + dst[c] as u32 * da * (255 - sa);
        out[c] = ((num + out_a / 2) / out_a) as u8;
    }
    out[3] = ((out_a + 127) / 255) as u8;
    out
}

/// A canvas of `width * height` pixels all set to the RGBA colour in `y` (0xRRGGBBAA, the `Y` key).
pub(crate) fn filled_canvas(width: u32, height: u32, color: u32) -> Vec<u8> {
    let px = color.to_be_bytes();
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..width as usize * height as usize {
        out.extend_from_slice(&px);
    }
    out
}
