//! `VelloGpuRenderer`: draws frames with `vello_gpu` on wgpu 30. Renders into an offscreen texture
//! (tests, the M2 measurement) or, in the app, into a window surface's texture.

use std::time::Instant;

use terminus_font::FontGrid;
use vello_common::color::AlphaColor;
use vello_common::kurbo::{BezPath, Rect};
use vello_common::peniko::FontData;
use vello_gpu::{RenderSize, RenderTargetConfig, Resources, Scene};

use crate::draw::{Canvas, draw_frame, hint_enabled, skew_transform};
use crate::frame::{Frame, GlyphRun, Rgba};
use crate::{FrameStats, Image, RenderError, Renderer};

/// The GPU renderer.
pub struct VelloGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
    renderer: vello_gpu::Renderer,
    resources: Resources,
    scene: Scene,
    target: wgpu::Texture,
    depth: wgpu::TextureView,
    atlas: bool,
}

/// Asks wgpu for an adapter and device. None when no adapter is usable.
pub fn request_device(fallback_adapter: bool) -> Option<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
    let instance = wgpu::Instance::default();
    let opts = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: fallback_adapter,
        compatible_surface: None,
        ..Default::default()
    };
    let adapter = pollster::block_on(instance.request_adapter(&opts)).ok()?;
    let info = adapter.get_info();
    let desc = wgpu::DeviceDescriptor {
        label: Some("terminus"),
        required_features: wgpu::Features::empty(),
        ..Default::default()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&desc)).ok()?;
    Some((device, queue, info))
}

/// Creates the offscreen texture frames are drawn into.
fn target_texture(device: &wgpu::Device, width: u16, height: u16) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terminus frame"),
        size: wgpu::Extent3d { width: width.into(), height: height.into(), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

impl VelloGpuRenderer {
    /// A renderer with an offscreen target of `width` by `height`, or None without a GPU.
    pub fn new_offscreen(width: u32, height: u32, atlas: bool) -> Option<VelloGpuRenderer> {
        let (device, queue, adapter_info) = request_device(false)?;
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        let target = target_texture(&device, w, h);
        let config = RenderTargetConfig { format: target.format(), width: w, height: h };
        let (renderer, resources) = vello_gpu::Renderer::new(&device, &config);
        let depth = vello_gpu::Renderer::create_depth_texture_view(&device, &RenderSize { width: w, height: h });
        Some(VelloGpuRenderer {
            device,
            queue,
            adapter_info,
            renderer,
            resources,
            scene: Scene::new(w, h),
            target,
            depth,
            atlas,
        })
    }

    /// The adapter the renderer runs on.
    pub fn adapter(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Waits until the GPU has finished every submitted frame.
    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

/// A frame dimension clamped to what vello accepts.
fn clamp_dim(v: u32) -> u16 {
    v.clamp(1, 8192) as u16
}

/// Converts a frame colour to vello's.
fn color(c: Rgba) -> AlphaColor<vello_common::color::Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// The canvas over a GPU scene.
struct GpuCanvas<'a> {
    scene: &'a mut Scene,
    resources: &'a mut Resources,
    atlas: bool,
}

impl Canvas for GpuCanvas<'_> {
    /// Sets the paint colour.
    fn paint(&mut self, c: Rgba) {
        self.scene.set_paint(color(c));
    }

    /// Fills a rectangle.
    fn rect(&mut self, r: Rect) {
        self.scene.fill_rect(&r);
    }

    /// Fills a path.
    fn path(&mut self, p: &BezPath) {
        self.scene.fill_path(p);
    }

    /// Draws glyphs through glifo.
    fn glyphs(&mut self, font: &FontData, run: &GlyphRun) {
        let glyphs = run.glyphs.iter().map(|g| glifo::Glyph { id: g.id, x: g.x, y: g.y });
        let mut b = self
            .scene
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

impl Renderer for VelloGpuRenderer {
    /// Recreates the target at the new size.
    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        self.target = target_texture(&self.device, w, h);
        self.depth = vello_gpu::Renderer::create_depth_texture_view(&self.device, &RenderSize { width: w, height: h });
        self.scene = Scene::new(w, h);
    }

    /// Encodes the scene, submits it, and waits for the GPU so the time measured is the whole frame.
    fn render(&mut self, frame: &Frame, grid: &FontGrid) -> Result<FrameStats, RenderError> {
        let start = Instant::now();
        self.scene.reset();
        {
            let mut canvas = GpuCanvas { scene: &mut self.scene, resources: &mut self.resources, atlas: self.atlas };
            draw_frame(&mut canvas, frame, grid);
        }
        let encode = start.elapsed();
        let view = self.target.create_view(&wgpu::TextureViewDescriptor::default());
        let size = RenderSize { width: self.target.width() as u16, height: self.target.height() as u16 };
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("terminus frame") });
        let init = vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::default());
        self.renderer
            .render(
                &self.scene,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &size,
                &view,
                Some(&self.depth),
                &vello_gpu::TextureBindings::new(),
                init,
            )
            .map_err(|e| RenderError::Gpu(format!("{e:?}")))?;
        self.queue.submit([encoder.finish()]);
        self.wait_idle();
        Ok(FrameStats { encode, total: start.elapsed(), glyphs: frame.glyph_count() })
    }

    /// Copies the target texture back to memory.
    fn read_pixels(&mut self) -> Result<Image, RenderError> {
        crate::readback::read_texture(&self.device, &self.queue, &self.target)
    }

    /// The renderer's name with the adapter.
    fn name(&self) -> String {
        format!(
            "vello_gpu on {} ({:?}){}",
            self.adapter_info.name,
            self.adapter_info.backend,
            if self.atlas { " (glyph atlas)" } else { "" }
        )
    }
}
