//! One window: its renderer and font grid, the layout of the active tab's panes, and drawing.
//!
//! Drawing holds each pane's terminal lock only to copy its render state (section 4.2); building
//! frames and painting happen after the lock is released.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use termoil_font::FontGrid;
use termoil_input::{Key, KeyNormalizer};
use termoil_render::{BuildOptions, Colors, Frame, FrameBuilder, Layer, RenderState, Rgba};
use termoil_vt::Terminal;
use termoil_vt::terminal::CursorShape;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::app::{App, Grid, Pane};
use crate::config::KeyChord;
use crate::gui::chrome::{self, Strip};
use crate::gui::look;
use crate::gui::paint::Painter;
use crate::gui::tab_bar::{self, TabHit, TabLayout};
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
    /// Where each tab and button is in the tab bar.
    pub tab_layout: TabLayout,
    /// The part of the tab bar under the pointer.
    pub tab_hover: Option<TabHit>,
    /// The theme's colours for the tab bar.
    pub chrome: crate::config::Chrome,
    /// Height of the tab bar (0 when hidden).
    pub tab_bar_h: f32,
    /// Height of the title strip Termoil draws itself on macOS when the window is translucent and
    /// the system title bar is made transparent (`title_points` at this window's scale; 0 otherwise).
    pub title_h: f32,
    /// The title strip's height in points: `window.macos_titlebar_height`, or 0 when the system
    /// draws the title bar.
    pub title_points: f32,
    /// The title strip.
    pub title_strip: Strip,
    /// Frames in a row that did not reach the screen, for the repaint backoff.
    pub repaint_misses: u32,
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
    /// Keys whose press ran a binding (or started a chained one). Their releases are not given to
    /// the program, which never saw the press.
    pub swallowed: HashSet<Key>,
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
    /// A frame has been presented since the window was shown: the first frame a person can see.
    pub presented_shown: bool,
    /// The window was shown without being activated and is activated by `finish_show`.
    pub pending_activation: bool,
    /// IME has been allowed for the window.
    pub ime_allowed: bool,
    /// Glyph hinting (`font.hinting`), kept for the GPU renderer when it takes over.
    pub hinting: bool,
    /// When the window last painted, for frame pacing.
    pub last_paint: Option<Instant>,
    /// When the person last typed, clicked or scrolled in this window.
    pub last_input: Option<Instant>,
    /// One refresh of the display the window is on: it never paints more often than this.
    pub frame_interval: std::time::Duration,
    /// Presents are not paced (`TERMOIL_PRESENT=immediate`, the latency benchmark).
    pub unpaced: bool,
    /// The window was made translucent (`window.opacity` below 1 and a GPU that can show it).
    pub translucent: bool,
    /// When to draw again for synchronized output or a timer.
    pub wake_at: Option<Instant>,
    /// The time a frame that came too soon was moved to; cleared when a frame is drawn. Wakes
    /// from this window's panes are held until then (`Gui::held_wakes`).
    pub paint_held_until: Option<Instant>,
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
    /// An answer that arrived while another overlay was open, shown when that overlay closes.
    pub answer_waiting: Option<crate::answer::Answer>,
    /// One small strip per hint label.
    pub hint_strips: Vec<Strip>,
    /// The GPU renderer being built, until the window switches to it.
    pub upgrade: Option<crate::gui::paint::GpuUpgrade>,
    /// The event loop's display, to start the GPU renderer later when a shader is set.
    pub display: winit::event_loop::OwnedDisplayHandle,
    /// Screen reader access, when `accessibility.enabled` was on as the window opened.
    pub access: Option<crate::gui::access::AccessState>,
    /// The custom shader (`window.shader`) and its error.
    pub shader: crate::gui::shader::ShaderState,
}

/// True when a saved window's top left area is on one of the connected monitors, so a window saved
/// on a monitor that has since been unplugged is not placed where it cannot be reached.
fn on_a_monitor(el: &ActiveEventLoop, g: &crate::restore::WindowGeometry) -> bool {
    let (px, py) = (g.x as i64 + 40, g.y as i64 + 40);
    el.available_monitors().any(|m| {
        let (p, s) = (m.position(), m.size());
        px >= p.x as i64 && px < p.x as i64 + s.width as i64 && py >= p.y as i64 && py < p.y as i64 + s.height as i64
    })
}

impl WinState {
    /// Creates the window for model window `id`, sized for the configured columns and rows, hidden
    /// until its first frame.
    /// `quick` names the screen edge when the window is the quick terminal.
    pub fn create(
        el: &ActiveEventLoop, app: &App, id: WinId, os_dark: bool, quick: Option<crate::config::Edge>,
        proxy: &winit::event_loop::EventLoopProxy<crate::gui::UserEvent>,
    ) -> Result<WinState, String> {
        let config = &app.config;
        let translucent =
            config.window.opacity < 1.0 && crate::gui::paint::translucent_gpu_ready(el.owned_display_handle());
        let attrs = translucent_attributes(Window::default_attributes(), translucent)
            .with_title("Termoil")
            .with_visible(false)
            .with_decorations(config.window.decorations)
            .with_theme(Some(window_theme(&look::colors(config, os_dark))));
        let attrs = with_app_icon(attrs);
        let titled = draws_own_title_bar(config, translucent, quick.is_some());
        let attrs = own_title_bar_attributes(attrs, titled);
        let attrs = if quick.is_some() { crate::gui::quick::attributes(attrs) } else { attrs };
        crate::gui::phase("window_attributes");
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("could not create a window: {e}"))?);
        crate::gui::phase("window_created");
        let access = config.accessibility.enabled.then(|| {
            let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, proxy.clone());
            crate::gui::access::AccessState::new(adapter)
        });
        crate::gui::phase("accessibility");
        let scale = window.scale_factor();
        let points = config.font.size as f32;
        let (grid, warm_cpu) = match crate::gui::warm::take(scale) {
            Some((grid, cpu)) => (grid, Some(cpu)),
            None => (look::font_grid(config, points, scale)?, None),
        };
        crate::gui::phase("fonts_loaded");
        let mut opts = look::build_options(config, scale);
        if translucent {
            opts.background_alpha = look::background_alpha(config);
        }
        let title_points = if titled { config.window.macos_titlebar_height as f32 } else { 0.0 };
        let (w, h) = look::window_size(config, grid.metrics(), &opts, scale);
        let h = h + (title_points as f64 * scale).round() as u32;
        match quick {
            Some(edge) => crate::gui::quick::place(&window, edge),
            None => match app.geometry.get(&id).filter(|g| on_a_monitor(el, g)) {
                Some(g) => {
                    let _ = window.request_inner_size(PhysicalSize::new(g.width.max(100), g.height.max(60)));
                    window.set_outer_position(winit::dpi::PhysicalPosition::new(g.x, g.y));
                }
                None => {
                    let _ = window.request_inner_size(PhysicalSize::new(w, h));
                }
            },
        }
        let (mut painter, upgrade) = if translucent {
            (crate::gui::paint::translucent_painter(&window)?, None)
        } else {
            let gpu = crate::gui::paint::wants_gpu(
                config.window.renderer,
                &config.window.shader,
                &config.window.cursor_effect,
            );
            Painter::new(&window, gpu, warm_cpu, el.owned_display_handle())?
        };
        crate::gui::backdrop::apply(&window, translucent, config.window.blur);
        painter.set_hinting(config.font.hinting);
        let mut shader = crate::gui::shader::ShaderState::new(config, &app.config_path);
        shader.apply(&mut painter, true);
        crate::gui::phase("renderer_ready");
        let colors = look::colors(config, os_dark);
        let interval = frame_interval(&window);
        Ok(WinState {
            id,
            window,
            painter,
            grid,
            points,
            scale,
            views: HashMap::new(),
            layout: Vec::new(),
            tab_layout: TabLayout::default(),
            tab_hover: None,
            chrome: look::chrome(config, os_dark),
            tab_bar_h: 0.0,
            title_h: 0.0,
            title_points,
            title_strip: Strip::new(1, colors.clone()),
            repaint_misses: 0,
            tab_strip: Strip::new(1, colors.clone()),
            banner_strip: Strip::new(1, colors.clone()),
            preedit_strip: Strip::new(1, colors.clone()),
            mods: ModifiersState::empty(),
            normalizer: KeyNormalizer::new(),
            chords: Vec::new(),
            swallowed: HashSet::new(),
            preedit: None,
            mouse: Mouse::default(),
            focused: true,
            opts,
            colors,
            shown: false,
            presented_shown: false,
            pending_activation: false,
            ime_allowed: false,
            hinting: config.font.hinting,
            last_paint: None,
            last_input: None,
            translucent,
            frame_interval: interval,
            unpaced: std::env::var("TERMOIL_PRESENT").is_ok_and(|v| v == "immediate"),
            wake_at: None,
            paint_held_until: None,
            upgrade,
            display: el.owned_display_handle(),
            access,
            shader,
            overlay: None,
            answer_waiting: None,
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
        if self.translucent {
            // A window made translucent follows `window.opacity` as it changes; an opaque window
            // stays opaque until it is opened again.
            self.opts.background_alpha = look::background_alpha(&app.config);
        }
        self.painter.set_hinting(app.config.font.hinting);
        self.hinting = app.config.font.hinting;
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
        self.tab_bar_h = look::tab_bar_height(&app.config, ch, self.scale, tabs) as f32;
        self.title_h = (self.title_points as f64 * self.scale).round() as f32;
        let banner_h = if self.banner_text(app).is_some() { ch as f32 + 4.0 } else { 0.0 };
        let top = self.title_h + self.tab_bar_h + banner_h;
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

    /// Pixels above and below the tab titles: `tab_bar.padding` at this window's scale.
    pub fn tab_bar_padding(&self, app: &App) -> u32 {
        (app.config.tab_bar.padding as f64 * self.scale).round() as u32
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
        self.paint_held_until = None;
        let had_shader_error = self.shader.error.is_some();
        self.take_gpu();
        if had_shader_error != self.shader.error.is_some() {
            self.relayout(app);
        }
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
        let (cw, ch) = self.cell();
        let shader_cursor = self.cursor_px(focus).map(|(x, y)| (x + cw as f32 / 2.0, y + ch as f32 / 2.0));
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
            (&self.title_strip, chrome.title),
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
        if !self.shown {
            crate::gui::phase("frame_built");
        }
        let window_colour = Rgba { a: self.opts.background_alpha, ..Rgba::rgb(bg.r, bg.g, bg.b) };
        self.painter.set_shader_cursor(shader_cursor);
        let painted = self.painter.paint(window_colour, &layers, &self.grid);
        if matches!(&painted, Ok(stats) if stats.presented) {
            self.repaint_misses = 0;
        } else {
            self.schedule_repaint(&app.config.window);
        }
        painted?;
        self.last_paint = Some(Instant::now());
        self.keep_animating();
        if self.shown {
            self.presented_shown = true;
        } else {
            crate::gui::phase("first_paint");
        }
        self.place_ime(focus);
        self.push_access(app, false);
        if !self.shown {
            crate::gui::showtrace::begin();
            self.pending_activation = crate::gui::show::first(&self.window);
            crate::gui::showtrace::end();
            self.shown = true;
            crate::gui::phase("shown");
        }
        Ok(())
    }

    /// Sends the window's accessibility tree to the screen reader when it changed, at most once per
    /// `accessibility.update_interval_ms` for a change of text. `force` sends it in any case and marks the
    /// first request from a client; before that nothing is captured or sent.
    /// A held back update wakes the window to try again.
    pub fn push_access(&mut self, app: &App, force: bool) {
        let Some(access) = self.access.as_mut() else { return };
        // `force` is the first request from a client; nothing is sent before one.
        access.requested |= force;
        if !access.requested {
            return;
        }
        let Some((info, text)) = crate::gui::access::snapshot(
            app,
            self.id,
            &self.layout,
            (self.grid.metrics().width, self.grid.metrics().height),
        ) else {
            return;
        };
        let interval = Duration::from_millis(app.config.accessibility.update_interval_ms);
        if let Some(due) = access.push(info, text, interval, force, Instant::now()) {
            self.wake_at = Some(self.wake_at.map_or(due, |w| w.min(due)));
        }
    }

    /// When a paint now would come sooner than one frame interval after the last one: the time it
    /// is due instead (PRD 5.2: paint on damage at the display rate and never faster). None means
    /// paint now. The interval is one display refresh, or `1 / window.output_fps` when nothing was
    /// typed, clicked or scrolled in the last `window.input_grace_ms` (task-2481: output that
    /// streams changes every row of every frame, and a frame costs the same however little of it a
    /// person can read).
    pub fn paint_due(&self, now: Instant, cfg: &crate::config::Window) -> Option<Instant> {
        // The first frame after the show is the one a person sees first (P4); it never waits.
        if self.unpaced || (self.shown && !self.presented_shown) {
            return None;
        }
        let due = self.last_paint? + output_interval(self.frame_interval, self.last_input, now, cfg);
        (now < due).then_some(due)
    }

    /// Moves a redraw that came too soon to `due`, so it happens once at the display rate.
    pub fn defer_paint(&mut self, due: Instant) {
        self.wake_at = Some(self.wake_at.map_or(due, |w| w.min(due)));
    }

    /// Draws again later after a frame did not reach the screen: the surface was covered or
    /// minimised, timed out, or was lost while the window was being resized. Without this the
    /// window kept showing the last frame it presented, at the old size, until the program printed
    /// something (task-2468). The wait starts at `window.repaint_retry_ms` and doubles with each
    /// further miss up to `window.repaint_retry_max_ms`, so a covered window does not spin.
    fn schedule_repaint(&mut self, cfg: &crate::config::Window) {
        let doubled = cfg.repaint_retry_ms.saturating_mul(1u64 << self.repaint_misses.min(16));
        let wait = doubled.min(cfg.repaint_retry_max_ms).max(1);
        self.repaint_misses = self.repaint_misses.saturating_add(1);
        self.defer_paint(Instant::now() + std::time::Duration::from_millis(wait));
    }

    /// The banner bytes: the configuration error when there is one, otherwise the shader's error.
    fn banner_text(&self, app: &App) -> Option<Vec<u8>> {
        match (&app.config_error, self.shader.banner()) {
            (Some(msg), _) => Some(chrome::banner(msg)),
            (None, Some(msg)) => Some(chrome::shader_banner(msg)),
            (None, None) => None,
        }
    }

    /// Applies `window.shader` and `window.cursor_effect` after the configuration changed: the file is read and compiled again
    /// when its text changed. A new banner changes the layout.
    pub fn config_shader(&mut self, app: &mut App) {
        self.shader.configure(&app.config, &app.config_path);
        let gpu = crate::gui::paint::wants_gpu(
            app.config.window.renderer,
            &app.config.window.shader,
            &app.config.window.cursor_effect,
        );
        if gpu && !self.painter.is_gpu() && self.upgrade.is_none() {
            self.upgrade = Some(crate::gui::paint::upgrade_to_gpu(&self.window, self.display.clone()));
        }
        if self.shader.apply(&mut self.painter, false) {
            self.relayout(app);
        }
        self.window.request_redraw();
    }

    /// Reloads the shader file when its modified time changed (checked at most once every
    /// `window.shader_poll_ms`). Returns when to look again, for the event loop's wake up.
    pub fn shader_tick(&mut self, app: &mut App, now: Instant) -> Option<Instant> {
        let (changed, next) = self.shader.poll(&mut self.painter, now);
        if changed {
            self.relayout(app);
            self.window.request_redraw();
        }
        next
    }

    /// While a shader that reads the time runs, schedules the next frame at the display rate.
    fn keep_animating(&mut self) {
        if self.painter.shader_animates() {
            self.defer_paint(Instant::now() + self.frame_interval);
        }
    }

    /// Reads the display's refresh rate again (the window moved to another monitor).
    pub fn refresh_frame_interval(&mut self) {
        self.frame_interval = frame_interval(&self.window);
    }

    /// Finishes showing the window once a frame has been presented to it while visible: activates
    /// it when the show left that for later, then allows IME. Both start the text services on this
    /// thread, which costs tens of milliseconds, so neither runs before that frame. Calling it
    /// again does nothing.
    pub fn finish_show(&mut self) {
        if !self.shown || (self.pending_activation && !self.presented_shown) {
            return;
        }
        if self.pending_activation {
            self.pending_activation = false;
            crate::gui::show::activate(&self.window);
            crate::gui::phase("activated");
        }
        if !self.ime_allowed {
            self.ime_allowed = true;
            self.window.set_ime_allowed(true);
            crate::gui::phase("ime_allowed");
        }
    }

    /// Switches to the GPU renderer once the background thread has built it. A failure keeps the
    /// CPU renderer and says why.
    fn take_gpu(&mut self) {
        let Some(rx) = &self.upgrade else { return };
        match rx.try_recv() {
            Ok(Ok(mut gpu)) => {
                self.upgrade = None;
                let size = self.window.inner_size();
                termoil_render::Renderer::resize(&mut gpu, size.width, size.height);
                termoil_render::Renderer::set_hinting(&mut gpu, self.hinting);
                match crate::gui::paint::attach(&mut gpu, &self.window) {
                    Ok(()) => {
                        self.painter = Painter::Gpu(Box::new(gpu));
                        // A new renderer has no shader yet; a shader error reaches the banner on this frame.
                        self.shader.apply(&mut self.painter, true);
                    }
                    Err(e) => eprintln!("termoil: GPU renderer unavailable, staying on the CPU renderer: {e}"),
                }
            }
            Ok(Err(e)) => {
                eprintln!("termoil: GPU renderer unavailable, staying on the CPU renderer: {e}");
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
            step_animations(&mut t, &mut self.wake_at);
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
        if self.title_h > 0.0 {
            self.build_title(app, cols, &opts);
            at.title = Some((0.0, 0.0));
        }
        if self.tab_bar_h > 0.0 {
            self.build_tab_bar(app, wi, cols, &opts);
            at.tab = Some((0.0, self.title_h));
        }
        if let Some(text) = self.banner_text(app) {
            self.banner_strip.frame(
                cols,
                1,
                &text,
                &self.colors,
                &mut self.grid,
                &BuildOptions { padding: (0, 2), ..opts.clone() },
            );
            at.banner = Some((0.0, self.title_h + self.tab_bar_h));
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

    /// Builds the title strip: the window's title centered on the tab bar's color, drawn where the
    /// transparent system title bar is, so the macOS buttons sit on a solid bar (task-2590).
    fn build_title(&mut self, app: &App, cols: u16, opts: &BuildOptions) {
        let background = [self.colors.background.r, self.colors.background.g, self.colors.background.b];
        let bar = tab_bar::bar_color(background, &app.config.tab_bar);
        let bytes = chrome::title(&self.window.title(), cols, bar, &self.chrome);
        let (_, ch) = self.cell();
        let pad = ((self.title_h as u32).saturating_sub(ch) / 2, 0);
        let opts = BuildOptions { padding: (0, pad.0), ..opts.clone() };
        self.title_strip.frame(cols, 1, &bytes, &self.colors, &mut self.grid, &opts);
        let width = self.window.inner_size().width;
        self.title_strip.decorate(width, self.title_h as u32, Rgba::rgb(bar[0], bar[1], bar[2]), Vec::new());
    }

    /// Builds the tab bar: lays the tabs out, writes their titles into the strip, and puts the tabs,
    /// crosses and the new tab button on its frame as shapes.
    fn build_tab_bar(&mut self, app: &App, wi: usize, cols: u16, opts: &BuildOptions) {
        let w = &app.model.windows[wi];
        let titles: Vec<String> = w.tabs.iter().map(|t| tab_label(app, t)).collect();
        self.tab_layout = tab_bar::layout(titles.len(), cols, &app.config.tab_bar);
        let background = [self.colors.background.r, self.colors.background.g, self.colors.background.b];
        let bar = tab_bar::bar_color(background, &app.config.tab_bar);
        let bytes = tab_bar::text(&titles, w.active, &self.tab_layout, &self.chrome, bar);
        let opts = BuildOptions { padding: (0, self.tab_bar_padding(app)), ..opts.clone() };
        self.tab_strip.frame(cols, 1, &bytes, &self.colors, &mut self.grid, &opts);
        let width = self.window.inner_size().width;
        let look = tab_bar::Look {
            chrome: &self.chrome,
            background,
            cell: self.cell(),
            width: width as f32,
            height: self.tab_bar_h,
            scale: self.scale as f32,
            sizes: &app.config.tab_bar,
            focused: self.focused,
        };
        let shapes = tab_bar::shapes(&self.tab_layout, w.active, self.tab_hover, &look);
        self.tab_strip.decorate(width, self.tab_bar_h as u32, Rgba::rgb(bar[0], bar[1], bar[2]), shapes);
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
                Some((x.max(0.0), self.title_h + self.tab_bar_h + ch as f32 / 2.0))
            }
            Overlay::Hints(_) => None,
            Overlay::Themes(t) => {
                let width = cols.saturating_sub(4).clamp(10, 110);
                let (bytes, rows) = crate::gui::overlay::themes_content(t, width, &self.colors);
                self.overlay_strip.frame(width, rows, &bytes, &self.colors, &mut self.grid, &opts);
                let x = (size.width as f32 - (width as u32 * cw + 2 * opts.padding.0) as f32) / 2.0;
                Some((x.max(0.0), self.title_h + self.tab_bar_h + ch as f32 / 2.0))
            }
            Overlay::Settings(s) => {
                let width = cols.saturating_sub(4).clamp(10, crate::gui::settings_view::SETTINGS_COLS);
                let room = (size.height as f32 - self.title_h - self.tab_bar_h) / ch as f32;
                let max_rows = (room as u16).saturating_sub(2).max(8);
                let (bytes, rows) = crate::gui::settings_view::settings_content(s, width, max_rows, &self.colors);
                self.overlay_strip.frame(width, rows, &bytes, &self.colors, &mut self.grid, &opts);
                let x = (size.width as f32 - (width as u32 * cw + 2 * opts.padding.0) as f32) / 2.0;
                Some((x.max(0.0), self.title_h + self.tab_bar_h + ch as f32 / 2.0))
            }
            Overlay::Answer(a) => {
                let width = cols.saturating_sub(4).clamp(10, PALETTE_COLS);
                let (bytes, rows) = crate::gui::overlay::answer_content(a, width, &self.colors);
                self.overlay_strip.frame(width, rows, &bytes, &self.colors, &mut self.grid, &opts);
                let x = (size.width as f32 - (width as u32 * cw + 2 * opts.padding.0) as f32) / 2.0;
                Some((x.max(0.0), self.title_h + self.tab_bar_h + ch as f32 / 2.0))
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
    crate::view::tab_label(app, t)
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
    title: Option<(f32, f32)>,
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

/// Whether Termoil draws the title bar itself: on macOS, for a translucent window with decorations
/// that is not the quick terminal. A translucent NSWindow has a clear background, and the system
/// title bar over it showed the desktop through it (task-2590).
fn draws_own_title_bar(config: &crate::config::Config, translucent: bool, quick: bool) -> bool {
    cfg!(target_os = "macos")
        && translucent
        && config.window.decorations
        && !quick
        && config.window.macos_titlebar_height > 0
}

/// Window attributes for a title bar Termoil draws: on macOS the system title bar becomes
/// transparent with its title hidden, and the content runs under it, so the close, minimize and zoom
/// buttons sit on Termoil's own strip. Elsewhere the attributes are unchanged.
fn own_title_bar_attributes(attrs: winit::window::WindowAttributes, titled: bool) -> winit::window::WindowAttributes {
    #[cfg(target_os = "macos")]
    if titled {
        use winit::platform::macos::WindowAttributesExtMacOS;
        return attrs.with_titlebar_transparent(true).with_fullsize_content_view(true).with_title_hidden(true);
    }
    let _ = titled;
    attrs
}

/// Window attributes for a translucent window: transparent, and on Windows without the GDI
/// surface, which DirectComposition would otherwise show under the translucent pixels.
fn translucent_attributes(
    attrs: winit::window::WindowAttributes, translucent: bool,
) -> winit::window::WindowAttributes {
    if !translucent {
        return attrs;
    }
    #[cfg(windows)]
    let attrs = winit::platform::windows::WindowAttributesExtWindows::with_no_redirection_bitmap(attrs, true);
    attrs.with_transparent(true)
}

/// The time between frames: one display refresh while the person is typing or clicking (input in
/// the last `input_grace_ms`) or when `output_fps` is 0, otherwise the longer of that and one
/// `output_fps` frame.
fn output_interval(
    display: Duration, last_input: Option<Instant>, now: Instant, cfg: &crate::config::Window,
) -> Duration {
    let typing =
        last_input.is_some_and(|t| now.saturating_duration_since(t) < Duration::from_millis(cfg.input_grace_ms));
    if typing || cfg.output_fps == 0 {
        return display;
    }
    display.max(Duration::from_secs(1) / cfg.output_fps)
}

/// The refresh rate assumed when the monitor does not report one.
const DEFAULT_REFRESH_MILLIHERTZ: u32 = 60_000;

/// One refresh of the monitor the window is on.
fn frame_interval(window: &winit::window::Window) -> std::time::Duration {
    let mhz = window
        .current_monitor()
        .and_then(|m| m.refresh_rate_millihertz())
        .filter(|&r| r > 0)
        .unwrap_or(DEFAULT_REFRESH_MILLIHERTZ);
    std::time::Duration::from_nanos(1_000_000_000_000 / mhz as u64)
}

/// The clock kitty graphics animations run on: milliseconds since the first call.
fn animation_clock() -> (Instant, u64) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    (start, start.elapsed().as_millis() as u64)
}

/// Moves the pane's running kitty graphics animations to now, so the frame being built shows
/// their current frames, and adds the next frame change to `wake_at`.
fn step_animations(t: &mut termoil_vt::Terminal, wake_at: &mut Option<Instant>) {
    let Some(due) = t.graphics().next_animation_due() else { return };
    let (start, now) = animation_clock();
    if due <= now {
        t.graphics_mut().advance_animations(now);
    }
    if let Some(next) = t.graphics().next_animation_due() {
        let at = start + std::time::Duration::from_millis(next.max(now + 1));
        *wake_at = Some(wake_at.map_or(at, |w| w.min(at)));
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
    let Ok(re) = termoil_vt::search::compile(&search.pattern, search.case_sensitive) else { return Vec::new() };
    let pages = &t.screen().pages;
    let top = pages.active_y(0).saturating_sub(p.scroll);
    let rows = t.rows() as usize;
    let cols = t.cols();
    let mut out = Vec::new();
    for m in termoil_vt::search::find_all(pages, &re, termoil_vt::text::line_start(pages, top)..top + rows) {
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
