//! Windows, tabs and split trees as plain data (`docs/TDD.md` section 4.8). A window holds tabs; a
//! tab holds a split tree whose leaves are panes. Nothing here knows about winit or sessions, so the
//! layout rules are tested without a window.

use serde::Serialize;

/// A pane's id, stable for its life and unique in the process.
pub type PaneId = u64;
/// A tab's id.
pub type TabId = u64;
/// A window's id (ours, not winit's).
pub type WinId = u64;

/// How a split divides its space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Axis {
    /// Side by side.
    Horizontal,
    /// One above the other.
    Vertical,
}

/// A direction for splitting and moving focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Left.
    Left,
    /// Right.
    Right,
    /// Up.
    Up,
    /// Down.
    Down,
}

impl Dir {
    /// Parses `left`, `right`, `up` or `down`.
    pub fn parse(s: &str) -> Option<Dir> {
        match s {
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            _ => None,
        }
    }

    /// The axis a split in this direction uses.
    fn axis(self) -> Axis {
        if matches!(self, Dir::Left | Dir::Right) { Axis::Horizontal } else { Axis::Vertical }
    }
}

/// A node of a split tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// One pane.
    Leaf(PaneId),
    /// Two children sharing the space; `ratio` is the first child's share.
    Split { axis: Axis, ratio: f32, first: Box<Node>, second: Box<Node> },
}

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
pub struct Rect {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// The centre point.
    fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

impl Node {
    /// The panes in order, left to right and top to bottom.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    /// Appends the panes in order.
    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(p) => out.push(*p),
            Node::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    /// Replaces the leaf `target` with a split holding it and `new`, on the side `dir`.
    pub fn split(&mut self, target: PaneId, new: PaneId, dir: Dir) -> bool {
        match self {
            Node::Leaf(p) if *p == target => {
                let (a, b) = if matches!(dir, Dir::Left | Dir::Up) { (new, target) } else { (target, new) };
                *self = Node::Split {
                    axis: dir.axis(),
                    ratio: 0.5,
                    first: Box::new(Node::Leaf(a)),
                    second: Box::new(Node::Leaf(b)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split(target, new, dir) || second.split(target, new, dir),
        }
    }

    /// Removes the leaf `target`, letting its sibling take the space. False when it is the only leaf
    /// or not found.
    pub fn remove(&mut self, target: PaneId) -> bool {
        let Node::Split { first, second, .. } = self else { return false };
        if **first == Node::Leaf(target) {
            *self = (**second).clone();
            return true;
        }
        if **second == Node::Leaf(target) {
            *self = (**first).clone();
            return true;
        }
        first.remove(target) || second.remove(target)
    }

    /// Lays the tree out in `area`, with `gap` pixels between siblings.
    pub fn layout(&self, area: Rect, gap: f32, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf(p) => out.push((*p, area)),
            Node::Split { axis, ratio, first, second } => {
                let (a, b) = split_rect(area, *axis, *ratio, gap);
                first.layout(a, gap, out);
                second.layout(b, gap, out);
            }
        }
    }

    /// Moves the divider of the nearest split above `target` on `axis` by `delta` (a share of the
    /// split's size), keeping both sides at least 10 % of it.
    pub fn resize(&mut self, target: PaneId, axis_wanted: Axis, delta: f32) -> bool {
        match self {
            Node::Leaf(_) => false,
            Node::Split { axis, ratio, first, second } => {
                let in_first = first.panes().contains(&target);
                let in_second = !in_first && second.panes().contains(&target);
                if !in_first && !in_second {
                    return false;
                }
                let child = if in_first { first } else { second };
                if child.resize(target, axis_wanted, delta) {
                    return true;
                }
                if *axis != axis_wanted {
                    return false;
                }
                *ratio = (*ratio + delta).clamp(0.1, 0.9);
                true
            }
        }
    }
}

/// Splits a rectangle in two along an axis.
fn split_rect(r: Rect, axis: Axis, ratio: f32, gap: f32) -> (Rect, Rect) {
    match axis {
        Axis::Horizontal => {
            let w1 = ((r.w - gap) * ratio).floor().max(0.0);
            (Rect { w: w1, ..r }, Rect { x: r.x + w1 + gap, w: (r.w - w1 - gap).max(0.0), ..r })
        }
        Axis::Vertical => {
            let h1 = ((r.h - gap) * ratio).floor().max(0.0);
            (Rect { h: h1, ..r }, Rect { y: r.y + h1 + gap, h: (r.h - h1 - gap).max(0.0), ..r })
        }
    }
}

/// Finds the pane whose rectangle is the nearest neighbour of `from` in direction `dir`.
pub fn neighbour(layout: &[(PaneId, Rect)], from: PaneId, dir: Dir) -> Option<PaneId> {
    let (_, r) = layout.iter().find(|(p, _)| *p == from)?;
    let (cx, cy) = r.centre();
    let candidates = layout.iter().filter(|(p, o)| {
        *p != from
            && match dir {
                Dir::Left => o.x + o.w <= r.x + 0.5 && overlaps(o.y, o.h, r.y, r.h),
                Dir::Right => o.x >= r.x + r.w - 0.5 && overlaps(o.y, o.h, r.y, r.h),
                Dir::Up => o.y + o.h <= r.y + 0.5 && overlaps(o.x, o.w, r.x, r.w),
                Dir::Down => o.y >= r.y + r.h - 0.5 && overlaps(o.x, o.w, r.x, r.w),
            }
    });
    candidates
        .min_by(|(_, a), (_, b)| {
            let da = dist(a.centre(), (cx, cy));
            let db = dist(b.centre(), (cx, cy));
            da.total_cmp(&db)
        })
        .map(|(p, _)| *p)
}

/// True when two spans overlap.
fn overlaps(a: f32, al: f32, b: f32, bl: f32) -> bool {
    a < b + bl && b < a + al
}

/// Squared distance between points.
fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// A tab.
#[derive(Clone, Debug)]
pub struct Tab {
    /// Its id.
    pub id: TabId,
    /// The split tree.
    pub root: Node,
    /// The focused pane.
    pub focus: PaneId,
    /// A title set by `set-title` or `new-tab --name`; the focused pane's title otherwise.
    pub title: Option<String>,
    /// The pane shown alone, when zoomed.
    pub zoomed: Option<PaneId>,
}

/// A window.
#[derive(Clone, Debug)]
pub struct Window {
    /// Its id.
    pub id: WinId,
    /// Tabs in order.
    pub tabs: Vec<Tab>,
    /// Index of the active tab.
    pub active: usize,
}

impl Window {
    /// The active tab.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    /// The active tab, mutably.
    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// The tab holding a pane.
    pub fn tab_of(&self, pane: PaneId) -> Option<usize> {
        self.tabs.iter().position(|t| t.root.panes().contains(&pane))
    }
}

/// All windows, plus id counters.
#[derive(Debug, Default)]
pub struct Model {
    /// Windows in creation order.
    pub windows: Vec<Window>,
    /// The focused window.
    pub focused: Option<WinId>,
    next_id: u64,
}

/// Where a pane lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneAt {
    /// Window index.
    pub win: usize,
    /// Tab index.
    pub tab: usize,
}

impl Model {
    /// A new id for a window, tab or pane.
    pub fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Adds a window with one tab holding `pane`; returns the window id.
    pub fn add_window(&mut self, pane: PaneId) -> WinId {
        let id = self.next_id();
        let tab = self.new_tab(pane, None);
        self.windows.push(Window { id, tabs: vec![tab], active: 0 });
        self.focused = Some(id);
        id
    }

    /// A tab holding one pane.
    pub fn new_tab(&mut self, pane: PaneId, title: Option<String>) -> Tab {
        Tab { id: self.next_id(), root: Node::Leaf(pane), focus: pane, title, zoomed: None }
    }

    /// The index of a window by id.
    pub fn window_index(&self, id: WinId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// The focused window's index, or the last window.
    pub fn focused_index(&self) -> Option<usize> {
        self.focused.and_then(|id| self.window_index(id)).or_else(|| self.windows.len().checked_sub(1))
    }

    /// The focused pane of the focused window.
    pub fn focused_pane(&self) -> Option<PaneId> {
        let w = &self.windows[self.focused_index()?];
        Some(w.tab().focus)
    }

    /// Where a pane is.
    pub fn locate(&self, pane: PaneId) -> Option<PaneAt> {
        self.windows.iter().enumerate().find_map(|(wi, w)| w.tab_of(pane).map(|ti| PaneAt { win: wi, tab: ti }))
    }

    /// Every pane in every window.
    pub fn all_panes(&self) -> Vec<PaneId> {
        self.windows.iter().flat_map(|w| w.tabs.iter().flat_map(|t| t.root.panes())).collect()
    }

    /// Removes a pane from its tab, the tab when it was the last pane, and the window when it was the
    /// last tab. Returns the window id when the window went away.
    pub fn remove_pane(&mut self, pane: PaneId) -> Option<WinId> {
        let at = self.locate(pane)?;
        let w = &mut self.windows[at.win];
        let tab = &mut w.tabs[at.tab];
        if tab.root.remove(pane) {
            if tab.zoomed == Some(pane) {
                tab.zoomed = None;
            }
            if tab.focus == pane {
                tab.focus = tab.root.panes()[0];
            }
            return None;
        }
        w.tabs.remove(at.tab);
        if w.tabs.is_empty() {
            let id = w.id;
            self.windows.remove(at.win);
            if self.focused == Some(id) {
                self.focused = self.windows.last().map(|w| w.id);
            }
            return Some(id);
        }
        w.active = w.active.min(w.tabs.len() - 1);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rectangle for the tests.
    fn area() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 }
    }

    #[test]
    fn split_and_layout() {
        let mut n = Node::Leaf(1);
        assert!(n.split(1, 2, Dir::Right));
        assert!(n.split(2, 3, Dir::Down));
        assert_eq!(n.panes(), vec![1, 2, 3]);
        let mut out = Vec::new();
        n.layout(area(), 0.0, &mut out);
        assert_eq!(out[0].1, Rect { x: 0.0, y: 0.0, w: 500.0, h: 600.0 });
        assert_eq!(out[1].1, Rect { x: 500.0, y: 0.0, w: 500.0, h: 300.0 });
        assert_eq!(out[2].1, Rect { x: 500.0, y: 300.0, w: 500.0, h: 300.0 });
        assert_eq!(neighbour(&out, 1, Dir::Right), Some(2));
        assert_eq!(neighbour(&out, 3, Dir::Up), Some(2));
        assert_eq!(neighbour(&out, 3, Dir::Left), Some(1));
        assert_eq!(neighbour(&out, 1, Dir::Left), None);
    }

    #[test]
    fn remove_collapses_split() {
        let mut n = Node::Leaf(1);
        n.split(1, 2, Dir::Left);
        assert_eq!(n.panes(), vec![2, 1]);
        assert!(n.remove(2));
        assert_eq!(n, Node::Leaf(1));
        assert!(!n.remove(1));
    }

    #[test]
    fn model_removes_tab_then_window() {
        let mut m = Model::default();
        let w = m.add_window(10);
        let t = m.new_tab(11, None);
        m.windows[0].tabs.push(t);
        assert_eq!(m.remove_pane(10), None);
        assert_eq!(m.windows[0].tabs.len(), 1);
        assert_eq!(m.remove_pane(11), Some(w));
        assert!(m.windows.is_empty());
    }

    #[test]
    fn resize_clamps() {
        let mut n = Node::Leaf(1);
        n.split(1, 2, Dir::Right);
        assert!(n.resize(1, Axis::Horizontal, 0.9));
        let Node::Split { ratio, .. } = n else { panic!() };
        assert_eq!(ratio, 0.9);
    }
}
