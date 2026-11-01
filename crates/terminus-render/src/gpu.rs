//! `VelloGpuRenderer`: draws frames with `vello_gpu` on wgpu 30. Renders into an offscreen texture
//! (tests, the M2 measurement) or, in the app, into a window surface's texture.

use std::time::Instant;

use terminus_font::FontGrid;
use vello_common::color::AlphaColor;
use vello_common::kurbo::{BezPath, Rect};
use vello_common::peniko::FontData;
use vello_gpu::{RenderSize, RenderTargetConfig, Resources, Scene};

use crate::draw::{Canvas, Layer, draw_layers, hint_enabled, skew_transform};
use crate::frame::{GlyphRun, Rgba};
use crate::{FrameStats, Image, RenderError, Renderer};

/// Where frames go: an offscreen texture (tests, measurements) or a window's surface.
enum Target {
    /// An offscreen texture that `read_pixels` copies back.
    Offscreen(wgpu::Texture),
    /// A window surface, presented after each frame.
    Surface { surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration },
}

/// The GPU renderer.
pub struct VelloGpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
    renderer: vello_gpu::Renderer,
    resources: Resources,
    scene: Scene,
    target: Target,
    depth: wgpu::TextureView,
    size: (u16, u16),
    atlas: bool,
}

/// Asks wgpu for an adapter and device. None when no adapter is usable.
pub fn request_device(fallback_adapter: bool) -> Option<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
    let instance = wgpu::Instance::default();
    let (_, device, queue, info) = request_device_for(&instance, None, fallback_adapter)?;
    Some((device, queue, info))
}

/// Asks an instance for an adapter (compatible with `surface` when given) and a device.
fn request_device_for(
    instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'static>>, fallback_adapter: bool,
) -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
    let opts = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: fallback_adapter,
        compatible_surface: surface,
        apply_limit_buckets: false,
    };
    let adapter = pollster::block_on(instance.request_adapter(&opts)).ok()?;
    let info = adapter.get_info();
    let desc = wgpu::DeviceDescriptor {
        label: Some("terminus"),
        required_features: wgpu::Features::empty(),
        ..Default::default()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&desc)).ok()?;
    Some((adapter, device, queue, info))
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

/// Picks the surface format (8 bit, not sRGB, because vello writes sRGB encoded values) and the
/// present mode (mailbox when offered, for the lowest latency without tearing, else FIFO).
fn surface_config(caps: &wgpu::SurfaceCapabilities, w: u16, h: u16) -> Option<wgpu::SurfaceConfiguration> {
    let format = [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8Unorm]
        .into_iter()
        .find(|f| caps.formats.contains(f))?;
    let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    let alpha_mode = caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto);
    Some(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: w.into(),
        height: h.into(),
        present_mode,
        desired_maximum_frame_latency: 1,
        alpha_mode,
        view_formats: vec![],
        color_space: wgpu::SurfaceColorSpace::Auto,
    })
}

impl VelloGpuRenderer {
    /// A renderer with an offscreen target of `width` by `height`, or None without a GPU.
    pub fn new_offscreen(width: u32, height: u32, atlas: bool) -> Option<VelloGpuRenderer> {
        let (device, queue, adapter_info) = request_device(false)?;
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        let target = target_texture(&device, w, h);
        Some(Self::assemble(
            device,
            queue,
            adapter_info,
            Target::Offscreen(target),
            wgpu::TextureFormat::Rgba8Unorm,
            (w, h),
            atlas,
        ))
    }

    /// A renderer that presents into a window. `window` is anything wgpu can make a surface from
    /// (a winit window in an `Arc`). Errors name what failed, so the app can fall back to the CPU
    /// renderer and say why.
    pub fn new_for_window(
        window: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32, atlas: bool,
    ) -> Result<VelloGpuRenderer, RenderError> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).map_err(|e| RenderError::Gpu(format!("create surface: {e}")))?;
        let (adapter, device, queue, info) = request_device_for(&instance, Some(&surface), false)
            .ok_or_else(|| RenderError::Gpu("no GPU adapter can draw to this window".into()))?;
        let caps = surface.get_capabilities(&adapter);
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        let config = surface_config(&caps, w, h)
            .ok_or_else(|| RenderError::Gpu("the surface offers no 8 bit RGBA or BGRA format".into()))?;
        surface.configure(&device, &config);
        let format = config.format;
        Ok(Self::assemble(device, queue, info, Target::Surface { surface, config }, format, (w, h), atlas))
    }

    /// Builds the renderer around a device and a target.
    fn assemble(
        device: wgpu::Device, queue: wgpu::Queue, adapter_info: wgpu::AdapterInfo, target: Target,
        format: wgpu::TextureFormat, (w, h): (u16, u16), atlas: bool,
    ) -> VelloGpuRenderer {
        let config = RenderTargetConfig { format, width: w, height: h };
        let (renderer, resources) = vello_gpu::Renderer::new(&device, &config);
        let depth = vello_gpu::Renderer::create_depth_texture_view(&device, &RenderSize { width: w, height: h });
        VelloGpuRenderer {
            device,
            queue,
            adapter_info,
            renderer,
            resources,
            scene: Scene::new(w, h),
            target,
            depth,
            size: (w, h),
            atlas,
        }
    }

    /// The adapter the renderer runs on.
    pub fn adapter(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Waits until the GPU has finished every submitted frame.
    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Encodes the scene into `view` and submits it.
    fn submit(&mut self, view: &wgpu::TextureView) -> Result<(), RenderError> {
        let size = RenderSize { width: self.size.0, height: self.size.1 };
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
                view,
                Some(&self.depth),
                &vello_gpu::TextureBindings::new(),
                init,
            )
            .map_err(|e| RenderError::Gpu(format!("{e:?}")))?;
        self.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// The surface texture for the next frame, or None when the window is occluded or the GPU timed
    /// out (skip the frame). Reconfigures and retries once when the surface is outdated or lost
    /// (after a resize or a display change).
    fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, RenderError> {
        let Target::Surface { surface, config } = &self.target else {
            return Err(RenderError::Gpu("not a surface".into()));
        };
        for attempt in 0..2 {
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) => return Ok(Some(t)),
                wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                    surface.configure(&self.device, config);
                    return Ok(Some(t));
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return Ok(None),
                _ if attempt == 0 => surface.configure(&self.device, config),
                _ => return Err(RenderError::Gpu("the window surface was lost".into())),
            }
        }
        Ok(None)
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

    /// Sets the scene's transform.
    fn transform(&mut self, t: vello_common::kurbo::Affine) {
        self.scene.set_transform(t);
    }
}

impl Renderer for VelloGpuRenderer {
    /// Recreates the depth buffer and scene at the new size, and the target or the surface
    /// configuration.
    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        self.size = (w, h);
        match &mut self.target {
            Target::Offscreen(t) => *t = target_texture(&self.device, w, h),
            Target::Surface { surface, config } => {
                config.width = w.into();
                config.height = h.into();
                surface.configure(&self.device, config);
            }
        }
        self.depth = vello_gpu::Renderer::create_depth_texture_view(&self.device, &RenderSize { width: w, height: h });
        self.scene = Scene::new(w, h);
    }

    /// Encodes the layers and submits them. Offscreen, it waits for the GPU so the time measured
    /// is the whole frame; on a surface it presents and returns without waiting.
    fn render_layers(
        &mut self, background: Rgba, layers: &[Layer<'_>], grid: &FontGrid,
    ) -> Result<FrameStats, RenderError> {
        let start = Instant::now();
        self.scene.reset();
        {
            let mut canvas = GpuCanvas { scene: &mut self.scene, resources: &mut self.resources, atlas: self.atlas };
            draw_layers(&mut canvas, (self.size.0.into(), self.size.1.into()), background, layers, grid);
        }
        let encode = start.elapsed();
        let glyphs = layers.iter().map(|l| l.frame.glyph_count()).sum();
        if let Target::Offscreen(t) = &self.target {
            let view = t.create_view(&wgpu::TextureViewDescriptor::default());
            self.submit(&view)?;
            self.wait_idle();
            return Ok(FrameStats { encode, total: start.elapsed(), glyphs });
        }
        let Some(frame) = self.acquire()? else { return Ok(FrameStats { encode, total: start.elapsed(), glyphs: 0 }) };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.submit(&view)?;
        self.queue.present(frame);
        Ok(FrameStats { encode, total: start.elapsed(), glyphs })
    }

    /// Copies the offscreen target back to memory. A window surface cannot be read back.
    fn read_pixels(&mut self) -> Result<Image, RenderError> {
        match &self.target {
            Target::Offscreen(t) => crate::readback::read_texture(&self.device, &self.queue, t),
            Target::Surface { .. } => Err(RenderError::Gpu("a window surface cannot be read back".into())),
        }
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
