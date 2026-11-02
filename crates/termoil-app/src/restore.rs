//! Session restore (`docs/PRD.md` section 5.4): the open windows, tabs, splits, working directories
//! and recent output written to `session.json` on a clean quit, and read back on the next start.
//!
//! Pane output is saved as text with SGR sequences that reproduce colours and attributes. The
//! alternate screen (a full screen program such as an editor) is not saved. Every function takes the
//! app or the file as a parameter; nothing here keeps state of its own.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::{App, Effect, Grid, cwd_path};
use crate::model::{Axis, Node, PaneId, Tab, WinId, Window};

/// The format version written to the file. A file with another version is not restored.
const FORMAT_VERSION: u32 = 1;
/// The most windows, panes and levels of splits a file may hold, so a damaged file cannot start
/// thousands of shells.
const MAX_WINDOWS: usize = 64;
const MAX_PANES: usize = 512;
const MAX_DEPTH: usize = 32;

/// Where a window is on screen, in physical pixels: the outer position and the inner size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    /// Outer left edge.
    pub x: i32,
    /// Outer top edge.
    pub y: i32,
    /// Inner width.
    pub width: u32,
    /// Inner height.
    pub height: u32,
}

/// The whole file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionFile {
    /// `FORMAT_VERSION`.
    pub version: u32,
    /// Index into `windows` of the focused window.
    pub focused: usize,
    /// The windows in creation order.
    pub windows: Vec<SavedWindow>,
}

/// One window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    /// Position and size, when the window layer knew them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<WindowGeometry>,
    /// Index of the active tab.
    pub active: usize,
    /// Tabs in order.
    pub tabs: Vec<SavedTab>,
}

/// One tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    /// The title set with `set-title` or `new-tab --name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Index, in left to right and top to bottom order, of the focused pane.
    pub focus: usize,
    /// Index of the zoomed pane, when one was zoomed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoomed: Option<usize>,
    /// The split tree.
    pub root: SavedNode,
}

/// A node of a saved split tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SavedNode {
    /// One pane.
    Pane {
        /// The working directory (the OSC 7 directory, or the one the pane started in).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// The command line when the pane did not run the configured shell.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// The last lines of history and screen as text with SGR sequences, rows joined by CR LF.
        #[serde(default)]
        text: String,
    },
    /// Two children sharing the space.
    Split {
        /// `horizontal` (side by side) or `vertical` (one above the other).
        axis: String,
        /// The first child's share, 0.1 to 0.9.
        ratio: f32,
        /// The left or top child.
        first: Box<SavedNode>,
        /// The right or bottom child.
        second: Box<SavedNode>,
    },
}

/// The session file's path in the data folder.
pub fn session_path() -> Option<PathBuf> {
    crate::paths::data_dir().map(|d| d.join("session.json"))
}

/// Captures one pane as a saved leaf.
fn save_pane(app: &App, id: PaneId) -> SavedNode {
    let Some(p) = app.panes.get(&id) else { return SavedNode::Pane { cwd: None, command: None, text: String::new() } };
    let lines = app.config.session.scrollback_lines.min(1_000_000) as usize;
    let text = crate::inspect::scrollback_ansi(&p.session.lock(), lines);
    let cwd = p.cwd.as_deref().and_then(cwd_path).map(|d| d.to_string_lossy().into_owned());
    SavedNode::Pane { cwd, command: p.command.clone(), text }
}

/// Captures a split tree.
fn save_node(app: &App, node: &Node) -> SavedNode {
    match node {
        Node::Leaf(id) => save_pane(app, *id),
        Node::Split { axis, ratio, first, second } => SavedNode::Split {
            axis: if *axis == Axis::Horizontal { "horizontal" } else { "vertical" }.into(),
            ratio: *ratio,
            first: Box::new(save_node(app, first)),
            second: Box::new(save_node(app, second)),
        },
    }
}

/// Captures a tab.
fn save_tab(app: &App, tab: &Tab) -> SavedTab {
    let order = tab.root.panes();
    SavedTab {
        title: tab.title.clone(),
        focus: order.iter().position(|p| *p == tab.focus).unwrap_or(0),
        zoomed: tab.zoomed.and_then(|z| order.iter().position(|p| *p == z)),
        root: save_node(app, &tab.root),
    }
}

/// Captures every window except `skip` (the quick terminal, which is not part of a session).
pub fn build_file(app: &App, skip: Option<WinId>) -> SessionFile {
    let wins: Vec<&Window> = app.model.windows.iter().filter(|w| Some(w.id) != skip).collect();
    let focused = wins.iter().position(|w| Some(w.id) == app.model.focused).unwrap_or(wins.len().saturating_sub(1));
    let windows = wins
        .iter()
        .map(|w| SavedWindow {
            geometry: app.geometry.get(&w.id).copied(),
            active: w.active,
            tabs: w.tabs.iter().map(|t| save_tab(app, t)).collect(),
        })
        .collect();
    SessionFile { version: FORMAT_VERSION, focused, windows }
}

/// Writes the file atomically: to a temporary file beside it, then renamed over it.
pub fn write_file(path: &Path, file: &SessionFile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, json).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("could not replace {}: {e}", path.display()))
}

/// Saves the app's windows to `path` (the session file by default) and returns the path and counts.
pub fn save_session(app: &App, path: Option<PathBuf>, skip: Option<WinId>) -> Result<(PathBuf, usize, usize), String> {
    let path = path.or_else(session_path).ok_or("no data folder is available to save the session in")?;
    let file = build_file(app, skip);
    let panes = app.model.all_panes().len();
    write_file(&path, &file)?;
    Ok((path, file.windows.len(), panes))
}

/// Saves the session on a clean quit when `session.restore` is on and a window is open. Failures
/// are reported on stderr; quitting goes on.
pub fn save_on_quit(app: &App, skip: Option<WinId>) {
    if !app.config.session.restore || app.model.windows.iter().all(|w| Some(w.id) == skip) {
        return;
    }
    if let Err(e) = save_session(app, None, skip) {
        eprintln!("termoil: could not save the session: {e}");
    }
}

/// Reads and checks a session file.
pub fn load_file(path: &Path) -> Result<SessionFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let file: SessionFile =
        serde_json::from_str(&text).map_err(|e| format!("{} is not a session file: {e}", path.display()))?;
    if file.version != FORMAT_VERSION {
        return Err(format!("{} has format version {}, expected {FORMAT_VERSION}", path.display(), file.version));
    }
    if file.windows.is_empty() {
        return Err(format!("{} holds no windows", path.display()));
    }
    if file.windows.len() > MAX_WINDOWS {
        return Err(format!(
            "{} holds {} windows, more than the limit of {MAX_WINDOWS}",
            path.display(),
            file.windows.len()
        ));
    }
    Ok(file)
}

/// The text fed to a restored pane before its shell starts: the saved output, then a dim line that
/// marks where the old session ends, then one screen of blank lines so the saved output has scrolled
/// into history before the shell starts (a shell or the Windows console may clear the visible screen
/// when it starts, and history is not cleared). A pane with no saved output gets nothing.
fn history_bytes(text: &str, rows: u16) -> Vec<u8> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let pad = "\r\n".repeat(rows as usize);
    format!("{text}\x1b[0m\r\n\x1b[2m-- restored --\x1b[0m{pad}").into_bytes()
}

/// Starts the panes of a saved tree and rebuilds its split tree. Every pane started is added to
/// `started`, so the caller can end them when a later pane fails.
fn restore_node(
    app: &mut App, grid: Grid, saved: &SavedNode, depth: usize, started: &mut Vec<PaneId>,
) -> Result<Node, String> {
    if depth > MAX_DEPTH || started.len() >= MAX_PANES {
        return Err("the session has too many panes or splits".into());
    }
    match saved {
        SavedNode::Pane { cwd, command, text } => {
            let cwd = cwd.as_deref().map(PathBuf::from).filter(|d| d.is_dir());
            let id = app
                .spawn_pane_with_history(grid, cwd, command.as_deref(), history_bytes(text, grid.rows))
                .map_err(|e| e.to_string())?;
            started.push(id);
            Ok(Node::Leaf(id))
        }
        SavedNode::Split { axis, ratio, first, second } => {
            let axis = if axis == "vertical" { Axis::Vertical } else { Axis::Horizontal };
            let ratio = if ratio.is_finite() { ratio.clamp(0.1, 0.9) } else { 0.5 };
            let first = Box::new(restore_node(app, grid, first, depth + 1, started)?);
            let second = Box::new(restore_node(app, grid, second, depth + 1, started)?);
            Ok(Node::Split { axis, ratio, first, second })
        }
    }
}

/// Rebuilds one tab.
fn restore_tab(app: &mut App, grid: Grid, saved: &SavedTab, started: &mut Vec<PaneId>) -> Result<Tab, String> {
    let root = restore_node(app, grid, &saved.root, 0, started)?;
    let order = root.panes();
    let focus = order.get(saved.focus).copied().unwrap_or(order[0]);
    let zoomed = saved.zoomed.and_then(|z| order.get(z).copied());
    Ok(Tab { id: app.model.next_id(), root, focus, title: saved.title.clone(), zoomed })
}

/// Rebuilds one window and queues the effect that opens it.
fn restore_window(app: &mut App, saved: &SavedWindow, started: &mut Vec<PaneId>) -> Result<WinId, String> {
    let grid = app.grid(None);
    let mut tabs = Vec::new();
    for t in &saved.tabs {
        tabs.push(restore_tab(app, grid, t, started)?);
    }
    if tabs.is_empty() {
        return Err("a window in the session has no tabs".into());
    }
    let id = app.model.next_id();
    let active = saved.active.min(tabs.len() - 1);
    app.model.windows.push(Window { id, tabs, active });
    if let Some(g) = saved.geometry {
        app.geometry.insert(id, g);
    }
    app.effects.push(Effect::OpenWindow(id));
    Ok(id)
}

/// Rebuilds every window of `file` in `app` and starts their shells. When anything fails, the panes
/// and windows already started are removed and the error is returned.
pub fn restore_file(app: &mut App, file: &SessionFile) -> Result<usize, String> {
    let mut started = Vec::new();
    let mut ids = Vec::new();
    for w in &file.windows {
        match restore_window(app, w, &mut started) {
            Ok(id) => ids.push(id),
            Err(e) => {
                abandon(app, &started, &ids);
                return Err(e);
            }
        }
    }
    app.model.focused = ids.get(file.focused).or(ids.last()).copied();
    Ok(ids.len())
}

/// Ends panes started for a restore that failed, and forgets the windows it added.
fn abandon(app: &mut App, started: &[PaneId], windows: &[WinId]) {
    for id in started {
        app.close_pane(*id);
    }
    app.model.windows.retain(|w| !windows.contains(&w.id));
    app.effects.retain(|e| {
        !matches!(e, Effect::OpenWindow(_) | Effect::CloseWindow(_) | Effect::Relayout(_) | Effect::LastWindowClosed)
    });
}

/// Restores the session file at `path` when it exists. The file is removed before any shell starts,
/// so a crash during or after the restore cannot restore it a second time; a file that cannot be
/// read is renamed to `session.json.bad` and the reason is printed. Returns true when windows were
/// restored.
pub fn restore_from(app: &mut App, path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let loaded = load_file(path);
    let mut bad = path.as_os_str().to_owned();
    bad.push(".bad");
    match loaded {
        Ok(file) => {
            let _ = std::fs::remove_file(path);
            match restore_file(app, &file) {
                Ok(n) => n > 0,
                Err(e) => {
                    eprintln!("termoil: could not restore the session: {e}");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("termoil: starting without the saved session: {e}");
            let _ = std::fs::rename(path, PathBuf::from(bad));
            false
        }
    }
}

/// Restores the session file in the data folder when `session.restore` is on. Returns true when
/// windows were restored.
pub fn restore_on_start(app: &mut App) -> bool {
    if !app.config.session.restore {
        return false;
    }
    match session_path() {
        Some(path) => restore_from(app, &path),
        None => false,
    }
}
