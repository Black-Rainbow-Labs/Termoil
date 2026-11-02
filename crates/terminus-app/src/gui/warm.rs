//! Start up work for the first window that does not need the window (P4,
//! `docs/decisions/startup.md`). Loading the font grid took 11 to 13 ms and the first CPU frame
//! took 16 ms more than later ones (the glyph atlas and the render threads start on it), all on
//! the UI thread before the first paint. A thread started as soon as the event loop exists does
//! both while the UI thread spawns the shell and creates the window: it loads the grid at the
//! scale the window will most likely have, and renders printable ASCII once into the CPU renderer
//! the window will use. The first window takes them when its scale matches; otherwise it loads
//! its own, as before.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use terminus_font::FontGrid;
use terminus_render::{Layer, Renderer, Rgba, VelloCpuRenderer};

use crate::config::Config;
use crate::gui::chrome::Strip;
use crate::gui::look;

/// What the thread made: the font grid and a CPU renderer that has drawn once.
type Made = Result<(FontGrid, Box<VelloCpuRenderer>), String>;

/// The thread's result and the scale it used.
static WARM: Mutex<Option<(f64, Receiver<Made>)>> = Mutex::new(None);

/// Starts the thread when this platform can say the scale before a window exists.
pub fn start(config: &Config) {
    let Some(scale) = likely_scale() else { return };
    let config = config.clone();
    let (tx, rx) = channel();
    let started = std::thread::Builder::new().name("terminus warm start".into()).spawn(move || {
        let _ = tx.send(make(&config, scale));
    });
    if started.is_ok()
        && let Ok(mut w) = WARM.lock()
    {
        *w = Some((scale, rx));
    }
}

/// The grid and renderer for a window at `scale`, once: None when the thread did not run, failed,
/// or used another scale.
pub fn take(scale: f64) -> Option<(FontGrid, Box<VelloCpuRenderer>)> {
    let (at, rx) = WARM.lock().ok()?.take()?;
    if (at - scale).abs() > f64::EPSILON {
        return None;
    }
    rx.recv().ok()?.ok()
}

/// Loads the grid and draws printable ASCII once with a renderer sized like the first window.
fn make(config: &Config, scale: f64) -> Made {
    let mut grid = look::font_grid(config, config.font.size as f32, scale)?;
    let opts = look::build_options(config, scale);
    let (w, h) = look::window_size(config, grid.metrics(), &opts, scale);
    let mut renderer = Box::new(VelloCpuRenderer::new(w, h, crate::gui::paint::cpu_threads(), true));
    let colors = look::colors(config, true);
    let text: Vec<u8> = (0x20u8..0x7f).collect();
    let mut strip = Strip::new(1, colors.clone());
    let frame = strip.frame(text.len() as u16, 1, &text, &colors, &mut grid, &opts);
    renderer.render_layers(Rgba::rgb(0, 0, 0), &[Layer { x: 0.0, y: 0.0, frame }], &grid).map_err(|e| e.to_string())?;
    Ok((grid, renderer))
}

/// The scale a new window will most likely get: the system DPI on Windows. Other platforms do
/// not say before a window exists, so they do not warm up.
fn likely_scale() -> Option<f64> {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; reads the DPI the process (made DPI aware by the event loop) sees.
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
        (dpi > 0).then(|| dpi as f64 / 96.0)
    }
    #[cfg(not(windows))]
    None
}
