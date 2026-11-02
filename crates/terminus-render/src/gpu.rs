//! `VelloGpuRenderer`: draws frames with `vello_gpu` on wgpu 30. Renders into an offscreen texture
//! (tests, the M2 measurement) or, in the app, into a window surface's texture.

use std::collections::HashMap;
use std::time::Instant;

use terminus_font::FontGrid;
use vello_common::color::AlphaColor;
use vello_common::kurbo::{BezPath, Rect};
use vello_common::peniko::{FontData, ImageSampler};
use vello_gpu::{
    Image as PaintImage, ImageSource, RectU16, RenderSize, RenderTargetConfig, Resources, Scene, TextureBindings,
    TextureId,
};

use crate::draw::{Canvas, Layer, draw_layers, hint_enabled, image_paint_transform, image_rect, skew_transform};
use crate::frame::{GlyphRun, ImageRecord, Rgba};
use crate::images::ImageData;
use crate::shader::{ShaderInputs, ShaderPass, frame_texture};
use crate::{FrameStats, Image, RenderError, Renderer};

/// Where frames go: an offscreen texture (tests, measurements) or a window's surface.
enum Target {
    /// An offscreen texture that `read_pixels` copies back.
    Offscreen(wgpu::Texture),
    /// A window surface, presented after each frame.
    Surface { surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration },
    /// Not attached to a window yet (see `VelloGpuRenderer::new_detached_in`).
    Detached,
}

/// The format a detached renderer is built for: the first one `surface_config` picks when the
/// surface offers it, which DX12, Metal and most Vulkan drivers do.
const DETACHED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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
    hint: bool,
    images: GpuImages,
    format: wgpu::TextureFormat,
    shader: Option<ShaderSlot>,
    cursor: Option<(f32, f32)>,
}

/// A compiled custom shader and the texture vello draws into before it runs.
struct ShaderSlot {
    pass: ShaderPass,
    frame: wgpu::TextureView,
}

/// Images uploaded as textures of their own, keyed by image id and generation. vello samples them
/// through the texture bindings passed with each render.
#[derive(Default)]
struct GpuImages {
    textures: HashMap<(u32, u64), (TextureId, wgpu::Texture)>,
    bindings: TextureBindings,
    next_id: u64,
}

impl GpuImages {
    /// The texture id of an image, uploading the pixels on first use. None when the image is empty
    /// or larger than the device's texture size limit.
    fn texture_for(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &ImageData) -> Option<TextureId> {
        let key = (image.id, image.generation);
        if let Some((id, _)) = self.textures.get(&key) {
            return Some(*id);
        }
        let limit = device.limits().max_texture_dimension_2d;
        let ok = image.width > 0 && image.height > 0 && image.width <= limit && image.height <= limit;
        if !ok || image.width > u16::MAX as u32 || image.height > u16::MAX as u32 {
            return None;
        }
        if image.rgba.len() != image.width as usize * image.height as usize * 4 {
            return None;
        }
        let texture = upload_image(device, queue, image);
        self.next_id += 1;
        let id = TextureId(self.next_id);
        self.bindings.insert(id, texture.create_view(&wgpu::TextureViewDescriptor::default()));
        self.textures.insert(key, (id, texture));
        Some(id)
    }

    /// Drops the textures of images the terminal no longer holds.
    fn retain(&mut self, live: &[(u32, u64)]) {
        let bindings = &mut self.bindings;
        self.textures.retain(|k, (id, _)| {
            let keep = live.contains(k);
            if !keep {
                bindings.remove(*id);
            }
            keep
        });
    }
}

/// Creates a texture holding the image premultiplied, the form vello blends.
fn upload_image(device: &wgpu::Device, queue: &wgpu::Queue, image: &ImageData) -> wgpu::Texture {
    let size = wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terminus image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut pixels = image.rgba.as_ref().clone();
    vello_common::pixmap::premultiply_rgba8(&mut pixels);
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(image.width * 4),
        rows_per_image: Some(image.height),
    };
    queue.write_texture(texture.as_image_copy(), &pixels, layout, size);
    texture
}

/// A GPU device the windows share: the instance, the adapter and its device and queue.
#[derive(Clone)]
pub struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
}

impl GpuContext {
    /// Starts a device on the high performance adapter of the primary backends (Vulkan, DX12,
    /// Metal), for drawing offscreen. See `for_display` for windows.
    pub fn new() -> Result<GpuContext, RenderError> {
        Self::with_backends(wgpu::Backends::PRIMARY)
    }

    /// Starts a device for windows on `display` (a winit window or its event loop): the primary
    /// backends, and OpenGL when none of them has a usable adapter (a virtual machine or an old
    /// driver). OpenGL is not tried first because its start up alone costs about 170 ms here, and
    /// it needs the display to make window surfaces. `TERMINUS_GPU_BACKEND=gl` uses OpenGL only,
    /// to measure and test it.
    pub fn for_display<D>(display: D) -> Result<GpuContext, RenderError>
    where
        D: wgpu::rwh::HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let gl = || {
            let desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display.clone()));
            Self::with_descriptor(wgpu::InstanceDescriptor { backends: wgpu::Backends::GL, ..desc })
        };
        if std::env::var("TERMINUS_GPU_BACKEND").is_ok_and(|v| v.eq_ignore_ascii_case("gl")) {
            return gl();
        }
        Self::with_backends(wgpu::Backends::PRIMARY).or_else(|_| gl())
    }

    /// Starts a device whose window surfaces can be translucent (`window.opacity` below 1). On
    /// Windows a swapchain made for a window ignores alpha, so this is DX12 presenting through
    /// DirectComposition, falling back to `for_display`. Elsewhere it is `for_display`.
    pub fn for_translucent_display<D>(display: D) -> Result<GpuContext, RenderError>
    where
        D: wgpu::rwh::HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        if cfg!(windows) {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = wgpu::Backends::DX12;
            desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
            if let Ok(ctx) = Self::with_descriptor(desc) {
                return Ok(ctx);
            }
        }
        Self::for_display(display)
    }

    /// Starts a device on the high performance adapter of the given backends, with no display.
    pub fn with_backends(backends: wgpu::Backends) -> Result<GpuContext, RenderError> {
        Self::with_descriptor(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    }

    /// Starts a device on the high performance adapter an instance made from `desc` offers.
    fn with_descriptor(desc: wgpu::InstanceDescriptor) -> Result<GpuContext, RenderError> {
        let backends = desc.backends;
        let instance = wgpu::Instance::new(desc);
        let (adapter, device, queue, info) = request_device_for(&instance, None, false)
            .ok_or_else(|| RenderError::Gpu(format!("no GPU adapter on {backends:?}")))?;
        Ok(GpuContext { instance, adapter, device, queue, info })
    }

    /// The adapter's description.
    pub fn info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }
}

/// Asks wgpu for an adapter and device. None when no adapter is usable.
pub fn request_device(fallback_adapter: bool) -> Option<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
    let instance = wgpu::Instance::default();
    let (_, device, queue, info) = request_device_for(&instance, None, fallback_adapter)?;
    Some((device, queue, info))
}

/// The adapter preference: the high performance GPU, or the integrated one when
/// `TERMINUS_GPU_POWER=low` (used to measure R4 on an integrated GPU).
fn power_preference() -> wgpu::PowerPreference {
    if std::env::var("TERMINUS_GPU_POWER").is_ok_and(|v| v == "low") {
        wgpu::PowerPreference::LowPower
    } else {
        wgpu::PowerPreference::HighPerformance
    }
}

/// Asks an instance for an adapter (compatible with `surface` when given) and a device.
fn request_device_for(
    instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'static>>, fallback_adapter: bool,
) -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
    let opts = wgpu::RequestAdapterOptions {
        power_preference: power_preference(),
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

/// The compositing mode: for a translucent window premultiplied alpha (vello's output) when the
/// surface offers it, else postmultiplied; otherwise the surface's first, usually opaque.
fn alpha_mode(caps: &wgpu::SurfaceCapabilities, translucent: bool) -> wgpu::CompositeAlphaMode {
    let wanted = [wgpu::CompositeAlphaMode::PreMultiplied, wgpu::CompositeAlphaMode::PostMultiplied];
    let found = translucent.then(|| wanted.into_iter().find(|m| caps.alpha_modes.contains(m))).flatten();
    found.or_else(|| caps.alpha_modes.first().copied()).unwrap_or(wgpu::CompositeAlphaMode::Auto)
}

/// Picks the surface format (8 bit, not sRGB, because vello writes sRGB encoded values) and the
/// present mode (mailbox when offered, for the lowest latency without tearing, else FIFO).
fn surface_config(
    caps: &wgpu::SurfaceCapabilities, w: u16, h: u16, unpaced: bool, translucent: bool,
) -> Option<wgpu::SurfaceConfiguration> {
    let format = [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8Unorm]
        .into_iter()
        .find(|f| caps.formats.contains(f))?;
    let present_mode = if unpaced && caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
        wgpu::PresentMode::Immediate
    } else if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    let alpha_mode = alpha_mode(caps, translucent);
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
    /// (a winit window in an `Arc`). `unpaced` presents immediately instead of at the display rate
    /// (the latency benchmark). Errors name what failed, so the app can fall back to the CPU
    /// renderer and say why.
    pub fn new_for_window(
        window: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32, atlas: bool, unpaced: bool,
    ) -> Result<VelloGpuRenderer, RenderError> {
        let ctx = GpuContext::new()?;
        Self::new_for_window_in(&ctx, window, width, height, atlas, unpaced)
    }

    /// A renderer for a window on a device that already exists. Starting a device takes most of a
    /// second on this machine, so the app makes one context and every window shares it.
    pub fn new_for_window_in(
        ctx: &GpuContext, window: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32, atlas: bool,
        unpaced: bool,
    ) -> Result<VelloGpuRenderer, RenderError> {
        let mut r = Self::new_detached_in(ctx, width, height, atlas);
        r.attach_window(ctx, window, unpaced, false)?;
        Ok(r)
    }

    /// A renderer on `ctx` with no window yet. Building vello's pipelines is the slow part of
    /// starting the GPU renderer (tens to hundreds of milliseconds), and it needs no window, so it
    /// can run on any thread; `attach_window` then runs on the thread that owns the window, which
    /// winit requires on Windows and macOS.
    pub fn new_detached_in(ctx: &GpuContext, width: u32, height: u32, atlas: bool) -> VelloGpuRenderer {
        let (w, h) = (clamp_dim(width), clamp_dim(height));
        let (device, queue, info) = (ctx.device.clone(), ctx.queue.clone(), ctx.info.clone());
        Self::assemble(device, queue, info, Target::Detached, DETACHED_FORMAT, (w, h), atlas)
    }

    /// Makes `window` this renderer's target: creates and configures its surface on `ctx` (the
    /// context the renderer was built on), and rebuilds the pipelines only when the surface uses
    /// another format than the one they were built for. `unpaced` presents immediately instead of
    /// at the display rate (the latency benchmark).
    pub fn attach_window(
        &mut self, ctx: &GpuContext, window: impl Into<wgpu::SurfaceTarget<'static>>, unpaced: bool, translucent: bool,
    ) -> Result<(), RenderError> {
        let surface =
            ctx.instance.create_surface(window).map_err(|e| RenderError::Gpu(format!("create surface: {e}")))?;
        if !ctx.adapter.is_surface_supported(&surface) {
            return Err(RenderError::Gpu(format!("{} cannot draw to this window", ctx.info.name)));
        }
        let caps = surface.get_capabilities(&ctx.adapter);
        let config = surface_config(&caps, self.size.0, self.size.1, unpaced, translucent)
            .ok_or_else(|| RenderError::Gpu("the surface offers no 8 bit RGBA or BGRA format".into()))?;
        surface.configure(&self.device, &config);
        if config.format != self.format {
            let rc = RenderTargetConfig { format: config.format, width: self.size.0, height: self.size.1 };
            (self.renderer, self.resources) = vello_gpu::Renderer::new(&self.device, &rc);
            self.images = GpuImages::default();
            self.format = config.format;
            // The shader pipeline was built for the old format; the app sets the shader again.
            self.shader = None;
        }
        self.target = Target::Surface { surface, config };
        Ok(())
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
            hint: hint_enabled(),
            images: GpuImages::default(),
            format,
            shader: None,
            cursor: None,
        }
    }

    /// How many image textures the renderer holds (for tests).
    pub fn cached_images(&self) -> usize {
        self.images.textures.len()
    }

    /// The adapter the renderer runs on.
    pub fn adapter(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Waits until the GPU has finished every submitted frame.
    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Compiles `source` (WGSL, see `docs/shaders.md`) as a pass over every frame, or removes the
    /// pass when `source` is None. A shader that does not compile leaves the renderer drawing
    /// without a pass and returns the error text.
    pub fn set_shader(&mut self, source: Option<&str>) -> Result<(), RenderError> {
        self.shader = None;
        let Some(source) = source else { return Ok(()) };
        let pass = ShaderPass::compile(&self.device, self.format, source)?;
        let frame = frame_texture(&self.device, self.format, self.size.0, self.size.1);
        self.shader = Some(ShaderSlot { pass, frame: frame.create_view(&wgpu::TextureViewDescriptor::default()) });
        Ok(())
    }

    /// True when a shader pass is active.
    pub fn has_shader(&self) -> bool {
        self.shader.is_some()
    }

    /// True when the active shader reads the time, so the window must redraw at the display rate.
    pub fn shader_animates(&self) -> bool {
        self.shader.as_ref().is_some_and(|s| s.pass.animated())
    }

    /// Sets the cursor position in pixels the shader reads, or None when no cursor is shown.
    pub fn set_shader_cursor(&mut self, px: Option<(f32, f32)>) {
        self.cursor = px;
    }

    /// Encodes the scene into `out` and submits it. With a shader pass, vello draws into the pass's
    /// frame texture and the pass draws that into `out`.
    fn submit(&mut self, out: &wgpu::TextureView) -> Result<(), RenderError> {
        let size = RenderSize { width: self.size.0, height: self.size.1 };
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("terminus frame") });
        let init = vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::default());
        let view = self.shader.as_ref().map_or(out, |s| &s.frame);
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
                &self.images.bindings,
                init,
            )
            .map_err(|e| RenderError::Gpu(format!("{e:?}")))?;
        if let Some(slot) = &self.shader {
            let (w, h) = (f32::from(self.size.0), f32::from(self.size.1));
            let inputs = ShaderInputs { size: (w, h), cursor: self.cursor.unwrap_or((-1.0, -1.0)) };
            slot.pass.run(&self.device, &self.queue, &mut encoder, &slot.frame, out, inputs);
        }
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
    hint: bool,
    images: &'a mut GpuImages,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
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
            .hint(self.hint)
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

    /// Fills the destination rectangle with the image's texture, uploading it on first use.
    fn image(&mut self, rec: &ImageRecord) {
        let Some(id) = self.images.texture_for(self.device, self.queue, &rec.image) else { return };
        let region = RectU16::new(0, 0, rec.image.width as u16, rec.image.height as u16);
        let image =
            PaintImage { image: ImageSource::external_texture(id, region, true), sampler: ImageSampler::default() };
        self.scene.set_paint(image);
        self.scene.set_paint_transform(image_paint_transform(rec));
        self.scene.fill_rect(&image_rect(rec));
        self.scene.reset_paint_transform();
    }

    /// Drops the textures of images the terminal no longer holds.
    fn retain_images(&mut self, live: &[(u32, u64)]) {
        self.images.retain(live);
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
            Target::Detached => {}
        }
        self.depth = vello_gpu::Renderer::create_depth_texture_view(&self.device, &RenderSize { width: w, height: h });
        self.scene = Scene::new(w, h);
        if let Some(slot) = &mut self.shader {
            let frame = frame_texture(&self.device, self.format, w, h);
            slot.frame = frame.create_view(&wgpu::TextureViewDescriptor::default());
        }
    }

    /// Encodes the layers and submits them. Offscreen, it waits for the GPU so the time measured
    /// is the whole frame; on a surface it presents and returns without waiting.
    fn render_layers(
        &mut self, background: Rgba, layers: &[Layer<'_>], grid: &FontGrid,
    ) -> Result<FrameStats, RenderError> {
        let start = Instant::now();
        self.scene.reset();
        {
            let mut canvas = GpuCanvas {
                scene: &mut self.scene,
                resources: &mut self.resources,
                atlas: self.atlas,
                hint: self.hint,
                images: &mut self.images,
                device: &self.device,
                queue: &self.queue,
            };
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
            Target::Surface { .. } | Target::Detached => {
                Err(RenderError::Gpu("only an offscreen target can be read back".into()))
            }
        }
    }

    /// The renderer's name with the adapter.
    fn set_hinting(&mut self, on: bool) {
        self.hint = on;
    }

    fn name(&self) -> String {
        format!(
            "vello_gpu on {} ({:?}){}",
            self.adapter_info.name,
            self.adapter_info.backend,
            if self.atlas { " (glyph atlas)" } else { "" }
        )
    }
}
