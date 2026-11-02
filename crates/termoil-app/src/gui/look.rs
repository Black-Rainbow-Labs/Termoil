//! How the configuration looks on screen: the font grid for a window's scale, the colours from the
//! theme and overrides, and the frame builder options.

use termoil_font::{FontConfig, FontGrid};
use termoil_render::{BuildOptions, Colors};
use termoil_vt::style::{Rgb, default_palette};
use termoil_vt::terminal::CursorShape;

use crate::config::{Config, CursorStyle};

/// Font size in pixels for a size in points at a display scale.
pub fn font_px(points: f32, scale: f64) -> f32 {
    points * scale as f32 * 96.0 / 72.0
}

/// The font grid for the configuration at `points` and `scale`. When none of the configured
/// families is installed, falls back to the font crate's default list.
pub fn font_grid(config: &Config, points: f32, scale: f64) -> Result<FontGrid, String> {
    let f = &config.font;
    let fc = FontConfig {
        families: f.family.clone(),
        fallback_families: f.fallback.clone(),
        size_px: font_px(points, scale),
        ligatures: f.ligatures,
        ligatures_off: f.ligatures_off.clone(),
        line_height: f.line_height as f32,
        ..FontConfig::default()
    };
    FontGrid::new(fc.clone())
        .or_else(|_| FontGrid::new(FontConfig { families: FontConfig::default().families, ..fc }))
        .map_err(|e| format!("no usable font: {e:?}"))
}

/// Converts a config colour.
fn rgb(c: [u8; 3]) -> Rgb {
    Rgb::new(c[0], c[1], c[2])
}

/// The 256 colour palette: the theme's 16, then the standard cube and greys, then overrides.
pub fn palette(config: &Config, os_dark: bool) -> [Rgb; 256] {
    let theme = config.theme(os_dark);
    let mut p = default_palette();
    for (i, c) in theme.palette.iter().enumerate() {
        p[i] = rgb(*c);
    }
    for (i, c) in config.colors.palette.iter().enumerate().take(256) {
        p[i] = rgb(c.0);
    }
    p
}

/// The default colours of every pane.
pub fn colors(config: &Config, os_dark: bool) -> Colors {
    let theme = config.theme(os_dark);
    let o = &config.colors;
    Colors {
        palette: palette(config, os_dark),
        foreground: rgb(o.foreground.map(|c| c.0).unwrap_or(theme.foreground)),
        background: rgb(o.background.map(|c| c.0).unwrap_or(theme.background)),
        cursor: rgb(o.cursor.map(|c| c.0).unwrap_or(theme.cursor)),
        selection_background: rgb(o.selection_background.map(|c| c.0).unwrap_or(theme.selection_background)),
        selection_foreground: o.selection_foreground.map(|c| c.0).or(theme.selection_foreground).map(rgb),
    }
}

/// The colours of the window's own controls for the theme in effect (`config::chrome_for`).
pub fn chrome(config: &Config, os_dark: bool) -> crate::config::Chrome {
    crate::config::chrome_for(&config.theme(os_dark))
}

/// The alpha of the window colour for `window.opacity` (0.0 to 1.0).
pub fn background_alpha(config: &Config) -> u8 {
    (config.window.opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The frame builder options. `padding` is in physical pixels.
pub fn build_options(config: &Config, scale: f64) -> BuildOptions {
    let pad = |v: u32| (v as f64 * scale).round() as u32;
    let blink = config.cursor.blink;
    BuildOptions {
        padding: (pad(config.window.padding[0]), pad(config.window.padding[1])),
        bold_is_bright: false,
        minimum_contrast: config.window.minimum_contrast as f32,
        all_rows: false,
        default_cursor: match config.cursor.style {
            CursorStyle::Block => CursorShape::Block { blink },
            CursorStyle::Bar => CursorShape::Bar { blink },
            CursorStyle::Underline => CursorShape::Underline { blink },
        },
        background_alpha: 255,
    }
}

/// The inner size in physical pixels of a new window: the configured columns and rows of cells
/// plus the padding on both sides, and the tab bar when it shows with one tab.
pub fn window_size(config: &Config, m: &termoil_font::CellMetrics, opts: &BuildOptions, scale: f64) -> (u32, u32) {
    let w = config.window.columns * m.width + 2 * opts.padding.0;
    let h = config.window.rows * m.height + 2 * opts.padding.1 + tab_bar_height(config, m.height, scale, 1);
    (w, h)
}

/// The tab bar's height in physical pixels for a window with `tabs` tabs: the cell height plus
/// `tab_bar.padding` above and below, or zero when the bar is hidden (`window.show_tab_bar` off, or one
/// tab and `tab_bar.show_with_one_tab` off).
pub fn tab_bar_height(config: &Config, cell_h: u32, scale: f64, tabs: usize) -> u32 {
    let shown = config.window.show_tab_bar && (tabs > 1 || config.tab_bar.show_with_one_tab);
    if !shown {
        return 0;
    }
    cell_h + 2 * (config.tab_bar.padding as f64 * scale).round() as u32
}

/// A colour between `a` and `b`: `t` of the way from `a`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}
