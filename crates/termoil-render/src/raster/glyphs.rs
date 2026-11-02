//! The glyph cache of the raster renderer: each glyph is rasterized once by `vello_cpu` into a
//! small scratch pixmap, cropped to its ink, and kept as a coverage mask (or as premultiplied
//! pixels for a colour face) until the cache is full.
//!
//! This replaces glifo's glyph atlas, whose first page alone is 4096 by 4096 pixels (64 MiB) and
//! cannot be made smaller in vello_cpu 0.3 (task-2481).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use termoil_font::FaceId;
use vello_common::kurbo::Diagonal2;
use vello_common::peniko::FontData;
use vello_cpu::{Level, Pixmap, RenderContext, RenderMode, RenderSettings, Resources};

use crate::draw::skew_transform;
use crate::frame::{GlyphPos, GlyphRun};

/// Subpixel positions per pixel a glyph is rasterized at, horizontally and vertically.
const SUBPIXEL_STEPS: f32 = 4.0;

/// What identifies one rasterized glyph.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Key {
    face: FaceId,
    id: u32,
    size_bits: u32,
    embolden: bool,
    skew: bool,
    hint: bool,
    fx: u8,
    fy: u8,
}

/// A fast hash for the small fixed size keys of the cache (the multiply and rotate of rustc's
/// FxHasher); the standard hasher was a measurable part of drawing a frame of 12,000 glyphs.
#[derive(Default)]
pub struct FxHasher(u64);

impl Hasher for FxHasher {
    /// Mixes bytes in eight at a time.
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(b));
        }
    }

    /// Mixes one word in.
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    /// Mixes a smaller integer in.
    fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }

    /// Mixes a smaller integer in.
    fn write_u16(&mut self, v: u16) {
        self.write_u64(u64::from(v));
    }

    /// Mixes a byte in.
    fn write_u8(&mut self, v: u8) {
        self.write_u64(u64::from(v));
    }

    /// The hash.
    fn finish(&self) -> u64 {
        self.0
    }
}

/// A rasterized glyph's pixels.
pub enum Pixels {
    /// Coverage, one byte per pixel, drawn in the run's colour.
    Coverage(Vec<u8>),
    /// Premultiplied colour pixels with alpha in the top byte, drawn as they are (colour faces).
    Color(Vec<u32>),
}

/// A rasterized glyph: its pixels and where its top left sits relative to the pen position.
pub struct Mask {
    /// Offset of the left edge from the pen's whole pixel x.
    pub left: i32,
    /// Offset of the top edge from the baseline's whole pixel y.
    pub top: i32,
    /// Width.
    pub width: usize,
    /// Height.
    pub height: usize,
    /// The pixels.
    pub pixels: Pixels,
}

/// The scratch context glyphs are rasterized in, and the em size it was made for.
struct Scratch {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    size: u16,
}

/// Glyph masks by key, with the scratch context that makes them.
pub struct GlyphCache {
    map: HashMap<Key, Mask, BuildHasherDefault<FxHasher>>,
    scratch: Option<Scratch>,
    /// The most glyphs kept; the cache is emptied when it would hold more.
    capacity: usize,
}

impl GlyphCache {
    /// An empty cache that keeps at most `capacity` glyphs.
    pub fn new(capacity: usize) -> GlyphCache {
        GlyphCache { map: HashMap::default(), scratch: None, capacity: capacity.max(1) }
    }

    /// How many glyphs are cached.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// The mask for glyph `g` of `run` and the whole pixel pen position it is drawn at, rasterizing
    /// it on first use. `color` says the face has colour glyphs.
    pub fn place(
        &mut self, font: &FontData, coords: &[i16], run: &GlyphRun, g: &GlyphPos, hint: bool, color: bool,
    ) -> (&Mask, i32, i32) {
        let (x, fx) = split(g.x);
        let (y, fy) = split(g.y);
        let key = Key {
            face: run.face,
            id: g.id,
            size_bits: run.size.to_bits(),
            embolden: run.embolden,
            skew: run.skew,
            hint,
            fx,
            fy,
        };
        if !self.map.contains_key(&key) {
            if self.map.len() >= self.capacity {
                self.map.clear();
            }
            let mask = self.rasterize(font, coords, run, g.id, (fx, fy), hint, color);
            self.map.insert(key, mask);
        }
        (&self.map[&key], x, y)
    }

    /// Rasterizes one glyph at the subpixel offset `frac` (in steps) into a cropped mask.
    fn rasterize(
        &mut self, font: &FontData, coords: &[i16], run: &GlyphRun, id: u32, frac: (u8, u8), hint: bool, color: bool,
    ) -> Mask {
        let s = self.scratch_for(run.size);
        let em = f32::from(s.size);
        // The pen sits one em from the left and two from the top of a three em square.
        let origin = (em + f32::from(frac.0) / SUBPIXEL_STEPS, 2.0 * em + f32::from(frac.1) / SUBPIXEL_STEPS);
        s.ctx.reset();
        s.ctx.set_paint(vello_common::color::AlphaColor::from_rgba8(255, 255, 255, 255));
        let mut b = s
            .ctx
            .glyph_run(&mut s.resources, font)
            .font_size(run.size)
            .hint(hint)
            .atlas_cache(false)
            .normalized_coords(coords)
            .glyph_transform(skew_transform(run));
        if run.embolden {
            b = b.font_embolden(glifo::FontEmbolden::new(Diagonal2 { xx: run.size as f64 / 40.0, yy: 0.0 }));
        }
        let _ = b.fill_glyphs(std::iter::once(vello_cpu::Glyph { id, x: origin.0, y: origin.1 }));
        s.ctx.flush();
        s.pixmap.data_as_u8_slice_mut().fill(0);
        let settings = vello_cpu::RasterizerSettings { render_mode: RenderMode::OptimizeSpeed, ..Default::default() };
        s.ctx.render_with(&mut s.pixmap, &mut s.resources, settings);
        crop(&s.pixmap, (em as i32, 2 * em as i32), color)
    }

    /// The scratch context, made (or remade larger) for glyphs of `size` pixels.
    fn scratch_for(&mut self, size: f32) -> &mut Scratch {
        let em = (size.ceil() as u16).clamp(4, 2048);
        if self.scratch.as_ref().is_none_or(|s| s.size < em) {
            let side = em.saturating_mul(3);
            let settings = RenderSettings { level: Level::new(), num_threads: 0 };
            self.scratch = Some(Scratch {
                ctx: RenderContext::new_with(side, side, settings),
                resources: Resources::new(),
                pixmap: Pixmap::new(side, side),
                size: em,
            });
        }
        self.scratch.as_mut().expect("made above")
    }
}

/// A coordinate's whole pixel and its fraction in subpixel steps.
fn split(v: f32) -> (i32, u8) {
    let whole = v.floor();
    let steps = ((v - whole) * SUBPIXEL_STEPS).round();
    if steps >= SUBPIXEL_STEPS { (whole as i32 + 1, 0) } else { (whole as i32, steps as u8) }
}

/// The inked part of the scratch pixmap as a mask; `pen` is where the pen was, in whole pixels.
fn crop(pixmap: &Pixmap, pen: (i32, i32), color: bool) -> Mask {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if data[y * w + x].a != 0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x0 >= x1 {
        return Mask { left: 0, top: 0, width: 0, height: 0, pixels: Pixels::Coverage(Vec::new()) };
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let at = |x: usize, y: usize| data[(y0 + y) * w + x0 + x];
    let pixels = if color {
        let px = (0..ch).flat_map(|y| (0..cw).map(move |x| (y, x)));
        Pixels::Color(
            px.map(|(y, x)| at(x, y))
                .map(|p| (u32::from(p.a) << 24) | (u32::from(p.r) << 16) | (u32::from(p.g) << 8) | u32::from(p.b))
                .collect(),
        )
    } else {
        Pixels::Coverage((0..ch).flat_map(|y| (0..cw).map(move |x| (y, x))).map(|(y, x)| at(x, y).a).collect())
    };
    Mask { left: x0 as i32 - pen.0, top: y0 as i32 - pen.1, width: cw, height: ch, pixels }
}
