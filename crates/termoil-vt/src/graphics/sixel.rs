//! Sixel decoding (DEC VT340 graphics) into an RGBA image placed at the cursor.
//!
//! Supported: raster attributes `"Pan;Pad;Ph;Pv`, colour introducers `#Pc;Pu;Px;Py;Pz` in HLS and RGB, repeat
//! `!Pn`, graphics carriage return `$`, graphics new line `-`, the background select parameter P2 and the
//! pixel aspect ratio P1. The palette has 256 registers and starts as the VT340 default colours for every image.
//! Data that is cut off, or has no terminator, still gives the part that was decoded.

use super::pixels::DecodeLimits;
use super::place::{NewImage, PlaceParams};
use super::{GraphicsContext, GraphicsResult, GraphicsStore};

/// Largest width or height in sixel pixels the decoder will draw.
const MAX_DIMENSION: usize = 16_384;

/// Most times a sixel pixel is repeated to honour a non square aspect ratio.
const MAX_ASPECT_REPEAT: usize = 8;

/// The VT340 default colour map for registers 0 to 15, as percentages of red, green and blue.
const DEFAULT_PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [20, 20, 80],
    [80, 13, 13],
    [20, 80, 20],
    [80, 20, 80],
    [20, 80, 80],
    [80, 80, 20],
    [53, 53, 53],
    [26, 26, 26],
    [33, 33, 60],
    [60, 26, 26],
    [33, 60, 33],
    [60, 33, 60],
    [33, 60, 60],
    [60, 60, 33],
    [80, 80, 80],
];

/// A decoded sixel image.
pub(super) struct SixelImage {
    /// Width in pixels after the aspect ratio was applied.
    pub width: u32,
    /// Height in pixels after the aspect ratio was applied.
    pub height: u32,
    /// RGBA pixels.
    pub rgba: Vec<u8>,
}

/// Decoder state while walking the data.
struct Decoder {
    palette: Vec<[u8; 3]>,
    color: usize,
    x: usize,
    band: usize,
    canvas: Vec<u8>,
    stride: usize,
    rows: usize,
    extent: (usize, usize),
    raster: (usize, usize),
    aspect: Option<(usize, usize)>,
    pixel_budget: usize,
}

impl Decoder {
    /// A decoder with the default palette that stops growing past `pixel_budget` pixels.
    fn new(pixel_budget: usize) -> Decoder {
        let mut palette = vec![[0u8; 3]; 256];
        for (i, c) in DEFAULT_PALETTE.iter().enumerate() {
            palette[i] = [percent(c[0]), percent(c[1]), percent(c[2])];
        }
        Decoder {
            palette,
            color: 0,
            x: 0,
            band: 0,
            canvas: Vec::new(),
            stride: 0,
            rows: 0,
            extent: (0, 0),
            raster: (0, 0),
            aspect: None,
            pixel_budget,
        }
    }

    /// The widest and tallest the image may be right now.
    fn limits(&self) -> (usize, usize) {
        let w = if self.raster.0 > 0 { self.raster.0 } else { MAX_DIMENSION };
        let h = if self.raster.1 > 0 { self.raster.1 } else { MAX_DIMENSION };
        (w.min(MAX_DIMENSION), h.min(MAX_DIMENSION))
    }

    /// Grows the canvas so it holds at least `w` by `h` pixels. Returns false when that is over the budget.
    fn ensure(&mut self, w: usize, h: usize) -> bool {
        let new_stride = if w > self.stride { w.max(self.stride * 2).min(MAX_DIMENSION) } else { self.stride };
        let new_rows = h.max(self.rows);
        if new_stride == self.stride && new_rows == self.rows {
            return true;
        }
        if new_stride * new_rows > self.pixel_budget {
            return false;
        }
        let mut grown = vec![0u8; new_stride * new_rows * 4];
        for row in 0..self.rows {
            let (from, to) = (row * self.stride * 4, row * new_stride * 4);
            grown[to..to + self.stride * 4].copy_from_slice(&self.canvas[from..from + self.stride * 4]);
        }
        self.canvas = grown;
        self.stride = new_stride;
        self.rows = new_rows;
        true
    }

    /// Draws `count` columns of the sixel `bits` (bit 0 is the top pixel) at the current position.
    fn draw(&mut self, bits: u8, count: usize) {
        let count = count.clamp(1, MAX_DIMENSION);
        let (max_w, max_h) = self.limits();
        let x_end = (self.x + count).min(max_w);
        if self.x >= max_w {
            self.x += count;
            return;
        }
        self.extent.0 = self.extent.0.max(x_end);
        let top = self.band * 6;
        if bits != 0 && top < max_h {
            let y_end = (top + 6).min(max_h);
            if self.ensure(x_end, y_end) {
                self.paint(bits, self.x, x_end, top, y_end);
                self.extent.1 = self.extent.1.max(y_end);
            }
        }
        self.x += count;
    }

    /// Writes the current colour into the pixels of `bits` between the given columns.
    fn paint(&mut self, bits: u8, x0: usize, x1: usize, top: usize, y_end: usize) {
        let c = self.palette[self.color];
        let rgba = [c[0], c[1], c[2], 255];
        for (bit, y) in (top..y_end).enumerate() {
            if bits & (1 << bit) == 0 {
                continue;
            }
            for x in x0..x1 {
                let at = (y * self.stride + x) * 4;
                self.canvas[at..at + 4].copy_from_slice(&rgba);
            }
        }
    }
}

/// Converts a 0 to 100 percentage to 0 to 255.
fn percent(v: u8) -> u8 {
    ((v.min(100) as u32 * 255 + 50) / 100) as u8
}

/// Reads a `;` separated list of numbers starting at `pos`, skipping whitespace. Returns the numbers and the
/// position of the first byte that is not part of the list.
fn read_numbers(data: &[u8], mut pos: usize) -> (Vec<u32>, usize) {
    let mut numbers = Vec::new();
    let mut current: Option<u32> = None;
    while pos < data.len() {
        match data[pos] {
            d @ b'0'..=b'9' => {
                current = Some(current.unwrap_or(0).saturating_mul(10).saturating_add((d - b'0') as u32))
            }
            b';' => numbers.push(current.take().unwrap_or(0)),
            b' ' | b'\n' | b'\r' | b'\t' => {}
            _ => break,
        }
        pos += 1;
    }
    if let Some(n) = current {
        numbers.push(n);
    }
    (numbers, pos)
}

/// Defines or selects a colour register: `#Pc` selects, `#Pc;Pu;Px;Py;Pz` defines (Pu 1 is HLS, 2 is RGB).
fn set_color(d: &mut Decoder, args: &[u32]) {
    let Some(&index) = args.first() else { return };
    let index = (index as usize) & 255;
    d.color = index;
    if args.len() < 5 {
        return;
    }
    let (x, y, z) = (args[2].min(360), args[3].min(100), args[4].min(100));
    d.palette[index] = match args[1] {
        1 => hls_to_rgb(x, y, z),
        2 => [percent(x.min(100) as u8), percent(y as u8), percent(z as u8)],
        _ => d.palette[index],
    };
}

/// Converts a DEC HLS colour (hue 0 is blue, lightness and saturation in percent) to RGB bytes.
fn hls_to_rgb(hue: u32, light: u32, sat: u32) -> [u8; 3] {
    let h = ((hue + 240) % 360) as f32 / 60.0;
    let (l, s) = (light as f32 / 100.0, sat as f32 / 100.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [byte(r), byte(g), byte(b)]
}

/// The pixel aspect (vertical, horizontal) the P1 parameter of the DCS selects. 0 and omitted mean the
/// device default, which this decoder treats as square pixels as modern terminals do.
fn aspect_from_p1(p1: u16) -> (usize, usize) {
    match p1 {
        1 | 5 | 6 => (2, 1),
        2 => (5, 1),
        3 | 4 => (3, 2),
        _ => (1, 1),
    }
}

/// Walks the sixel data and fills the decoder.
fn run(d: &mut Decoder, data: &[u8]) {
    let mut pos = 0;
    let mut repeat = 1usize;
    while pos < data.len() {
        let byte = data[pos];
        pos += 1;
        match byte {
            b'?'..=b'~' => {
                d.draw(byte - b'?', repeat);
                repeat = 1;
            }
            b'!' => {
                let (n, next) = read_numbers(data, pos);
                repeat = n.first().copied().unwrap_or(1).max(1) as usize;
                pos = next;
            }
            b'#' => {
                let (args, next) = read_numbers(data, pos);
                set_color(d, &args);
                pos = next;
            }
            b'"' => {
                let (args, next) = read_numbers(data, pos);
                raster_attributes(d, &args);
                pos = next;
            }
            b'$' => d.x = 0,
            b'-' => {
                d.band += 1;
                d.x = 0;
            }
            0x18 | 0x1b | 0x9c => break,
            _ => {}
        }
    }
}

/// Applies raster attributes: aspect Pan:Pad (first one wins) and the image size Ph by Pv (largest wins).
fn raster_attributes(d: &mut Decoder, args: &[u32]) {
    let get = |i: usize| args.get(i).copied().unwrap_or(0) as usize;
    if d.aspect.is_none() && get(0) > 0 && get(1) > 0 {
        d.aspect = Some((get(0), get(1)));
    }
    if get(2) > 0 && get(3) > 0 {
        d.raster = (d.raster.0.max(get(2)), d.raster.1.max(get(3)));
    }
}

/// Decodes a sixel image. `params` are the DCS parameters before `q`. Returns `None` for an empty image.
pub(super) fn decode_sixel(params: &[u16], data: &[u8], limits: &DecodeLimits) -> Option<SixelImage> {
    let budget = (limits.max_pixels.min(limits.quota as u64 / 4) as usize).max(1);
    let mut d = Decoder::new(budget);
    run(&mut d, data);
    let (w, h) = if d.raster.0 > 0 && d.raster.1 > 0 { d.raster } else { d.extent };
    let w = w.min(MAX_DIMENSION).min(budget);
    let h = h.min(MAX_DIMENSION).min((budget / w.max(1)).max(1));
    if w == 0 || h == 0 {
        return None;
    }
    let (v_rep, h_rep) =
        aspect_repeat(d.aspect.unwrap_or_else(|| aspect_from_p1(params.first().copied().unwrap_or(0))), w, h, budget);
    let transparent = params.get(1).copied() == Some(1);
    Some(assemble(&d, (w, h), (h_rep, v_rep), transparent))
}

/// Turns an aspect ratio into whole repeat counts (vertical, horizontal), capped and kept inside the budget.
fn aspect_repeat(aspect: (usize, usize), w: usize, h: usize, budget: usize) -> (usize, usize) {
    let round = |a: usize, b: usize| ((a as f32 / b as f32).round() as usize).clamp(1, MAX_ASPECT_REPEAT);
    let (mut v, mut hh) = (round(aspect.0, aspect.1), round(aspect.1, aspect.0));
    if aspect.0 >= aspect.1 {
        hh = 1;
    } else {
        v = 1;
    }
    while (v > 1 || hh > 1) && w * hh * h * v > budget {
        v = v.saturating_sub(1).max(1);
        hh = hh.saturating_sub(1).max(1);
    }
    (v, hh)
}

/// Builds the final RGBA buffer: crops or pads to `size`, repeats pixels for the aspect, and fills the
/// pixels nothing drew with register 0 (P2 of 0 or 2) or leaves them transparent (P2 of 1).
fn assemble(d: &Decoder, size: (usize, usize), rep: (usize, usize), transparent: bool) -> SixelImage {
    let (w, h) = size;
    let bg = d.palette[0];
    let (out_w, out_h) = (w * rep.0, h * rep.1);
    let mut rgba = vec![0u8; out_w * out_h * 4];
    for y in 0..out_h {
        let sy = y / rep.1;
        for x in 0..out_w {
            let sx = x / rep.0;
            let drawn = sx < d.stride && sy < d.rows;
            let px =
                if drawn { &d.canvas[(sy * d.stride + sx) * 4..(sy * d.stride + sx) * 4 + 4] } else { &[0u8; 4][..] };
            let at = (y * out_w + x) * 4;
            let value = if px[3] != 0 {
                [px[0], px[1], px[2], 255]
            } else if transparent {
                [0, 0, 0, 0]
            } else {
                [bg[0], bg[1], bg[2], 255]
            };
            rgba[at..at + 4].copy_from_slice(&value);
        }
    }
    SixelImage { width: out_w as u32, height: out_h as u32, rgba }
}

impl GraphicsStore {
    /// Handles a sixel image: the DCS parameters and the payload after `q`. The image is stored and placed at
    /// the cursor; the cursor then goes to column 0 of the line after the image.
    pub fn sixel(&mut self, params: &[u16], data: &[u8], ctx: &GraphicsContext) -> GraphicsResult {
        let Some(image) = decode_sixel(params, data, &self.decode_limits()) else {
            return GraphicsResult::default();
        };
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
        let params = PlaceParams::default();
        match self.place_image(id, &params, ctx) {
            Ok(placed) => {
                let rows = placed.move_cursor.map(|(_, r)| r).unwrap_or(1);
                GraphicsResult {
                    move_cursor: Some((0, rows)),
                    cursor_to_column_zero: true,
                    dirty: true,
                    ..GraphicsResult::default()
                }
            }
            Err(_) => GraphicsResult::default(),
        }
    }
}
