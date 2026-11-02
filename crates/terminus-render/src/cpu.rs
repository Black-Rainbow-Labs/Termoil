//! `VelloCpuRenderer`: draws frames into a pixmap with `vello_cpu`. Used when no GPU adapter is
//! usable, when the configuration asks for it, and by the pixel tests. The app presents the pixmap
//! with `softbuffer`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use terminus_font::FontGrid;
use vello_common::color::AlphaColor;
use vello_common::kurbo::{BezPath, Rect};
use vello_common::peniko::{FontData, ImageAlphaType, ImageSampler};
use vello_common::pixmap::PixelMetadata;
use vello_cpu::{
    Image as PaintImage, ImageSource, Level, Pixmap, RenderContext, RenderMode, RenderSettings, Resources,
};

use crate::draw::{Canvas, Layer, draw_layers, hint_enabled, image_paint_transform, image_rect, skew_transform};
use crate::frame::{GlyphRun, ImageRecord, Rgba};
use crate::images::ImageData;
use crate::{FrameStats, Image, RenderError, Renderer};

/// The CPU renderer.
pub struct VelloCpuRenderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    atlas: bool,
    images: HashMap<(u32, u64), Arc<Pixmap>>,
}

impl VelloCpuRenderer {
    /// A renderer for a `width` by `height` frame. `threads` above zero renders on that many
    /// extra threads; `atlas` turns on glifo's glyph atlas.
    pub fn new(width: u32, height: u32, threads: u16, atlas: bool) -> VelloCpuRenderer {
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        let settings = RenderSettings { level: Level::new(), num_threads: threads };
        VelloCpuRenderer {
            ctx: RenderContext::new_with(w, h, settings),
            resources: Resources::new(),
            pixmap: Pixmap::new(w, h),
            atlas,
            images: HashMap::new(),
        }
    }

    /// How many decoded images the renderer holds (for tests).
    pub fn cached_images(&self) -> usize {
        self.images.len()
    }

    /// The rendered pixels, premultiplied RGBA8, for presenting.
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }
}

/// A frame dimension clamped to what vello accepts.
fn clamp_dim(v: u32) -> u16 {
    v.clamp(1, u16::MAX as u32) as u16
}

/// Converts a frame colour to vello's.
fn color(c: Rgba) -> AlphaColor<vello_common::color::Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// The canvas over a CPU render context.
struct CpuCanvas<'a> {
    ctx: &'a mut RenderContext,
    resources: &'a mut Resources,
    atlas: bool,
    images: &'a mut HashMap<(u32, u64), Arc<Pixmap>>,
}

/// A premultiplied pixmap of an image, or None when the image is empty or too large for vello.
fn pixmap_of(image: &ImageData) -> Option<Arc<Pixmap>> {
    let (w, h) = (u16::try_from(image.width).ok()?, u16::try_from(image.height).ok()?);
    if w == 0 || h == 0 || image.rgba.len() != w as usize * h as usize * 4 {
        return None;
    }
    let meta = PixelMetadata { may_have_transparency: true, alpha_type: ImageAlphaType::Alpha };
    Some(Arc::new(Pixmap::from_parts(image.rgba.as_ref().clone(), w, h, meta)))
}

impl Canvas for CpuCanvas<'_> {
    /// Sets the paint colour.
    fn paint(&mut self, c: Rgba) {
        self.ctx.set_paint(color(c));
    }

    /// Fills a rectangle.
    fn rect(&mut self, r: Rect) {
        self.ctx.fill_rect(&r);
    }

    /// Fills a path.
    fn path(&mut self, p: &BezPath) {
        self.ctx.fill_path(p);
    }

    /// Draws glyphs through glifo.
    fn glyphs(&mut self, font: &FontData, run: &GlyphRun) {
        let glyphs = run.glyphs.iter().map(|g| vello_cpu::Glyph { id: g.id, x: g.x, y: g.y });
        let mut b = self
            .ctx
            .glyph_run(self.resources, font)
            .font_size(run.size)
            .hint(hint_enabled())
            .atlas_cache(self.atlas)
            .glyph_transform(skew_transform(run));
        if run.embolden {
            b = b.font_embolden(glifo::FontEmbolden::new(vello_common::kurbo::Diagonal2 {
                xx: run.size as f64 / 40.0,
                yy: 0.0,
            }));
        }
        let _ = b.fill_glyphs(glyphs);
    }

    /// Sets the context's transform.
    fn transform(&mut self, t: vello_common::kurbo::Affine) {
        self.ctx.set_transform(t);
    }

    /// Fills the destination rectangle with the image as a paint, decoding it once per generation.
    fn image(&mut self, rec: &ImageRecord) {
        let key = (rec.image.id, rec.image.generation);
        let pixmap = match self.images.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let Some(pixmap) = pixmap_of(&rec.image) else { return };
                e.insert(pixmap).clone()
            }
        };
        self.ctx.set_paint(PaintImage { image: ImageSource::Pixmap(pixmap), sampler: ImageSampler::default() });
        self.ctx.set_paint_transform(image_paint_transform(rec));
        self.ctx.fill_rect(&image_rect(rec));
        self.ctx.reset_paint_transform();
    }

    /// Drops cached images the terminal no longer holds.
    fn retain_images(&mut self, live: &[(u32, u64)]) {
        self.images.retain(|k, _| live.contains(k));
    }
}

impl Renderer for VelloCpuRenderer {
    /// Resizes the pixmap and the context.
    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        self.ctx.reset_and_resize(w, h);
        self.pixmap = Pixmap::new(w, h);
    }

    /// Draws the layers into the pixmap.
    fn render_layers(
        &mut self, background: Rgba, layers: &[Layer<'_>], grid: &FontGrid,
    ) -> Result<FrameStats, RenderError> {
        let start = Instant::now();
        self.ctx.reset();
        let size = (self.pixmap.width() as u32, self.pixmap.height() as u32);
        {
            let mut canvas = CpuCanvas {
                ctx: &mut self.ctx,
                resources: &mut self.resources,
                atlas: self.atlas,
                images: &mut self.images,
            };
            draw_layers(&mut canvas, size, background, layers, grid);
        }
        self.ctx.flush();
        let encode = start.elapsed();
        let settings = vello_cpu::RasterizerSettings { render_mode: RenderMode::OptimizeSpeed, ..Default::default() };
        self.ctx.render_with(&mut self.pixmap, &mut self.resources, settings);
        Ok(FrameStats { encode, total: start.elapsed(), glyphs: layers.iter().map(|l| l.frame.glyph_count()).sum() })
    }

    /// The pixmap as straight RGBA8.
    fn read_pixels(&mut self) -> Result<Image, RenderError> {
        let data = self.pixmap.clone().take_rgba8(vello_common::peniko::ImageAlphaType::Alpha);
        Ok(Image { width: self.pixmap.width() as u32, height: self.pixmap.height() as u32, rgba: data })
    }

    /// The renderer's name.
    fn name(&self) -> String {
        format!("vello_cpu{}", if self.atlas { " (glyph atlas)" } else { "" })
    }
}
