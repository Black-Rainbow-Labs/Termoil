//! E7: keyboard shortcuts on a real winit window with a real shell (task-2589,
//! `docs/decisions/keyboard-shortcuts.md`).
//!
//! Every key is pressed the way a US keyboard sends it (`common/keyboard.rs`): the plus sign is
//! Shift and `=`, and the text is what winit gives with the modifiers held. Keys enter at the
//! `RawKey` step, right after the winit conversion, as in E6. The session records every byte
//! written to the shell, so each step checks that a shortcut reached Termoil and nothing reached
//! the shell. Skipped, with a message, when no window can be created, and on macOS, where winit
//! needs the main thread.
//!
//! - K1: every default binding of the Windows and Linux table and of the macOS table runs its own
//!   binding. Each action is replaced by a `set-title` probe naming the binding, so the test sees
//!   which binding ran without opening windows.
//! - K2: the font size keys with the real bindings, for both tables: `=`, Shift and `=`, holding the
//!   key, `-` and `0`.
//! - K3: under the kitty protocol with event types and every key as an escape code, a key that ran a binding sends neither its
//!   press nor its release, and a key that did not still sends both.
//! - K4: Super keys with no binding send nothing to a legacy program, and `+` and `=` typed with no
//!   modifier still reach the shell.

#[path = "common/keyboard.rs"]
mod keyboard;

use std::time::{Duration, Instant};

use keyboard::{release_of, us_key, us_typed};
use termoil_app::config::{Binding, Config, Platform, default_keybindings_for};
use termoil_app::gui::{Gui, UserEvent, handle};
use termoil_app::keymap::resolve;
use termoil_input::{Key, KeyEventKind, Modifiers, NamedKey, RawKey};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
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
                    println!("E7: {name}");
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

/// The bytes sent to the shell since `before`, written so a failure message can show them.
fn sent_since(gui: &Gui, before: usize) -> String {
    let all = sent(gui);
    all[before.min(all.len())..].escape_ascii().to_string()
}

/// Gives a raw key to the only window, as the winit conversion does.
fn feed(gui: &mut Gui, raw: &RawKey) {
    let (win, _) = focus(gui).expect("a window");
    let Gui { app, windows, bound, .. } = gui;
    let ws = windows.get_mut(&win).expect("window state");
    handle::raw_key(ws, app, bound, raw);
}

/// Presses and lets go of a key.
fn tap(gui: &mut Gui, raw: &RawKey) {
    feed(gui, raw);
    feed(gui, &release_of(raw));
}

/// The first window's font size in points.
fn points(gui: &Gui) -> f32 {
    let (win, _) = focus(gui).expect("a window");
    gui.windows[&win].points
}

/// True while the visual bell is showing in the first window.
fn bell_showing(gui: &Gui) -> bool {
    let (win, _) = focus(gui).expect("a window");
    gui.windows[&win].bell_until.is_some_and(|t| Instant::now() < t)
}

/// The title the active tab was given by `set-title`.
fn tab_title(gui: &Gui) -> Option<String> {
    gui.app.model.windows.first()?.tab().title.clone()
}

/// The default bindings of a platform with each action replaced by `set-title` naming the binding.
fn probes(platform: Platform) -> Vec<Binding> {
    default_keybindings_for(platform)
        .iter()
        .map(|b| {
            let mut probe = Binding::new(&b.keys, "set-title");
            probe.args.insert("title".into(), toml::Value::String(format!("probe {} {}", b.keys, b.action)));
            probe
        })
        .collect()
}

/// K1 for one platform's table: presses every default binding as a US keyboard sends it and
/// checks its own probe ran, the shell got nothing and the bell did not ring.
fn every_binding_runs(g: &mut Gui, platform: Platform) -> Result<bool, String> {
    let saved = std::mem::replace(&mut g.bound, resolve(&probes(platform)));
    let mut problems = Vec::new();
    let mut pressed = 0;
    for b in default_keybindings_for(platform) {
        set_tab_title(g, None);
        let before = sent(g).len();
        for chord in b.chords().map_err(|e| format!("{}: {e}", b.keys))? {
            let raw = us_key(&chord, platform).ok_or(format!("no US key for {}", b.keys))?;
            tap(g, &raw);
        }
        pressed += 1;
        let want = format!("probe {} {}", b.keys, b.action);
        if tab_title(g).as_deref() != Some(want.as_str()) {
            problems.push(format!("{} ({}): ran {:?}", b.keys, b.action, tab_title(g)));
        }
        let extra = sent_since(g, before);
        if !extra.is_empty() {
            problems.push(format!("{} ({}): the shell was sent {extra:?}", b.keys, b.action));
        }
        if bell_showing(g) {
            problems.push(format!("{} ({}): the bell rang", b.keys, b.action));
        }
    }
    g.bound = saved;
    set_tab_title(g, None);
    println!("E7 K1: {pressed} {platform:?} bindings pressed as a US keyboard sends them");
    if problems.is_empty() { Ok(true) } else { Err(problems.join("\n")) }
}

/// Clears or sets the active tab's title directly.
fn set_tab_title(g: &mut Gui, title: Option<&str>) {
    if let Some(w) = g.app.model.windows.first_mut() {
        let active = w.active;
        w.tabs[active].title = title.map(str::to_string);
    }
}

/// The modifier the font size keys use on a platform's table.
fn platform_mod(platform: Platform) -> &'static str {
    if platform == Platform::MacOs { "cmd" } else { "ctrl" }
}

/// Presses a chord written as a binding, as a US keyboard sends it, and lets it go.
fn tap_chord(g: &mut Gui, keys: &str, platform: Platform) -> Result<RawKey, String> {
    let chord = termoil_app::keymap::parse_chord(keys)?;
    let raw = us_key(&chord, platform).ok_or(format!("no US key for {keys}"))?;
    tap(g, &raw);
    Ok(raw)
}

/// K2 for one platform's table: the font size keys change the size, a held key repeats, and
/// nothing reaches the shell.
fn font_size_keys(g: &mut Gui, el: &ActiveEventLoop, platform: Platform) -> Result<bool, String> {
    let saved = std::mem::replace(&mut g.bound, resolve(&default_keybindings_for(platform)));
    let m = platform_mod(platform);
    let base = points(g);
    let before = sent(g).len();
    let mut problems = Vec::new();
    let mut expect = |g: &mut Gui, what: &str, want: f32| {
        g.flush(el);
        if (points(g) - want).abs() > 0.01 {
            problems.push(format!("{what}: the font is {} points, expected {want}", points(g)));
        }
        if bell_showing(g) {
            problems.push(format!("{what}: the bell rang"));
        }
    };
    tap_chord(g, &format!("{m}+="), platform)?;
    expect(g, &format!("{m} and ="), base + 1.0);
    tap_chord(g, &format!("{m}+plus"), platform)?;
    expect(g, &format!("{m}, shift and ="), base + 2.0);
    let held = us_key(&termoil_app::keymap::parse_chord(&format!("{m}+="))?, platform).ok_or("key")?;
    feed(g, &held);
    feed(g, &held.clone().with_kind(KeyEventKind::Repeat));
    feed(g, &held.clone().with_kind(KeyEventKind::Repeat));
    feed(g, &release_of(&held));
    expect(g, &format!("{m} and = held for two repeats"), base + 5.0);
    tap_chord(g, &format!("{m}+-"), platform)?;
    expect(g, &format!("{m} and -"), base + 4.0);
    tap_chord(g, &format!("{m}+0"), platform)?;
    expect(g, &format!("{m} and 0"), base);
    let extra = sent_since(g, before);
    if !extra.is_empty() {
        problems.push(format!("the font size keys sent {extra:?} to the shell"));
    }
    g.bound = saved;
    if problems.is_empty() { Ok(true) } else { Err(problems.join("\n")) }
}

/// Writes bytes to the focused pane's terminal as if the program had printed them.
fn program_prints(g: &Gui, bytes: &[u8]) {
    let (_, pane) = focus(g).expect("a pane");
    g.app.panes[&pane].session.lock().advance(bytes);
}

/// K3: with kitty flags 1, 2 and 8 on (every key an escape code, releases reported), a bound key sends neither its press nor its release, and an
/// unbound key sends both.
fn kitty_releases(g: &mut Gui, el: &ActiveEventLoop) -> Result<bool, String> {
    let platform = Platform::current();
    let m = platform_mod(platform);
    program_prints(g, b"\x1b[>11u");
    let before = sent(g).len();
    let base = points(g);
    tap_chord(g, &format!("{m}+="), platform)?;
    tap_chord(g, &format!("{m}+0"), platform)?;
    g.flush(el);
    let bound_sent = sent_since(g, before);
    let before_a = sent(g).len();
    tap(g, &us_typed('a', platform));
    let a_sent = sent_since(g, before_a);
    program_prints(g, b"\x1b[<u");
    tap(g, &RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    if !bound_sent.is_empty() {
        return Err(format!("a bound key under kitty flags 11 sent {bound_sent:?}"));
    }
    if (points(g) - base).abs() > 0.01 {
        return Err(format!("the font size keys under kitty flags left the font at {}", points(g)));
    }
    if a_sent != "\\x1b[97u\\x1b[97;1:3u" {
        return Err(format!("an unbound key under kitty flags 11 sent {a_sent:?}, expected a press and a release"));
    }
    Ok(true)
}

/// K4: Super keys with no binding send nothing to a legacy program, and `+` and `=` typed with no
/// modifier reach the shell.
fn unbound_and_typed(g: &mut Gui) -> Result<bool, String> {
    let platform = Platform::current();
    let before = sent(g).len();
    for keys in ["super+j", "super+enter", "super+shift+=", "super+/"] {
        let chord = termoil_app::keymap::parse_chord(keys)?;
        if g.bound.iter().any(|b| b.chords == [chord.clone()]) {
            return Err(format!("{keys} is bound on this platform; pick another unbound key"));
        }
        let raw = us_key(&chord, platform).ok_or(format!("no US key for {keys}"))?;
        tap(g, &raw);
        let extra = sent_since(g, before);
        if !extra.is_empty() {
            return Err(format!("{keys} with no binding sent {extra:?}"));
        }
    }
    for c in ['+', '=', '>', '_'] {
        tap(g, &us_typed(c, platform));
    }
    let typed = sent_since(g, before);
    if typed != "+=>_" {
        return Err(format!("typing + = > _ sent {typed:?}"));
    }
    Ok(true)
}

/// Waits until the typed text shows on the prompt line, then clears it with Escape.
fn typed_shows_then_clear(g: &mut Gui) -> Result<bool, String> {
    if !screen(g).contains("+=>_") {
        return Ok(false);
    }
    tap(g, &RawKey::new(Key::Named(NamedKey::Escape), Modifiers::NONE));
    Ok(true)
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
        ("K1 every Windows and Linux default binding runs", Box::new(|g, _| every_binding_runs(g, Platform::Windows))),
        ("K1 every macOS default binding runs", Box::new(|g, _| every_binding_runs(g, Platform::MacOs))),
        ("K2 font size keys, Windows and Linux table", Box::new(|g, el| font_size_keys(g, el, Platform::Windows))),
        ("K2 font size keys, macOS table", Box::new(|g, el| font_size_keys(g, el, Platform::MacOs))),
        ("K3 kitty releases of bound keys are not sent", Box::new(kitty_releases)),
        ("K4 unbound Super keys send nothing, typed symbols reach the shell", Box::new(|g, _| unbound_and_typed(g))),
        ("K4 the typed symbols show at the prompt", Box::new(|g, _| typed_shows_then_clear(g))),
        (
            "nothing a shortcut sent shows at the prompt",
            Box::new(|g: &mut Gui, _: &ActiveEventLoop| {
                let text = screen(g);
                for junk in [";9u", ";5u", ";6u", ";10u"] {
                    if text.contains(junk) {
                        return Err(format!("the screen shows {junk:?}:\n{text}"));
                    }
                }
                Ok(true)
            }),
        ),
    ]
}

#[test]
fn e7_keyboard_shortcuts_as_a_us_keyboard_sends_them() {
    if cfg!(target_os = "macos") {
        return println!("winit needs the main thread on macOS; skipping E7");
    }
    let dir = std::env::temp_dir().join(format!("termoil-e7-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    let shell = if cfg!(windows) { "cmd.exe" } else { "bash" };
    let body = format!("[shell]\nprogram = \"{shell}\"\n\n[updates]\ncheck = \"off\"\n");
    // SAFETY: set before the event loop and the front end start any thread that reads the environment.
    unsafe {
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
        winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    }
    let Ok(event_loop) = builder.build() else { return println!("no display; skipping E7") };
    let socket = if cfg!(windows) {
        format!(r"\\.\pipe\termoil-e7-{}", std::process::id())
    } else {
        dir.join("e7.sock").to_string_lossy().into_owned()
    };
    let mut gui = Gui::new(&event_loop, config, config_path, &socket, None, None).unwrap().expect("own instance");
    gui.app.record_input = true;
    let mut h = Harness { gui, steps: steps(), current: 0, started: None, failure: None, finished: false };
    event_loop.run_app(&mut h).unwrap();
    assert!(h.failure.is_none(), "{}", h.failure.unwrap_or_default());
    assert!(h.finished, "the script stopped at step {}", h.current);
}
