//! Putting frames on a window: `RasterRenderer` presented through `softbuffer`, which draws on the
//! CPU with no GPU device and is the window's renderer unless the configuration needs the GPU, or
//! `VelloGpuRenderer` on the window's surface when `window.renderer` is `gpu` or a custom shader is
//! set (task-2481, `docs/decisions/efficiency.md`).

use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};

use termoil_font::FontGrid;
use termoil_render::{FrameStats, GpuContext, Layer, RasterRenderer, Renderer, Rgba, VelloGpuRenderer};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

use crate::config::Renderer as Choice;

/// A window's renderer.
pub enum Painter {
    /// The GPU renderer, presenting itself.
    Gpu(Box<VelloGpuRenderer>),
    /// The CPU renderer and the surface its pixels are copied to.
    Cpu { renderer: Box<RasterRenderer>, surface: softbuffer::Surface<Arc<Window>, Arc<Window>> },
}

/// The GPU renderer being built for a window on a background thread.
pub type GpuUpgrade = std::sync::mpsc::Receiver<Result<VelloGpuRenderer, String>>;

/// The GPU device every window shares, started once by the first window's background thread.
static SHARED_GPU: OnceLock<Result<GpuContext, String>> = OnceLock::new();

impl Painter {
    /// Creates a window's painter. The window starts on the CPU renderer, ready in milliseconds.
    /// When the GPU is wanted (`wants_gpu`), the GPU renderer is built on a background thread; the
    /// receiver delivers it and the window switches on its next frame (P4,
    /// `docs/decisions/renderer.md`). Otherwise no GPU device is ever made, which is most of the
    /// memory the window saves (task-2481).
    pub fn new(
        window: &Arc<Window>, wants_gpu: bool, warm: Option<Box<RasterRenderer>>, display: OwnedDisplayHandle,
    ) -> Result<(Painter, Option<GpuUpgrade>), String> {
        let size = window.inner_size();
        let upgrade = wants_gpu.then(|| start_gpu(window.clone(), display));
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
                let stats = render_and_present(renderer, surface, background, layers, grid)?;
                // The first CPU frame's render and present are separate startup phases (P4).
                let first = !FIRST_CPU_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed);
                if first {
                    crate::gui::phase("cpu_rendered");
                }
                Ok(stats)
            }
        }
    }

    /// Turns glyph hinting on or off (`font.hinting`).
    pub fn set_hinting(&mut self, on: bool) {
        match self {
            Painter::Gpu(r) => r.set_hinting(on),
            Painter::Cpu { renderer, .. } => renderer.set_hinting(on),
        }
    }

    /// Sets the custom shader (`window.shader`) the GPU renderer runs over each frame, or removes it
    /// with None. The CPU renderer ignores shaders and always succeeds. An error means the shader
    /// did not compile and frames are drawn without it.
    pub fn set_shader(&mut self, source: Option<&str>) -> Result<(), String> {
        match self {
            Painter::Gpu(r) => r.set_shader(source).map_err(|e| e.to_string()),
            Painter::Cpu { .. } => Ok(()),
        }
    }

    /// Tells the shader where the cursor is, in pixels (None when it is not shown).
    pub fn set_shader_cursor(&mut self, px: Option<(f32, f32)>) {
        if let Painter::Gpu(r) = self {
            r.set_shader_cursor(px);
        }
    }

    /// True when a shader that reads the time is running, so the window must keep redrawing.
    pub fn shader_animates(&self) -> bool {
        matches!(self, Painter::Gpu(r) if r.shader_animates())
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

/// Whether a window with this configuration draws on the GPU: `window.renderer = "gpu"`, or
/// `auto` with a custom shader or a cursor effect, which only the GPU renderer runs.
pub fn wants_gpu(choice: Choice, shader: &str, cursor_effect: &str) -> bool {
    let pass = !shader.trim().is_empty() || !cursor_effect.trim().is_empty();
    choice == Choice::Gpu || (choice == Choice::Auto && pass)
}

/// Starts building the GPU renderer for a window that is drawing on the CPU (a shader was set
/// after the window opened). The window takes it with its next frame.
pub fn upgrade_to_gpu(window: &Arc<Window>, display: OwnedDisplayHandle) -> GpuUpgrade {
    start_gpu(window.clone(), display)
}

/// The CPU renderer presented through softbuffer: `warm` when the start up thread made one,
/// otherwise a new one.
fn cpu_painter(
    window: &Arc<Window>, width: u32, height: u32, warm: Option<Box<RasterRenderer>>,
) -> Result<Painter, String> {
    let context = softbuffer::Context::new(window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
    let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| format!("softbuffer: {e}"))?;
    let renderer = warm.unwrap_or_else(|| Box::new(RasterRenderer::new(width, height, glyph_capacity())));
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
    let _ = std::thread::Builder::new().name("termoil gpu start".into()).spawn(move || {
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
pub fn attach(gpu: &mut VelloGpuRenderer, window: &Arc<Window>) -> Result<(), String> {
    let Some(Ok(ctx)) = SHARED_GPU.get() else { return Err("the GPU device did not start".into()) };
    gpu.attach_window(ctx, window.clone(), unpaced(), false).map_err(|e| e.to_string())
}

/// `TERMOIL_PRESENT=immediate`: present without waiting for the display (the latency benchmark).
fn unpaced() -> bool {
    std::env::var("TERMOIL_PRESENT").is_ok_and(|v| v == "immediate")
}

/// The GPU device translucent windows share (`window.opacity` below 1): on Windows it presents
/// through DirectComposition, which keeps alpha. Started on the UI thread before such a window is
/// created, because the window has no surface the CPU renderer could show while it starts.
static TRANSLUCENT_GPU: OnceLock<Result<GpuContext, String>> = OnceLock::new();

/// Starts the translucent device when it has not started yet. False when no GPU can draw a
/// translucent window, and the window is then made opaque.
pub fn translucent_gpu_ready(display: OwnedDisplayHandle) -> bool {
    TRANSLUCENT_GPU.get_or_init(|| GpuContext::for_translucent_display(display).map_err(|e| e.to_string())).is_ok()
}

/// The painter of a translucent window: the GPU renderer on the translucent device, attached at
/// once (see `TRANSLUCENT_GPU`).
pub fn translucent_painter(window: &Arc<Window>) -> Result<Painter, String> {
    let Some(Ok(ctx)) = TRANSLUCENT_GPU.get() else { return Err("the GPU device did not start".into()) };
    let size = window.inner_size();
    let mut gpu = VelloGpuRenderer::new_detached_in(ctx, size.width, size.height, true);
    gpu.attach_window(ctx, window.clone(), unpaced(), true).map_err(|e| e.to_string())?;
    Ok(Painter::Gpu(Box::new(gpu)))
}

/// Glyphs the CPU renderer caches.
pub fn glyph_capacity() -> usize {
    termoil_render::DEFAULT_GLYPH_CAPACITY
}

/// Draws the layers straight into the window's softbuffer and presents the bands that changed.
/// The buffer holds the last frame when its age is 1; otherwise the renderer draws it whole.
fn render_and_present(
    renderer: &mut RasterRenderer, surface: &mut softbuffer::Surface<Arc<Window>, Arc<Window>>, background: Rgba,
    layers: &[Layer<'_>], grid: &FontGrid,
) -> Result<FrameStats, String> {
    let (w, h) = renderer.size();
    let mut buf = surface.buffer_mut().map_err(|e| format!("softbuffer: {e}"))?;
    if buf.len() < w as usize * h as usize {
        return Err("softbuffer: the buffer is smaller than the frame".into());
    }
    let valid = buf.age() == 1;
    let stats = renderer.render_into(&mut buf, valid, background, layers, grid);
    let rects: Vec<softbuffer::Rect> = renderer
        .damage()
        .iter()
        .filter_map(|&(top, bottom)| {
            let (top, bottom) = (top.min(h), bottom.min(h));
            Some(softbuffer::Rect {
                x: 0,
                y: top,
                width: NonZeroU32::new(w)?,
                height: NonZeroU32::new(bottom.checked_sub(top)?)?,
            })
        })
        .collect();
    if rects.is_empty() {
        return Ok(stats);
    }
    buf.present_with_damage(&rects).map_err(|e| format!("softbuffer: {e}"))?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `auto` draws on the CPU unless a shader needs the GPU; `gpu` and `cpu` do what they say.
    #[test]
    fn only_gpu_or_a_shader_starts_the_gpu() {
        assert!(!wants_gpu(Choice::Auto, "", ""));
        assert!(!wants_gpu(Choice::Auto, "  ", ""));
        assert!(wants_gpu(Choice::Auto, "crt.wgsl", ""));
        assert!(wants_gpu(Choice::Auto, "", "sonar"));
        assert!(wants_gpu(Choice::Gpu, "", ""));
        assert!(!wants_gpu(Choice::Cpu, "crt.wgsl", "sonar"));
    }
}
