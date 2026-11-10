//! The winit front end (`docs/TDD.md` section 4.8): one event loop, many windows. Sessions wake the
//! loop through user events; IPC requests and configuration reloads arrive the same way. Every
//! action, from a key, the mouse, IPC or the CLI, goes through `App::run_action`, and the effects
//! it queues are applied here.

pub mod access;
mod backdrop;
pub mod bench;
pub mod chrome;
pub mod handle;
pub mod input;
pub mod look;
pub mod overlay;
pub mod paint;
pub mod quick;
pub mod settings_view;
pub mod shader;
mod show;
mod showtrace;
pub mod tab_bar;
mod taskbar;
mod warm;
pub mod window;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crossbeam_channel::Sender;
use termoil_protocol::ActionCall;
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

/// Startup phases and when each ended, in milliseconds since the start, for the probe.
static PHASES: std::sync::Mutex<Vec<(&'static str, f64)>> = std::sync::Mutex::new(Vec::new());

/// Records that a startup phase ended.
pub fn phase(name: &'static str) {
    if std::env::var("TERMOIL_TRACE_STARTUP").is_ok() {
        eprintln!("{:8.2} {name}", since_start());
    }
    if let (Some(s), Ok(mut p)) = (STARTED.get(), PHASES.lock()) {
        p.push((name, s.elapsed().as_secs_f64() * 1000.0));
    }
}

/// Milliseconds since the process started.
pub(crate) fn since_start() -> f64 {
    STARTED.get().map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0)
}

/// Waits until the compositor has put the frames presented so far on the screen (`DwmFlush` on
/// Windows). Elsewhere it returns at once and the present is the end point.
fn wait_composited() {
    #[cfg(windows)]
    // SAFETY: DwmFlush takes no arguments and only blocks until the next composition.
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmFlush();
    }
}

/// The name of a window event, for the startup phases.
fn event_name(ev: &WindowEvent) -> &'static str {
    match ev {
        WindowEvent::Resized(_) => "ev_resized",
        WindowEvent::Moved(_) => "ev_moved",
        WindowEvent::Focused(_) => "ev_focused",
        WindowEvent::RedrawRequested => "ev_redraw",
        WindowEvent::ModifiersChanged(_) => "ev_modifiers",
        WindowEvent::Ime(_) => "ev_ime",
        WindowEvent::ThemeChanged(_) => "ev_theme",
        WindowEvent::ScaleFactorChanged { .. } => "ev_scale",
        WindowEvent::Occluded(_) => "ev_occluded",
        WindowEvent::CursorEntered { .. } => "ev_cursor_entered",
        WindowEvent::CursorMoved { .. } => "ev_cursor_moved",
        _ => "ev_other",
    }
}

/// Events from other threads.
pub enum UserEvent {
    /// A pane has output or events; the flag is cleared when handled.
    Wake(PaneId, Arc<AtomicBool>),
    /// An IPC request and where to send its outcome.
    Ipc(ActionCall, Sender<Outcome>),
    /// The configuration file changed.
    Config(Box<LoadResult>),
    /// The quick terminal's global hotkey was pressed.
    QuickTerminal,
    /// A screen reader asked for the accessibility tree or for an action on a node.
    Access(accesskit_winit::Event),
    /// An update job wants a notification shown, or a checked update installed.
    Update(crate::update::actions::Event),
    /// A job a person started from the window finished; its answer is shown (`crate::answer`).
    Answer(crate::answer::Answer),
}

impl From<accesskit_winit::Event> for UserEvent {
    /// Wraps an AccessKit event so the adapters can send it through the event loop proxy.
    fn from(ev: accesskit_winit::Event) -> UserEvent {
        UserEvent::Access(ev)
    }
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
    /// Wakes from panes whose window has a frame already scheduled: their flags stay set, so the
    /// session sends no more wakes, until that frame is drawn (task-2481).
    held_wakes: Vec<(PaneId, Arc<AtomicBool>)>,
    /// When the first frame with the shell's output was painted, for the probe.
    painted_ms: Option<f64>,
    /// The latency benchmark, when running one.
    pub bench: Option<bench::LatencyBench>,
    /// The quick terminal's window, once created.
    pub quick: Option<WinId>,
    /// A window being opened as the quick terminal.
    pending_quick: Option<WinId>,
    /// Sends events to the loop, for the accessibility adapters.
    proxy: EventLoopProxy<UserEvent>,
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
    bench: Option<bench::LatencyBench>,
) -> Result<(), String> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().map_err(|e| format!("no event loop: {e}"))?;
    phase("event_loop");
    warm::start(&config);
    let (cwd_given, command_given) = (cwd.is_some(), command.is_some());
    match Gui::new(&event_loop, config, config_path, socket, cwd, command)? {
        Some(mut gui) => {
            let env = crate::telemetry::environment();
            let permission = crate::telemetry::Permission::for_the_binary(false, bench.is_some(), env.as_deref());
            let launch = crate::telemetry::launch_kind(cwd_given, command_given);
            crate::telemetry::start(&gui.app.config, permission, launch);
            gui.bench = bench;
            event_loop.run_app(&mut gui).map_err(|e| format!("event loop: {e}"))
        }
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
            Err(ServeError::Io(e)) => eprintln!("termoil: IPC is off: {e}"),
        }
        let cfg_proxy = proxy.clone();
        let watcher = Watcher::start(config_path.clone(), DEFAULT_DEBOUNCE, move |r| {
            let _ = cfg_proxy.send_event(UserEvent::Config(Box::new(r)));
        })
        .ok();
        let bound = resolve(&config.resolved_keybindings(Platform::current()));
        if !crate::app::instance_mode().separate
            && let Ok(chord) = crate::keymap::parse_chord(&config.window.quick_terminal_hotkey)
            && let Err(e) = quick::listen(&chord, proxy.clone())
        {
            eprintln!("termoil: no quick terminal hotkey: {e}");
        }
        let mut app =
            App::new(config, config_path, Box::new(crate::platform::SystemClipboard), proxy_wakers(proxy.clone()));
        let update_proxy = proxy.clone();
        app.updates.sink = Some(Arc::new(move |ev| {
            let _ = update_proxy.send_event(UserEvent::Update(ev));
        }));
        let answer_proxy = proxy.clone();
        app.answers = Some(Arc::new(move |answer| {
            let _ = answer_proxy.send_event(UserEvent::Answer(answer));
        }));
        Ok(Some(Gui {
            app,
            windows: HashMap::new(),
            by_winit: HashMap::new(),
            bound,
            initial: Some((cwd, command)),
            os_dark: true,
            _watcher: watcher,
            probe_done: false,
            held_wakes: Vec::new(),
            painted_ms: None,
            bench: None,
            quick: None,
            pending_quick: None,
            proxy,
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

/// Reopens the session the update helper named, once, and deletes its file. Answers whether it opened
/// any window.
fn restore_after_update(app: &mut App) -> bool {
    let Some(path) = crate::update::apply::pending_restore() else { return false };
    let opened = crate::restore::restore_from(app, &path);
    let _ = std::fs::remove_file(&path);
    opened
}

impl Gui {
    /// Carries out what an update job asked: a notification, or saving the session, starting the helper
    /// and quitting.
    fn update_event(&mut self, ev: crate::update::actions::Event) {
        use crate::update::actions::Event;
        match ev {
            Event::Notify { title, body } => crate::platform::notify(&title, &body),
            Event::Restart(prepared, target) => match crate::update::actions::restart(&self.app, &prepared, &target) {
                Ok(()) => self.app.effects.push(Effect::Quit),
                Err(e) => crate::platform::notify("Termoil could not install the update", &e),
            },
        }
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
    let mut client = termoil_protocol::Client::connect(std::time::Duration::from_secs(5))
        .map_err(|e| format!("another Termoil is running but did not answer: {e}"))?;
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
                if self.quick == Some(id) {
                    self.quick = None;
                }
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
                let h = rows as u32 * ch + 2 * ws.opts.padding.1 + ws.tab_bar_h as u32 + ws.title_h as u32;
                let _ = ws.window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
            }),
            Effect::OpenPath(p) => crate::platform::open(&p.to_string_lossy()),
            Effect::ConfigChanged => {
                crate::telemetry::configure(&self.app.config);
                self.config_changed()
            }
            Effect::LastWindowClosed | Effect::Quit => {
                crate::restore::save_on_quit(&self.app, self.quick);
                for id in self.app.panes.keys().copied().collect::<Vec<_>>() {
                    self.app.close_pane(id);
                }
                el.exit();
            }
            Effect::ShowPalette(id) => {
                let entries = crate::palette::entries(&self.app.config.resolved_keybindings(Platform::current()));
                self.with_window(id, |ws, _| {
                    ws.overlay = Some(overlay::Overlay::Palette(overlay::PaletteUi::new(entries)));
                    ws.window.request_redraw();
                });
            }
            Effect::ShowSearch(id) => self.with_window(id, |ws, app| {
                if let Some(pane) = handle::focus_of(ws, app) {
                    let query = app
                        .panes
                        .get(&pane)
                        .and_then(|p| p.search.as_ref())
                        .map(|s| s.pattern.clone())
                        .unwrap_or_default();
                    ws.overlay = Some(overlay::Overlay::Search(overlay::SearchUi {
                        query,
                        case_sensitive: false,
                        pane,
                        found: None,
                        error: None,
                    }));
                    ws.window.request_redraw();
                }
            }),
            Effect::ToggleQuickTerminal => self.toggle_quick(el),
            Effect::ShowThemes(id) => {
                let original = self.app.config.theme.name.clone();
                let shader = self.app.config.window.shader.clone();
                self.with_window(id, |ws, _| {
                    ws.overlay = Some(overlay::Overlay::Themes(overlay::ThemesUi::new(original, shader)));
                    ws.window.request_redraw();
                });
            }
            Effect::ShowSettings(id) => {
                let ui = overlay::Overlay::Settings(settings_view::SettingsUi::new(&self.app.config));
                self.with_window(id, |ws, _| {
                    ws.overlay = Some(ui);
                    ws.window.request_redraw();
                });
            }
            Effect::ShowHints(id, pane) => self.with_window(id, |ws, app| {
                let Some(p) = app.panes.get(&pane) else { return };
                let links = {
                    let t = p.session.lock();
                    let top = t.screen().pages.active_y(0).saturating_sub(p.scroll);
                    crate::links::visible_links(&t, top, t.rows() as usize)
                };
                if links.is_empty() {
                    return;
                }
                let labels = crate::links::hint_labels(links.len());
                let items = labels.into_iter().zip(links).map(|(l, (link, y, c))| (l, link, y, c)).collect();
                ws.overlay = Some(overlay::Overlay::Hints(overlay::HintsUi { pane, items, typed: String::new() }));
                ws.window.request_redraw();
            }),
            Effect::ShowAnswer(answer) => self.show_answer(answer),
        }
    }

    /// Shows an answer in the window the action was run in, or the focused window when that one has
    /// closed: at once when no other overlay is open, otherwise when that overlay closes. With no window
    /// open it becomes a desktop notification.
    fn show_answer(&mut self, answer: crate::answer::Answer) {
        let target = answer
            .window
            .filter(|w| self.windows.contains_key(w))
            .or_else(|| self.windows.iter().find(|(_, ws)| ws.focused).map(|(id, _)| *id))
            .or_else(|| self.windows.keys().next().copied());
        let Some(ws) = target.and_then(|id| self.windows.get_mut(&id)) else {
            return crate::platform::notify(
                &answer.title,
                &answer.lines.join(
                    "
",
                ),
            );
        };
        match ws.overlay {
            None | Some(overlay::Overlay::Answer(_)) => {
                ws.overlay = Some(overlay::Overlay::Answer(overlay::AnswerUi::new(answer)))
            }
            Some(_) => ws.answer_waiting = Some(answer),
        }
        ws.window.request_redraw();
    }

    /// Runs `f` on a window when it exists.
    fn with_window(&mut self, id: WinId, f: impl FnOnce(&mut WinState, &mut App)) {
        if let Some(ws) = self.windows.get_mut(&id) {
            f(ws, &mut self.app);
        }
    }

    /// Creates the window for a model window.
    fn open_window(&mut self, el: &ActiveEventLoop, id: WinId) {
        let edge = (self.pending_quick == Some(id)).then_some(self.app.config.window.quick_terminal_edge);
        if edge.is_some() {
            self.pending_quick = None;
            self.quick = Some(id);
        }
        match WinState::create(el, &self.app, id, self.os_dark, edge, &self.proxy) {
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
                    eprintln!("termoil: draw failed: {e}");
                }
                self.startup_probe(el, id);
                if let Some(ws) = self.windows.get_mut(&id) {
                    ws.finish_show();
                }
            }
            Err(e) => {
                eprintln!("termoil: {e}");
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

    /// Shows the quick terminal, creating it the first time, or hides it when it is showing.
    fn toggle_quick(&mut self, el: &ActiveEventLoop) {
        if let Some(id) = self.quick {
            match self.windows.get(&id) {
                // Hiding closes the window and keeps its sessions: hiding a window is not
                // supported on Wayland, and closing works the same everywhere.
                Some(ws) if ws.focused || !cfg!(windows) => {
                    if let Some(ws) = self.windows.remove(&id) {
                        self.by_winit.remove(&ws.window.id());
                    }
                }
                Some(ws) => {
                    ws.window.focus_window();
                    self.app.model.focused = Some(id);
                }
                None if self.app.model.window_index(id).is_some() => {
                    self.pending_quick = Some(id);
                    self.open_window(el, id);
                    self.app.model.focused = Some(id);
                }
                None => {
                    self.quick = None;
                    self.toggle_quick(el);
                }
            }
            return;
        }
        match crate::actions::new_window(&mut self.app, None, None) {
            Ok(v) => self.pending_quick = v["window"].as_u64(),
            Err(e) => eprintln!("termoil: the quick terminal could not start: {e}"),
        }
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
            let chrome = look::chrome(&self.app.config, self.os_dark);
            self.with_window(id, |ws, app| {
                ws.colors = colors.clone();
                ws.chrome = chrome;
                for v in ws.views.values_mut() {
                    v.state.defaults = colors.clone();
                    v.state.needs_full = true;
                }
                ws.refont(app);
                ws.config_shader(app);
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
            match n {
                Notice::Bell(pane) => self.bell(pane),
                Notice::Notification { title, body } => {
                    if !self.any_focused() {
                        crate::platform::notify(if title.is_empty() { "Termoil" } else { &title }, &body);
                    }
                }
                Notice::CommandFinished { pane, code, seconds } => self.command_finished(pane, code, seconds),
                Notice::Progress(pane) => self.progress_changed(pane),
            }
        }
    }

    /// Answers a screen reader: sends the whole tree when it first asks for it, and moves the focus
    /// to a pane when it asks to focus that pane's node.
    fn access_event(&mut self, ev: accesskit_winit::Event) {
        let Some(&id) = self.by_winit.get(&ev.window_id) else { return };
        match ev.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                self.with_window(id, |ws, app| ws.push_access(app, true))
            }
            accesskit_winit::WindowEvent::ActionRequested(req) if req.action == accesskit::Action::Focus => {
                let pane = access::pane_of(req.target_node);
                if crate::actions::focus_pane(&mut self.app, pane).is_ok() {
                    self.with_window(id, |ws, _| ws.window.request_redraw());
                }
            }
            _ => {}
        }
    }

    /// Shows a pane's progress on its window's taskbar button when it is the focused pane, and
    /// redraws the tab bar, whose label carries the progress too.
    fn progress_changed(&mut self, pane: PaneId) {
        let Some(win) = self.app.window_of(pane) else { return };
        let Some(wi) = self.app.model.window_index(win) else { return };
        let focus = self.app.model.windows[wi].tab().focus;
        let progress = self.app.panes.get(&focus).and_then(|p| p.progress);
        if let Some(ws) = self.windows.get(&win) {
            if focus == pane {
                taskbar::show(&ws.window, progress);
            }
            ws.window.request_redraw();
        }
    }

    /// True when any Termoil window has the keyboard focus.
    fn any_focused(&self) -> bool {
        self.windows.values().any(|w| w.focused)
    }

    /// The bell: a flash of the window (and the taskbar when unfocused), a sound, or nothing, as
    /// `notify.on_bell` says.
    fn bell(&mut self, pane: PaneId) {
        let mode = self.app.config.notify.on_bell;
        let Some(ws) = self.app.window_of(pane).and_then(|w| self.windows.get_mut(&w)) else { return };
        match mode {
            crate::config::BellMode::Visual => {
                if !ws.focused {
                    ws.window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
                }
                ws.bell_until = Some(Instant::now() + std::time::Duration::from_millis(120));
                ws.window.request_redraw();
            }
            crate::config::BellMode::Audible => crate::platform::beep(),
            crate::config::BellMode::None => {}
        }
    }

    /// Notifies that a command finished, when no Termoil window has focus and it ran at least
    /// `notify.after_seconds`.
    fn command_finished(&mut self, pane: PaneId, code: Option<i32>, seconds: u64) {
        let n = &self.app.config.notify;
        if !n.command_finished || seconds < n.after_seconds || self.any_focused() {
            return;
        }
        let title = self
            .app
            .window_of(pane)
            .and_then(|w| self.app.model.window_index(w))
            .map(|wi| crate::view::tab_title(&self.app, self.app.model.windows[wi].tab()))
            .unwrap_or_default();
        let result = match code {
            Some(0) => "succeeded".to_string(),
            Some(c) => format!("failed with exit code {c}"),
            None => "finished".to_string(),
        };
        crate::platform::notify("Command finished", &format!("{title}: {result} after {seconds} s"));
    }

    /// Injects the next benchmark key when it is due, writes the report when every key is measured,
    /// and returns when the benchmark next needs to wake.
    fn bench_step(&mut self, el: &ActiveEventLoop, now: Instant) -> Option<Instant> {
        let b = self.bench.as_mut()?;
        if b.done() {
            let name = self.windows.values().next().map(|w| w.painter.name()).unwrap_or_default();
            let report = b.report(&name);
            let _ = std::fs::write(&b.out, &report);
            print!("{report}");
            self.bench = None;
            self.app.effects.push(Effect::Quit);
            self.apply_effects(el);
            return None;
        }
        if let Some(raw) = b.due(now)
            && let Some(ws) = self.windows.values_mut().next()
        {
            handle::raw_key(ws, &mut self.app, &self.bound, &raw);
        }
        self.bench.as_ref().and_then(|b| b.next_at())
    }

    /// Applies a pane's events and updates its window's title.
    fn pump_pane(&mut self, pane: PaneId) {
        let notices = crate::events::pump(&mut self.app, pane, self.os_dark);
        self.notices(notices);
        if let Some(w) = self.app.window_of(pane)
            && let Some(ws) = self.windows.get(&w)
        {
            update_title(ws, &self.app);
        }
    }

    /// Whether the window holding `pane` already has a frame scheduled, which will show whatever
    /// output arrives before it.
    fn paint_scheduled(&self, pane: PaneId) -> bool {
        let now = Instant::now();
        let ws = self.app.window_of(pane).and_then(|w| self.windows.get(&w));
        ws.is_some_and(|ws| ws.paint_held_until.is_some_and(|t| t > now))
    }

    /// Lets the panes whose scheduled frame has been drawn (or is overdue) wake the loop again,
    /// and applies the events they queued meanwhile.
    fn release_wakes(&mut self) {
        if self.held_wakes.is_empty() {
            return;
        }
        let held = std::mem::take(&mut self.held_wakes);
        for (pane, flag) in held {
            if self.paint_scheduled(pane) {
                self.held_wakes.push((pane, flag));
                continue;
            }
            flag.store(false, Ordering::Release);
            self.pump_pane(pane);
        }
    }

    /// Writes the startup probe (P4) when `TERMOIL_STARTUP_PROBE` names a file. `painted_ms` is
    /// the first frame with the shell's output; `visible_ms`, the gate, is when the compositor has
    /// shown the first frame with that output presented after the window became visible.
    fn startup_probe(&mut self, el: &ActiveEventLoop, id: WinId) {
        if self.probe_done {
            return;
        }
        let Ok(path) = std::env::var("TERMOIL_STARTUP_PROBE") else {
            self.probe_done = true;
            return;
        };
        let Some(ws) = self.windows.get(&id) else { return };
        let output = ws
            .views
            .values()
            .any(|v| v.state.lines.iter().any(|l| l.cells.iter().any(|c| c.text.first().is_some_and(|ch| ch != ' '))));
        if !output {
            return;
        }
        let painted = *self.painted_ms.get_or_insert_with(since_start);
        if !ws.presented_shown {
            return;
        }
        self.probe_done = true;
        let renderer = ws.painter.name();
        wait_composited();
        let visible = since_start();
        phase("visible");
        // Activation is timed too, so the work moved after the visible frame stays in view.
        if let Some(ws) = self.windows.get_mut(&id) {
            showtrace::begin();
            ws.finish_show();
            showtrace::end();
        }
        let active = since_start();
        let phases: Vec<String> =
            PHASES.lock().map(|p| p.iter().map(|(n, t)| format!("[\"{n}\",{t:.2}]")).collect()).unwrap_or_default();
        let _ = std::fs::write(
            &path,
            format!(
                "{{\"visible_ms\":{visible:.2},\"painted_ms\":{painted:.2},\"active_ms\":{active:.2},\"renderer\":\"{}\",\"phases\":[{}],\"show_messages\":{}}}\n",
                renderer,
                phases.join(","),
                showtrace::json()
            ),
        );
        if std::env::var("TERMOIL_STARTUP_PROBE_EXIT").is_ok_and(|v| v == "1") {
            self.app.effects.push(Effect::Quit);
            self.apply_effects(el);
        }
    }
}

/// Gives the latency benchmark the frame just painted.
fn bench_after_paint(b: &mut bench::LatencyBench, ws: &WinState, app: &App) {
    let painted = Instant::now();
    // Measure the renderer the window keeps: while a switch to the GPU is pending, wait for it.
    if ws.upgrade.is_some() {
        return;
    }
    let Some(wi) = app.model.window_index(ws.id) else { return };
    let Some(view) = ws.views.get(&app.model.windows[wi].tab().focus) else { return };
    let lines = &view.state.lines;
    let cell = |r: usize, c: usize| lines.get(r).and_then(|l| l.cells.get(c)).and_then(|x| x.text.first());
    let prompt = || {
        lines.iter().position(|l| {
            let mut s = String::new();
            l.cells.iter().take(21).for_each(|c| c.text.push_to(&mut s));
            s.starts_with("termoil latency echo")
        })
    };
    b.after_paint(painted, cell, prompt);
}

/// Records a window's position and size in the app, for session restore. A minimized or hidden
/// window reports no position or a zero size, and is left as it was.
fn note_geometry(ws: &WinState, app: &mut App) {
    let size = ws.window.inner_size();
    let Ok(pos) = ws.window.outer_position() else { return };
    if size.width == 0 || size.height == 0 || ws.window.is_minimized() == Some(true) || ws.window.fullscreen().is_some()
    {
        return;
    }
    let g = crate::restore::WindowGeometry { x: pos.x, y: pos.y, width: size.width, height: size.height };
    app.geometry.insert(ws.id, g);
}

/// Sets the window title to the active tab's title.
fn update_title(ws: &WinState, app: &App) {
    let Some(wi) = app.model.window_index(ws.id) else { return };
    // The label carries a failed command's exit code, so it shows even with the tab bar hidden.
    let t = crate::view::tab_label(app, app.model.windows[wi].tab());
    let title = if t.is_empty() { "Termoil".to_string() } else { t };
    if ws.window.title() != title {
        ws.window.set_title(&title);
    }
}

impl ApplicationHandler<UserEvent> for Gui {
    /// Opens the first window.
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if let Some((cwd, command)) = self.initial.take() {
            phase("resumed");
            let plain_start = cwd.is_none() && command.is_none() && !crate::app::instance_mode().separate;
            if plain_start && self.bench.is_none() {
                crate::update::actions::start_automatic(&self.app);
            }
            if plain_start && (restore_after_update(&mut self.app) || crate::restore::restore_on_start(&mut self.app)) {
                self.apply_effects(el);
                return;
            }
            if let Err(e) = crate::actions::new_window(&mut self.app, cwd.map(PathBuf::from), command.as_deref()) {
                eprintln!("termoil: {e}");
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
                phase("wake");
                if self.paint_scheduled(pane) {
                    self.held_wakes.push((pane, flag));
                } else {
                    flag.store(false, Ordering::Release);
                }
                self.pump_pane(pane);
            }
            UserEvent::Ipc(call, reply) => {
                crate::telemetry::note_action(crate::telemetry::Category::Ipc, &call.action);
                let _ = reply.send(self.app.run_action(&call));
            }
            UserEvent::Config(r) => self.config_loaded(*r),
            UserEvent::QuickTerminal => self.toggle_quick(el),
            UserEvent::Access(ev) => self.access_event(ev),
            UserEvent::Update(ev) => self.update_event(ev),
            UserEvent::Answer(answer) => self.app.effects.push(Effect::ShowAnswer(answer)),
        }
        self.apply_effects(el);
    }

    /// Window events: input, resize, redraw, close.
    fn window_event(&mut self, el: &ActiveEventLoop, wid: WindowId, ev: WindowEvent) {
        let Some(&id) = self.by_winit.get(&wid) else { return };
        if !self.probe_done {
            phase(event_name(&ev));
        }
        let Some(ws) = self.windows.get_mut(&id) else { return };
        if let Some(access) = ws.access.as_mut() {
            access.process_event(&ws.window, &ev);
        }
        let app = &mut self.app;
        if matches!(
            ev,
            WindowEvent::KeyboardInput { .. }
                | WindowEvent::Ime(_)
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. }
        ) {
            ws.last_input = Some(Instant::now());
            crate::telemetry::input();
        }
        match ev {
            WindowEvent::CloseRequested => {
                if app.model.windows.len() == 1 {
                    crate::restore::save_on_quit(app, None);
                }
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
                note_geometry(ws, app);
            }
            WindowEvent::Moved(_) => {
                ws.refresh_frame_interval();
                note_geometry(ws, app);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                ws.scale = scale_factor;
                ws.refont(app);
                ws.relayout(app);
            }
            WindowEvent::RedrawRequested => {
                phase("redraw_requested");
                if let Some(due) = ws.paint_due(Instant::now(), &app.config.window) {
                    // Too soon after the last paint: paint once at the display rate instead.
                    ws.defer_paint(due);
                    ws.paint_held_until = Some(due);
                    self.apply_effects(el);
                    return;
                }
                if let Err(e) = ws.draw(app) {
                    eprintln!("termoil: draw failed: {e}");
                }
                if let Some(b) = &mut self.bench {
                    bench_after_paint(b, ws, app);
                }
                self.startup_probe(el, id);
                if let Some(ws) = self.windows.get_mut(&id) {
                    ws.finish_show();
                }
            }
            WindowEvent::ModifiersChanged(m) => ws.mods = m.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if let Some(raw) = input::raw_key(&event, ws.mods) {
                    handle::raw_key(ws, app, &self.bound, &raw);
                }
            }
            WindowEvent::Ime(ime) => handle::ime(ws, app, ime),
            WindowEvent::Focused(f) => {
                crate::telemetry::focus(f);
                if f {
                    app.model.focused = Some(id);
                }
                handle::focus(ws, app, f);
            }
            WindowEvent::CursorMoved { position, .. } => handle::cursor_moved(ws, app, position.x, position.y),
            WindowEvent::CursorLeft { .. } => handle::cursor_left(ws),
            WindowEvent::MouseInput { state, button, .. } => handle::button(ws, app, state, button),
            WindowEvent::MouseWheel { delta, .. } => handle::wheel(ws, app, delta),
            WindowEvent::DroppedFile(path) => handle::dropped_file(ws, app, &path),
            WindowEvent::ThemeChanged(t) => {
                self.os_dark = t == winit::window::Theme::Dark;
                app.effects.push(Effect::ConfigChanged);
            }
            _ => {}
        }
        self.apply_effects(el);
    }

    /// Wakes windows waiting on synchronized output, and drives the latency benchmark.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.release_wakes();
        let now = Instant::now();
        let mut next: Option<Instant> = self.bench_step(el, now);
        for ws in self.windows.values_mut() {
            if let Some(t) = ws.shader_tick(&mut self.app, now) {
                next = Some(next.map_or(t, |n| n.min(t)));
            }
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
