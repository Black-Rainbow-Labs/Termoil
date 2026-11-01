//! The winit front end (`docs/TDD.md` section 4.8): one event loop, many windows. Sessions wake the
//! loop through user events; IPC requests and configuration reloads arrive the same way. Every
//! action, from a key, the mouse, IPC or the CLI, goes through `App::run_action`, and the effects
//! it queues are applied here.

pub mod chrome;
pub mod handle;
pub mod input;
pub mod look;
pub mod paint;
pub mod window;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crossbeam_channel::Sender;
use terminus_protocol::ActionCall;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Fullscreen, WindowId};

use crate::app::{App, Effect, Outcome, WakerFactory};
use crate::config::{Config, DEFAULT_DEBOUNCE, LoadResult, Platform, Watcher};
use crate::events::Notice;
use crate::ipc_server::{ServeError, bridge, start};
use crate::keymap::{Bound, resolve};
use crate::model::{PaneId, WinId};
use window::WinState;

/// When the process started, for the startup probe (P4).
static STARTED: OnceLock<Instant> = OnceLock::new();

/// Records the process start; `main` calls it first.
pub fn mark_start() {
    let _ = STARTED.set(Instant::now());
}

/// Events from other threads.
pub enum UserEvent {
    /// A pane has output or events; the flag is cleared when handled.
    Wake(PaneId, Arc<AtomicBool>),
    /// An IPC request and where to send its outcome.
    Ipc(ActionCall, Sender<Outcome>),
    /// The configuration file changed.
    Config(Box<LoadResult>),
}

/// The front end's state.
pub struct Gui {
    /// The application core.
    pub app: App,
    /// Windows by model id.
    pub windows: HashMap<WinId, WinState>,
    by_winit: HashMap<WindowId, WinId>,
    /// The resolved keybindings.
    pub bound: Vec<Bound>,
    initial: Option<(Option<String>, Option<String>)>,
    os_dark: bool,
    _watcher: Option<Watcher>,
    probe_done: bool,
}

/// Wakers that send one user event per burst of output.
fn proxy_wakers(proxy: EventLoopProxy<UserEvent>) -> WakerFactory {
    Box::new(move |id| {
        let proxy = proxy.clone();
        let pending = Arc::new(AtomicBool::new(false));
        Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
                let _ = proxy.send_event(UserEvent::Wake(id, pending.clone()));
            }
        })
    })
}

/// Runs the window front end until the last window closes. When another instance already serves
/// the socket, asks it to open a window instead and returns.
pub fn run(
    config: Config, config_path: PathBuf, socket: &str, cwd: Option<String>, command: Option<String>,
) -> Result<(), String> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().map_err(|e| format!("no event loop: {e}"))?;
    match Gui::new(&event_loop, config, config_path, socket, cwd, command)? {
        Some(mut gui) => event_loop.run_app(&mut gui).map_err(|e| format!("event loop: {e}")),
        None => Ok(()),
    }
}

impl Gui {
    /// Starts the IPC server and the config watcher and builds the front end for `event_loop`.
    /// Returns None when another instance serves the socket and took the window instead.
    pub fn new(
        event_loop: &EventLoop<UserEvent>, config: Config, config_path: PathBuf, socket: &str, cwd: Option<String>,
        command: Option<String>,
    ) -> Result<Option<Gui>, String> {
        let proxy = event_loop.create_proxy();
        let ipc_proxy = proxy.clone();
        match start(socket, bridge(move |call, reply| ipc_proxy.send_event(UserEvent::Ipc(call, reply)).is_ok())) {
            Ok(()) => {}
            Err(ServeError::AlreadyRunning) => return hand_to_running(cwd, command).map(|_| None),
            Err(ServeError::Io(e)) => eprintln!("terminus: IPC is off: {e}"),
        }
        let cfg_proxy = proxy.clone();
        let watcher = Watcher::start(config_path.clone(), DEFAULT_DEBOUNCE, move |r| {
            let _ = cfg_proxy.send_event(UserEvent::Config(Box::new(r)));
        })
        .ok();
        let bound = resolve(&config.resolved_keybindings(Platform::current()));
        let app = App::new(config, config_path, Box::new(crate::platform::SystemClipboard), proxy_wakers(proxy));
        Ok(Some(Gui {
            app,
            windows: HashMap::new(),
            by_winit: HashMap::new(),
            bound,
            initial: Some((cwd, command)),
            os_dark: true,
            _watcher: watcher,
            probe_done: false,
        }))
    }

    /// The resolved keybindings, for the E6 tests that press keys.
    pub fn bindings(&self) -> &[Bound] {
        &self.bound
    }

    /// Applies queued effects; the E6 harness calls it after acting on the app directly.
    pub fn flush(&mut self, el: &ActiveEventLoop) {
        self.apply_effects(el);
    }
}

/// Opens the window in the instance that is already running.
fn hand_to_running(cwd: Option<String>, command: Option<String>) -> Result<(), String> {
    let mut args = serde_json::Map::new();
    if let Some(c) = cwd {
        args.insert("cwd".into(), c.into());
    }
    if let Some(c) = command {
        args.insert("command".into(), c.into());
    }
    let mut client = terminus_protocol::Client::connect(std::time::Duration::from_secs(5))
        .map_err(|e| format!("another Terminus is running but did not answer: {e}"))?;
    let r = client.call("new-window", args).map_err(|e| e.to_string())?;
    if r.ok { Ok(()) } else { Err(r.error.unwrap_or_default()) }
}

impl Gui {
    /// Applies the effects the app queued.
    fn apply_effects(&mut self, el: &ActiveEventLoop) {
        loop {
            let effects = self.app.take_effects();
            if effects.is_empty() {
                return;
            }
            for e in effects {
                self.apply(el, e);
            }
        }
    }

    /// Applies one effect.
    fn apply(&mut self, el: &ActiveEventLoop, e: Effect) {
        match e {
            Effect::OpenWindow(id) => self.open_window(el, id),
            Effect::CloseWindow(id) => {
                if let Some(ws) = self.windows.remove(&id) {
                    self.by_winit.remove(&ws.window.id());
                }
            }
            Effect::Redraw(id) => self.with_window(id, |ws, _| ws.window.request_redraw()),
            Effect::Relayout(id) => self.relayout(id),
            Effect::SetClipboard(t) => self.app.clipboard.set(&t),
            Effect::ToggleFullscreen(id) => self.with_window(id, |ws, _| {
                let full = ws.window.fullscreen().is_some();
                ws.window.set_fullscreen(if full { None } else { Some(Fullscreen::Borderless(None)) });
            }),
            Effect::FontSize(id, delta) => self.font_size(id, delta),
            Effect::ResizeWindow(id, _, cols, rows) => self.with_window(id, |ws, _| {
                let (cw, ch) = ws.cell();
                let w = cols as u32 * cw + 2 * ws.opts.padding.0;
                let h = rows as u32 * ch + 2 * ws.opts.padding.1 + ws.tab_bar_h as u32;
                let _ = ws.window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
            }),
            Effect::OpenPath(p) => crate::platform::open(&p.to_string_lossy()),
            Effect::ConfigChanged => self.config_changed(),
            Effect::LastWindowClosed | Effect::Quit => {
                for id in self.app.panes.keys().copied().collect::<Vec<_>>() {
                    self.app.close_pane(id);
                }
                el.exit();
            }
            Effect::ShowPalette(_) | Effect::ShowSearch(_) | Effect::ToggleQuickTerminal => {
                eprintln!("terminus: the command palette, search bar and quick terminal arrive at M4")
            }
        }
    }

    /// Runs `f` on a window when it exists.
    fn with_window(&mut self, id: WinId, f: impl FnOnce(&mut WinState, &mut App)) {
        if let Some(ws) = self.windows.get_mut(&id) {
            f(ws, &mut self.app);
        }
    }

    /// Creates the window for a model window.
    fn open_window(&mut self, el: &ActiveEventLoop, id: WinId) {
        match WinState::create(el, &self.app, id, self.os_dark) {
            Ok(ws) => {
                self.os_dark = ws.window.theme().is_none_or(|t| t == winit::window::Theme::Dark);
                self.by_winit.insert(ws.window.id(), id);
                self.windows.insert(id, ws);
                self.relayout(id);
                // Windows sends no paint to a hidden window, so the first frame is drawn here and
                // the window is shown by it.
                if let Some(ws) = self.windows.get_mut(&id)
                    && let Err(e) = ws.draw(&mut self.app)
                {
                    eprintln!("terminus: draw failed: {e}");
                }
            }
            Err(e) => {
                eprintln!("terminus: {e}");
                let panes: Vec<PaneId> = self
                    .app
                    .model
                    .windows
                    .iter()
                    .filter(|w| w.id == id)
                    .flat_map(|w| w.tabs.iter().flat_map(|t| t.root.panes()))
                    .collect();
                for p in panes {
                    self.app.close_pane(p);
                }
            }
        }
    }

    /// Lays a window out again, sets its title, and gives new panes the theme's palette.
    fn relayout(&mut self, id: WinId) {
        let palette = look::palette(&self.app.config, self.os_dark);
        let Some(ws) = self.windows.get_mut(&id) else { return };
        let before: Vec<PaneId> = ws.views.keys().copied().collect();
        ws.relayout(&mut self.app);
        for (pane, _) in &ws.layout {
            if !before.contains(pane)
                && let Some(p) = self.app.panes.get(pane)
            {
                p.session.lock().set_palette(palette);
            }
        }
        update_title(ws, &self.app);
    }

    /// Changes a window's font size by points, or resets it.
    fn font_size(&mut self, id: WinId, delta: Option<f32>) {
        let base = self.app.config.font.size as f32;
        self.with_window(id, |ws, app| {
            ws.points = delta.map(|d| (ws.points + d).clamp(6.0, 72.0)).unwrap_or(base);
            ws.refont(app);
        });
        self.relayout(id);
    }

    /// Applies a changed configuration to every window and pane.
    fn config_changed(&mut self) {
        self.bound = resolve(&self.app.config.resolved_keybindings(Platform::current()));
        let palette = look::palette(&self.app.config, self.os_dark);
        for p in self.app.panes.values() {
            p.session.lock().set_palette(palette);
        }
        let ids: Vec<WinId> = self.windows.keys().copied().collect();
        for id in ids {
            let colors = look::colors(&self.app.config, self.os_dark);
            self.with_window(id, |ws, app| {
                ws.colors = colors.clone();
                for v in ws.views.values_mut() {
                    v.state.defaults = colors.clone();
                    v.state.needs_full = true;
                }
                ws.refont(app);
            });
            self.relayout(id);
        }
    }

    /// Handles a reloaded configuration file.
    fn config_loaded(&mut self, r: LoadResult) {
        match r {
            Ok((config, _warnings)) => {
                self.app.config = config;
                self.app.config_error = None;
            }
            Err(e) => self.app.config_error = Some(format!("line {}, column {}: {}", e.line, e.column, e.message)),
        }
        self.app.effects.push(Effect::ConfigChanged);
    }

    /// Acts on the notices a pane's events produced.
    fn notices(&mut self, notices: Vec<Notice>) {
        for n in notices {
            if let Notice::Bell(pane) = n
                && let Some(ws) = self.app.window_of(pane).and_then(|w| self.windows.get(&w))
                && !ws.focused
            {
                ws.window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
            }
        }
    }

    /// Writes the startup probe (P4) after the first frame that shows the shell's output, when
    /// `TERMINUS_STARTUP_PROBE` names a file.
    fn startup_probe(&mut self, el: &ActiveEventLoop, id: WinId) {
        if self.probe_done {
            return;
        }
        let Ok(path) = std::env::var("TERMINUS_STARTUP_PROBE") else {
            self.probe_done = true;
            return;
        };
        let Some(ws) = self.windows.get(&id) else { return };
        let shown = ws
            .views
            .values()
            .any(|v| v.state.lines.iter().any(|l| l.cells.iter().any(|c| c.text.first().is_some_and(|ch| ch != ' '))));
        if !shown {
            return;
        }
        self.probe_done = true;
        let ms = STARTED.get().map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0);
        let _ =
            std::fs::write(&path, format!("{{\"first_frame_ms\":{ms:.2},\"renderer\":\"{}\"}}\n", ws.painter.name()));
        if std::env::var("TERMINUS_STARTUP_PROBE_EXIT").is_ok_and(|v| v == "1") {
            self.app.effects.push(Effect::Quit);
            self.apply_effects(el);
        }
    }
}

/// Sets the window title to the active tab's title.
fn update_title(ws: &WinState, app: &App) {
    let Some(wi) = app.model.window_index(ws.id) else { return };
    let t = crate::view::tab_title(app, app.model.windows[wi].tab());
    let title = if t.is_empty() { "Terminus".to_string() } else { t };
    if ws.window.title() != title {
        ws.window.set_title(&title);
    }
}

impl ApplicationHandler<UserEvent> for Gui {
    /// Opens the first window.
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if let Some((cwd, command)) = self.initial.take() {
            if let Err(e) = crate::actions::new_window(&mut self.app, cwd.map(PathBuf::from), command.as_deref()) {
                eprintln!("terminus: {e}");
                el.exit();
                return;
            }
            self.apply_effects(el);
        }
    }

    /// Session wakes, IPC requests and configuration reloads.
    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wake(pane, flag) => {
                flag.store(false, Ordering::Release);
                let notices = crate::events::pump(&mut self.app, pane, self.os_dark);
                self.notices(notices);
                if let Some(w) = self.app.window_of(pane)
                    && let Some(ws) = self.windows.get(&w)
                {
                    update_title(ws, &self.app);
                }
            }
            UserEvent::Ipc(call, reply) => {
                let _ = reply.send(self.app.run_action(&call));
            }
            UserEvent::Config(r) => self.config_loaded(*r),
        }
        self.apply_effects(el);
    }

    /// Window events: input, resize, redraw, close.
    fn window_event(&mut self, el: &ActiveEventLoop, wid: WindowId, ev: WindowEvent) {
        let Some(&id) = self.by_winit.get(&wid) else { return };
        let Some(ws) = self.windows.get_mut(&id) else { return };
        let app = &mut self.app;
        match ev {
            WindowEvent::CloseRequested => {
                let panes: Vec<PaneId> = app
                    .model
                    .windows
                    .iter()
                    .filter(|w| w.id == id)
                    .flat_map(|w| w.tabs.iter().flat_map(|t| t.root.panes()))
                    .collect();
                for p in panes {
                    app.close_pane(p);
                }
            }
            WindowEvent::Resized(size) => {
                ws.painter.resize(size.width, size.height);
                ws.relayout(app);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                ws.scale = scale_factor;
                ws.refont(app);
                ws.relayout(app);
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = ws.draw(app) {
                    eprintln!("terminus: draw failed: {e}");
                }
                self.startup_probe(el, id);
            }
            WindowEvent::ModifiersChanged(m) => ws.mods = m.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if let Some(raw) = input::raw_key(&event, ws.mods) {
                    handle::raw_key(ws, app, &self.bound, &raw);
                }
            }
            WindowEvent::Ime(ime) => handle::ime(ws, app, ime),
            WindowEvent::Focused(f) => {
                if f {
                    app.model.focused = Some(id);
                }
                handle::focus(ws, app, f);
            }
            WindowEvent::CursorMoved { position, .. } => handle::cursor_moved(ws, app, position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => handle::button(ws, app, state, button),
            WindowEvent::MouseWheel { delta, .. } => handle::wheel(ws, app, delta),
            WindowEvent::ThemeChanged(t) => {
                self.os_dark = t == winit::window::Theme::Dark;
                app.effects.push(Effect::ConfigChanged);
            }
            _ => {}
        }
        self.apply_effects(el);
    }

    /// Wakes windows waiting on synchronized output.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for ws in self.windows.values_mut() {
            if let Some(t) = ws.wake_at {
                if t <= now {
                    ws.wake_at = None;
                    ws.window.request_redraw();
                } else {
                    next = Some(next.map_or(t, |n| n.min(t)));
                }
            }
        }
        el.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}
