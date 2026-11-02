//! The bundled colour schemes (the iTerm2 colour schemes set, Termoil Dark and Termoil Light, and the themes
//! Unluminous ships) and the resolution of `[theme]` and `[colors]` into the colours a window draws with,
//! including the colours of the window's own controls ([`Chrome`]).
use std::sync::OnceLock;

use super::Config;

/// The data file written by `tools/themes`, one theme per line.
const THEME_DATA: &str = include_str!("../../themes/themes.txt");

/// The chrome colours of the themes taken from Unluminous, one theme per line.
const CHROME_DATA: &str = include_str!("../../themes/chrome.txt");

/// The theme used when none is configured or the configured name does not exist: Unluminous's own dark
/// theme, so Termoil and the Unluminous editor look like one family (task-2468).
pub const DEFAULT_THEME: &str = "Unluminous Dark";

/// A colour scheme: default colours, selection colours and the sixteen ANSI palette entries.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cursor: [u8; 3],
    pub selection_background: [u8; 3],
    pub selection_foreground: Option<[u8; 3]>,
    pub palette: [[u8; 3]; 16],
}

/// The colours of the window's own controls: the tab bar, its tabs and their buttons. The roles are the
/// ones Unluminous's palette names, so a theme taken from Unluminous colours Termoil's tabs the way
/// Unluminous colours its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    /// Behind the tabs (Unluminous's `toolbar`).
    pub bar: [u8; 3],
    /// The tab that is showing, and a tab under the pointer (`control`).
    pub tab: [u8; 3],
    /// The edge of the tab that is showing (`control_border`).
    pub border: [u8; 3],
    /// The line under the tab that is showing (`accent`).
    pub accent: [u8; 3],
    /// The title of the tab that is showing (`text_strong`).
    pub text_strong: [u8; 3],
    /// The title of any other tab (`text_control`).
    pub text: [u8; 3],
    /// The close cross and the new tab button (`text_dim`).
    pub text_dim: [u8; 3],
    /// Behind the close cross under the pointer (`close`).
    pub close: [u8; 3],
}

/// Parses one line of `chrome.txt`: `name|bar|tab|border|accent|text_strong|text|text_dim|close`.
fn parse_chrome_line(line: &str) -> Option<(String, Chrome)> {
    let f: Vec<&str> = line.split('|').collect();
    if f.len() != 9 || line.starts_with('#') {
        return None;
    }
    let chrome = Chrome {
        bar: hex_color(f[1])?,
        tab: hex_color(f[2])?,
        border: hex_color(f[3])?,
        accent: hex_color(f[4])?,
        text_strong: hex_color(f[5])?,
        text: hex_color(f[6])?,
        text_dim: hex_color(f[7])?,
        close: hex_color(f[8])?,
    };
    Some((f[0].to_lowercase(), chrome))
}

/// Every line of `chrome.txt`, parsed once: the lowercased theme name and its chrome, in file order.
fn listed() -> &'static [(String, Chrome)] {
    static LISTED: OnceLock<Vec<(String, Chrome)>> = OnceLock::new();
    LISTED.get_or_init(|| CHROME_DATA.lines().filter_map(parse_chrome_line).collect())
}

/// The chrome listed for a theme by name, ignoring case.
fn listed_chrome(name: &str) -> Option<Chrome> {
    let wanted = name.trim().to_lowercase();
    listed().iter().find(|(n, _)| *n == wanted).map(|(_, c)| *c)
}

/// The themes Unluminous ships, in the order the theme switcher lists them first: every bundled theme
/// `chrome.txt` names.
pub fn featured_themes() -> Vec<&'static str> {
    listed().iter().filter_map(|(n, _)| theme(n)).map(|t| t.name.as_str()).collect()
}

/// A colour `t` of the way from `a` to `b`.
fn blend(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Chrome worked out from a theme's own colours, for a theme `chrome.txt` does not list. The bar is a
/// step from the background towards the text, the tab that is showing a larger step, and the accent is
/// the theme's blue, the way Unluminous's own palette is built.
pub fn derived_chrome(t: &Theme) -> Chrome {
    let (fg, bg) = (t.foreground, t.background);
    let dark = relative_luminance(bg) < 0.4;
    let strong = if dark { blend(fg, [255; 3], 0.6) } else { blend(fg, [0; 3], 0.6) };
    Chrome {
        bar: blend(bg, fg, if dark { 0.03 } else { 0.04 }),
        tab: if dark { blend(bg, fg, 0.14) } else { blend(bg, [255; 3], 0.8) },
        border: blend(bg, fg, 0.2),
        accent: t.palette[4],
        text_strong: strong,
        text: blend(fg, bg, 0.12),
        text_dim: blend(fg, bg, 0.42),
        close: t.palette[1],
    }
}

/// The chrome for a theme: its line in `chrome.txt` when it has one, otherwise worked out from its colours.
pub fn chrome_for(t: &Theme) -> Chrome {
    listed_chrome(&t.name).unwrap_or_else(|| derived_chrome(t))
}

/// Parses six hex digits into a colour.
fn hex_color(text: &str) -> Option<[u8; 3]> {
    if text.len() != 6 || !text.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(text, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Parses one line of `themes.txt`: `name|fg|bg|cursor|selection_bg|selection_fg|c0|...|c15`.
pub fn parse_theme_line(line: &str) -> Option<Theme> {
    let fields: Vec<&str> = line.split('|').collect();
    if fields.len() != 22 {
        return None;
    }
    let mut palette = [[0u8; 3]; 16];
    for (slot, text) in palette.iter_mut().zip(&fields[6..]) {
        *slot = hex_color(text)?;
    }
    Some(Theme {
        name: fields[0].to_string(),
        foreground: hex_color(fields[1])?,
        background: hex_color(fields[2])?,
        cursor: hex_color(fields[3])?,
        selection_background: hex_color(fields[4])?,
        selection_foreground: hex_color(fields[5]),
        palette,
    })
}

/// Every bundled theme, parsed once on first use and sorted by name.
pub fn themes() -> &'static [Theme] {
    static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
    THEMES.get_or_init(|| THEME_DATA.lines().filter_map(parse_theme_line).collect())
}

/// Finds a bundled theme by name, ignoring case.
pub fn theme(name: &str) -> Option<&'static Theme> {
    let wanted = name.trim().to_lowercase();
    themes().iter().find(|t| t.name.to_lowercase() == wanted)
}

/// The relative luminance of an sRGB colour as defined by WCAG.
pub fn relative_luminance(c: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// The WCAG contrast ratio between two colours, from 1.0 to 21.0.
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

impl Config {
    /// The name of the theme in effect: `theme.dark` or `theme.light` by the OS appearance when both are set,
    /// otherwise `theme.name`.
    pub fn theme_name(&self, os_dark: bool) -> &str {
        match (&self.theme.dark, &self.theme.light) {
            (Some(dark), Some(light)) => {
                if os_dark {
                    dark
                } else {
                    light
                }
            }
            _ => &self.theme.name,
        }
    }

    /// The colours to draw with: the selected theme (Unluminous Dark when the name is unknown) with every
    /// `[colors]` override applied on top.
    pub fn theme(&self, os_dark: bool) -> Theme {
        let base = theme(self.theme_name(os_dark)).or_else(|| theme(DEFAULT_THEME));
        let mut result = base.cloned().unwrap_or_else(fallback_theme);
        let c = &self.colors;
        if let Some(v) = c.foreground {
            result.foreground = v.0;
        }
        if let Some(v) = c.background {
            result.background = v.0;
        }
        if let Some(v) = c.cursor {
            result.cursor = v.0;
        }
        if let Some(v) = c.selection_background {
            result.selection_background = v.0;
        }
        if let Some(v) = c.selection_foreground {
            result.selection_foreground = Some(v.0);
        }
        for (slot, v) in result.palette.iter_mut().zip(&c.palette) {
            *slot = v.0;
        }
        result
    }
}

/// A theme used only if the bundled data cannot be read, so a window can still draw.
fn fallback_theme() -> Theme {
    let mut palette = [[0xdc; 3]; 16];
    palette[0] = [0x45, 0x47, 0x5a];
    Theme {
        name: DEFAULT_THEME.into(),
        foreground: [0xdc; 3],
        background: [0x1e; 3],
        cursor: [0xf5; 3],
        selection_background: [0x3a, 0x4a, 0x63],
        selection_foreground: None,
        palette,
    }
}
