//! The tab bar (task-2468, redesigned in task-2487). The bar is a shade darker than the terminal (lighter
//! on a light theme). The tab that is showing is filled with the terminal's own background and runs down
//! into the terminal below it, with rounded top corners and a thin accent line along its top while the
//! window has the keyboard. The other tabs sit flat on the bar with a dim title and a short divider
//! between them, and get a rounded fill under the pointer. A tab shows its close cross only when it is
//! showing or under the pointer. The new tab button is a plus at the right end of the bar.
//!
//! The titles are terminal text in a [`crate::gui::chrome::Strip`], so they get the same shaping and
//! fallback as everything else. The tabs, crosses and the plus are [`Shape`]s drawn between the cell
//! backgrounds and the text. Tabs are laid out in whole columns so a title starts on a cell.

use termoil_render::{Area, Rgba, Shape};
use termoil_vt::style::Rgb;

use crate::config::{Chrome, TabBar};
use crate::gui::chrome::sgr_colors;

/// Columns the new tab button takes.
const NEW_TAB_COLS: u16 = 3;
/// Columns kept empty between the last tab and the new tab button.
const NEW_TAB_GAP: u16 = 1;
/// The column the first tab starts at. Each tab is drawn half a column left of its columns, so this leaves
/// half a column of bar before the first tab.
const FIRST_COL: u16 = 1;
/// Columns at the right end of a tab that hold its close cross (two) and the space after it (one).
const CLOSE_COLS: u16 = 3;
/// The narrowest a tab is ever drawn, in columns: one space, two title cells, the close cross.
const SMALLEST_TAB: u16 = 6;

/// One tab's place in the bar, in columns. The title starts one column after `start`; the tab's shape is
/// drawn half a column left of its columns, so the title and the close cross have the same space on each
/// side.
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
    /// The first column of the new tab button, which sits at the right end of the bar, when it fits.
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

/// What the bar needs to know to draw: the theme's chrome, the terminal's background, the cell size, the
/// bar's width and height in pixels, the display scale, the `[tab_bar]` settings and whether the window has
/// the keyboard.
pub struct Look<'a> {
    pub chrome: &'a Chrome,
    pub background: [u8; 3],
    pub cell: (u32, u32),
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub sizes: &'a TabBar,
    pub focused: bool,
}

/// Lays out `count` tabs across `cols` columns: equal widths between `min_columns` and `max_columns` from
/// the left, narrower when even the minimum does not fit, the tabs past the room left off, and the new tab
/// button in the last columns of the bar.
pub fn layout(count: usize, cols: u16, sizes: &TabBar) -> TabLayout {
    let new_tab = (cols >= NEW_TAB_COLS).then(|| cols - NEW_TAB_COLS);
    let room = cols.saturating_sub(NEW_TAB_COLS + NEW_TAB_GAP + FIRST_COL);
    let n = count.max(1) as u16;
    let mut each = (room / n).clamp(sizes.min_columns.min(sizes.max_columns), sizes.max_columns.max(SMALLEST_TAB));
    if each * n > room {
        each = (room / n).max(SMALLEST_TAB);
    }
    let mut tabs = Vec::new();
    for i in 0..count as u16 {
        let start = FIRST_COL + i * each;
        if start + each > room + FIRST_COL {
            break;
        }
        tabs.push(TabSpan { start, end: start + each });
    }
    TabLayout { tabs, new_tab }
}

/// What window x is over: a tab, its close cross, or the new tab button, which takes every column from its
/// first to the right edge of the bar. Tabs are drawn half a column left of their columns, and so are hit;
/// the close cross is drawn on its own two columns.
pub fn hit(layout: &TabLayout, x: f64, cell_w: u32) -> Option<TabHit> {
    let cw = cell_w.max(1) as f64;
    let col = (x.max(0.0) / cw) as u16;
    if let Some(c) = layout.new_tab
        && col >= c
    {
        return Some(TabHit::NewTab);
    }
    let shifted = (x.max(0.0) / cw + 0.5) as u16;
    let i = layout.tabs.iter().position(|t| shifted >= t.start && shifted < t.end)?;
    let close = layout.tabs[i].close();
    Some(if col >= close && col < close + 2 { TabHit::Close(i) } else { TabHit::Tab(i) })
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

/// Whether `c` is a light colour.
fn is_light(c: [u8; 3]) -> bool {
    crate::config::relative_luminance(c) > 0.4
}

/// The bar's colour: the terminal background moved `tab_bar.bar_shade` of the way to black on a dark theme,
/// and a third of that on a light theme, where the same step looks much heavier. The tab that is showing is
/// the terminal background, so this difference is what makes it stand out.
pub fn bar_color(background: [u8; 3], sizes: &TabBar) -> [u8; 3] {
    let shade = sizes.bar_shade.clamp(0.0, 1.0);
    if is_light(background) { mix(background, [0; 3], shade / 3.0) } else { mix(background, [0; 3], shade) }
}

/// The fill of a tab under the pointer and of the new tab button under the pointer: part of the way from
/// the bar towards the title colour, so it reads as a raised tab but not as the one that is showing.
pub fn hover_fill(bar: [u8; 3], chrome: &Chrome) -> [u8; 3] {
    mix(bar, chrome.text, if is_light(bar) { 0.08 } else { 0.10 })
}

/// The colour of the dim marks: the dividers between tabs and the plus and crosses when not under the pointer.
fn dim(bar: [u8; 3], chrome: &Chrome) -> [u8; 3] {
    mix(bar, chrome.text, 0.45)
}

/// The bar's text: the bar colour across the row, then each title in its tab, in the strong colour on the
/// tab that is showing and a dimmer one elsewhere.
pub fn text(titles: &[String], active: usize, layout: &TabLayout, chrome: &Chrome, bar: [u8; 3]) -> Vec<u8> {
    let mut out = format!("{}\x1b[K", sgr_colors(rgb(chrome.text), rgb(bar)));
    for (i, span) in layout.tabs.iter().enumerate() {
        let room = span.close().saturating_sub(span.start + 1) as usize;
        let title = titles.get(i).map(String::as_str).filter(|t| !t.is_empty()).unwrap_or("Termoil");
        let fg = if i == active { chrome.text_strong } else { mix(bar, chrome.text, 0.78) };
        out.push_str(&format!("\x1b[{}G{}{}", span.start + 2, sgr_colors(rgb(fg), rgb(bar)), fit(title, room)));
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

/// The bar's shapes: the tab that is showing, the fill of a tab under the pointer, the dividers between
/// quiet tabs, the close crosses on the tab that is showing and the one under the pointer, and the new tab
/// button.
pub fn shapes(layout: &TabLayout, active: usize, hover: Option<TabHit>, look: &Look<'_>) -> Vec<Shape> {
    let bar = bar_color(look.background, look.sizes);
    let hovered = |j: usize| matches!(hover, Some(TabHit::Tab(h) | TabHit::Close(h)) if h == j);
    let mut out = Vec::new();
    for (i, span) in layout.tabs.iter().enumerate() {
        if i == active {
            active_tab(&mut out, *span, look);
        } else if hovered(i) {
            hovered_tab(&mut out, *span, bar, look);
        }
        let quiet = |j: usize| j != active && !hovered(j);
        if quiet(i) && layout.tabs.get(i + 1).is_some() && quiet(i + 1) {
            divider(&mut out, span.end, bar, look);
        }
        if i == active || hovered(i) {
            let color = if i == active { look.chrome.text } else { dim(bar, look.chrome) };
            close_cross(&mut out, *span, color, hover == Some(TabHit::Close(i)), bar, look);
        }
    }
    if layout.new_tab.is_some() {
        new_tab_button(&mut out, hover == Some(TabHit::NewTab), bar, look);
    }
    out
}

/// The horizontal extent of a tab in pixels: its columns moved half a column left, with half of
/// `tab_bar.inset` taken off each side so neighbouring tabs have a gap between them.
fn tab_x(span: TabSpan, look: &Look<'_>) -> (f32, f32) {
    let (cw, gap) = (look.cell.0 as f32, look.sizes.inset * look.scale / 2.0);
    ((span.start as f32 - 0.5) * cw + gap, (span.end as f32 - 0.5) * cw - gap)
}

/// The tab that is showing: the terminal background from `tab_bar.inset` below the top of the bar down to
/// its bottom edge, so it joins the terminal, with rounded top corners and square bottom ones, and the
/// accent line along its top while the window has the keyboard.
fn active_tab(out: &mut Vec<Shape>, span: TabSpan, look: &Look<'_>) {
    let s = look.scale;
    let (x0, x1) = tab_x(span, look);
    let top = look.sizes.inset * s;
    let radius = (look.sizes.corner_radius * s).min((look.height - top) / 2.0);
    let fill = rgba(look.background);
    out.push(Shape::RoundRect { area: Area { x0, y0: top, x1, y1: look.height }, radius, color: fill });
    let lower = Area { x0, y0: top + radius, x1, y1: look.height };
    out.push(Shape::RoundRect { area: lower, radius: 0.0, color: fill });
    if look.focused && look.sizes.underline > 0.0 {
        let line = look.sizes.underline * s;
        let accent = Area { x0: x0 + radius, y0: top, x1: x1 - radius, y1: top + line };
        out.push(Shape::RoundRect { area: accent, radius: line / 2.0, color: rgba(look.chrome.accent) });
    }
}

/// A tab under the pointer: a rounded fill inset from the top and bottom of the bar.
fn hovered_tab(out: &mut Vec<Shape>, span: TabSpan, bar: [u8; 3], look: &Look<'_>) {
    let s = look.scale;
    let (x0, x1) = tab_x(span, look);
    let top = look.sizes.inset * s;
    let area = Area { x0, y0: top, x1, y1: look.height - top };
    out.push(Shape::RoundRect {
        area,
        radius: look.sizes.corner_radius * s,
        color: rgba(hover_fill(bar, look.chrome)),
    });
}

/// The short line between two quiet tabs, at column `col`.
fn divider(out: &mut Vec<Shape>, col: u16, bar: [u8; 3], look: &Look<'_>) {
    let x = (col as f32 - 0.5) * look.cell.0 as f32;
    let (top, bottom) = (look.height * 0.34, look.height * 0.70);
    let color = mix(bar, look.chrome.text, 0.22);
    out.push(Shape::Line { from: (x, top), to: (x, bottom), width: look.scale.max(1.0), color: rgba(color) });
}

/// The close cross over a tab's close columns: under the pointer, a round fill with the cross in the
/// strong colour on it.
fn close_cross(out: &mut Vec<Shape>, span: TabSpan, color: [u8; 3], hovered: bool, bar: [u8; 3], look: &Look<'_>) {
    let (cw, ch) = (look.cell.0 as f32, look.cell.1 as f32);
    let (cx, cy) = ((span.close() + 1) as f32 * cw, tab_middle(look));
    let color = if hovered {
        let r = (ch * 0.42).min(cw * 0.95);
        let area = Area { x0: cx - r, y0: cy - r, x1: cx + r, y1: cy + r };
        out.push(Shape::RoundRect { area, radius: r * 0.45, color: rgba(hover_fill(bar, look.chrome)) });
        look.chrome.text_strong
    } else {
        color
    };
    let arm = (ch * 0.15).max(3.0);
    let width = (1.3 * look.scale).max(1.1);
    out.push(Shape::Line { from: (cx - arm, cy - arm), to: (cx + arm, cy + arm), width, color: rgba(color) });
    out.push(Shape::Line { from: (cx - arm, cy + arm), to: (cx + arm, cy - arm), width, color: rgba(color) });
}

/// The vertical middle of the titles, which is the middle of the bar.
fn tab_middle(look: &Look<'_>) -> f32 {
    look.height / 2.0
}

/// The new tab button against the right edge of the bar: a plus, on a rounded square of the hover fill
/// when the pointer is over it.
fn new_tab_button(out: &mut Vec<Shape>, hovered: bool, bar: [u8; 3], look: &Look<'_>) {
    let (cw, ch, s) = (look.cell.0 as f32, look.cell.1 as f32, look.scale);
    let half = (look.height / 2.0 - look.sizes.inset * s).min(NEW_TAB_COLS as f32 * cw / 2.0);
    let (cx, cy) = (look.width - look.sizes.inset * s - half, tab_middle(look));
    if hovered {
        let area = Area { x0: cx - half, y0: cy - half, x1: cx + half, y1: cy + half };
        out.push(Shape::RoundRect {
            area,
            radius: look.sizes.corner_radius * s,
            color: rgba(hover_fill(bar, look.chrome)),
        });
    }
    let color = if hovered { look.chrome.text_strong } else { mix(bar, look.chrome.text, 0.7) };
    let arm = (ch * 0.24).max(4.0);
    let width = (1.5 * s).max(1.2);
    out.push(Shape::Line { from: (cx - arm, cy), to: (cx + arm, cy), width, color: rgba(color) });
    out.push(Shape::Line { from: (cx, cy - arm), to: (cx, cy + arm), width, color: rgba(color) });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default `[tab_bar]` settings.
    fn sizes() -> TabBar {
        TabBar::default()
    }

    #[test]
    fn tabs_share_the_width_up_to_the_maximum() {
        let l = layout(2, 120, &sizes());
        let max = sizes().max_columns;
        assert_eq!(l.tabs, vec![TabSpan { start: 1, end: 1 + max }, TabSpan { start: 1 + max, end: 1 + 2 * max }]);
    }

    #[test]
    fn the_new_tab_button_is_at_the_right_end() {
        assert_eq!(layout(1, 120, &sizes()).new_tab, Some(117));
        assert_eq!(layout(2, 80, &sizes()).new_tab, Some(77));
    }

    #[test]
    fn many_tabs_shrink_and_the_ones_past_the_edge_are_left_off() {
        let l = layout(30, 80, &sizes());
        assert!(l.tabs.iter().all(|t| t.end - t.start >= SMALLEST_TAB));
        assert!(l.tabs.len() < 30);
        assert!(l.tabs.last().unwrap().end + NEW_TAB_COLS + NEW_TAB_GAP <= 80);
        assert_eq!(l.tabs[0].start, FIRST_COL);
    }

    #[test]
    fn a_click_on_the_cross_is_a_close_and_elsewhere_a_tab() {
        let l = layout(2, 120, &sizes());
        let cw = 10;
        assert_eq!(hit(&l, 15.0, cw), Some(TabHit::Tab(0)));
        let close = l.tabs[1].close();
        assert_eq!(hit(&l, close as f64 * 10.0, cw), Some(TabHit::Close(1)));
        assert_eq!(hit(&l, (l.tabs[1].start as f64 - 0.4) * 10.0, cw), Some(TabHit::Tab(1)), "the shifted left edge");
        assert_eq!(hit(&l, 100.0 * 10.0, cw), None);
        assert_eq!(hit(&l, 118.0 * 10.0, cw), Some(TabHit::NewTab));
        assert_eq!(hit(&l, 125.0 * 10.0, cw), Some(TabHit::NewTab), "the slack past the last column is the button");
    }

    #[test]
    fn the_bar_is_darker_than_a_dark_terminal_and_a_little_darker_than_a_light_one() {
        let dark = bar_color([0x1b, 0x1f, 0x26], &sizes());
        assert!(dark[0] < 0x1b && dark[2] < 0x26);
        let light = bar_color([0xfa, 0xfa, 0xfa], &sizes());
        assert!(light[0] < 0xfa && light[0] > 0xe0);
    }

    #[test]
    fn a_long_title_ends_in_an_ellipsis() {
        assert_eq!(fit("abcdef", 4), "abc\u{2026}");
        assert_eq!(fit("abc", 4), "abc");
        assert_eq!(fit("ab cdef", 4), "ab\u{2026}", "no space before the ellipsis");
    }
}
