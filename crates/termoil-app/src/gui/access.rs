//! Screen reader access (PRD 5.4): the window as an AccessKit tree. AccessKit turns the tree into
//! UI Automation on Windows, NSAccessibility on macOS and AT-SPI on Linux. The focused pane is a
//! `Terminal` node whose children are one `TextRun` per screen row, with the text selection set to
//! the cursor, so a screen reader can read the current line and follow the output. The other panes
//! are `Terminal` nodes that have a name and no text.

use std::time::{Duration, Instant};

use accesskit::{Action, Node, NodeId, Rect as AkRect, Role, TextPosition, TextSelection, Tree, TreeId, TreeUpdate};
use termoil_vt::Terminal;
use termoil_vt::cell::Wide;
use winit::window::Window;

use crate::app::App;
use crate::model::{PaneId, Rect, WinId};

/// One screen cluster: its text and how many cells it covers.
pub type Cluster = (String, u8);

/// The visible rows of a pane as clusters, and where the cursor is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneText {
    /// One list of clusters per visible row, trailing blanks removed.
    pub lines: Vec<Vec<Cluster>>,
    /// The cursor as (row, cluster index) when it is on the visible rows.
    pub cursor: Option<(usize, usize)>,
}

/// What the tree needs to know about a window besides the focused pane's text.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    /// The window title.
    pub title: String,
    /// Each visible pane, its title and its rectangle in window pixels.
    pub panes: Vec<(PaneId, String, Rect)>,
    /// The focused pane.
    pub focus: PaneId,
    /// Cell width and height in pixels.
    pub cell: (f32, f32),
}

/// The node id of the window.
const WINDOW_NODE: NodeId = NodeId(0);

/// The node id of a pane.
pub fn pane_node(pane: PaneId) -> NodeId {
    NodeId(pane << 16)
}

/// The node id of one text row of a pane.
pub fn row_node(pane: PaneId, row: usize) -> NodeId {
    NodeId((pane << 16) | (row as u64 + 1).min(0xffff))
}

/// The pane a node id belongs to, whether it is the pane or one of its rows.
pub fn pane_of(node: NodeId) -> PaneId {
    node.0 >> 16
}

/// Reads the visible rows of a terminal as clusters, `scroll` rows above the bottom as the pane
/// is drawn. The row of the cursor is padded with blanks up to the cursor so the caret has a
/// character to stand at.
pub fn capture(term: &Terminal, scroll: usize) -> PaneText {
    let pages = &term.screen().pages;
    let top = pages.active_y(0).saturating_sub(scroll);
    let rows = term.rows() as usize;
    let c = &term.screen().cursor;
    let cursor_abs = pages.active_y(c.row);
    let cursor_row = cursor_abs.checked_sub(top).filter(|r| *r < rows);
    let cursor_col = c.col as usize;
    let mut lines: Vec<Vec<Cluster>> = (0..rows).map(|r| pages.with_row(top + r, row_clusters)).collect();
    let cursor = cursor_row.map(|r| (r, place_caret(&mut lines[r], cursor_col)));
    PaneText { lines, cursor }
}

/// One row as clusters: a cluster for each cell that holds text or a blank, wide characters as one
/// cluster of two cells, trailing blanks removed.
fn row_clusters(page: &termoil_vt::page::Page, i: usize) -> Vec<Cluster> {
    let mut out: Vec<Cluster> = Vec::new();
    for (col, cell) in page.cells(i).iter().enumerate() {
        match cell.wide() {
            Wide::SpacerTail | Wide::SpacerHead => continue,
            _ => {}
        }
        let mut text = String::new();
        match cell.char() {
            Some(ch) => {
                text.push(ch);
                text.extend(page.grapheme(page.cell_index(i, col)));
            }
            None => text.push(' '),
        }
        out.push((text, if cell.wide() == Wide::Wide { 2 } else { 1 }));
    }
    while out.last().is_some_and(|(t, _)| t == " ") {
        out.pop();
    }
    out
}

/// Pads `line` with blanks so the cell `col` is covered, and returns the index of the cluster the
/// caret stands before.
pub fn place_caret(line: &mut Vec<Cluster>, col: usize) -> usize {
    let mut cells = 0usize;
    for (i, (_, w)) in line.iter().enumerate() {
        if cells >= col {
            return i;
        }
        cells += *w as usize;
    }
    while cells < col {
        line.push((" ".to_string(), 1));
        cells += 1;
    }
    line.len()
}

/// Builds the whole tree for a window: the window node, every pane, and the rows of the focused
/// pane with the caret. Every update is a full tree, so rows that no longer exist disappear.
pub fn build_update(win: &WindowInfo, text: &PaneText) -> TreeUpdate {
    let mut nodes: Vec<(NodeId, Node)> = Vec::new();
    let mut root = Node::new(Role::Window);
    root.set_label(win.title.clone());
    root.set_children(win.panes.iter().map(|(id, _, _)| pane_node(*id)).collect::<Vec<_>>());
    nodes.push((WINDOW_NODE, root));
    for (id, title, rect) in &win.panes {
        let mut node = Node::new(Role::Terminal);
        node.set_label(title.clone());
        node.set_bounds(AkRect {
            x0: rect.x as f64,
            y0: rect.y as f64,
            x1: (rect.x + rect.w) as f64,
            y1: (rect.y + rect.h) as f64,
        });
        node.add_action(Action::Focus);
        if *id == win.focus {
            fill_text_node(&mut node, &mut nodes, *id, rect, win.cell, text);
        }
        nodes.push((pane_node(*id), node));
    }
    let focus = if win.panes.iter().any(|(id, _, _)| *id == win.focus) { pane_node(win.focus) } else { WINDOW_NODE };
    let mut tree = Tree::new(WINDOW_NODE);
    tree.toolkit_name = Some("Termoil".to_string());
    TreeUpdate { nodes, tree: Some(tree), tree_id: TreeId::ROOT, focus }
}

/// Gives the focused pane's node its row children and the caret, and adds the rows to `nodes`.
fn fill_text_node(
    node: &mut Node, nodes: &mut Vec<(NodeId, Node)>, pane: PaneId, rect: &Rect, cell: (f32, f32), text: &PaneText,
) {
    node.set_read_only();
    let mut children = Vec::with_capacity(text.lines.len());
    for (row, line) in text.lines.iter().enumerate() {
        let id = row_node(pane, row);
        children.push(id);
        nodes.push((id, row_run(line, rect, cell, row)));
    }
    node.set_children(children);
    if let Some((row, index)) = text.cursor {
        let at = TextPosition { node: row_node(pane, row), character_index: index };
        node.set_text_selection(TextSelection { anchor: at, focus: at });
    }
}

/// One screen row as a text run: the text and a newline, the byte length of each character, the
/// start of each word, and the position of each character within the row.
fn row_run(line: &[Cluster], rect: &Rect, cell: (f32, f32), row: usize) -> Node {
    let mut run = Node::new(Role::TextRun);
    let mut value = String::new();
    let mut lengths: Vec<u8> = Vec::with_capacity(line.len() + 1);
    let mut positions: Vec<f32> = Vec::with_capacity(line.len() + 1);
    let mut widths: Vec<f32> = Vec::with_capacity(line.len() + 1);
    let mut cells = 0usize;
    for (text, w) in line {
        value.push_str(text);
        lengths.push(text.len().min(255) as u8);
        positions.push(cells as f32 * cell.0);
        widths.push(*w as f32 * cell.0);
        cells += *w as usize;
    }
    value.push('\n');
    lengths.push(1);
    positions.push(cells as f32 * cell.0);
    widths.push(0.0);
    let y = rect.y as f64 + row as f64 * cell.1 as f64;
    run.set_bounds(AkRect {
        x0: rect.x as f64,
        y0: y,
        x1: rect.x as f64 + cells as f64 * cell.0 as f64,
        y1: y + cell.1 as f64,
    });
    run.set_word_starts(word_starts(line));
    run.set_value(value);
    run.set_character_lengths(lengths);
    run.set_character_positions(positions);
    run.set_character_widths(widths);
    run
}

/// The character indices where a word begins: a character that is not a blank after a blank or at
/// the start of the row. Indices past 255 are left out because AccessKit stores them as bytes.
fn word_starts(line: &[Cluster]) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut prev_blank = true;
    for (i, (text, _)) in line.iter().enumerate() {
        let blank = text.trim().is_empty();
        if prev_blank && !blank && i < 256 {
            starts.push(i as u8);
        }
        prev_blank = blank;
    }
    starts
}

/// What to do with a freshly read window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Nothing changed.
    Skip,
    /// Send the tree now.
    Send,
    /// Only the text changed and the last update was too recent: try again at this time.
    Wait(Instant),
}

/// Decides whether to send a tree. A change of focus, panes or titles is sent at once; a change
/// of text only when `interval` has passed since the last update. `force` sends in any case, for
/// the first tree a screen reader asks for.
pub fn decide(
    last: Option<&(WindowInfo, PaneText)>, last_sent: Option<Instant>, now_state: (&WindowInfo, &PaneText),
    interval: Duration, force: bool, now: Instant,
) -> Decision {
    let structure_same = last.is_some_and(|(i, _)| i == now_state.0);
    let text_same = last.is_some_and(|(_, t)| t == now_state.1);
    if force {
        return Decision::Send;
    }
    if structure_same && text_same {
        return Decision::Skip;
    }
    match last_sent.map(|t| t + interval) {
        Some(due) if structure_same && due > now => Decision::Wait(due),
        _ => Decision::Send,
    }
}

/// The accessibility state of one window: the adapter, and what was last sent.
pub struct AccessState {
    /// The AccessKit adapter for the winit window.
    pub adapter: accesskit_winit::Adapter,
    /// When the last tree update was sent.
    pub last_sent: Option<Instant>,
    /// What the last tree update described.
    pub last: Option<(WindowInfo, PaneText)>,
    /// A screen reader or other accessibility client has asked for the tree. Until then the
    /// adapter sends nothing, so the window does not capture its text (task-2481: the capture made
    /// a string per cell on every frame).
    pub requested: bool,
}

impl AccessState {
    /// Creates the adapter for a window that is not shown yet.
    pub fn new(adapter: accesskit_winit::Adapter) -> AccessState {
        AccessState { adapter, last_sent: None, last: None, requested: false }
    }

    /// Sends the tree when it changed, as `decide` says. Returns when to try again when a text
    /// change was held back.
    pub fn push(
        &mut self, info: WindowInfo, text: PaneText, interval: Duration, force: bool, now: Instant,
    ) -> Option<Instant> {
        match decide(self.last.as_ref(), self.last_sent, (&info, &text), interval, force, now) {
            Decision::Skip => None,
            Decision::Wait(due) => Some(due),
            Decision::Send => {
                self.adapter.update_if_active(|| build_update(&info, &text));
                self.last_sent = Some(now);
                self.last = Some((info, text));
                None
            }
        }
    }

    /// Passes a window event to the adapter; it must see every event before the window handles it.
    pub fn process_event(&mut self, window: &Window, event: &winit::event::WindowEvent) {
        self.adapter.process_event(window, event);
    }
}

/// What the tree needs from the app for window `win`: the panes of the active tab with their
/// titles, and the text of the focused pane. None when the window has no panes.
pub fn snapshot(app: &App, win: WinId, layout: &[(PaneId, Rect)], cell: (u32, u32)) -> Option<(WindowInfo, PaneText)> {
    let wi = app.model.window_index(win)?;
    let tab = app.model.windows[wi].tab();
    let panes = layout
        .iter()
        .filter_map(|(id, rect)| app.panes.get(id).map(|p| (*id, pane_name(p), *rect)))
        .collect::<Vec<_>>();
    let focused = app.panes.get(&tab.focus)?;
    let text = capture(&focused.session.lock(), focused.scroll);
    let title = crate::view::tab_label(app, tab);
    let title = if title.is_empty() { "Termoil".to_string() } else { title };
    Some((WindowInfo { title, panes, focus: tab.focus, cell: (cell.0 as f32, cell.1 as f32) }, text))
}

/// The name a screen reader gives a pane: its title, or "Terminal" when the program set none.
fn pane_name(p: &crate::app::Pane) -> String {
    if p.title.is_empty() { "Terminal".to_string() } else { p.title.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane text from plain lines, one cluster per character.
    fn text(lines: &[&str], cursor: Option<(usize, usize)>) -> PaneText {
        let lines = lines.iter().map(|l| l.chars().map(|c| (c.to_string(), 1)).collect()).collect();
        PaneText { lines, cursor }
    }

    /// A window with one pane of 8 by 16 pixel cells.
    fn info(panes: &[(PaneId, &str)], focus: PaneId) -> WindowInfo {
        let rect = Rect { x: 0.0, y: 0.0, w: 640.0, h: 320.0 };
        WindowInfo {
            title: "shell".to_string(),
            panes: panes.iter().map(|(i, t)| (*i, t.to_string(), rect)).collect(),
            focus,
            cell: (8.0, 16.0),
        }
    }

    /// Finds a node by id in an update.
    fn node(u: &TreeUpdate, id: NodeId) -> &Node {
        &u.nodes.iter().find(|(i, _)| *i == id).expect("node").1
    }

    /// The focused pane is a Terminal with a run per row whose text is the screen, and the
    /// selection is a caret at the cursor.
    #[test]
    fn focused_pane_has_the_text_and_a_caret() {
        let t = text(&["C:\\>dir", "file.txt", "C:\\>"], Some((2, 4)));
        let u = build_update(&info(&[(1, "cmd")], 1), &t);
        assert_eq!(u.focus, pane_node(1));
        assert_eq!(node(&u, WINDOW_NODE).role(), Role::Window);
        let pane = node(&u, pane_node(1));
        assert_eq!(pane.role(), Role::Terminal);
        assert_eq!(pane.label(), Some("cmd"));
        assert_eq!(pane.children().len(), 3);
        let mut all = String::new();
        for id in pane.children() {
            let run = node(&u, *id);
            assert_eq!(run.role(), Role::TextRun);
            all.push_str(run.value().unwrap());
        }
        assert_eq!(all, "C:\\>dir\nfile.txt\nC:\\>\n");
        let sel = pane.text_selection().expect("caret");
        assert_eq!(sel.anchor, sel.focus);
        assert_eq!(sel.focus.node, row_node(1, 2));
        assert_eq!(sel.focus.character_index, 4);
        let run = node(&u, row_node(1, 0));
        assert_eq!(run.character_lengths().len(), "C:\\>dir".len() + 1);
        assert_eq!(run.word_starts(), &[0]);
    }

    /// Other panes are named nodes with no text, and focus stays on the focused pane.
    #[test]
    fn other_panes_have_a_name_and_no_rows() {
        let t = text(&["one"], Some((0, 3)));
        let u = build_update(&info(&[(1, "a"), (2, "b")], 2), &t);
        assert_eq!(u.focus, pane_node(2));
        assert!(node(&u, pane_node(1)).children().is_empty());
        assert_eq!(node(&u, pane_node(1)).label(), Some("a"));
        assert_eq!(node(&u, pane_node(2)).children().len(), 1);
        assert_eq!(node(&u, WINDOW_NODE).children(), &[pane_node(1), pane_node(2)]);
        assert_eq!(pane_of(row_node(2, 7)), 2);
    }

    /// The caret goes past trailing blanks by padding the row, and wide characters count once.
    #[test]
    fn caret_follows_blanks_and_wide_characters() {
        let mut line: Vec<Cluster> = vec![("a".into(), 1), ("\u{4e2d}".into(), 2), ("b".into(), 1)];
        assert_eq!(place_caret(&mut line, 0), 0);
        assert_eq!(place_caret(&mut line, 3), 2);
        assert_eq!(place_caret(&mut line, 4), 3);
        assert_eq!(place_caret(&mut line, 6), 5);
        assert_eq!(line.len(), 5);
        let multi: Vec<Cluster> = vec![("\u{4e2d}".into(), 2)];
        assert_eq!(
            row_run(&multi, &Rect { x: 0.0, y: 0.0, w: 80.0, h: 16.0 }, (8.0, 16.0), 0).character_lengths(),
            &[3, 1]
        );
    }

    /// Capturing a real terminal reads the screen text and puts the caret after the prompt.
    #[test]
    fn capture_reads_a_terminal() {
        let mut t = Terminal::new(20, 4, termoil_vt::TerminalConfig::default());
        t.advance(b"one\r\ntwo\r\n> ");
        let p = capture(&t, 0);
        assert_eq!(p.lines.len(), 4);
        let row = |i: usize| p.lines[i].iter().map(|(s, _)| s.as_str()).collect::<String>();
        assert_eq!(row(0), "one");
        assert_eq!(row(1), "two");
        assert_eq!(p.cursor, Some((2, 2)));
        assert_eq!(row(2), "> ");
        assert_eq!(p.lines[2].len(), 2);
    }

    /// A change of text inside the interval is held back; a change of focus, or the first tree, is
    /// not.
    #[test]
    fn update_throttling_distinguishes_text_from_structure() {
        let t0 = Instant::now();
        let gap = Duration::from_millis(250);
        let a = info(&[(1, "a"), (2, "b")], 1);
        let ta = text(&["x"], Some((0, 1)));
        let held = (a.clone(), ta.clone());
        let soon = t0 + Duration::from_millis(10);
        assert_eq!(decide(None, None, (&a, &ta), gap, false, t0), Decision::Send);
        assert_eq!(decide(Some(&held), Some(t0), (&a, &ta), gap, false, soon), Decision::Skip);
        let tb = text(&["xy"], Some((0, 2)));
        assert_eq!(decide(Some(&held), Some(t0), (&a, &tb), gap, false, soon), Decision::Wait(t0 + gap));
        assert_eq!(decide(Some(&held), Some(t0), (&a, &tb), gap, false, t0 + gap), Decision::Send);
        let moved = info(&[(1, "a"), (2, "b")], 2);
        assert_eq!(decide(Some(&held), Some(t0), (&moved, &ta), gap, false, soon), Decision::Send);
        assert_eq!(decide(Some(&held), Some(t0), (&a, &ta), gap, true, soon), Decision::Send);
    }
}
