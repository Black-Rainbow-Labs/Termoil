//! The application core: the model, the panes and their sessions, and `App::run_action`, the one
//! place an action becomes a change (`docs/TDD.md` section 4.9).
//!
//! `App` knows nothing about winit. What the window layer must do after an action (open or close a
//! window, redraw, put text on the clipboard) is queued as an [`Effect`] and drained by the caller,
//! so the same code runs under the GUI and under the headless tests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value};
use terminus_protocol::{ActionCall, ActionError};
use terminus_vt::TerminalConfig;
use terminus_vt::pagelist::ScrollbackLimit;
use terminus_vt::search::SearchMatch;
use terminus_vt::selection::Selection;

use crate::config::Config;
use crate::model::{Model, PaneId, WinId};
use crate::session::{PromptWaiter, Session, SessionOptions, Waker};

/// Something the window layer does after an action.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Create the window for a model window.
    OpenWindow(WinId),
    /// Destroy a window.
    CloseWindow(WinId),
    /// Draw a window again.
    Redraw(WinId),
    /// Lay a window out again (panes added, removed or zoomed) and redraw it.
    Relayout(WinId),
    /// Put text on the clipboard.
    SetClipboard(String),
    /// Toggle full screen.
    ToggleFullscreen(WinId),
    /// Change the font size by points, or reset it with None.
    FontSize(WinId, Option<f32>),
    /// Size a window so a pane gets this many columns and rows.
    ResizeWindow(WinId, PaneId, u16, u16),
    /// Open the command palette.
    ShowPalette(WinId),
    /// Open the search bar.
    ShowSearch(WinId),
    /// Show or hide the quick terminal.
    ToggleQuickTerminal,
    /// Open a file with the platform opener.
    OpenPath(PathBuf),
    /// The configuration changed; reapply fonts, colours and bindings.
    ConfigChanged,
    /// The last window closed. The window runtime exits; the headless runtime keeps serving IPC.
    LastWindowClosed,
    /// Exit the process.
    Quit,
}

/// Reads and writes the system clipboard.
pub trait Clipboard: Send {
    /// The clipboard's text.
    fn get(&mut self) -> Option<String>;
    /// Replaces the clipboard's text.
    fn set(&mut self, text: &str);
}

/// A clipboard that holds text in memory, for headless use.
#[derive(Default)]
pub struct MemoryClipboard(pub Option<String>);

impl Clipboard for MemoryClipboard {
    /// The stored text.
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }

    /// Stores the text.
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

/// The size of a window's cells, so new sessions start at the right size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Cell width in pixels.
    pub cell_w: u16,
    /// Cell height in pixels.
    pub cell_h: u16,
}

/// Search state of a pane.
#[derive(Clone, Debug)]
pub struct PaneSearch {
    /// The pattern.
    pub pattern: String,
    /// Match case.
    pub case_sensitive: bool,
    /// The selected match.
    pub current: Option<SearchMatch>,
}

/// One pane: a session and what the view of it adds (scroll position, selection, search).
pub struct Pane {
    /// Its id.
    pub id: PaneId,
    /// The session.
    pub session: Session,
    /// Rows the view is scrolled above the bottom.
    pub scroll: usize,
    /// The selection.
    pub selection: Option<Selection>,
    /// The selection was started by a single click and has not been dragged yet.
    pub selection_empty: bool,
    /// The search.
    pub search: Option<PaneSearch>,
    /// The title from the program.
    pub title: String,
    /// The working directory from OSC 7.
    pub cwd: Option<String>,
}

/// What `run_action` produced.
pub enum Outcome {
    /// Finished.
    Done(Result<Value, ActionError>),
    /// Wait on another thread for the shell to settle, then answer with the exit code.
    Wait(PromptWaiter, Duration),
}

impl Outcome {
    /// A successful result.
    pub fn ok(v: Value) -> Outcome {
        Outcome::Done(Ok(v))
    }

    /// A failed result.
    pub fn err(e: ActionError) -> Outcome {
        Outcome::Done(Err(e))
    }
}

/// Makes a waker for a new pane.
pub type WakerFactory = Box<dyn Fn(PaneId) -> Waker + Send>;

/// The application core.
pub struct App {
    /// Windows, tabs and splits.
    pub model: Model,
    /// Panes by id.
    pub panes: HashMap<PaneId, Pane>,
    /// The configuration in effect.
    pub config: Config,
    /// The configuration file's path.
    pub config_path: PathBuf,
    /// The last configuration error, shown as a banner.
    pub config_error: Option<String>,
    /// Effects for the window layer.
    pub effects: Vec<Effect>,
    /// Cell geometry of each window.
    pub grids: HashMap<WinId, Grid>,
    /// The clipboard.
    pub clipboard: Box<dyn Clipboard>,
    wakers: WakerFactory,
}

impl App {
    /// An app with no windows.
    pub fn new(config: Config, config_path: PathBuf, clipboard: Box<dyn Clipboard>, wakers: WakerFactory) -> App {
        App {
            model: Model::default(),
            panes: HashMap::new(),
            config,
            config_path,
            config_error: None,
            effects: Vec::new(),
            grids: HashMap::new(),
            clipboard,
            wakers,
        }
    }

    /// Takes the queued effects.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// The grid of a window, or the configured default size.
    pub fn grid(&self, win: Option<WinId>) -> Grid {
        win.and_then(|w| self.grids.get(&w).copied()).unwrap_or(Grid {
            cols: self.config.window.columns.clamp(2, 1000) as u16,
            rows: self.config.window.rows.clamp(1, 500) as u16,
            cell_w: 8,
            cell_h: 16,
        })
    }

    /// Starts a session for a new pane and returns its id. `cwd` and `command` override the
    /// configured shell.
    pub fn spawn_pane(
        &mut self, grid: Grid, cwd: Option<PathBuf>, command: Option<&str>,
    ) -> Result<PaneId, ActionError> {
        let id = self.model.next_id();
        let opts = self.session_options(grid, cwd, command);
        let waker = (self.wakers)(id);
        let session =
            Session::spawn(opts, waker).map_err(|e| ActionError::Failed(format!("could not start the shell: {e}")))?;
        session.resize(grid.cols, grid.rows, grid.cell_w, grid.cell_h);
        let cwd = session.start_cwd().map(|p| p.to_string_lossy().into_owned());
        self.panes.insert(
            id,
            Pane {
                id,
                session,
                scroll: 0,
                selection: None,
                selection_empty: false,
                search: None,
                title: String::new(),
                cwd,
            },
        );
        Ok(id)
    }

    /// The session options from the configuration.
    fn session_options(&self, grid: Grid, cwd: Option<PathBuf>, command: Option<&str>) -> SessionOptions {
        let mut opts = SessionOptions::shell(grid.cols, grid.rows);
        let shell = &self.config.shell;
        if let Some(cmd) = command {
            let mut words = split_command(cmd).into_iter();
            opts.spawn.program = words.next().unwrap_or_default();
            opts.spawn.args = words.collect();
        } else if let Some(p) = &shell.program {
            opts.spawn.program = p.clone();
            opts.spawn.args = shell.args.clone();
        }
        opts.spawn.cwd = cwd.or_else(|| shell.cwd.as_ref().map(PathBuf::from)).filter(|p| p.is_dir()).or_else(home_dir);
        opts.spawn.env.extend(shell.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        opts.spawn.env.push(("TERMINUS_SOCKET".into(), terminus_protocol::socket_name()));
        if shell.integration && command.is_none() {
            inject_shell_integration(&mut opts.spawn);
        }
        opts.terminal = self.terminal_config();
        opts
    }

    /// The terminal settings from the configuration.
    pub fn terminal_config(&self) -> TerminalConfig {
        let mut t = TerminalConfig::default();
        let sb = &self.config.scrollback;
        t.scrollback = ScrollbackLimit { bytes: sb.limit_bytes as usize, lines: sb.limit_lines.map(|l| l as usize) };
        t.allow_osc52_read = self.config.clipboard.allow_osc52_read;
        t
    }

    /// A pane by id, or the focused pane when `id` is None.
    pub fn pane_id(&self, id: Option<PaneId>) -> Result<PaneId, ActionError> {
        let id = match id {
            Some(id) => id,
            None => self.model.focused_pane().ok_or_else(|| ActionError::NotFound("no pane is open".into()))?,
        };
        if self.panes.contains_key(&id) { Ok(id) } else { Err(ActionError::NotFound(format!("pane {id}"))) }
    }

    /// The pane named by the `pane` argument, or the focused pane.
    pub fn arg_pane(&self, args: &Map<String, Value>) -> Result<PaneId, ActionError> {
        let id = match args.get("pane") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| ActionError::BadArgs("pane must be a pane id".into()))?,
            ),
        };
        self.pane_id(id)
    }

    /// The window holding a pane.
    pub fn window_of(&self, pane: PaneId) -> Option<WinId> {
        self.model.locate(pane).map(|at| self.model.windows[at.win].id)
    }

    /// Queues a redraw of the window holding a pane.
    pub fn redraw_pane(&mut self, pane: PaneId) {
        if let Some(w) = self.window_of(pane) {
            self.effects.push(Effect::Redraw(w));
        }
    }

    /// Ends a pane's session and removes it, closing its tab and window when they empty.
    pub fn close_pane(&mut self, pane: PaneId) {
        let win = self.window_of(pane);
        if let Some(mut p) = self.panes.remove(&pane) {
            if let Some(sel) = p.selection.take() {
                sel.release(&mut p.session.lock().screen_mut().pages);
            }
            p.session.kill();
        }
        match self.model.remove_pane(pane) {
            Some(closed) => {
                self.grids.remove(&closed);
                self.effects.push(Effect::CloseWindow(closed));
                if self.model.windows.is_empty() {
                    self.effects.push(Effect::LastWindowClosed);
                }
            }
            None => {
                if let Some(w) = win {
                    self.effects.push(Effect::Relayout(w));
                }
            }
        }
    }

    /// Runs an action from a keybinding, the palette, the CLI or IPC.
    pub fn run_action(&mut self, call: &ActionCall) -> Outcome {
        if terminus_protocol::action(&call.action).is_none() {
            return Outcome::err(ActionError::Unknown(call.action.clone()));
        }
        crate::actions::dispatch(self, &call.action, &call.args)
    }

    /// Runs an action by name with no arguments and returns its result, for keybindings.
    pub fn run_named(&mut self, name: &str) -> Result<Value, ActionError> {
        match self.run_action(&ActionCall { action: name.into(), args: Map::new() }) {
            Outcome::Done(r) => r,
            Outcome::Wait(..) => Ok(Value::Null),
        }
    }
}

/// Starts the shell with the integration scripts loaded, when the shell has them and they are
/// installed beside the executable or in the data folder.
fn inject_shell_integration(spawn: &mut terminus_pty::SpawnOptions) {
    use crate::shell_integration::{detect, injection, source_dir};
    let Some(dir) = source_dir() else { return };
    if let Some(inj) = injection(detect(&spawn.program), &dir, &spawn.args) {
        spawn.args = inj.args;
        spawn.env.extend(inj.env);
    }
}

/// Splits a command line into words, honouring double quotes.
pub fn split_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in cmd.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// The user's home directory.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Turns an OSC 7 value (`file://host/path` or a plain path) into a local path.
pub fn cwd_path(value: &str) -> Option<PathBuf> {
    let path = match value.strip_prefix("file://") {
        Some(rest) => {
            let p = &rest[rest.find('/')?..];
            let decoded = percent_decode(p);
            if cfg!(windows) && decoded.len() > 2 && decoded.as_bytes()[2] == b':' {
                decoded[1..].to_string()
            } else {
                decoded
            }
        }
        None => value.to_string(),
    };
    let p = PathBuf::from(path);
    p.is_dir().then_some(p)
}

/// Decodes `%xx` escapes.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_commands() {
        assert_eq!(split_command(r#"cmd /c "echo hi" x"#), vec!["cmd", "/c", "echo hi", "x"]);
        assert_eq!(split_command("  "), Vec::<String>::new());
        assert_eq!(split_command(r#""""#), vec![""]);
    }

    #[test]
    fn decodes_percent() {
        assert_eq!(percent_decode("/C:/a%20b"), "/C:/a b");
        assert_eq!(percent_decode("%2"), "%2");
    }
}
