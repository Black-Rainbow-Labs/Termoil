//! The tab bar, drawn the way Unluminous draws its file tabs with the Black Rainbow Labs rux style
//! (task-2468): the tab that is showing is a rounded tab in the theme's control colour with an accent
//! line along its bottom, a tab under the pointer gets a quieter fill, the others sit flat on the bar
//! with a thin divider between them, every tab has a close cross, and a plus button after the last tab
//! opens a new one.
//!
//! The titles are terminal text in a [`crate::gui::chrome::Strip`], so they get the same shaping and
//! fallback as everything else. The tabs, crosses and the plus are [`Shape`]s drawn between the cell
//! backgrounds and the text. Everything is laid out in whole columns so a title starts on a cell.

use terminus_render::{Area, Rgba, Shape};
use terminus_vt::style::Rgb;

use crate::config::{Chrome, TabBar};
use crate::gui::chrome::sgr_colors;

/// Columns the new tab button takes.
const NEW_TAB_COLS: u16 = 3;
/// Columns at the right end of a tab that hold its close cross (two) and the space after it (one).
const CLOSE_COLS: u16 = 3;
/// The narrowest a tab is ever drawn, in columns: one space, two title cells, the close cross.
const SMALLEST_TAB: u16 = 6;

/// One tab's place in the bar, in columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabSpan {
    /// First column.
    pub start: u16,
    /// One past the last column.
    pub end: u16,
}

impl TabSpan {
    /// The first of the two columns the close cross is drawn over.
    pub fn close(&self) -> u16 {
        self.end.saturating_sub(CLOSE_COLS)
    }
}

/// Where everything in the tab bar is, in columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabLayout {
    /// The tabs that fit, in order.
    pub tabs: Vec<TabSpan>,
    /// The first column of the new tab button, when it fits.
    pub new_tab: Option<u16>,
}

/// What a point in the tab bar is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabHit {
    /// A tab, by index, away from its close cross.
    Tab(usize),
    /// A tab's close cross.
    Close(usize),
    /// The new tab button.
    NewTab,
}

/// What the bar needs to know to draw: the theme's chrome, the cell size, the bar's height, the display
/// scale, the `[tab_bar]` sizes and whether the window has the keyboard.
pub struct Look<'a> {
    pub chrome: &'a Chrome,
    pub cell: (u32, u32),
    pub height: f32,
    pub scale: f32,
    pub sizes: &'a TabBar,
    pub focused: bool,
}

/// Lays out `count` tabs across `cols` columns: equal widths between `min_columns` and `max_columns`,
/// narrower when even the minimum does not fit, and the tabs past the right edge left off.
pub fn layout(count: usize, cols: u16, sizes: &TabBar) -> TabLayout {
    let room = cols.saturating_sub(NEW_TAB_COLS);
    let n = count.max(1) as u16;
    let mut each = (room / n).clamp(sizes.min_columns.min(sizes.max_columns), sizes.max_columns.max(SMALLEST_TAB));
    if each * n > room {
        each = (room / n).max(SMALLEST_TAB);
    }
    let mut tabs = Vec::new();
    for i in 0..count as u16 {
        let start = i * each;
        if start + each > room {
            break;
        }
        tabs.push(TabSpan { start, end: start + each });
    }
    let after = tabs.last().map_or(0, |t| t.end);
    let new_tab = (after + NEW_TAB_COLS <= cols).then_some(after);
    TabLayout { tabs, new_tab }
}

/// What the column under window x is: a tab, its close cross, or the new tab button.
pub fn hit(layout: &TabLayout, x: f64, cell_w: u32) -> Option<TabHit> {
    let col = (x.max(0.0) / cell_w.max(1) as f64) as u16;
    if let Some(i) = layout.tabs.iter().position(|t| col >= t.start && col < t.end) {
        let close = layout.tabs[i].close();
        return Some(if col >= close && col < close + 2 { TabHit::Close(i) } else { TabHit::Tab(i) });
    }
    match layout.new_tab {
        Some(c) if col >= c && col < c + NEW_TAB_COLS => Some(TabHit::NewTab),
        _ => None,
    }
}

/// Converts a theme colour.
fn rgb(c: [u8; 3]) -> Rgb {
    Rgb::new(c[0], c[1], c[2])
}

/// Converts a theme colour to an opaque shape colour.
fn rgba(c: [u8; 3]) -> Rgba {
    Rgba::rgb(c[0], c[1], c[2])
}

/// A colour `t` of the way from `a` to `b`.
fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// The fill of a tab under the pointer. On a dark bar it is between the bar and the tab that is
/// showing. On a light bar the tab that is showing is lighter than the bar, so a step between them is
/// nearly white and reads as a second tab that is showing; there it is a step towards the text instead.
pub fn hover_fill(chrome: &Chrome) -> [u8; 3] {
    if crate::config::relative_luminance(chrome.bar) > 0.5 {
        mix(chrome.bar, chrome.text, 0.07)
    } else {
        mix(chrome.bar, chrome.tab, 0.55)
    }
}

/// The bar's text: the bar colour across the row, then each title in its tab, in the strong colour on the
/// tab that is showing and the ordinary one elsewhere.
pub fn text(titles: &[String], active: usize, layout: &TabLayout, chrome: &Chrome) -> Vec<u8> {
    let bar = rgb(chrome.bar);
    let mut out = format!("{}\x1b[K", sgr_colors(rgb(chrome.text), bar));
    for (i, span) in layout.tabs.iter().enumerate() {
        let room = span.close().saturating_sub(span.start + 1) as usize;
        let title = titles.get(i).map(String::as_str).filter(|t| !t.is_empty()).unwrap_or("Terminus");
        let fg = if i == active { chrome.text_strong } else { chrome.text };
        out.push_str(&format!("\x1b[{}G{}{}", span.start + 2, sgr_colors(rgb(fg), bar), fit(title, room)));
    }
    out.into_bytes()
}

/// Cuts a title to `max` characters, ending in an ellipsis when cut.
pub fn fit(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.truncate(t.trim_end().len());
    t.push('\u{2026}');
    t
}

/// The bar's shapes: each tab's fill, the accent line under the one that is showing, the dividers
/// between quiet tabs, each close cross, and the new tab button.
pub fn shapes(layout: &TabLayout, active: usize, hover: Option<TabHit>, look: &Look<'_>) -> Vec<Shape> {
    let mut out = Vec::new();
    for (i, span) in layout.tabs.iter().enumerate() {
        let hovered = matches!(hover, Some(TabHit::Tab(h) | TabHit::Close(h)) if h == i);
        tab_fill(&mut out, *span, i == active, hovered, look);
        let quiet = |j: usize| j != active && !matches!(hover, Some(TabHit::Tab(h) | TabHit::Close(h)) if h == j);
        if quiet(i) && layout.tabs.get(i + 1).is_some() && quiet(i + 1) {
            divider(&mut out, span.end, look);
        }
        let cross = if i == active { look.chrome.text } else { look.chrome.text_dim };
        close_cross(&mut out, *span, cross, hover == Some(TabHit::Close(i)), look);
    }
    if let Some(col) = layout.new_tab {
        new_tab_button(&mut out, col, hover == Some(TabHit::NewTab), look);
    }
    out
}

/// A tab's rounded fill: the control colour with a border and the accent line for the tab that is
/// showing, the hover fill for one under the pointer, nothing for the rest.
fn tab_fill(out: &mut Vec<Shape>, span: TabSpan, active: bool, hovered: bool, look: &Look<'_>) {
    let (cw, s) = (look.cell.0 as f32, look.scale);
    let inset = look.sizes.inset * s;
    let area = Area {
        x0: span.start as f32 * cw + inset,
        y0: inset,
        x1: span.end as f32 * cw - inset,
        y1: look.height - inset,
    };
    let radius = look.sizes.corner_radius * s;
    if active {
        out.push(Shape::RoundRect { area, radius, color: rgba(look.chrome.border) });
        let inner = Area { x0: area.x0 + s, y0: area.y0 + s, x1: area.x1 - s, y1: area.y1 - s };
        out.push(Shape::RoundRect { area: inner, radius: (radius - s).max(0.0), color: rgba(look.chrome.tab) });
        let accent = if look.focused { look.chrome.accent } else { mix(look.chrome.tab, look.chrome.accent, 0.35) };
        let line = look.sizes.underline * s;
        let under =
            Area { x0: area.x0 + radius, y0: area.y1 - line - 2.0 * s, x1: area.x1 - radius, y1: area.y1 - 2.0 * s };
        out.push(Shape::RoundRect { area: under, radius: line / 2.0, color: rgba(accent) });
    } else if hovered {
        out.push(Shape::RoundRect { area, radius, color: rgba(hover_fill(look.chrome)) });
    }
}

/// The thin line between two quiet tabs, at column `col`.
fn divider(out: &mut Vec<Shape>, col: u16, look: &Look<'_>) {
    let x = col as f32 * look.cell.0 as f32;
    let (top, bottom) = (look.height * 0.32, look.height * 0.68);
    out.push(Shape::Line {
        from: (x, top),
        to: (x, bottom),
        width: look.scale.max(1.0),
        color: rgba(look.chrome.border),
    });
}

/// The close cross over a tab's close columns: a round fill in the close colour under the pointer, with
/// the cross drawn in the strong colour on it.
fn close_cross(out: &mut Vec<Shape>, span: TabSpan, color: [u8; 3], hovered: bool, look: &Look<'_>) {
    let (cw, ch) = (look.cell.0 as f32, look.cell.1 as f32);
    let (cx, cy) = ((span.close() + 1) as f32 * cw, look.height / 2.0);
    let color = if hovered {
        let r = (ch * 0.42).min(cw * 0.95);
        out.push(Shape::RoundRect {
            area: Area { x0: cx - r, y0: cy - r, x1: cx + r, y1: cy + r },
            radius: r,
            color: rgba(look.chrome.close),
        });
        [255; 3]
    } else {
        color
    };
    let arm = (ch * 0.17).max(3.0);
    let width = (1.4 * look.scale).max(1.2);
    out.push(Shape::Line { from: (cx - arm, cy - arm), to: (cx + arm, cy + arm), width, color: rgba(color) });
    out.push(Shape::Line { from: (cx - arm, cy + arm), to: (cx + arm, cy - arm), width, color: rgba(color) });
}

/// The new tab button at column `col`: a plus, on the hover fill when the pointer is over it.
fn new_tab_button(out: &mut Vec<Shape>, col: u16, hovered: bool, look: &Look<'_>) {
    let (cw, ch, s) = (look.cell.0 as f32, look.cell.1 as f32, look.scale);
    let (cx, cy) = ((col as f32 + NEW_TAB_COLS as f32 / 2.0) * cw, look.height / 2.0);
    if hovered {
        let r = ch * 0.55;
        let area = Area { x0: cx - r, y0: cy - r, x1: cx + r, y1: cy + r };
        out.push(Shape::RoundRect { area, radius: look.sizes.corner_radius * s, color: rgba(hover_fill(look.chrome)) });
    }
    let color = if hovered { look.chrome.text_strong } else { look.chrome.text_dim };
    let arm = (ch * 0.24).max(4.0);
    let width = (1.5 * s).max(1.2);
    out.push(Shape::Line { from: (cx - arm, cy), to: (cx + arm, cy), width, color: rgba(color) });
    out.push(Shape::Line { from: (cx, cy - arm), to: (cx, cy + arm), width, color: rgba(color) });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default `[tab_bar]` sizes.
    fn sizes() -> TabBar {
        TabBar::default()
    }

    #[test]
    fn tabs_share_the_width_up_to_the_maximum() {
        let l = layout(2, 120, &sizes());
        assert_eq!(l.tabs, vec![TabSpan { start: 0, end: 32 }, TabSpan { start: 32, end: 64 }]);
        assert_eq!(l.new_tab, Some(64));
    }

    #[test]
    fn many_tabs_shrink_and_the_ones_past_the_edge_are_left_off() {
        let l = layout(30, 80, &sizes());
        assert!(l.tabs.iter().all(|t| t.end - t.start >= SMALLEST_TAB));
        assert!(l.tabs.len() < 30);
        assert!(l.tabs.last().unwrap().end + NEW_TAB_COLS <= 80);
    }

    #[test]
    fn a_click_on_the_cross_is_a_close_and_elsewhere_a_tab() {
        let l = layout(2, 120, &sizes());
        let cw = 10;
        assert_eq!(hit(&l, 5.0, cw), Some(TabHit::Tab(0)));
        let close = l.tabs[1].close();
        assert_eq!(hit(&l, (close as f64 + 0.5) * 10.0, cw), Some(TabHit::Close(1)));
        assert_eq!(hit(&l, 65.0 * 10.0, cw), Some(TabHit::NewTab));
        assert_eq!(hit(&l, 100.0 * 10.0, cw), None);
    }

    #[test]
    fn a_long_title_ends_in_an_ellipsis() {
        assert_eq!(fit("abcdef", 4), "abc\u{2026}");
        assert_eq!(fit("abc", 4), "abc");
        assert_eq!(fit("ab cdef", 4), "ab\u{2026}", "no space before the ellipsis");
    }
}
