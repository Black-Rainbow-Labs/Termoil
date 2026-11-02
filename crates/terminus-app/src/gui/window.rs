//! One window: its renderer and font grid, the layout of the active tab's panes, and drawing.
//!
//! Drawing holds each pane's terminal lock only to copy its render state (section 4.2); building
//! frames and painting happen after the lock is released.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminus_font::FontGrid;
use terminus_input::KeyNormalizer;
use terminus_render::{BuildOptions, Colors, Frame, FrameBuilder, Layer, RenderState, Rgba};
use terminus_vt::Terminal;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::app::{App, Grid, Pane};
use crate::config::KeyChord;
use crate::gui::chrome::{self, Strip, TabSpan};
use crate::gui::look;
use crate::gui::paint::Painter;
use crate::model::{PaneId, Rect, WinId};

/// How long synchronized output (mode 2026) may hold a frame back.
const SYNC_TIMEOUT: Duration = Duration::from_millis(150);

/// What a window keeps per visible pane.
pub struct PaneView {
    /// The render state copied from the terminal.
    pub state: RenderState,
    /// The frame builder with its row cache.
    pub builder: FrameBuilder,
    /// The last frame.
    pub frame: Option<Frame>,
    /// The pane's rectangle in window pixels.
    pub rect: Rect,
    /// When synchronized output started holding this pane's frame.
    pub sync_since: Option<Instant>,
}

/// Mouse state.
#[derive(Default)]
pub struct Mouse {
    /// Cursor position in window pixels.
    pub pos: (f64, f64),
    /// A selection is being dragged in this pane.
    pub selecting: Option<PaneId>,
    /// Buttons held, for motion reports.
    pub buttons: u8,
    /// Time, cell and count of the last left click, for double and triple clicks.
    pub last_click: Option<(Instant, (usize, u16), u8)>,
    /// The cell of the last motion report.
    pub last_report: Option<(u32, u32)>,
}

/// One window.
pub struct WinState {
    /// The model window.
    pub id: WinId,
    /// The winit window.
    pub window: Arc<Window>,
    /// The renderer.
    pub painter: Painter,
    /// Fonts at this window's size and scale.
    pub grid: FontGrid,
    /// Font size in points (changed by the font size actions).
    pub points: f32,
    /// Display scale.
    pub scale: f64,
    /// Views of the visible panes.
    pub views: HashMap<PaneId, PaneView>,
    /// The visible panes and their rectangles.
    pub layout: Vec<(PaneId, Rect)>,
    /// Where each tab is in the tab bar.
    pub tab_spans: Vec<TabSpan>,
    /// Height of the tab bar (0 when hidden).
    pub tab_bar_h: f32,
    /// The tab bar.
    pub tab_strip: Strip,
    /// The config error banner.
    pub banner_strip: Strip,
    /// The IME preedit.
    pub preedit_strip: Strip,
    /// Held modifiers.
    pub mods: ModifiersState,
    /// Dead key and AltGr state.
    pub normalizer: KeyNormalizer,
    /// Keys pressed so far of a chained binding.
    pub chords: Vec<KeyChord>,
    /// IME composition text.
    pub preedit: Option<String>,
    /// Mouse state.
    pub mouse: Mouse,
    /// The window has keyboard focus.
    pub focused: bool,
    /// Frame builder options.
    pub opts: BuildOptions,
    /// Pane colours.
    pub colors: Colors,
    /// The window has been shown (after its first frame).
    pub shown: bool,
    /// When to draw again for synchronized output or a timer.
    pub wake_at: Option<Instant>,
    /// The GPU renderer being built, until the window switches to it.
    pub upgrade: Option<crate::gui::paint::GpuUpgrade>,
}

impl WinState {
    /// Creates the window for model window `id`, sized for the configured columns and rows, hidden
    /// until its first frame.
    pub fn create(el: &ActiveEventLoop, app: &App, id: WinId, os_dark: bool) -> Result<WinState, String> {
        let config = &app.config;
        let attrs = Window::default_attributes()
            .with_title("Terminus")
            .with_visible(false)
            .with_decorations(config.window.decorations);
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("could not create a window: {e}"))?);
        crate::gui::phase("window_created");
        let scale = window.scale_factor();
        let points = config.font.size as f32;
        let grid = look::font_grid(config, points, scale)?;
        crate::gui::phase("fonts_loaded");
        let opts = look::build_options(config, scale);
        let m = *grid.metrics();
        let w = config.window.columns * m.width + 2 * opts.padding.0;
        let h = config.window.rows * m.height + 2 * opts.padding.1;
        let _ = window.request_inner_size(PhysicalSize::new(w, h));
        let (painter, upgrade) = Painter::new(&window, config.window.renderer)?;
        crate::gui::phase("renderer_ready");
        let colors = look::colors(config, os_dark);
        Ok(WinState {
            id,
            window,
            painter,
            grid,
            points,
            scale,
            views: HashMap::new(),
            layout: Vec::new(),
            tab_spans: Vec::new(),
            tab_bar_h: 0.0,
            tab_strip: Strip::new(1, colors.clone()),
            banner_strip: Strip::new(1, colors.clone()),
            preedit_strip: Strip::new(1, colors.clone()),
            mods: ModifiersState::empty(),
            normalizer: KeyNormalizer::new(),
            chords: Vec::new(),
            preedit: None,
            mouse: Mouse::default(),
            focused: true,
            opts,
            colors,
            shown: false,
            wake_at: None,
            upgrade,
        })
    }

    /// Cell width and height in pixels.
    pub fn cell(&self) -> (u32, u32) {
        let m = self.grid.metrics();
        (m.width, m.height)
    }

    /// Rebuilds the font grid after a size or scale change.
    pub fn refont(&mut self, app: &App) {
        if let Ok(g) = look::font_grid(&app.config, self.points, self.scale) {
            self.grid = g;
        }
        self.opts = look::build_options(&app.config, self.scale);
        for v in self.views.values_mut() {
            v.builder = FrameBuilder::new();
            v.state.needs_full = true;
        }
    }

    /// Lays out the active tab: the tab bar when there is more than one tab, then the split tree
    /// (or the zoomed pane), resizing each pane's session to its rectangle.
    pub fn relayout(&mut self, app: &mut App) {
        let Some(wi) = app.model.window_index(self.id) else { return };
        let size = self.window.inner_size();
        let (cw, ch) = self.cell();
        let tabs = app.model.windows[wi].tabs.len();
        self.tab_bar_h = if tabs > 1 && app.config.window.show_tab_bar { ch as f32 + 6.0 } else { 0.0 };
        let banner_h = if app.config_error.is_some() { ch as f32 + 4.0 } else { 0.0 };
        let top = self.tab_bar_h + banner_h;
        let area = Rect { x: 0.0, y: top, w: size.width as f32, h: (size.height as f32 - top).max(0.0) };
        let tab = app.model.windows[wi].tab();
        let mut layout = Vec::new();
        match tab.zoomed {
            Some(p) => layout.push((p, area)),
            None => tab.root.layout(area, (2.0 * self.scale as f32).round(), &mut layout),
        }
        let pad = self.opts.padding;
        for (pane, r) in &layout {
            let cols = ((r.w as u32).saturating_sub(2 * pad.0) / cw).clamp(2, 1000) as u16;
            let rows = ((r.h as u32).saturating_sub(2 * pad.1) / ch).clamp(1, 500) as u16;
            if let Some(p) = app.panes.get(pane) {
                p.session.resize(cols, rows, cw as u16, ch as u16);
            }
        }
        let full_cols = ((area.w as u32).saturating_sub(2 * pad.0) / cw).clamp(2, 1000) as u16;
        let full_rows = ((area.h as u32).saturating_sub(2 * pad.1) / ch).clamp(1, 500) as u16;
        app.grids.insert(self.id, Grid { cols: full_cols, rows: full_rows, cell_w: cw as u16, cell_h: ch as u16 });
        self.views.retain(|p, _| layout.iter().any(|(q, _)| q == p));
        self.layout = layout;
        self.window.request_redraw();
    }

    /// The pane under a window position, with the cell (column, row) and the pixel offset inside the
    /// pane's text area.
    pub fn pane_at(&self, x: f64, y: f64) -> Option<(PaneId, u16, u16, (u32, u32))> {
        let (cw, ch) = self.cell();
        let pad = self.opts.padding;
        let (pane, r) = self
            .layout
            .iter()
            .find(|(_, r)| x >= r.x as f64 && x < (r.x + r.w) as f64 && y >= r.y as f64 && y < (r.y + r.h) as f64)?;
        let px = (x - r.x as f64 - pad.0 as f64).max(0.0);
        let py = (y - r.y as f64 - pad.1 as f64).max(0.0);
        let cols = ((r.w as u32).saturating_sub(2 * pad.0) / cw).max(1);
        let rows = ((r.h as u32).saturating_sub(2 * pad.1) / ch).max(1);
        let col = ((px as u32) / cw).min(cols - 1) as u16;
        let row = ((py as u32) / ch).min(rows - 1) as u16;
        Some((*pane, col, row, (px as u32, py as u32)))
    }

    /// Draws the window: each visible pane, the tab bar, the banner and the preedit.
    pub fn draw(&mut self, app: &mut App) -> Result<(), String> {
        self.take_gpu();
        let Some(wi) = app.model.window_index(self.id) else { return Ok(()) };
        let focus = app.model.windows[wi].tab().focus;
        self.wake_at = None;
        let layout = self.layout.clone();
        for (pane, rect) in &layout {
            if let Some(p) = app.panes.get(pane) {
                let focused = self.focused && *pane == focus;
                self.update_view(*pane, *rect, p, focused, &app.config.clipboard.word_separators);
            }
        }
        let bg = look::mix(self.colors.background, self.colors.foreground, 0.2);
        let chrome = self.build_chrome(app, wi, focus);
        let mut layers: Vec<Layer<'_>> = Vec::new();
        for (pane, rect) in &layout {
            if let Some(f) = self.views.get(pane).and_then(|v| v.frame.as_ref()) {
                layers.push(Layer { x: rect.x, y: rect.y, frame: f });
            }
        }
        let strips =
            [(&self.tab_strip, chrome.tab), (&self.banner_strip, chrome.banner), (&self.preedit_strip, chrome.preedit)];
        for (strip, at) in strips {
            if let (Some((x, y)), Some(f)) = (at, strip.last_frame()) {
                layers.push(Layer { x, y, frame: f });
            }
        }
        self.painter.paint(Rgba::rgb(bg.r, bg.g, bg.b), &layers, &self.grid)?;
        if !self.shown {
            crate::gui::phase("first_paint");
        }
        self.place_ime(focus);
        if !self.shown {
            self.window.set_visible(true);
            self.shown = true;
            crate::gui::phase("shown");
            // Allowing IME starts the text services on this thread, which costs tens of
            // milliseconds; it is done after the first frame is on screen.
            self.window.set_ime_allowed(true);
            crate::gui::phase("ime_allowed");
        }
        Ok(())
    }

    /// Switches to the GPU renderer once the background thread has built it. A failure keeps the
    /// CPU renderer and says why.
    fn take_gpu(&mut self) {
        let Some(rx) = &self.upgrade else { return };
        match rx.try_recv() {
            Ok(Ok(mut gpu)) => {
                let size = self.window.inner_size();
                terminus_render::Renderer::resize(&mut gpu, size.width, size.height);
                self.painter = Painter::Gpu(Box::new(gpu));
                self.upgrade = None;
            }
            Ok(Err(e)) => {
                eprintln!("terminus: GPU renderer unavailable, staying on the CPU renderer: {e}");
                self.upgrade = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.upgrade = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Copies a pane's state under its lock and builds its frame.
    fn update_view(&mut self, pane: PaneId, rect: Rect, p: &Pane, focused: bool, separators: &str) {
        let colors = self.colors.clone();
        let view = self.views.entry(pane).or_insert_with(|| PaneView {
            state: RenderState::new(1, 1, colors),
            builder: FrameBuilder::new(),
            frame: None,
            rect,
            sync_since: None,
        });
        view.rect = rect;
        {
            let mut t = p.session.lock();
            if t.sync_pending() {
                let since = *view.sync_since.get_or_insert_with(Instant::now);
                if since.elapsed() < SYNC_TIMEOUT && view.frame.is_some() {
                    self.wake_at = Some(since + SYNC_TIMEOUT);
                    return;
                }
                t.sync_timeout();
            }
            view.sync_since = None;
            let sel = selection_rows(p, &t, separators);
            if sel != view.state.selection || view.state.focused != focused {
                view.state.selection = sel;
                view.state.focused = focused;
                view.state.needs_full = true;
            }
            view.state.update(&mut t, p.scroll);
        }
        let mut frame = view.builder.build(&view.state, &mut self.grid, &self.opts);
        frame.width = rect.w.max(1.0) as u32;
        frame.height = rect.h.max(1.0) as u32;
        view.frame = Some(frame);
    }

    /// Builds the tab bar, banner and preedit strips; returns where each goes.
    fn build_chrome(&mut self, app: &App, wi: usize, focus: PaneId) -> ChromeAt {
        let mut at = ChromeAt::default();
        let (cw, _) = self.cell();
        let cols = (self.window.inner_size().width / cw.max(1)) as u16;
        let opts = BuildOptions { padding: (0, 3), all_rows: true, minimum_contrast: 1.0, ..self.opts.clone() };
        if self.tab_bar_h > 0.0 {
            let w = &app.model.windows[wi];
            let titles: Vec<String> = w.tabs.iter().map(|t| crate::view::tab_title(app, t)).collect();
            let (bytes, spans) = chrome::tab_bar(&titles, w.active, cols, &self.colors);
            self.tab_strip.frame(cols, &bytes, &self.colors, &mut self.grid, &opts);
            self.tab_spans = spans;
            at.tab = Some((0.0, 0.0));
        }
        if let Some(msg) = &app.config_error {
            self.banner_strip.frame(
                cols,
                &chrome::banner(msg),
                &self.colors,
                &mut self.grid,
                &BuildOptions { padding: (0, 2), ..opts.clone() },
            );
            at.banner = Some((0.0, self.tab_bar_h));
        }
        if let (Some(text), Some((x, y))) = (self.preedit.clone(), self.cursor_px(focus)) {
            let n = text.chars().count() as u16 * 2 + 1;
            self.preedit_strip.frame(
                n,
                &chrome::preedit(&text, &self.colors),
                &self.colors,
                &mut self.grid,
                &BuildOptions { padding: (0, 0), ..opts },
            );
            at.preedit = Some((x, y));
        }
        at
    }

    /// The window pixel position of a pane's cursor cell.
    fn cursor_px(&self, pane: PaneId) -> Option<(f32, f32)> {
        let v = self.views.get(&pane)?;
        let c = v.state.cursor?;
        let (cw, ch) = self.cell();
        Some((
            v.rect.x + self.opts.padding.0 as f32 + (c.col as u32 * cw) as f32,
            v.rect.y + self.opts.padding.1 as f32 + (c.row as u32 * ch) as f32,
        ))
    }

    /// Tells the IME where the cursor is, so its candidate window opens there.
    fn place_ime(&self, focus: PaneId) {
        if let Some((x, y)) = self.cursor_px(focus) {
            let (cw, ch) = self.cell();
            self.window.set_ime_cursor_area(PhysicalPosition::new(x as f64, y as f64), PhysicalSize::new(cw, ch));
        }
    }
}

/// Where the chrome strips go this frame.
#[derive(Default)]
struct ChromeAt {
    tab: Option<(f32, f32)>,
    banner: Option<(f32, f32)>,
    preedit: Option<(f32, f32)>,
}

impl Strip {
    /// The frame built last, if any.
    pub fn last_frame(&self) -> Option<&Frame> {
        self.frame_ref()
    }
}

/// The selected cells of each visible row, as (row, first column, last column).
fn selection_rows(p: &Pane, t: &Terminal, separators: &str) -> Vec<(u16, u16, u16)> {
    let Some(sel) = &p.selection else { return Vec::new() };
    let pages = &t.screen().pages;
    let Some(r) = sel.range(pages, separators) else { return Vec::new() };
    let top = pages.active_y(0).saturating_sub(p.scroll);
    let cols = t.cols();
    let mut out = Vec::new();
    for row in 0..t.rows() {
        let y = top + row as usize;
        if y < r.start.0 || y > r.end.0 {
            continue;
        }
        let (a, b) = if r.block {
            (r.start.1.min(r.end.1), r.start.1.max(r.end.1))
        } else {
            (if y == r.start.0 { r.start.1 } else { 0 }, if y == r.end.0 { r.end.1 } else { cols - 1 })
        };
        out.push((row, a, b.min(cols - 1)));
    }
    out
}
