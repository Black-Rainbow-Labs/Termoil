//! E6: the window (docs/TEST-SUITE.md section 11), on a real winit window with a real shell.
//!
//! winit's key events cannot be built outside winit, so keys enter at the `RawKey` step, right after
//! the winit conversion, and go through the bindings and the encoder exactly as typed keys do. IME
//! and focus events go through `ApplicationHandler::window_event` as winit delivers them. Sessions
//! record what is written to the shell, so the test can check that a preedit is never sent and a
//! paste is bracketed. The quick terminal and command palette parts of E6 belong to M4. Skipped,
//! with a message, when no window can be created.

use std::time::{Duration, Instant};

use terminus_app::app::MemoryClipboard;
use terminus_app::config::Config;
use terminus_app::gui::{Gui, UserEvent, handle};
use terminus_input::{Key, Modifiers, NamedKey, RawKey};
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
    terminus_app::inspect::screen_text(&p.session.lock(), 0).join("\n")
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

/// Presses the keys bound to an action in the default configuration.
fn press_binding(gui: &mut Gui, action: &str) -> Result<(), String> {
    let chords = gui
        .bound
        .iter()
        .find(|b| b.action == action)
        .map(|b| b.chords.clone())
        .ok_or(format!("no binding for {action}"))?;
    for chord in chords {
        let ev = terminus_app::keymap::chord_to_event(&chord).ok_or("chord")?;
        press(gui, RawKey::new(ev.key, ev.mods));
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
            "the window switches to the GPU renderer when the machine has a GPU",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                // The surface must be made on the window's own thread; made on the start thread it
                // always failed and every window stayed on the CPU renderer (0.1.1).
                static HAS_GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                if !*HAS_GPU.get_or_init(|| terminus_render::GpuContext::new().is_ok()) {
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
        ("hints label the links on screen", Box::new(hints_step)),
        ("the theme switcher previews and keeps a theme", Box::new(theme_step)),
        ("the command palette runs an action", Box::new(palette_step)),
        ("the search bar finds text as it is typed", Box::new(search_step)),
        ("the quick terminal opens", Box::new(quick_open_step)),
        ("the quick terminal is shown and then hidden", Box::new(quick_toggle_step)),
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
    let dir = std::env::temp_dir().join(format!("terminus-e6-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    std::fs::write(&config_path, format!("[shell]\nprogram = \"{shell}\"\n")).unwrap();
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
        format!(r"\\.\pipe\terminus-e6-{}", std::process::id())
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
        Some(terminus_app::gui::overlay::Overlay::Search(s)) if s.found == Some(true) => {}
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
        terminus_app::model::Node::Split { ratio, .. } => Some(*ratio),
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
        Some(terminus_app::gui::overlay::Overlay::Hints(h))
            if h.items
                .iter()
                .any(|(_, l, ..)| matches!(l, terminus_app::links::Link::Url(u) if u == "https://example.com/e6")) => {}
        _ => return Err("show-hints did not label the URL".into()),
    }
    press(g, RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    if g.windows[&win].overlay.is_some() {
        return Err("Escape did not close the hints".into());
    }
    Ok(true)
}

/// Opens the theme switcher, filters to Terminus Light, sees it previewed, and keeps it with Enter,
/// which writes it to the configuration file.
fn theme_step(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    g.app.run_named("choose-theme").map_err(|e| e.to_string())?;
    g.flush(el);
    type_text(g, "terminus light");
    g.flush(el);
    if g.app.config.theme.name != "Terminus Light" {
        return Err(format!("the preview shows {}", g.app.config.theme.name));
    }
    press(g, RawKey::new(Key::Named(NamedKey::Enter), Modifiers::NONE));
    g.flush(el);
    let text = std::fs::read_to_string(&g.app.config_path).map_err(|e| e.to_string())?;
    if !text.contains("theme = \"Terminus Light\"") {
        return Err(format!("the config file was not updated: {text}"));
    }
    let (win, _) = focus(g).ok_or("window")?;
    let bg = g.windows[&win].colors.background;
    if bg.r < 128 {
        return Err("the window kept the dark background".into());
    }
    Ok(true)
}
