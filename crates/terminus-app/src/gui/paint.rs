//! Putting frames on a window: `VelloGpuRenderer` on the window's surface, or `VelloCpuRenderer`
//! presented through `softbuffer` while the GPU starts, when no GPU can draw to the window, or when
//! the configuration asks for the CPU (`window.renderer`).

use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};

use terminus_font::FontGrid;
use terminus_render::{FrameStats, GpuContext, Layer, Renderer, Rgba, VelloCpuRenderer, VelloGpuRenderer};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

use crate::config::Renderer as Choice;

/// A window's renderer.
pub enum Painter {
    /// The GPU renderer, presenting itself.
    Gpu(Box<VelloGpuRenderer>),
    /// The CPU renderer and the surface its pixels are copied to.
    Cpu { renderer: Box<VelloCpuRenderer>, surface: softbuffer::Surface<Arc<Window>, Arc<Window>> },
}

/// The GPU renderer being built for a window on a background thread.
pub type GpuUpgrade = std::sync::mpsc::Receiver<Result<VelloGpuRenderer, String>>;

/// The GPU device every window shares, started once by the first window's background thread.
static SHARED_GPU: OnceLock<Result<GpuContext, String>> = OnceLock::new();

impl Painter {
    /// Creates a window's painter. Starting a GPU device takes most of a second, so unless the
    /// configuration asks for the CPU only, the window starts on the CPU renderer (ready in
    /// milliseconds) and the GPU renderer is built on a background thread; the receiver delivers it
    /// and the window switches on its next frame (P4, `docs/decisions/renderer.md`).
    pub fn new(
        window: &Arc<Window>, choice: Choice, warm: Option<Box<VelloCpuRenderer>>, display: OwnedDisplayHandle,
    ) -> Result<(Painter, Option<GpuUpgrade>), String> {
        let size = window.inner_size();
        let upgrade = (choice != Choice::Cpu).then(|| start_gpu(window.clone(), display));
        match cpu_painter(window, size.width, size.height, warm) {
            Ok(p) => Ok((p, upgrade)),
            Err(e) => match upgrade.map(|rx| rx.recv()) {
                Some(Ok(Ok(mut g))) => {
                    attach(&mut g, window).map_err(|g| format!("{e}; and the GPU renderer: {g}"))?;
                    Ok((Painter::Gpu(Box::new(g)), None))
                }
                Some(Ok(Err(g))) => Err(format!("{e}; and the GPU renderer: {g}")),
                _ => Err(e),
            },
        }
    }

    /// True when drawing with the GPU.
    pub fn is_gpu(&self) -> bool {
        matches!(self, Painter::Gpu(_))
    }

    /// Resizes the target.
    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            Painter::Gpu(r) => r.resize(width, height),
            Painter::Cpu { renderer, surface } => {
                renderer.resize(width, height);
                if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
                    let _ = surface.resize(w, h);
                }
            }
        }
    }

    /// Draws and presents.
    pub fn paint(&mut self, background: Rgba, layers: &[Layer<'_>], grid: &FontGrid) -> Result<FrameStats, String> {
        match self {
            Painter::Gpu(r) => r.render_layers(background, layers, grid).map_err(|e| e.to_string()),
            Painter::Cpu { renderer, surface } => {
                let stats = renderer.render_layers(background, layers, grid).map_err(|e| e.to_string())?;
                // The first CPU frame's render and present are separate startup phases (P4).
                let first = !FIRST_CPU_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed);
                if first {
                    crate::gui::phase("cpu_rendered");
                }
                present_pixmap(renderer, surface)?;
                Ok(stats)
            }
        }
    }

    /// The renderer's name, for the log and `list`.
    pub fn name(&self) -> String {
        match self {
            Painter::Gpu(r) => r.name(),
            Painter::Cpu { renderer, .. } => renderer.name(),
        }
    }
}

/// Set once the first CPU frame has been rendered in this process.
static FIRST_CPU_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How many threads the CPU renderer uses.
pub fn cpu_threads() -> u16 {
    std::thread::available_parallelism().map(|n| n.get().min(4) as u16).unwrap_or(0)
}

/// The CPU renderer presented through softbuffer: `warm` when the start up thread made one,
/// otherwise a new one.
fn cpu_painter(
    window: &Arc<Window>, width: u32, height: u32, warm: Option<Box<VelloCpuRenderer>>,
) -> Result<Painter, String> {
    let context = softbuffer::Context::new(window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
    let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
    let renderer = warm.unwrap_or_else(|| Box::new(VelloCpuRenderer::new(width, height, cpu_threads(), true)));
    let mut p = Painter::Cpu { renderer, surface };
    p.resize(width, height);
    Ok(p)
}

/// Builds the GPU renderer for a window on a background thread, on the shared device, and asks
/// the window to redraw when it is ready. The renderer comes back without a surface: winit gives
/// out a window's handle only on the thread that owns the window, so `attach` makes the surface
/// there. `display` is the event loop's display, which the OpenGL backend needs.
fn start_gpu(window: Arc<Window>, display: OwnedDisplayHandle) -> GpuUpgrade {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("terminus gpu start".into()).spawn(move || {
        let ctx = SHARED_GPU.get_or_init(|| GpuContext::for_display(display).map_err(|e| e.to_string()));
        let size = window.inner_size();
        let r = match ctx {
            Ok(ctx) => Ok(VelloGpuRenderer::new_detached_in(ctx, size.width, size.height, true)),
            Err(e) => Err(e.clone()),
        };
        let _ = tx.send(r);
        window.request_redraw();
    });
    rx
}

/// Gives a renderer from `start_gpu` its window surface. Runs on the thread that owns `window`.
/// `TERMINUS_PRESENT=immediate` presents without waiting for the display (the latency benchmark).
pub fn attach(gpu: &mut VelloGpuRenderer, window: &Arc<Window>) -> Result<(), String> {
    let Some(Ok(ctx)) = SHARED_GPU.get() else { return Err("the GPU device did not start".into()) };
    let unpaced = std::env::var("TERMINUS_PRESENT").is_ok_and(|v| v == "immediate");
    gpu.attach_window(ctx, window.clone(), unpaced).map_err(|e| e.to_string())
}

/// Copies the CPU renderer's premultiplied RGBA pixels into the softbuffer as 0RGB words.
fn present_pixmap(
    renderer: &VelloCpuRenderer, surface: &mut softbuffer::Surface<Arc<Window>, Arc<Window>>,
) -> Result<(), String> {
    let pixmap = renderer.pixmap();
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let mut buf = surface.buffer_mut().map_err(|e| format!("softbuffer: {e}"))?;
    let src = pixmap.data_as_u8_slice();
    for (dst, px) in buf.iter_mut().zip(src.chunks_exact(4)).take(w * h) {
        *dst = (u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2]);
    }
    buf.present().map_err(|e| format!("softbuffer: {e}"))
}
