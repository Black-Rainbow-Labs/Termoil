//! The body of `App::run_action`: one function per group of catalogue actions. Every row in the
//! catalogue has an arm here; the A3 test runs each one against a headless app.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value, json};
use termoil_protocol::ActionError;

use crate::app::{App, Effect, Outcome, cwd_path, instance_mode};
use crate::model::{Dir, PaneId, neighbour};

/// Runs a catalogue action by name.
pub fn dispatch(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    match name {
        "new-window" | "new-tab" | "split" | "split-right" | "split-down" | "close" => layout_action(app, name, args),
        "focus" | "focus-left" | "focus-right" | "focus-up" | "focus-down" | "next-tab" | "previous-tab"
        | "zoom-pane" | "resize-split" => focus_action(app, name, args),
        "move-tab-to-window" => done(app.arg_pane(args).and_then(|p| move_tab_to_window(app, p))),
        "close-tab" => done(app.arg_pane(args).and_then(|p| close_tab(app, p))),
        "send-text" | "send-key" | "screen" | "scrollback" | "wait-prompt" | "resize" | "set-title"
        | "reset-terminal" => pane_io_action(app, name, args),
        "list"
        | "run-action"
        | "show-config"
        | "reload-config"
        | "open-config"
        | "install-shell-integration"
        | "version"
        | "quit"
        | "save-session"
        | "quick-terminal" => global_action(app, name, args),
        "copy" | "paste" | "select-all" | "clear-selection" => crate::view::clipboard_action(app, name, args),
        "scroll-page-up" | "scroll-page-down" | "scroll-line-up" | "scroll-line-down" | "scroll-to-top"
        | "scroll-to-bottom" | "clear-scrollback" | "previous-prompt" | "next-prompt" | "select-last-output" => {
            crate::view::scroll_action(app, name, args)
        }
        "search" | "search-next" | "search-previous" => crate::view::search_action(app, name, args),
        "activate-key-table" | "deactivate-key-table" | "deactivate-all-key-tables" => {
            done(key_table_action(app, name, args))
        }
        "set-theme" => done(
            req_str(args, "name")
                .map(String::from)
                .and_then(|n| crate::themes::set(app, &n).map(|_| json!({ "theme": n })).map_err(ActionError::Failed)),
        ),
        "list-themes" => done(Ok(list_themes(app))),
        "check-for-updates" | "list-updates" | "install-update" | "skip-update" => {
            crate::update::actions::dispatch(app, name, args)
        }
        "choose-theme" => done(match app.model.focused_index() {
            Some(wi) => {
                app.effects.push(Effect::ShowThemes(app.model.windows[wi].id));
                Ok(Value::Null)
            }
            None => Err(ActionError::NotFound("no window".into())),
        }),
        "open-settings" => done(match app.model.focused_index() {
            Some(wi) => {
                app.effects.push(Effect::ShowSettings(app.model.windows[wi].id));
                Ok(Value::Null)
            }
            None => Err(ActionError::NotFound("no window".into())),
        }),
        "show-hints" => done(app.arg_pane(args).and_then(|p| {
            let win = app.window_of(p).ok_or_else(|| ActionError::NotFound(format!("pane {p} has no window")))?;
            app.effects.push(Effect::ShowHints(win, p));
            Ok(Value::Null)
        })),
        "increase-font-size" | "decrease-font-size" | "reset-font-size" | "toggle-fullscreen" | "command-palette" => {
            window_action(app, name)
        }
        _ => Outcome::err(ActionError::Unknown(name.to_string())),
    }
}

/// A string argument.
pub fn arg_str<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// A required string argument.
pub fn req_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, ActionError> {
    arg_str(args, key).ok_or_else(|| ActionError::BadArgs(format!("--{key} is required")))
}

/// An integer argument, given as a number or a string.
pub fn arg_int(args: &Map<String, Value>, key: &str) -> Result<Option<i64>, ActionError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| ActionError::BadArgs(format!("--{key} must be an integer"))),
    }
}

/// A boolean argument, given as a bool or the strings true and false.
pub fn arg_bool(args: &Map<String, Value>, key: &str) -> bool {
    match args.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.as_str(), "true" | "1" | "yes"),
        _ => false,
    }
}

/// Turns a Result into an Outcome.
fn done(r: Result<Value, ActionError>) -> Outcome {
    Outcome::Done(r)
}

/// new-window, new-tab, split and close.
fn layout_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    done(match name {
        "new-window" if arg_bool(args, "separate") => {
            separate_window(app, arg_str(args, "cwd").map(PathBuf::from), arg_str(args, "command"))
        }
        "new-window" => new_window(app, arg_str(args, "cwd").map(PathBuf::from), arg_str(args, "command")),
        "new-tab" => new_tab(app, args),
        "split" => {
            let dir = match arg_str(args, "direction") {
                None => Dir::Right,
                Some(d) => match Dir::parse(d) {
                    Some(d) => d,
                    None => {
                        return Outcome::err(ActionError::BadArgs(format!(
                            "direction '{d}' is not left, right, up or down"
                        )));
                    }
                },
            };
            app.arg_pane(args).and_then(|p| split(app, p, dir, arg_str(args, "cwd").map(PathBuf::from)))
        }
        "split-right" => app.pane_id(None).and_then(|p| split(app, p, Dir::Right, None)),
        "split-down" => app.pane_id(None).and_then(|p| split(app, p, Dir::Down, None)),
        _ => app.arg_pane(args).map(|p| {
            app.close_pane(p);
            json!({ "closed": p })
        }),
    })
}

/// Starts `termoil-gui --separate` (headless when this instance is), in the given directory or the
/// focused pane's, and answers with the new instance's socket name.
fn separate_window(app: &App, cwd: Option<PathBuf>, command: Option<&str>) -> Result<Value, ActionError> {
    let exe = std::env::current_exe().map_err(|e| ActionError::Failed(e.to_string()))?;
    let cwd = cwd.or_else(|| focused_cwd(app));
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--separate").env_remove("TERMOIL_SOCKET");
    if instance_mode().headless {
        cmd.arg("--headless");
    }
    if let Some(d) = &cwd {
        cmd.arg("--cwd").arg(d);
    }
    if let Some(c) = command {
        cmd.arg("--command").arg(c);
    }
    let child = cmd.spawn().map_err(|e| ActionError::Failed(format!("could not start a separate instance: {e}")))?;
    Ok(json!({ "pid": child.id(), "socket": termoil_protocol::separate_socket_name(child.id()) }))
}

/// Opens a window with one tab.
pub fn new_window(app: &mut App, cwd: Option<PathBuf>, command: Option<&str>) -> Result<Value, ActionError> {
    let grid = app.grid(app.model.focused);
    let pane = app.spawn_pane(grid, cwd, command)?;
    crate::gui::phase("shell_spawned");
    let win = app.model.add_window(pane);
    app.effects.push(Effect::OpenWindow(win));
    Ok(json!({ "window": win, "pane": pane }))
}

/// Opens a tab in the focused window (or a new window when there is none), in the given directory or
/// the focused pane's.
fn new_tab(app: &mut App, args: &Map<String, Value>) -> Result<Value, ActionError> {
    let cwd = arg_str(args, "cwd").map(PathBuf::from).or_else(|| focused_cwd(app));
    let Some(wi) = app.model.focused_index() else {
        return new_window(app, cwd, arg_str(args, "command"));
    };
    let win = app.model.windows[wi].id;
    let pane = app.spawn_pane(app.grid(Some(win)), cwd, arg_str(args, "command"))?;
    let tab = app.model.new_tab(pane, arg_str(args, "name").map(String::from));
    let tab_id = tab.id;
    let w = &mut app.model.windows[wi];
    w.tabs.push(tab);
    w.active = w.tabs.len() - 1;
    app.effects.push(Effect::Relayout(win));
    Ok(json!({ "window": win, "tab": tab_id, "pane": pane }))
}

/// The focused pane's working directory, when the shell reported one.
fn focused_cwd(app: &App) -> Option<PathBuf> {
    let p = app.model.focused_pane()?;
    app.panes.get(&p)?.cwd.as_deref().and_then(cwd_path)
}

/// Splits a pane, starting a new shell in the new half.
fn split(app: &mut App, pane: PaneId, dir: Dir, cwd: Option<PathBuf>) -> Result<Value, ActionError> {
    let at = app.model.locate(pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let win = app.model.windows[at.win].id;
    let mut grid = app.grid(Some(win));
    if matches!(dir, Dir::Left | Dir::Right) {
        grid.cols = (grid.cols / 2).max(2);
    } else {
        grid.rows = (grid.rows / 2).max(1);
    }
    let cwd = cwd.or_else(|| app.panes.get(&pane).and_then(|p| p.cwd.as_deref()).and_then(cwd_path));
    let new = app.spawn_pane(grid, cwd, None)?;
    let tab = &mut app.model.windows[at.win].tabs[at.tab];
    tab.root.split(pane, new, dir);
    tab.focus = new;
    tab.zoomed = None;
    app.effects.push(Effect::Relayout(win));
    Ok(json!({ "pane": new }))
}

/// focus, focus-<dir>, next-tab, previous-tab and zoom-pane.
fn focus_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    let dir = match name {
        "focus-left" => Some(Dir::Left),
        "focus-right" => Some(Dir::Right),
        "focus-up" => Some(Dir::Up),
        "focus-down" => Some(Dir::Down),
        "focus" => arg_str(args, "direction").and_then(Dir::parse),
        _ => None,
    };
    done(match name {
        "next-tab" | "previous-tab" => cycle_tab(app, name == "next-tab"),
        "zoom-pane" => zoom(app),
        "resize-split" => app.arg_pane(args).and_then(|p| resize_split(app, p, args)),
        _ if dir.is_some() => app.arg_pane(args).and_then(|p| focus_dir(app, p, dir.unwrap_or(Dir::Right))),
        _ => app.arg_pane(args).and_then(|p| focus_pane(app, p)),
    })
}

/// Moves the divider of the nearest split beside a pane: right and down move it right or down,
/// left and up move it back, by `percent` of the split (5 by default).
fn resize_split(app: &mut App, pane: PaneId, args: &Map<String, Value>) -> Result<Value, ActionError> {
    let dir = req_str(args, "direction").and_then(|d| {
        Dir::parse(d).ok_or_else(|| ActionError::BadArgs(format!("direction '{d}' is not left, right, up or down")))
    })?;
    let percent = arg_int(args, "percent")?.unwrap_or(5).clamp(1, 80) as f32 / 100.0;
    let (axis, delta) = match dir {
        Dir::Left => (crate::model::Axis::Horizontal, -percent),
        Dir::Right => (crate::model::Axis::Horizontal, percent),
        Dir::Up => (crate::model::Axis::Vertical, -percent),
        Dir::Down => (crate::model::Axis::Vertical, percent),
    };
    let at = app.model.locate(pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let moved = app.model.windows[at.win].tabs[at.tab].root.resize(pane, axis, delta);
    let win = app.model.windows[at.win].id;
    app.effects.push(Effect::Relayout(win));
    Ok(json!({ "moved": moved }))
}

/// Moves the tab holding a pane out of its window into a new window. A window's only tab stays.
fn move_tab_to_window(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let at = app.model.locate(pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let w = &mut app.model.windows[at.win];
    if w.tabs.len() == 1 {
        return Ok(json!({ "window": w.id, "moved": false }));
    }
    let tab = w.tabs.remove(at.tab);
    w.active = w.active.min(w.tabs.len() - 1);
    let old = w.id;
    let id = app.model.next_id();
    app.model.windows.push(crate::model::Window { id, tabs: vec![tab], active: 0 });
    app.model.focused = Some(id);
    app.effects.push(Effect::Relayout(old));
    app.effects.push(Effect::OpenWindow(id));
    Ok(json!({ "window": id, "moved": true }))
}

/// Closes the tab holding `pane`: every pane in it is closed, which ends their shells. Closing the
/// last tab closes the window, as closing its last pane does.
fn close_tab(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let at = app.model.locate(pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let panes = app.model.windows[at.win].tabs[at.tab].root.panes();
    for p in &panes {
        app.close_pane(*p);
    }
    Ok(json!({ "closed": panes }))
}

/// Every bundled theme name for `list-themes`: the themes Unluminous ships first, then the rest by
/// name, with which ones are featured and which one is in effect, so an agent can pick a name for
/// `set-theme` without guessing.
fn list_themes(app: &App) -> Value {
    let featured = crate::config::featured_themes();
    let rest = crate::config::themes().iter().map(|t| t.name.as_str()).filter(|n| !featured.contains(n));
    let all: Vec<&str> = featured.iter().copied().chain(rest).collect();
    json!({ "current": app.config.theme.name, "featured": featured, "themes": all })
}

/// Focuses a pane: its window, its tab, and the pane in the tab.
pub fn focus_pane(app: &mut App, pane: PaneId) -> Result<Value, ActionError> {
    let at = app.model.locate(pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let w = &mut app.model.windows[at.win];
    w.active = at.tab;
    w.tabs[at.tab].focus = pane;
    let win = w.id;
    app.model.focused = Some(win);
    app.effects.push(Effect::Relayout(win));
    Ok(json!({ "pane": pane }))
}

/// Moves focus to the neighbour of `pane` in a direction.
fn focus_dir(app: &mut App, pane: PaneId, dir: Dir) -> Result<Value, ActionError> {
    let at = app.model.locate(pane).ok_or_else(|| ActionError::NotFound(format!("pane {pane}")))?;
    let tab = &app.model.windows[at.win].tabs[at.tab];
    let mut layout = Vec::new();
    tab.root.layout(crate::model::Rect { x: 0.0, y: 0.0, w: 1000.0, h: 1000.0 }, 0.0, &mut layout);
    match neighbour(&layout, pane, dir) {
        Some(n) => focus_pane(app, n),
        None => Ok(json!({ "pane": pane })),
    }
}

/// Switches to the next or previous tab of the focused window.
fn cycle_tab(app: &mut App, forward: bool) -> Result<Value, ActionError> {
    let wi = app.model.focused_index().ok_or_else(|| ActionError::NotFound("no window".into()))?;
    let w = &mut app.model.windows[wi];
    let n = w.tabs.len();
    w.active = if forward { (w.active + 1) % n } else { (w.active + n - 1) % n };
    let (win, tab) = (w.id, w.tab().id);
    app.effects.push(Effect::Relayout(win));
    Ok(json!({ "tab": tab }))
}

/// Shows the focused pane alone, or restores the split.
fn zoom(app: &mut App) -> Result<Value, ActionError> {
    let wi = app.model.focused_index().ok_or_else(|| ActionError::NotFound("no window".into()))?;
    let w = &mut app.model.windows[wi];
    let tab = w.tab_mut();
    tab.zoomed = if tab.zoomed.is_some() { None } else { Some(tab.focus) };
    let zoomed = tab.zoomed.is_some();
    let win = w.id;
    app.effects.push(Effect::Relayout(win));
    Ok(json!({ "zoomed": zoomed }))
}

/// send-text, send-key, screen, scrollback, wait-prompt, resize, set-title and reset-terminal.
fn pane_io_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    let pane = match app.arg_pane(args) {
        Ok(p) => p,
        Err(e) => return Outcome::err(e),
    };
    if name == "wait-prompt" {
        let secs = match arg_int(args, "timeout") {
            Ok(t) => t.unwrap_or(600).clamp(0, 86_400) as u64,
            Err(e) => return Outcome::err(e),
        };
        return Outcome::Wait(app.panes[&pane].session.prompt_waiter(), Duration::from_secs(secs));
    }
    done(match name {
        "send-text" => req_str(args, "text").and_then(|t| crate::view::send_text(app, pane, t)),
        "send-key" => req_str(args, "key").and_then(|k| crate::view::send_key(app, pane, k)),
        "screen" => Ok(crate::view::screen(app, pane, arg_bool(args, "json"))),
        "scrollback" => arg_int(args, "lines")
            .map(|n| crate::view::scrollback(app, pane, n.unwrap_or(500).clamp(1, 10_000_000) as usize)),
        "resize" => resize(app, pane, args),
        "set-title" => req_str(args, "title").map(|t| set_title(app, pane, t)),
        _ => Ok(crate::view::reset_terminal(app, pane)),
    })
}

/// Resizes a pane (and asks the window to fit it).
fn resize(app: &mut App, pane: PaneId, args: &Map<String, Value>) -> Result<Value, ActionError> {
    let cols = arg_int(args, "cols")?.ok_or_else(|| ActionError::BadArgs("--cols is required".into()))?;
    let rows = arg_int(args, "rows")?.ok_or_else(|| ActionError::BadArgs("--rows is required".into()))?;
    let (cols, rows) = (cols.clamp(2, 1000) as u16, rows.clamp(1, 500) as u16);
    let win = app.window_of(pane);
    let grid = app.grid(win);
    app.panes[&pane].session.resize(cols, rows, grid.cell_w, grid.cell_h);
    if let Some(w) = win {
        app.effects.push(Effect::ResizeWindow(w, pane, cols, rows));
    }
    Ok(json!({ "cols": cols, "rows": rows }))
}

/// Sets the title of the tab holding a pane.
fn set_title(app: &mut App, pane: PaneId, title: &str) -> Value {
    if let Some(at) = app.model.locate(pane) {
        app.model.windows[at.win].tabs[at.tab].title = (!title.is_empty()).then(|| title.to_string());
        let win = app.model.windows[at.win].id;
        app.effects.push(Effect::Redraw(win));
    }
    json!({ "title": title })
}

/// Font size, full screen and the palette: effects for the window layer.
fn window_action(app: &mut App, name: &str) -> Outcome {
    let Some(wi) = app.model.focused_index() else {
        return Outcome::err(ActionError::NotFound("no window".into()));
    };
    let win = app.model.windows[wi].id;
    app.effects.push(match name {
        "increase-font-size" => Effect::FontSize(win, Some(1.0)),
        "decrease-font-size" => Effect::FontSize(win, Some(-1.0)),
        "reset-font-size" => Effect::FontSize(win, None),
        "toggle-fullscreen" => Effect::ToggleFullscreen(win),
        _ => Effect::ShowPalette(win),
    });
    Outcome::ok(Value::Null)
}

/// Activates or deactivates key tables, answering with the tables active afterwards. A table
/// activated with `once` ends after the next key (`gui::handle`).
fn key_table_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Result<Value, ActionError> {
    match name {
        "activate-key-table" => {
            let table = req_str(args, "name")?.to_string();
            app.key_tables.push(crate::keymap::ActiveTable { name: table, once: arg_bool(args, "once") });
        }
        "deactivate-key-table" => {
            app.key_tables.pop();
        }
        _ => app.key_tables.clear(),
    }
    Ok(json!({ "active": app.key_tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>() }))
}

/// list, run-action, the config actions, install-shell-integration, version, quit and quick-terminal.
fn global_action(app: &mut App, name: &str, args: &Map<String, Value>) -> Outcome {
    done(match name {
        "list" => Ok(crate::view::list(app)),
        "run-action" => return run_nested(app, args),
        "show-config" => Ok(Value::String(if arg_bool(args, "default") {
            crate::config::Config::default_file_text()
        } else {
            app.config.effective_text()
        })),
        "reload-config" => crate::view::reload_config(app),
        "open-config" => crate::view::open_config(app),
        "install-shell-integration" => install_shell_integration(args),
        "version" => Ok(version_json()),
        "quick-terminal" => {
            app.effects.push(Effect::ToggleQuickTerminal);
            Ok(Value::Null)
        }
        "save-session" => save_session(app, args),
        _ => {
            app.effects.push(Effect::Quit);
            Ok(Value::Null)
        }
    })
}

/// save-session: writes the open windows to the session file (or to `--path`) now.
fn save_session(app: &mut App, args: &Map<String, Value>) -> Result<Value, ActionError> {
    let (path, windows, panes) = crate::restore::save_session(app, arg_str(args, "path").map(PathBuf::from), None)
        .map_err(ActionError::Failed)?;
    Ok(json!({ "path": path.to_string_lossy(), "windows": windows, "panes": panes }))
}

/// run-action: runs another action from its name and a JSON object of arguments.
fn run_nested(app: &mut App, args: &Map<String, Value>) -> Outcome {
    let name = match req_str(args, "name") {
        Ok(n) => n.to_string(),
        Err(e) => return Outcome::err(e),
    };
    if name == "run-action" {
        return Outcome::err(ActionError::BadArgs("run-action cannot run itself".into()));
    }
    let inner = match args.get("args") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(Value::String(s)) if s.trim().is_empty() => Map::new(),
        Some(Value::String(s)) => match serde_json::from_str::<Map<String, Value>>(s) {
            Ok(m) => m,
            Err(e) => return Outcome::err(ActionError::BadArgs(format!("--args is not a JSON object: {e}"))),
        },
        Some(_) => return Outcome::err(ActionError::BadArgs("--args must be a JSON object".into())),
    };
    app.run_action(&termoil_protocol::ActionCall { action: name, args: inner })
}

/// The version and build date.
pub fn version_json() -> Value {
    json!({ "version": termoil_protocol::VERSION, "build_date": option_env!("TERMOIL_BUILD_DATE").unwrap_or("development build") })
}

/// install-shell-integration: copies the scripts and hooks the named shells' startup files.
pub fn install_shell_integration(args: &Map<String, Value>) -> Result<Value, ActionError> {
    use crate::shell_integration::{Shell, detect, install, source_dir};
    let shells: Vec<Shell> = match arg_str(args, "shells") {
        Some(list) => list.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(detect).collect(),
        None => vec![detect(
            &termoil_pty::SpawnOptions::default_shell(termoil_pty::PtySize {
                cols: 80,
                rows: 24,
                pixel_width: 0,
                pixel_height: 0,
            })
            .program,
        )],
    };
    if let Some(bad) = shells.iter().position(|s| *s == Shell::Unknown) {
        let given = arg_str(args, "shells").unwrap_or_default();
        return Err(ActionError::BadArgs(format!("unknown shell in '{given}' (item {})", bad + 1)));
    }
    let dir = source_dir().ok_or_else(|| {
        ActionError::Failed("the shell-integration folder was not found beside the executable".into())
    })?;
    let lines = install(&shells, &dir, arg_bool(args, "dry-run")).map_err(ActionError::Failed)?;
    Ok(json!({ "changes": lines }))
}
