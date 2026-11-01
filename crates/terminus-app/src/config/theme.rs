//! The bundled colour schemes (the iTerm2 colour schemes set plus Terminus Dark and Terminus Light) and the
//! resolution of `[theme]` and `[colors]` into the colours a window draws with.
use std::sync::OnceLock;

use super::Config;

/// The data file written by `tools/themes`, one theme per line.
const THEME_DATA: &str = include_str!("../../themes/themes.txt");

/// The theme used when the configured name does not exist.
pub const DEFAULT_THEME: &str = "Terminus Dark";

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

    /// The colours to draw with: the selected theme (Terminus Dark when the name is unknown) with every
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
