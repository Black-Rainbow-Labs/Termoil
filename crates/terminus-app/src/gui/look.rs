//! How the configuration looks on screen: the font grid for a window's scale, the colours from the
//! theme and overrides, and the frame builder options.

use terminus_font::{FontConfig, FontGrid};
use terminus_render::{BuildOptions, Colors};
use terminus_vt::style::{Rgb, default_palette};
use terminus_vt::terminal::CursorShape;

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
    }
}

/// A colour between `a` and `b`: `t` of the way from `a`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}
