//! E6: the window (docs/TEST-SUITE.md section 11), on a real winit window with a real shell.
//!
//! winit's key events cannot be built outside winit, so keys enter at the `RawKey` step, right after
//! the winit conversion, and go through the bindings and the encoder exactly as typed keys do. IME
//! and focus events go through `ApplicationHandler::window_event` as winit delivers them. Sessions
//! record what is written to the shell, so the test can check that a preedit is never sent and a
//! paste is bracketed. The update actions ask a release server in this process (task-2586), so the
//! palette's answers can be checked without the network. Skipped, with a message, when no window can be
//! created.

#[path = "common/keyboard.rs"]
mod keyboard;

use std::time::{Duration, Instant};

use termoil_app::answer::Answer;
use termoil_app::app::MemoryClipboard;
use termoil_app::config::Config;
use termoil_app::gui::{Gui, UserEvent, handle};
use termoil_input::{Key, Modifiers, NamedKey, RawKey};
use winit::application::ApplicationHandler;
use winit::event::{Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

/// One scripted step: returns Ok(true) when done, Ok(false) to be polled again, Err to fail.
type Step = Box<dyn FnMut(&mut Gui, &ActiveEventLoop) -> Result<bool, String>>;

/// Wraps the front end and runs the script between events.
struct Harness {
    gui: Gui,
    steps: Vec<(&'static str, Step)>,
    current: usize,
    started: Option<Instant>,
    failure: Option<String>,
    finished: bool,
}

impl Harness {
    /// Polls the current step, moving on when it is done and failing it after 30 seconds.
    fn tick(&mut self, el: &ActiveEventLoop) {
        while self.current < self.steps.len() && self.failure.is_none() {
            let started = *self.started.get_or_insert_with(Instant::now);
            let (name, step) = &mut self.steps[self.current];
            match step(&mut self.gui, el) {
                Ok(true) => {
                    self.current += 1;
                    self.started = None;
                }
                Ok(false) if started.elapsed() > Duration::from_secs(30) => {
                    self.failure = Some(format!("step '{name}' timed out"))
                }
                Ok(false) => break,
                Err(e) => self.failure = Some(format!("step '{name}': {e}")),
            }
            self.gui.flush(el);
        }
        if self.failure.is_some() || self.current == self.steps.len() {
            self.finished = self.current == self.steps.len();
            el.exit();
        }
    }
}

impl ApplicationHandler<UserEvent> for Harness {
    /// Starts the front end, then the script.
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.gui.resumed(el);
        self.tick(el);
    }

    /// Forwards and ticks.
    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        self.gui.user_event(el, ev);
        self.tick(el);
    }

    /// Forwards and ticks.
    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, ev: WindowEvent) {
        self.gui.window_event(el, id, ev);
        self.tick(el);
    }

    /// Polls every 20 ms while a step waits.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.gui.about_to_wait(el);
        self.tick(el);
        if !self.finished && self.failure.is_none() {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(20)));
        }
    }
}

/// The only window's id and its focused pane.
fn focus(gui: &Gui) -> Option<(u64, u64)> {
    let w = gui.app.model.windows.first()?;
    Some((w.id, w.tab().focus))
}

/// The focused pane's screen text.
fn screen(gui: &Gui) -> String {
    let Some((_, pane)) = focus(gui) else { return String::new() };
    let p = &gui.app.panes[&pane];
    termoil_app::inspect::screen_text(&p.session.lock(), 0).join("\n")
}

/// What the focused pane's shell has been sent.
fn sent(gui: &Gui) -> Vec<u8> {
    focus(gui).map(|(_, p)| gui.app.panes[&p].session.recorded_input()).unwrap_or_default()
}

/// Presses a raw key in the only window.
fn press(gui: &mut Gui, raw: RawKey) {
    let (win, _) = focus(gui).expect("a window");
    let Gui { app, windows, bound, .. } = gui;
    let ws = windows.get_mut(&win).expect("window state");
    handle::raw_key(ws, app, bound, &raw);
}

/// Types text as key presses.
fn type_text(gui: &mut Gui, text: &str) {
    for c in text.chars() {
        let raw = if c == '\r' {
            RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE)
        } else {
            RawKey::new(Key::Char(c), Modifiers::NONE).with_text(&c.to_string())
        };
        press(gui, raw);
    }
}

/// Presses and lets go of the keys bound to an action in the default configuration.
fn press_binding(gui: &mut Gui, action: &str) -> Result<(), String> {
    let chords = gui
        .bound
        .iter()
        .find(|b| b.action == action)
        .map(|b| b.chords.clone())
        .ok_or(format!("no binding for {action}"))?;
    // Each chord is pressed the way a US keyboard sends it: `plus` is Shift and `=` (task-2589).
    for chord in chords {
        let raw = keyboard::us_key(&chord, termoil_app::config::Platform::current()).ok_or("no US key")?;
        press(gui, raw.clone());
        press(gui, keyboard::release_of(&raw));
    }
    Ok(())
}

/// Sends a window event through the front end.
fn window_event(gui: &mut Gui, el: &ActiveEventLoop, ev: WindowEvent) {
    let (win, _) = focus(gui).expect("a window");
    let id = gui.windows[&win].window.id();
    gui.window_event(el, id, ev);
}

/// The number of tabs and the panes of the active tab.
fn layout(gui: &Gui) -> (usize, Vec<u64>) {
    let w = &gui.app.model.windows[0];
    (w.tabs.len(), w.tab().root.panes())
}

/// The script.
fn steps() -> Vec<(&'static str, Step)> {
    vec![
        (
            "the shell shows a prompt",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                Ok(screen(g).contains(|c| c == '>' || (!cfg!(windows) && (c == '$' || c == '#'))))
            }),
        ),
        (
            "typing reaches the shell",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                type_text(g, "echo e6typed\r");
                Ok(true)
            }),
        ),
        (
            "the output appears",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                Ok(screen(g).lines().filter(|l| l.trim() == "e6typed").count() == 1)
            }),
        ),
        (
            "with renderer = \"gpu\" the window switches to the GPU renderer when the machine has a GPU",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                // The surface must be made on the window's own thread; made on the start thread it
                // always failed and every window stayed on the CPU renderer (0.1.1).
                static HAS_GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                if !*HAS_GPU.get_or_init(|| termoil_render::GpuContext::new().is_ok()) {
                    return Ok(true);
                }
                let (win, _) = focus(g).ok_or("window")?;
                let Gui { app, windows, .. } = g;
                let ws = windows.get_mut(&win).ok_or("window")?;
                ws.draw(app)?;
                Ok(ws.painter.is_gpu())
            }),
        ),
        (
            "a preedit is drawn and not sent",
            Box::new(|g: &mut Gui, el: &ActiveEventLoop| {
                let before = sent(g);
                window_event(g, el, WindowEvent::Ime(Ime::Preedit("\u{306b}\u{307b}".into(), Some((0, 0)))));
                let (win, _) = focus(g).ok_or("window")?;
                let Gui { app, windows, .. } = g;
                let ws = windows.get_mut(&win).ok_or("window")?;
                ws.draw(app)?;
                if ws.preedit.as_deref() != Some("\u{306b}\u{307b}") || ws.preedit_strip.frame_ref().is_none() {
                    return Err("the preedit was not kept and drawn".into());
                }
                if sent(g) != before {
                    return Err("the preedit reached the shell".into());
                }
                window_event(g, el, WindowEvent::Ime(Ime::Commit("\u{65e5}\u{672c}".into())));
                let after = sent(g);
                if !after.ends_with("\u{65e5}\u{672c}".as_bytes()) {
                    return Err(format!("the commit was not sent: {:?}", String::from_utf8_lossy(&after)));
                }
                Ok(true)
            }),
        ),
        (
            "a cancelled or interrupted composition sends nothing",
            Box::new(|g: &mut Gui, el: &ActiveEventLoop| {
                let before = sent(g);
                window_event(g, el, WindowEvent::Ime(Ime::Preedit("x".into(), None)));
                window_event(g, el, WindowEvent::Ime(Ime::Preedit(String::new(), None)));
                window_event(g, el, WindowEvent::Ime(Ime::Preedit("y".into(), None)));
                window_event(g, el, WindowEvent::Focused(false));
                let (win, _) = focus(g).ok_or("window")?;
                if let Some(p) = &g.windows[&win].preedit {
                    return Err(format!("the preedit {p:?} survived focus loss"));
                }
                // ConPTY turns on focus reporting (mode 1004), so the focus out report is expected.
                let after = sent(g);
                let extra = String::from_utf8_lossy(&after[before.len().min(after.len())..]).replace("[O", "");
                if !extra.is_empty() {
                    return Err(format!("a cancelled composition sent {extra:?}"));
                }
                window_event(g, el, WindowEvent::Focused(true));
                type_text(g, "\r");
                Ok(true)
            }),
        ),
        (
            "paste is bracketed when the mode is on",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                let (_, pane) = focus(g).ok_or("pane")?;
                g.app.clipboard = Box::new(MemoryClipboard(Some("one\ntwo".into())));
                g.app.panes[&pane].session.lock().advance(b"\x1b[?2004h");
                g.app.run_named("paste").map_err(|e| e.to_string())?;
                g.app.panes[&pane].session.lock().advance(b"\x1b[?2004l");
                let s = sent(g);
                if !s.ends_with(b"\x1b[200~one\rtwo\x1b[201~") && !s.ends_with(b"\x1b[200~one\ntwo\x1b[201~") {
                    return Err(format!(
                        "not bracketed: {:?}",
                        String::from_utf8_lossy(&s[s.len().saturating_sub(30)..])
                    ));
                }
                type_text(g, "\x1b");
                Ok(true)
            }),
        ),
        (
            "a tab opens from its key",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                press_binding(g, "new-tab")?;
                Ok(true)
            }),
        ),
        ("the second tab is there", Box::new(|g: &mut Gui, _: &ActiveEventLoop| Ok(layout(g).0 == 2))),
        (
            "the tab closes from its key",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                press_binding(g, "close")?;
                Ok(layout(g).0 == 1)
            }),
        ),
        (
            "a split is created and focus moves",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                let (_, first) = focus(g).ok_or("pane")?;
                press_binding(g, "split-right")?;
                let (_, panes) = layout(g);
                let (_, new) = focus(g).ok_or("pane")?;
                if panes.len() != 2 || new == first {
                    return Err(format!("split gave panes {panes:?} with focus {new}"));
                }
                press_binding(g, "focus-left")?;
                let (_, back) = focus(g).ok_or("pane")?;
                if back != first {
                    return Err(format!("focus-left moved to {back}, not {first}"));
                }
                Ok(true)
            }),
        ),
        (
            "the window draws both panes",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                let (win, _) = focus(g).ok_or("window")?;
                let Gui { app, windows, .. } = g;
                let ws = windows.get_mut(&win).ok_or("window")?;
                ws.draw(app)?;
                Ok(ws.views.len() == 2 && ws.views.values().all(|v| v.frame.is_some()))
            }),
        ),
        ("the split resizes from its keys and by dragging", Box::new(resize_step)),
        ("the tab bar closes a tab from its cross and opens one from its plus", Box::new(tab_bar_step)),
        ("hints label the links on screen", Box::new(hints_step)),
        ("the theme switcher previews and keeps a theme", Box::new(theme_step)),
        ("the settings view edits font.size and saves it", Box::new(settings_step)),
        ("the command palette runs an action", Box::new(palette_step)),
        (
            "the palette checks for updates and shows the answer",
            answer_step("check-for-updates", &[], |a| {
                a.title == "Termoil 99.0.0 is available" && a.lines[0].starts_with("You have ")
            }),
        ),
        (
            "the palette lists updates",
            answer_step("list-updates", &[], |a| {
                a.lines.iter().any(|l| l == "stable:") && a.lines.iter().any(|l| l.trim_start().starts_with("99.0.0"))
            }),
        ),
        (
            "the palette shows a failed action's error",
            answer_step("set-theme", &["No Such Theme E6"], |a| a.failed && a.title == "set-theme failed"),
        ),
        ("the search bar finds text as it is typed", Box::new(search_step)),
        ("the quick terminal opens", Box::new(quick_open_step)),
        ("the quick terminal is shown and then hidden", Box::new(quick_toggle_step)),
        ("every palette action runs from the palette", every_action_step()),
        (
            "quit",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                g.app.run_named("quit").map_err(|e| e.to_string())?;
                Ok(true)
            }),
        ),
    ]
}

#[test]
fn e6_window_input_ime_paste_tabs_splits() {
    if cfg!(target_os = "macos") {
        // winit on macOS creates its event loop only on the main thread, and the test harness runs
        // each test on another thread. E6 runs on Windows and Linux.
        return println!("winit needs the main thread on macOS; skipping E6");
    }
    let dir = std::env::temp_dir().join(format!("termoil-e6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    // `renderer = "gpu"`: the window starts on the CPU renderer and switches to the GPU, the path a
    // step below checks. The default (`auto`) never starts a GPU device (task-2481).
    let body = format!(
        "[shell]\nprogram = \"{shell}\"\n\n[window]\nrenderer = \"gpu\"\n\n[updates]\ncheck = \"off\"\nchannel = \"nightly\"\n"
    );
    let releases = release_server();
    // SAFETY: set before the event loop and the front end start any thread that reads the environment.
    unsafe {
        std::env::set_var("TERMOIL_RELEASES_STABLE", format!("{releases}/stable.json"));
        std::env::set_var("TERMOIL_RELEASES_NIGHTLY", format!("{releases}/nightly.json"));
        std::env::set_var("TERMOIL_DATA_DIR", dir.join("data"));
        std::env::set_var("TERMOIL_TELEMETRY", "off");
    }
    std::fs::write(&config_path, body).unwrap();
    let (config, _) = Config::load(&config_path).unwrap();
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(windows)]
    winit::platform::windows::EventLoopBuilderExtWindows::with_any_thread(&mut builder, true);
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // The test harness runs each test on a thread other than the main thread.
        winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    }
    let Ok(event_loop) = builder.build() else { return println!("no display; skipping E6") };
    let socket = if cfg!(windows) {
        format!(r"\\.\pipe\termoil-e6-{}", std::process::id())
    } else {
        dir.join("e6.sock").to_string_lossy().into_owned()
    };
    let mut gui = Gui::new(&event_loop, config, config_path, &socket, None, None).unwrap().expect("own instance");
    gui.app.record_input = true;
    let mut h = Harness { gui, steps: steps(), current: 0, started: None, failure: None, finished: false };
    event_loop.run_app(&mut h).unwrap();
    assert!(h.failure.is_none(), "{}", h.failure.unwrap_or_default());
    assert!(h.finished, "the script stopped at step {}", h.current);
    println!("E6: {} of {} steps passed", h.current, h.steps.len());
}

/// Opens the palette from its key, filters to zoom-pane and runs it with Enter.
fn palette_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    press_binding(g, "command-palette")?;
    g.flush(el);
    let (win, _) = focus(g).ok_or("window")?;
    if g.windows[&win].overlay.is_none() {
        return Err("the palette did not open".into());
    }
    type_text(g, "zoompane");
    let before = g.app.model.windows[0].tab().zoomed.is_some();
    press(g, RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE));
    let after = g.app.model.windows[0].tab().zoomed.is_some();
    if g.windows[&win].overlay.is_some() || before == after {
        return Err("Enter in the palette did not run zoom-pane and close".into());
    }
    Ok(true)
}

/// Opens the search bar from its key, types a word on screen, checks it was found and selected,
/// and closes the bar with Escape.
fn search_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    press_binding(g, "search")?;
    g.flush(el);
    type_text(g, "e6typed");
    let (win, pane) = focus(g).ok_or("window")?;
    match &g.windows[&win].overlay {
        Some(termoil_app::gui::overlay::Overlay::Search(s)) if s.found == Some(true) => {}
        _ => return Err("the search bar did not find e6typed".into()),
    }
    if g.app.panes[&pane].selection.is_none() {
        return Err("the match was not selected".into());
    }
    press(g, RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    if g.windows[&win].overlay.is_some() {
        return Err("Escape did not close the search bar".into());
    }
    Ok(true)
}

/// Runs the quick-terminal action, which creates the quick terminal window the first time.
fn quick_open_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    g.app.run_named("quick-terminal").map_err(|e| e.to_string())?;
    g.flush(el);
    Ok(true)
}

/// The quick terminal window exists, the action hides it (closing its window and keeping its
/// session), and a third press shows it again with the same pane.
fn quick_toggle_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    let Some(id) = g.quick else { return Ok(false) };
    let Some(ws) = g.windows.get_mut(&id) else { return Ok(false) };
    ws.focused = true;
    let wi = g.app.model.window_index(id).ok_or("the quick terminal has no model window")?;
    let pane = g.app.model.windows[wi].tab().focus;
    g.app.run_named("quick-terminal").map_err(|e| e.to_string())?;
    g.flush(el);
    if g.windows.contains_key(&id) {
        return Err("a second quick-terminal did not hide the window".into());
    }
    if !g.app.panes.contains_key(&pane) {
        return Err("hiding the quick terminal ended its session".into());
    }
    g.app.run_named("quick-terminal").map_err(|e| e.to_string())?;
    g.flush(el);
    if !g.windows.contains_key(&id) {
        return Err("a third quick-terminal did not show the window again".into());
    }
    Ok(true)
}

/// The resize keys move the divider of the split made earlier, and so does dragging it.
fn resize_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    let ratio = |g: &Gui| match &g.app.model.windows[0].tab().root {
        termoil_app::model::Node::Split { ratio, .. } => Some(*ratio),
        _ => None,
    };
    if g.app.model.windows[0].tab().zoomed.is_some() {
        g.app.run_named("zoom-pane").map_err(|e| e.to_string())?;
        g.flush(el);
    }
    let before = ratio(g).ok_or("no split to resize")?;
    press_binding(g, "resize-split")?;
    g.flush(el);
    let after_key = ratio(g).ok_or("the split went away")?;
    if (after_key - before).abs() < 0.01 {
        return Err(format!("the resize key left the ratio at {before}"));
    }
    let (win, _) = focus(g).ok_or("window")?;
    let Gui { app, windows, .. } = g;
    let ws = windows.get_mut(&win).ok_or("window")?;
    let (first, r) = ws.layout.first().copied().ok_or("no layout")?;
    let (x, y) = ((r.x + r.w) as f64 + 1.0, (r.y + r.h / 2.0) as f64);
    if ws.divider_at(x, y).map(|(p, _)| p) != Some(first) {
        return Err(format!("no divider found at ({x}, {y})"));
    }
    handle::cursor_moved(ws, app, x, y);
    handle::button(ws, app, winit::event::ElementState::Pressed, winit::event::MouseButton::Left);
    handle::cursor_moved(ws, app, x + 120.0, y);
    handle::button(ws, app, winit::event::ElementState::Released, winit::event::MouseButton::Left);
    let after_drag = ratio(g).ok_or("the split went away")?;
    if after_drag <= after_key + 0.01 {
        return Err(format!("dragging the divider right left the ratio at {after_drag}"));
    }
    Ok(true)
}

/// The tab bar with the pointer (task-2468): with a third tab open, the pointer over the last tab's cross
/// is a hover on that cross, a click there closes the tab through `close-tab`, the bar is hidden if that
/// left one tab (task-2487), and a click at the right end of the bar, where the plus is, opens a tab
/// through `new-tab`.
fn tab_bar_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    use termoil_app::gui::tab_bar::TabHit;
    let (_, before) = focus(g).ok_or("pane")?;
    g.app.run_named("new-tab").map_err(|e| e.to_string())?;
    g.flush(el);
    let tabs = g.app.model.windows[0].tabs.len();
    let (win, _) = focus(g).ok_or("window")?;
    let Gui { app, windows, .. } = g;
    let ws = windows.get_mut(&win).ok_or("window")?;
    ws.relayout(app);
    ws.draw(app)?;
    let last = ws.tab_layout.tabs.len().checked_sub(1).ok_or("the tab bar laid out no tabs")?;
    let (cw, y) = (ws.cell().0 as f64, ws.tab_bar_h as f64 / 2.0);
    let cross = (ws.tab_layout.tabs[last].close() as f64 + 1.0) * cw;
    handle::cursor_moved(ws, app, cross, y);
    if ws.tab_hover != Some(TabHit::Close(last)) {
        return Err(format!("the pointer on the cross is {:?}, not Close({last})", ws.tab_hover));
    }
    handle::button(ws, app, winit::event::ElementState::Pressed, winit::event::MouseButton::Left);
    handle::button(ws, app, winit::event::ElementState::Released, winit::event::MouseButton::Left);
    if app.model.windows[0].tabs.len() != tabs - 1 {
        return Err(format!("the cross left {} tabs, not {}", app.model.windows[0].tabs.len(), tabs - 1));
    }
    ws.relayout(app);
    ws.draw(app)?;
    if app.model.windows[0].tabs.len() == 1 && ws.tab_bar_h != 0.0 {
        return Err(format!("with one tab the bar is {} pixels high, not hidden", ws.tab_bar_h));
    }
    app.run_named("new-tab").map_err(|e| e.to_string())?;
    let tabs = app.model.windows[0].tabs.len();
    ws.relayout(app);
    ws.draw(app)?;
    let y = ws.tab_bar_h as f64 / 2.0;
    let width = ws.window.inner_size().width as f64;
    handle::cursor_moved(ws, app, width - cw, y);
    if ws.tab_hover != Some(TabHit::NewTab) {
        return Err(format!("the pointer at the right end of the bar is {:?}, not the plus", ws.tab_hover));
    }
    handle::button(ws, app, winit::event::ElementState::Pressed, winit::event::MouseButton::Left);
    handle::button(ws, app, winit::event::ElementState::Released, winit::event::MouseButton::Left);
    if app.model.windows[0].tabs.len() != tabs + 1 {
        return Err(format!("the plus left {} tabs, not {}", app.model.windows[0].tabs.len(), tabs + 1));
    }
    // Put things back for the steps after this one: close the tabs this step opened, and focus the pane
    // that had the keyboard before.
    while app.model.windows[0].tabs.len() > tabs - 1 {
        let opened = app.model.windows[0].tab().focus;
        let _ = app.run_action(&termoil_protocol::ActionCall {
            action: "close-tab".into(),
            args: serde_json::Map::from_iter([("pane".to_string(), serde_json::json!(opened))]),
        });
    }
    termoil_app::actions::focus_pane(app, before).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Prints a URL, then shows hints: the URL gets a label, and Escape closes the labels.
fn hints_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    if !screen(g).contains("https://example.com/e6") {
        if !sent(g).ends_with(b"echo https://example.com/e6\r") {
            type_text(g, "echo https://example.com/e6\r");
        }
        return Ok(false);
    }
    g.app.run_named("show-hints").map_err(|e| e.to_string())?;
    g.flush(el);
    let (win, _) = focus(g).ok_or("window")?;
    match &g.windows[&win].overlay {
        Some(termoil_app::gui::overlay::Overlay::Hints(h))
            if h.items
                .iter()
                .any(|(_, l, ..)| matches!(l, termoil_app::links::Link::Url(u) if u == "https://example.com/e6")) => {}
        _ => return Err("show-hints did not label the URL".into()),
    }
    press(g, RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    if g.windows[&win].overlay.is_some() {
        return Err("Escape did not close the hints".into());
    }
    Ok(true)
}

/// Opens the theme switcher, filters to Termoil Light, sees it previewed, and keeps it with Enter,
/// which writes it to the configuration file.
fn theme_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    g.app.run_named("choose-theme").map_err(|e| e.to_string())?;
    g.flush(el);
    type_text(g, "termoil light");
    g.flush(el);
    if g.app.config.theme.name != "Termoil Light" {
        return Err(format!("the preview shows {}", g.app.config.theme.name));
    }
    press(g, RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE));
    g.flush(el);
    let text = std::fs::read_to_string(&g.app.config_path).map_err(|e| e.to_string())?;
    if !text.contains("theme = \"Termoil Light\"") {
        return Err(format!("the config file was not updated: {text}"));
    }
    let (win, _) = focus(g).ok_or("window")?;
    let bg = g.windows[&win].colors.background;
    if bg.r < 128 {
        return Err("the window kept the dark background".into());
    }
    shader_list_step(g, el)?;
    Ok(true)
}

/// task-2590: the shader list beside the themes. Right moves to it, typing filters it and previews
/// the shader, Enter writes the shader and keeps the theme, and Escape after moving restores the
/// shader that was in effect.
fn shader_list_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<(), String> {
    let key = |k| RawKey::new(Key::Named(k), Modifiers::NONE);
    g.app.run_named("choose-theme").map_err(|e| e.to_string())?;
    g.flush(el);
    press(g, key(NamedKey::Right));
    type_text(g, "neon");
    g.flush(el);
    if g.app.config.window.shader != "neon" {
        return Err(format!("the shader preview is '{}'", g.app.config.window.shader));
    }
    press(g, key(NamedKey::Enter));
    g.flush(el);
    let text = std::fs::read_to_string(&g.app.config_path).map_err(|e| e.to_string())?;
    if !text.contains("shader = \"neon\"") || !text.contains("theme = \"Termoil Light\"") {
        return Err(format!("Enter did not keep the theme and the shader: {text}"));
    }
    let (win, _) = focus(g).ok_or("window")?;
    if let Some(error) = g.windows[&win].shader.banner() {
        return Err(format!("the neon shader did not compile in the window: {error}"));
    }
    g.app.run_named("choose-theme").map_err(|e| e.to_string())?;
    g.flush(el);
    press(g, key(NamedKey::Right));
    press(g, key(NamedKey::Down));
    g.flush(el);
    if g.app.config.window.shader == "neon" {
        return Err("Down in the shader list did not preview the next shader".into());
    }
    press(g, key(NamedKey::Escape));
    g.flush(el);
    if g.app.config.window.shader != "neon" {
        return Err(format!("Escape left the shader at '{}'", g.app.config.window.shader));
    }
    g.app.run_action(&termoil_protocol::ActionCall { action: "set-shader".into(), args: json_name("none") });
    g.flush(el);
    cursor_list_step(g, el)
}

/// task-2593: the cursor effect list right of the shaders. Right twice moves to it, typing filters it
/// and previews the cursor effect, Enter writes it beside the shader, and the window compiles the
/// cursor effect over the shader.
fn cursor_list_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<(), String> {
    let key = |k| RawKey::new(Key::Named(k), Modifiers::NONE);
    g.app.run_action(&termoil_protocol::ActionCall { action: "set-shader".into(), args: json_name("cursor-glow") });
    g.app.run_named("choose-theme").map_err(|e| e.to_string())?;
    g.flush(el);
    press(g, key(NamedKey::Right));
    press(g, key(NamedKey::Right));
    type_text(g, "sonar");
    g.flush(el);
    if g.app.config.window.cursor_effect != "sonar" || g.app.config.window.shader != "cursor-glow" {
        let w = &g.app.config.window;
        return Err(format!("the cursor effect preview is '{}' over '{}'", w.cursor_effect, w.shader));
    }
    press(g, key(NamedKey::Enter));
    g.flush(el);
    let text = std::fs::read_to_string(&g.app.config_path).map_err(|e| e.to_string())?;
    if !text.contains("cursor_effect = \"sonar\"") || !text.contains("shader = \"cursor-glow\"") {
        return Err(format!("Enter did not keep the shader and the cursor effect: {text}"));
    }
    let (win, _) = focus(g).ok_or("window")?;
    if let Some(error) = g.windows[&win].shader.banner() {
        return Err(format!("sonar over cursor-glow did not compile in the window: {error}"));
    }
    for (action, name) in [("set-cursor-effect", "none"), ("set-shader", "none")] {
        g.app.run_action(&termoil_protocol::ActionCall { action: action.into(), args: json_name(name) });
    }
    g.flush(el);
    Ok(())
}

/// Arguments naming one value, for an action that takes `name`.
fn json_name(name: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut args = serde_json::Map::new();
    args.insert("name".into(), serde_json::Value::String(name.into()));
    args
}

/// Opens the settings view through its action, tries an out of range value (refused, file untouched),
/// edits `font.size` to a new value, and checks the file and the live configuration both changed.
fn settings_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    let enter = || RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE);
    g.app.run_named("open-settings").map_err(|e| e.to_string())?;
    g.flush(el);
    let (win, _) = focus(g).ok_or("window")?;
    if g.windows[&win].overlay.is_none() {
        return Err("open-settings did not open the view".into());
    }
    let Gui { app, windows, .. } = g;
    windows.get_mut(&win).ok_or("window")?.draw(app)?;
    type_text(g, "window.opacity");
    press(g, enter());
    type_text(g, "9");
    press(g, enter());
    g.flush(el);
    let text = std::fs::read_to_string(&g.app.config_path).map_err(|e| e.to_string())?;
    if text.contains("opacity") || g.app.config.window.opacity != 1.0 {
        return Err(format!("an opacity above 1 was written: {text}"));
    }
    press(g, RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    for _ in 0..14 {
        press(g, RawKey::new(Key::Named(NamedKey::Backspace), Modifiers::NONE));
    }
    type_text(g, "font.size");
    press(g, enter());
    for _ in 0..8 {
        press(g, RawKey::new(Key::Named(NamedKey::Backspace), Modifiers::NONE));
    }
    type_text(g, "18");
    press(g, enter());
    g.flush(el);
    let text = std::fs::read_to_string(&g.app.config_path).map_err(|e| e.to_string())?;
    if !text.contains("size = 18.0") || !text.contains("theme = \"Termoil Light\"") {
        return Err(format!("font.size was not written beside the rest of the file: {text}"));
    }
    if g.app.config.font.size != 18.0 {
        return Err(format!("the live font size is {}", g.app.config.font.size));
    }
    press(g, RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    if g.windows[&win].overlay.is_some() {
        return Err("Escape did not close the settings view".into());
    }
    Ok(true)
}

/// A release server in this process for the update actions: stable offers the running version,
/// nightly offers 99.0.0. Answers its base URL.
fn release_server() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind the release server");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let entry = |version: &str| {
        let file =
            serde_json::json!({ "file": "pkg.bin", "url": format!("{base}/pkg.bin"), "bytes": 1, "sha256": "00" });
        serde_json::json!({ "version": version, "date": "2026-11-09", "notes": "", "files": { "windows": file, "macos": file, "deb": file, "appimage": file } })
    };
    let manifest = |channel: &str, e: serde_json::Value| {
        let mut top = e.clone();
        top["product"] = serde_json::json!("Termoil");
        top["channel"] = serde_json::json!(channel);
        top["versions"] = serde_json::json!([e]);
        top.to_string()
    };
    let stable = manifest("stable", entry(termoil_protocol::VERSION));
    let nightly = manifest("nightly", entry("99.0.0"));
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            while reader.read_line(&mut String::new()).is_ok_and(|n| n > 2) {}
            let body = match line.split_whitespace().nth(1) {
                Some("/stable.json") => stable.as_str(),
                Some("/nightly.json") => nightly.as_str(),
                _ => "",
            };
            let status = if body.is_empty() { "404 Not Found" } else { "200 OK" };
            let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes()));
        }
    });
    base
}

/// The answer overlay of a window, when one is open.
fn answer_of(g: &Gui, win: u64) -> Option<&Answer> {
    match g.windows.get(&win)?.overlay.as_ref()? {
        termoil_app::gui::overlay::Overlay::Answer(a) => Some(&a.answer),
        _ => None,
    }
}

/// Opens the palette in the first window from its key, types `action`, selects its entry, presses
/// Enter, and types each of `args` with Enter when the palette asks for a required parameter.
fn run_from_palette(g: &mut Gui, el: &ActiveEventLoop, action: &str, args: &[&str]) -> Result<(), String> {
    press_binding(g, "command-palette")?;
    g.flush(el);
    let (win, _) = focus(g).ok_or("window")?;
    type_text(g, action);
    match g.windows.get_mut(&win).and_then(|ws| ws.overlay.as_mut()) {
        Some(termoil_app::gui::overlay::Overlay::Palette(p)) => {
            let at = p.visible().iter().position(|e| e.action == action && e.preset.is_none());
            p.selected = at.ok_or(format!("the palette does not list {action}"))?;
        }
        _ => return Err("the palette did not open".into()),
    }
    press(g, RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE));
    for arg in args {
        type_text(g, arg);
        press(g, RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE));
    }
    g.flush(el);
    Ok(())
}

/// A step that runs an action from the palette, waits for its answer in the window, checks it with
/// `ok`, closes it with Escape, and checks the next key reaches the shell again.
fn answer_step(action: &'static str, args: &'static [&'static str], ok: fn(&Answer) -> bool) -> Step {
    let mut started = false;
    Box::new(move |g: &mut Gui, el: &ActiveEventLoop| {
        let (win, _) = focus(g).ok_or("window")?;
        if !started {
            run_from_palette(g, el, action, args)?;
            started = true;
            if answer_of(g, win).is_none() {
                return Err(format!("{action} from the palette opened no answer"));
            }
        }
        let Some(answer) = answer_of(g, win).cloned() else { return Err(format!("the answer to {action} closed")) };
        if answer.pending {
            return Ok(false);
        }
        if !ok(&answer) {
            return Err(format!("{action} answered {answer:?}"));
        }
        let Gui { app, windows, .. } = g;
        windows.get_mut(&win).ok_or("window")?.draw(app)?;
        press(g, RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
        if answer_of(g, win).is_some() {
            return Err("Escape did not close the answer".into());
        }
        let before = sent(g).len();
        type_text(g, "x");
        if sent(g).len() == before {
            return Err("the key after the answer did not reach the shell".into());
        }
        press(g, RawKey::new(Key::Named(NamedKey::Backspace), Modifiers::NONE));
        Ok(true)
    })
}

/// The palette entries the every action step leaves to A5, which runs them with arguments that keep
/// them inside the test folder, and why.
const LEFT_TO_A5: &[(&str, &str)] = &[
    ("quit", "ends the test; the last step runs it"),
    ("close", "would close the window the remaining steps use"),
    ("close-tab", "would close the window the remaining steps use"),
    ("move-tab-to-window", "would move the window's only tab away"),
    ("install-shell-integration", "would change this machine's shell startup files"),
    ("install-update", "would download and install over the build under test"),
    ("open-config", "would open an editor"),
    ("wait-prompt", "waits 600 seconds without a timeout argument"),
];

/// Actions that may fail in this window, and why; a failure must still be shown with its reason.
const MAY_FAIL: &[(&str, &str)] = &[
    ("copy", "the selection the search step made may be gone"),
    ("previous-prompt", "cmd sends no OSC 133 prompt marks"),
    ("next-prompt", "cmd sends no OSC 133 prompt marks"),
    ("select-last-output", "cmd sends no OSC 133 prompt marks"),
    ("search-next", "the search bar was closed, which ends the search"),
    ("search-previous", "the search bar was closed, which ends the search"),
];

/// What the palette types for an action's required parameters in the every action step.
fn palette_args(action: &str) -> &'static [&'static str] {
    match action {
        "resize-split" => &["right"],
        "send-text" => &["echo e6palette"],
        "send-key" => &["escape"],
        "resize" => &["90", "26"],
        "set-title" => &["e6 title"],
        "run-action" => &["version"],
        "set-theme" => &["Termoil Light"],
        "set-shader" => &["none"],
        "set-cursor-effect" => &["none"],
        "activate-key-table" => &["e6"],
        _ => &[],
    }
}

/// Every palette entry, run from the palette in the first window one after another: each one must
/// reach `run_action`, an action whose result is information must show it, and none may fail apart
/// from the ones in `MAY_FAIL`. Overlays an action opens are closed before the next one.
fn every_action_step() -> Step {
    let mut queue: Vec<&'static str> = Vec::new();
    let mut waiting: Option<&'static str> = None;
    let mut problems: Vec<String> = Vec::new();
    let mut loaded = false;
    let mut ran = 0usize;
    Box::new(move |g: &mut Gui, el: &ActiveEventLoop| {
        if !loaded {
            let entries = termoil_app::palette::entries(
                &g.app.config.resolved_keybindings(termoil_app::config::Platform::current()),
            );
            queue = entries.iter().filter(|e| e.preset.is_none()).map(|e| e.action).collect();
            queue.retain(|a| !LEFT_TO_A5.iter().any(|(n, _)| n == a));
            queue.reverse();
            loaded = true;
        }
        let (win, first) = focus(g).ok_or("window")?;
        if let Some(action) = waiting {
            let Some(answer) = answer_of(g, win).cloned() else { return Err(format!("the answer to {action} closed")) };
            if answer.pending {
                return Ok(false);
            }
            judge(action, Some(&answer), &mut problems);
            waiting = None;
        }
        let Some(action) = queue.pop() else {
            reset_window(g, el, win, first)?;
            println!("E6: {ran} palette actions run from the palette");
            return if problems.is_empty() { Ok(true) } else { Err(problems.join("\n")) };
        };
        reset_window(g, el, win, first)?;
        run_from_palette(g, el, action, palette_args(action))?;
        ran += 1;
        let answer = answer_of(g, win).cloned();
        if answer.as_ref().is_some_and(|a| a.pending) {
            waiting = Some(action);
            return Ok(false);
        }
        judge(action, answer.as_ref(), &mut problems);
        Ok(false)
    })
}

/// Records what is wrong with the answer an action from the palette showed, if anything.
fn judge(action: &str, answer: Option<&Answer>, problems: &mut Vec<String>) {
    let informs = termoil_app::answer::shows_result(action);
    match answer {
        None if informs => problems.push(format!("{action}: answers information but showed nothing")),
        None => {}
        Some(a) if a.lines.iter().chain([&a.title]).any(|l| l.contains("unknown action")) => {
            problems.push(format!("{action}: did not reach run_action"))
        }
        Some(a) if a.failed && a.lines.is_empty() => problems.push(format!("{action}: failed with no reason")),
        Some(a) if a.failed && !MAY_FAIL.iter().any(|(n, _)| *n == action) => {
            problems.push(format!("{action}: failed: {}", a.lines.join(" ")))
        }
        Some(a) if !a.failed && !informs => problems.push(format!("{action}: acts, but showed {a:?}")),
        Some(_) => {}
    }
}

/// Puts the first window back for the next action: no overlay in any window, no full screen, the
/// first window's first pane focused, and the font at its configured size.
fn reset_window(g: &mut Gui, el: &ActiveEventLoop, win: u64, first: u64) -> Result<(), String> {
    for ws in g.windows.values_mut() {
        ws.overlay = None;
        ws.answer_waiting = None;
        if ws.window.fullscreen().is_some() {
            ws.window.set_fullscreen(None);
        }
    }
    g.app.key_tables.clear();
    if g.app.model.window_index(win).is_some() {
        let _ = termoil_app::actions::focus_pane(&mut g.app, first);
    }
    g.flush(el);
    Ok(())
}
