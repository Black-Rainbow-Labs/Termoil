//! Putting frames on a window: `VelloGpuRenderer` on the window's surface, or `VelloCpuRenderer`
//! presented through `softbuffer` when no GPU can draw to the window or the configuration asks for
//! the CPU (`window.renderer`).

use std::num::NonZeroU32;
use std::sync::Arc;

use terminus_font::FontGrid;
use terminus_render::{FrameStats, Layer, Renderer, Rgba, VelloCpuRenderer, VelloGpuRenderer};
use winit::window::Window;

use crate::config::Renderer as Choice;

/// A window's renderer.
pub enum Painter {
    /// The GPU renderer, presenting itself.
    Gpu(Box<VelloGpuRenderer>),
    /// The CPU renderer and the surface its pixels are copied to.
    Cpu { renderer: Box<VelloCpuRenderer>, surface: softbuffer::Surface<Arc<Window>, Arc<Window>> },
}

impl Painter {
    /// Creates the painter the configuration asks for, falling back to the CPU when the GPU
    /// cannot draw to the window. The second value says why it fell back, for the log.
    pub fn new(window: &Arc<Window>, choice: Choice) -> Result<(Painter, Option<String>), String> {
        let size = window.inner_size();
        let mut why = None;
        if choice != Choice::Cpu {
            match VelloGpuRenderer::new_for_window(window.clone(), size.width, size.height, true) {
                Ok(r) => return Ok((Painter::Gpu(Box::new(r)), None)),
                Err(e) if choice == Choice::Gpu => {
                    return Err(format!("the GPU renderer failed and window.renderer is \"gpu\": {e}"));
                }
                Err(e) => why = Some(format!("GPU renderer unavailable, using the CPU renderer: {e}")),
            }
        }
        let context = softbuffer::Context::new(window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
        let threads = std::thread::available_parallelism().map(|n| n.get().min(4) as u16).unwrap_or(0);
        let mut p =
            Painter::Cpu { renderer: Box::new(VelloCpuRenderer::new(size.width, size.height, threads, true)), surface };
        p.resize(size.width, size.height);
        Ok((p, why))
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
