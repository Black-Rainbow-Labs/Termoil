//! Putting frames on a window: `VelloGpuRenderer` on the window's surface, or `VelloCpuRenderer`
//! presented through `softbuffer` while the GPU starts, when no GPU can draw to the window, or when
//! the configuration asks for the CPU (`window.renderer`).

use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};

use terminus_font::FontGrid;
use terminus_render::{FrameStats, GpuContext, Layer, Renderer, Rgba, VelloCpuRenderer, VelloGpuRenderer};
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
    pub fn new(window: &Arc<Window>, choice: Choice) -> Result<(Painter, Option<GpuUpgrade>), String> {
        let size = window.inner_size();
        let upgrade = (choice != Choice::Cpu).then(|| start_gpu(window.clone()));
        match cpu_painter(window, size.width, size.height) {
            Ok(p) => Ok((p, upgrade)),
            Err(e) => match upgrade.map(|rx| rx.recv()) {
                Some(Ok(Ok(g))) => Ok((Painter::Gpu(Box::new(g)), None)),
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

/// The CPU renderer presented through softbuffer.
fn cpu_painter(window: &Arc<Window>, width: u32, height: u32) -> Result<Painter, String> {
    let context = softbuffer::Context::new(window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
    let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
    let threads = std::thread::available_parallelism().map(|n| n.get().min(4) as u16).unwrap_or(0);
    let mut p = Painter::Cpu { renderer: Box::new(VelloCpuRenderer::new(width, height, threads, true)), surface };
    p.resize(width, height);
    Ok(p)
}

/// Builds the GPU renderer for a window on a background thread, on the shared device, and asks
/// the window to redraw when it is ready.
fn start_gpu(window: Arc<Window>) -> GpuUpgrade {
    let (tx, rx) = std::sync::mpsc::channel();
    let unpaced = std::env::var("TERMINUS_PRESENT").is_ok_and(|v| v == "immediate");
    let _ = std::thread::Builder::new().name("terminus gpu start".into()).spawn(move || {
        let ctx = SHARED_GPU.get_or_init(|| GpuContext::new().map_err(|e| e.to_string()));
        let size = window.inner_size();
        let r = match ctx {
            Ok(ctx) => VelloGpuRenderer::new_for_window_in(ctx, window.clone(), size.width, size.height, true, unpaced)
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.clone()),
        };
        let _ = tx.send(r);
        window.request_redraw();
    });
    rx
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
