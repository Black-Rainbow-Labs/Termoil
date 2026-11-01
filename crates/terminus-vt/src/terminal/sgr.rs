//! SGR: setting the pen's colours and attributes.

use super::Terminal;
use crate::parser::Params;
use crate::style::{Color, Style, Underline, attr};

impl Terminal {
    /// Applies an SGR sequence to the cursor's pen.
    pub(super) fn sgr(&mut self, params: &Params) {
        let mut style = self.screen().cursor.style;
        if params.is_empty() {
            style = Style::default();
        }
        let mut i = 0;
        while i < params.len() {
            i += apply_sgr(&mut style, params, i);
        }
        let s = self.screen_mut();
        if s.cursor.style != style {
            s.cursor.style = style;
            s.cursor.style_cache = None;
        }
    }
}

/// Applies the SGR attribute starting at group `i`. Returns how many groups it used.
fn apply_sgr(style: &mut Style, params: &Params, i: usize) -> usize {
    let group = params.group(i);
    let code = group[0];
    match code {
        0 => *style = Style::default(),
        1 => style.attrs |= attr::BOLD,
        2 => style.attrs |= attr::FAINT,
        3 => style.attrs |= attr::ITALIC,
        4 => style.underline = underline_from(group),
        5 | 6 => style.attrs |= attr::BLINK,
        7 => style.attrs |= attr::INVERSE,
        8 => style.attrs |= attr::INVISIBLE,
        9 => style.attrs |= attr::STRIKE,
        21 => style.underline = Underline::Double,
        22 => style.attrs &= !(attr::BOLD | attr::FAINT),
        23 => style.attrs &= !attr::ITALIC,
        24 => style.underline = Underline::None,
        25 => style.attrs &= !attr::BLINK,
        27 => style.attrs &= !attr::INVERSE,
        28 => style.attrs &= !attr::INVISIBLE,
        29 => style.attrs &= !attr::STRIKE,
        30..=37 => style.fg = Color::Palette((code - 30) as u8),
        38 => return set_extended(&mut style.fg, params, i),
        39 => style.fg = Color::Default,
        40..=47 => style.bg = Color::Palette((code - 40) as u8),
        48 => return set_extended(&mut style.bg, params, i),
        49 => style.bg = Color::Default,
        53 => style.attrs |= attr::OVERLINE,
        55 => style.attrs &= !attr::OVERLINE,
        58 => return set_extended(&mut style.underline_color, params, i),
        59 => style.underline_color = Color::Default,
        90..=97 => style.fg = Color::Palette((code - 90 + 8) as u8),
        100..=107 => style.bg = Color::Palette((code - 100 + 8) as u8),
        _ => {}
    }
    1
}

/// The underline style of `4` or `4:n`.
fn underline_from(group: &[u16]) -> Underline {
    match group.get(1) {
        None => Underline::Single,
        Some(&n) => Underline::from_sub(n).unwrap_or(Underline::Single),
    }
}

/// Sets `target` from an extended colour at group `i` and returns the groups consumed.
fn set_extended(target: &mut Color, params: &Params, i: usize) -> usize {
    let (color, used) = extended_color(params, i);
    if let Some(c) = color {
        *target = c;
    }
    used
}

/// Reads a 38, 48 or 58 colour in either form: colon sub parameters (`38:5:n`, `38:2::r:g:b`,
/// `38:2:r:g:b`) or semicolon parameters (`38;5;n`, `38;2;r;g;b`). Returns the colour and how
/// many groups were consumed.
/// An incomplete semicolon form consumes the rest of the sequence and sets nothing.
fn extended_color(params: &Params, i: usize) -> (Option<Color>, usize) {
    let group = params.group(i);
    if group.len() > 1 {
        return (colon_color(group), 1);
    }
    match params.get(i + 1) {
        5 if params.len() > i + 2 => (Some(Color::Palette(params.get(i + 2).min(255) as u8)), 3),
        2 if params.len() > i + 4 => {
            let (r, g, b) = (params.get(i + 2), params.get(i + 3), params.get(i + 4));
            (Some(Color::Rgb(r.min(255) as u8, g.min(255) as u8, b.min(255) as u8)), 5)
        }
        5 | 2 => (None, params.len() - i),
        _ => (None, 1),
    }
}

/// Reads the colon form of an extended colour.
fn colon_color(group: &[u16]) -> Option<Color> {
    match group.get(1)? {
        5 => Some(Color::Palette((*group.get(2)?).min(255) as u8)),
        2 => {
            // With a colour space id the components are at 3..6; without one, at 2..5.
            let rgb = if group.len() >= 6 { &group[3..6] } else { group.get(2..5)? };
            Some(Color::Rgb(rgb[0].min(255) as u8, rgb[1].min(255) as u8, rgb[2].min(255) as u8))
        }
        _ => None,
    }
}
