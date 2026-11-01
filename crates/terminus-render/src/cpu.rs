//! `VelloCpuRenderer`: draws frames into a pixmap with `vello_cpu`. Used when no GPU adapter is
//! usable, when the configuration asks for it, and by the pixel tests. The app presents the pixmap
//! with `softbuffer`.

use std::time::Instant;

use terminus_font::FontGrid;
use vello_common::color::AlphaColor;
use vello_common::kurbo::{BezPath, Rect};
use vello_common::peniko::FontData;
use vello_cpu::{Level, Pixmap, RenderContext, RenderMode, RenderSettings, Resources};

use crate::draw::{Canvas, draw_frame, hint_enabled, skew_transform};
use crate::frame::{Frame, GlyphRun, Rgba};
use crate::{FrameStats, Image, RenderError, Renderer};

/// The CPU renderer.
pub struct VelloCpuRenderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    atlas: bool,
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
        }
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
}

impl Renderer for VelloCpuRenderer {
    /// Resizes the pixmap and the context.
    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        self.ctx.reset_and_resize(w, h);
        self.pixmap = Pixmap::new(w, h);
    }

    /// Draws the frame into the pixmap.
    fn render(&mut self, frame: &Frame, grid: &FontGrid) -> Result<FrameStats, RenderError> {
        let start = Instant::now();
        self.ctx.reset();
        {
            let mut canvas = CpuCanvas { ctx: &mut self.ctx, resources: &mut self.resources, atlas: self.atlas };
            draw_frame(&mut canvas, frame, grid);
        }
        self.ctx.flush();
        let encode = start.elapsed();
        let settings = vello_cpu::RasterizerSettings { render_mode: RenderMode::OptimizeSpeed, ..Default::default() };
        self.ctx.render_with(&mut self.pixmap, &mut self.resources, settings);
        Ok(FrameStats { encode, total: start.elapsed(), glyphs: frame.glyph_count() })
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
