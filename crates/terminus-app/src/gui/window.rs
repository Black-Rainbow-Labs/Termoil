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
use terminus_vt::terminal::CursorShape;
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

/// Set by a key press so the blinking cursor shows at once.
pub static BLINK_RESET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    /// The last frame was drawn inverted for the visual bell.
    pub flashed: bool,
    /// The absolute row shown at the top of the pane.
    pub top: usize,
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
    /// The tab being dragged in the tab bar, by index.
    pub tab_drag: Option<usize>,
    /// A divider being dragged: the pane on its left or top side, the split's axis, and the
    /// pointer position the last move was applied at.
    pub divider: Option<(PaneId, crate::model::Axis, (f64, f64))>,
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
    /// When the blinking cursor last changed between shown and hidden, and whether it shows.
    pub blink: (Instant, bool),
    /// The link under the pointer: its pane and cells as (absolute row, first column, last column).
    pub hover: Option<crate::links::Hover>,
    /// The visual bell shows until this moment.
    pub bell_until: Option<Instant>,
    /// The open palette or search bar.
    pub overlay: Option<crate::gui::overlay::Overlay>,
    /// The overlay's strip.
    pub overlay_strip: Strip,
    /// One small strip per hint label.
    pub hint_strips: Vec<Strip>,
    /// The GPU renderer being built, until the window switches to it.
    pub upgrade: Option<crate::gui::paint::GpuUpgrade>,
}

impl WinState {
    /// Creates the window for model window `id`, sized for the configured columns and rows, hidden
    /// until its first frame.
    /// `quick` names the screen edge when the window is the quick terminal.
    pub fn create(
        el: &ActiveEventLoop, app: &App, id: WinId, os_dark: bool, quick: Option<crate::config::Edge>,
    ) -> Result<WinState, String> {
        let config = &app.config;
        let attrs = Window::default_attributes()
            .with_title("Terminus")
            .with_visible(false)
            .with_decorations(config.window.decorations)
            .with_theme(Some(window_theme(&look::colors(config, os_dark))));
        let attrs = with_app_icon(attrs);
        let attrs = if quick.is_some() { crate::gui::quick::attributes(attrs) } else { attrs };
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
        match quick {
            Some(edge) => crate::gui::quick::place(&window, edge),
            None => {
                let _ = window.request_inner_size(PhysicalSize::new(w, h));
            }
        }
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
            overlay: None,
            hint_strips: Vec::new(),
            bell_until: None,
            hover: None,
            blink: (Instant::now(), true),
            overlay_strip: Strip::new(1, look::colors(config, os_dark)),
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

    /// The divider under a window position: the pane whose right (or bottom) edge it is, and the
    /// axis of the split it belongs to.
    pub fn divider_at(&self, x: f64, y: f64) -> Option<(PaneId, crate::model::Axis)> {
        let slack = 4.0 * self.scale;
        self.layout.iter().find_map(|(pane, r)| {
            let (right, bottom) = ((r.x + r.w) as f64, (r.y + r.h) as f64);
            let in_rows = y >= r.y as f64 && y < bottom;
            let in_cols = x >= r.x as f64 && x < right;
            if in_rows
                && x >= right - 1.0
                && x < right + slack
                && self.layout.iter().any(|(_, o)| (o.x as f64 - right).abs() <= slack + 1.0)
            {
                return Some((*pane, crate::model::Axis::Horizontal));
            }
            if in_cols
                && y >= bottom - 1.0
                && y < bottom + slack
                && self.layout.iter().any(|(_, o)| (o.y as f64 - bottom).abs() <= slack + 1.0)
            {
                return Some((*pane, crate::model::Axis::Vertical));
            }
            None
        })
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
                self.update_view(
                    *pane,
                    *rect,
                    p,
                    focused,
                    &app.config.clipboard.word_separators,
                    app.config.cursor.blink_interval_ms,
                );
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
        for (strip, (x, y)) in self.hint_strips.iter().zip(chrome.hints.iter().copied()) {
            if let Some(f) = strip.last_frame() {
                layers.push(Layer { x, y, frame: f });
            }
        }
        let strips = [
            (&self.tab_strip, chrome.tab),
            (&self.banner_strip, chrome.banner),
            (&self.preedit_strip, chrome.preedit),
            (&self.overlay_strip, chrome.overlay),
        ];
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
    fn update_view(&mut self, pane: PaneId, rect: Rect, p: &Pane, focused: bool, separators: &str, blink_ms: u64) {
        let colors = self.colors.clone();
        let view = self.views.entry(pane).or_insert_with(|| PaneView {
            state: RenderState::new(1, 1, colors),
            builder: FrameBuilder::new(),
            frame: None,
            rect,
            flashed: false,
            top: 0,
            sync_since: None,
        });
        view.rect = rect;
        let flash = self.bell_until.is_some_and(|t| Instant::now() < t);
        if let Some(t) = self.bell_until.filter(|_| flash) {
            self.wake_at = Some(self.wake_at.map_or(t, |w| w.min(t)));
        }
        if view.flashed != flash {
            view.flashed = flash;
            view.state.needs_full = true;
        }
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
            let hover = hover_rows(self.hover.as_ref(), pane, &t, p.scroll);
            let hits = search_rows(p, &t);
            if sel != view.state.selection
                || view.state.focused != focused
                || hover != view.state.link_hover
                || hits != view.state.search_hits
            {
                view.state.selection = sel;
                view.state.focused = focused;
                view.state.link_hover = hover;
                view.state.search_hits = hits;
                view.state.needs_full = true;
            }
            view.state.update(&mut t, p.scroll);
            view.top = t.screen().pages.active_y(0).saturating_sub(p.scroll);
        }
        // The visual bell draws the pane inverted until `bell_until`.
        if flash {
            view.state.reverse_video = !view.state.reverse_video;
        }
        let blink =
            blink_phase(&mut self.blink, &mut self.wake_at, self.opts.default_cursor, focused, &view.state, blink_ms);
        if let Some(c) = view.state.cursor.as_mut().filter(|_| !blink) {
            c.visible = false;
            if let Some(line) = view.state.lines.get_mut(c.row as usize) {
                line.dirty = true;
            }
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
            let titles: Vec<String> = w.tabs.iter().map(|t| tab_label(app, t)).collect();
            let (bytes, spans) = chrome::tab_bar(&titles, w.active, cols, &self.colors);
            self.tab_strip.frame(cols, 1, &bytes, &self.colors, &mut self.grid, &opts);
            self.tab_spans = spans;
            at.tab = Some((0.0, 0.0));
        }
        if let Some(msg) = &app.config_error {
            self.banner_strip.frame(
                cols,
                1,
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
                1,
                &chrome::preedit(&text, &self.colors),
                &self.colors,
                &mut self.grid,
                &BuildOptions { padding: (0, 0), ..opts },
            );
            at.preedit = Some((x, y));
        }
        at.overlay = self.build_overlay(cols, &opts);
        at.hints = self.build_hints(&opts);
        at
    }

    /// Builds the palette or search bar strip and returns where it goes: the palette centred near
    /// the top, the search bar along the bottom.
    fn build_overlay(&mut self, cols: u16, opts: &BuildOptions) -> Option<(f32, f32)> {
        use crate::gui::overlay::{Overlay, PALETTE_COLS, palette_content, search_content};
        let (cw, ch) = self.cell();
        let size = self.window.inner_size();
        let opts = BuildOptions { padding: (cw / 2, 4), ..opts.clone() };
        match self.overlay.as_ref()? {
            Overlay::Palette(p) => {
                let width = cols.saturating_sub(4).clamp(10, PALETTE_COLS);
                let (bytes, rows) = palette_content(p, width, &self.colors);
                self.overlay_strip.frame(width, rows, &bytes, &self.colors, &mut self.grid, &opts);
                let x = (size.width as f32 - (width as u32 * cw + 2 * opts.padding.0) as f32) / 2.0;
                Some((x.max(0.0), self.tab_bar_h + ch as f32 / 2.0))
            }
            Overlay::Hints(_) => None,
            Overlay::Themes(t) => {
                let width = cols.saturating_sub(4).clamp(10, 60);
                let (bytes, rows) = crate::gui::overlay::themes_content(t, width, &self.colors);
                self.overlay_strip.frame(width, rows, &bytes, &self.colors, &mut self.grid, &opts);
                let x = (size.width as f32 - (width as u32 * cw + 2 * opts.padding.0) as f32) / 2.0;
                Some((x.max(0.0), self.tab_bar_h + ch as f32 / 2.0))
            }
            Overlay::Search(s) => {
                let bytes = search_content(s, cols, &self.colors);
                self.overlay_strip.frame(cols.saturating_sub(1).max(1), 1, &bytes, &self.colors, &mut self.grid, &opts);
                Some((0.0, size.height as f32 - (ch + 2 * opts.padding.1) as f32))
            }
        }
    }

    /// Builds one strip per hint label still matching what was typed, and returns where each goes:
    /// over the first cell of its link.
    fn build_hints(&mut self, opts: &BuildOptions) -> Vec<(f32, f32)> {
        use crate::gui::overlay::{Overlay, hint_content};
        let Some(Overlay::Hints(h)) = self.overlay.as_ref() else { return Vec::new() };
        let Some(view) = self.views.get(&h.pane) else { return Vec::new() };
        let (cw, ch) = self.cell();
        let rect = view.rect;
        let top = view.top;
        let items: Vec<(String, usize, u16)> =
            h.items.iter().filter(|(l, ..)| l.starts_with(&h.typed)).map(|(l, _, y, c)| (l.clone(), *y, *c)).collect();
        let typed = h.typed.clone();
        while self.hint_strips.len() < items.len() {
            self.hint_strips.push(Strip::new(1, self.colors.clone()));
        }
        let opts = BuildOptions { padding: (0, 0), ..opts.clone() };
        let mut at = Vec::new();
        for (i, (label, y, col)) in items.iter().enumerate() {
            let Some(row) = y.checked_sub(top) else { continue };
            let n = label.chars().count() as u16;
            self.hint_strips[i].frame(n, 1, &hint_content(label, &typed), &self.colors, &mut self.grid, &opts);
            at.push((
                rect.x + self.opts.padding.0 as f32 + (*col as u32 * cw) as f32,
                rect.y + self.opts.padding.1 as f32 + (row as u32 * ch) as f32,
            ));
        }
        self.hint_strips.truncate(items.len().max(1));
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

/// A tab's label in the tab bar: its title, and the exit code of its focused pane's last command
/// when that command failed.
fn tab_label(app: &App, t: &crate::model::Tab) -> String {
    let title = crate::view::tab_title(app, t);
    match app.panes.get(&t.focus).and_then(|p| p.session.prompt_state().last_exit) {
        Some(code) if code != 0 => format!("{title} \u{2717}{code}"),
        _ => title,
    }
}

/// The title bar theme matching the terminal background: dark for a dark background.
pub fn window_theme(colors: &Colors) -> winit::window::Theme {
    let b = colors.background;
    let luma = 0.2126 * b.r as f32 + 0.7152 * b.g as f32 + 0.0722 * b.b as f32;
    if luma < 128.0 { winit::window::Theme::Dark } else { winit::window::Theme::Light }
}

/// Adds the icon embedded in the executable (resource 1) as the window and taskbar icon.
fn with_app_icon(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    #[cfg(windows)]
    {
        use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
        if let Ok(icon) = winit::window::Icon::from_resource(1, None) {
            return attrs.with_window_icon(Some(icon.clone())).with_taskbar_icon(Some(icon));
        }
    }
    attrs
}

/// Where the chrome strips go this frame.
#[derive(Default)]
struct ChromeAt {
    tab: Option<(f32, f32)>,
    banner: Option<(f32, f32)>,
    preedit: Option<(f32, f32)>,
    overlay: Option<(f32, f32)>,
    hints: Vec<(f32, f32)>,
}

impl Strip {
    /// The frame built last, if any.
    pub fn last_frame(&self) -> Option<&Frame> {
        self.frame_ref()
    }
}

/// Whether a blinking cursor shows in this frame. Only the focused pane's cursor blinks, and only
/// when its shape asks for it (DECSCUSR, or `cursor.blink` for the default shape). A key press shows
/// it at once. `blink` is when the cursor last changed and whether it shows; the next change is
/// added to `wake_at`.
fn blink_phase(
    blink: &mut (Instant, bool), wake_at: &mut Option<Instant>, default: CursorShape, focused: bool,
    state: &RenderState, interval_ms: u64,
) -> bool {
    let wants = |s: CursorShape| {
        matches!(
            s,
            CursorShape::Block { blink: true }
                | CursorShape::Underline { blink: true }
                | CursorShape::Bar { blink: true }
        )
    };
    let blinks = match state.cursor.map(|c| c.shape) {
        Some(CursorShape::Default) => wants(default),
        Some(shape) => wants(shape),
        None => false,
    };
    if !focused || !blinks {
        return true;
    }
    let interval = Duration::from_millis(interval_ms.max(100));
    if BLINK_RESET.swap(false, std::sync::atomic::Ordering::Relaxed) {
        *blink = (Instant::now(), true);
    } else if blink.0.elapsed() >= interval {
        *blink = (Instant::now(), !blink.1);
    }
    let next = blink.0 + interval;
    *wake_at = Some(wake_at.map_or(next, |w| w.min(next)));
    blink.1
}

/// The hovered link's cells on each visible row of `pane`.
fn hover_rows(hover: Option<&crate::links::Hover>, pane: PaneId, t: &Terminal, scroll: usize) -> Vec<(u16, u16, u16)> {
    let Some((p, rows)) = hover else { return Vec::new() };
    if *p != pane {
        return Vec::new();
    }
    let top = t.screen().pages.active_y(0).saturating_sub(scroll);
    rows.iter()
        .filter(|(y, _, _)| *y >= top && *y < top + t.rows() as usize)
        .map(|&(y, a, b)| ((y - top) as u16, a, b))
        .collect()
}

/// The visible cells matching the pane's active search, per row.
fn search_rows(p: &Pane, t: &Terminal) -> Vec<(u16, u16, u16)> {
    let Some(search) = p.search.as_ref().filter(|s| !s.pattern.is_empty()) else { return Vec::new() };
    let Ok(re) = terminus_vt::search::compile(&search.pattern, search.case_sensitive) else { return Vec::new() };
    let pages = &t.screen().pages;
    let top = pages.active_y(0).saturating_sub(p.scroll);
    let rows = t.rows() as usize;
    let cols = t.cols();
    let mut out = Vec::new();
    for m in terminus_vt::search::find_all(pages, &re, terminus_vt::text::line_start(pages, top)..top + rows) {
        for y in m.start.0..=m.end.0 {
            if y < top || y >= top + rows {
                continue;
            }
            let a = if y == m.start.0 { m.start.1 } else { 0 };
            let b = if y == m.end.0 { m.end.1 } else { cols - 1 };
            out.push(((y - top) as u16, a, b));
        }
    }
    out
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
