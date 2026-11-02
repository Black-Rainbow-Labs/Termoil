//! Colour specifications as xterm's OSC 4, 5 and 10 to 19 accept them: `rgb:`, `#` forms, `rgbi:`,
//! the CIE spaces, TekHVC and the common X11 colour names.
//!
//! The device independent spaces are converted to sRGB with the published formulas (the sRGB
//! matrix, the D65 white point and the sRGB transfer curve). xterm asks Xlib to convert them with
//! the colour data of the X screen, so its numbers differ from these on most displays.

use crate::style::Rgb;

/// The D65 white point, the reference white of sRGB.
const WHITE: (f64, f64, f64) = (0.950_456, 1.0, 1.088_754);

/// Parses a colour specification into an 8 bit colour; None when it is not valid.
pub fn parse_color(spec: &str) -> Option<Rgb> {
    if let Some(body) = spec.strip_prefix('#') {
        return parse_hash(body);
    }
    if let Some((prefix, body)) = spec.split_once(':') {
        return parse_prefixed(&prefix.to_ascii_lowercase(), body);
    }
    named_color(spec)
}

/// Parses the form that follows a `prefix:`.
fn parse_prefixed(prefix: &str, body: &str) -> Option<Rgb> {
    if prefix == "rgb" {
        return parse_rgb_hex(body);
    }
    let v = parse_floats(body)?;
    match prefix {
        "rgbi" => Some(Rgb::new(unit_to_byte(v[0]), unit_to_byte(v[1]), unit_to_byte(v[2]))),
        "ciexyz" => Some(xyz_to_rgb(v[0], v[1], v[2])),
        "ciexyy" => Some(xyy_to_rgb(v[0], v[1], v[2])),
        "cieuvy" => Some(uvy_to_rgb(v[0], v[1], v[2])),
        "cielab" => Some(lab_to_rgb(v[0], v[1], v[2])),
        "cieluv" => Some(luv_to_rgb(v[0], v[1], v[2])),
        "tekhvc" => Some(hvc_to_rgb(v[0], v[1], v[2])),
        _ => None,
    }
}

/// Three floats separated by `/`.
fn parse_floats(body: &str) -> Option<[f64; 3]> {
    let mut parts = body.split('/');
    let mut out = [0.0; 3];
    for slot in out.iter_mut() {
        *slot = parts.next()?.parse::<f64>().ok().filter(|f| f.is_finite())?;
    }
    parts.next().is_none().then_some(out)
}

/// `rgb:r/g/b` with 1 to 4 hex digits each, scaled to the full range of the digits given.
fn parse_rgb_hex(body: &str) -> Option<Rgb> {
    let parts: Vec<&str> = body.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    Some(Rgb::new(scale_hex(parts[0])?, scale_hex(parts[1])?, scale_hex(parts[2])?))
}

/// `#rgb`, `#rrggbb`, `#rrrgggbbb` and `#rrrrggggbbbb`: each component is the high digits of a 16
/// bit value, so `#fff` is 0xf000 and reports as f0f0.
fn parse_hash(hex: &str) -> Option<Rgb> {
    if hex.is_empty() || !hex.len().is_multiple_of(3) || hex.len() > 12 {
        return None;
    }
    let n = hex.len() / 3;
    let part = |i: usize| -> Option<u8> {
        let v = u32::from_str_radix(&hex[i * n..(i + 1) * n], 16).ok()?;
        Some(((v << (16 - 4 * n)) >> 8) as u8)
    };
    Some(Rgb::new(part(0)?, part(1)?, part(2)?))
}

/// Scales a 1 to 4 digit hex component to 8 bits.
fn scale_hex(s: &str) -> Option<u8> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    let max = (1u32 << (4 * s.len())) - 1;
    Some(((v * 255 + max / 2) / max) as u8)
}

/// A value from 0 to 1 as 0 to 255, clamped.
fn unit_to_byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// CIE XYZ (white is 1.0) to sRGB.
fn xyz_to_rgb(x: f64, y: f64, z: f64) -> Rgb {
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    Rgb::new(unit_to_byte(encode_gamma(r)), unit_to_byte(encode_gamma(g)), unit_to_byte(encode_gamma(b)))
}

/// The sRGB transfer curve, applied to a linear component.
fn encode_gamma(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// CIE xyY to sRGB.
fn xyy_to_rgb(x: f64, y: f64, big_y: f64) -> Rgb {
    if y <= 0.0 {
        return xyz_to_rgb(0.0, 0.0, 0.0);
    }
    xyz_to_rgb(x * big_y / y, big_y, (1.0 - x - y) * big_y / y)
}

/// CIE u'v'Y to sRGB.
fn uvy_to_rgb(u: f64, v: f64, big_y: f64) -> Rgb {
    if v <= 0.0 {
        return xyz_to_rgb(0.0, 0.0, 0.0);
    }
    let x = big_y * 9.0 * u / (4.0 * v);
    let z = big_y * (12.0 - 3.0 * u - 20.0 * v) / (4.0 * v);
    xyz_to_rgb(x, big_y, z)
}

/// CIE L*a*b* (L from 0 to 100) to sRGB.
fn lab_to_rgb(l: f64, a: f64, b: f64) -> Rgb {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let inverse = |t: f64| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f64 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    xyz_to_rgb(WHITE.0 * inverse(fx), WHITE.1 * inverse(fy), WHITE.2 * inverse(fz))
}

/// CIE L*u*v* (L from 0 to 100) to sRGB.
fn luv_to_rgb(l: f64, u: f64, v: f64) -> Rgb {
    if l <= 0.0 {
        return xyz_to_rgb(0.0, 0.0, 0.0);
    }
    let denominator = WHITE.0 + 15.0 * WHITE.1 + 3.0 * WHITE.2;
    let (un, vn) = (4.0 * WHITE.0 / denominator, 9.0 * WHITE.1 / denominator);
    let (u_prime, v_prime) = (u / (13.0 * l) + un, v / (13.0 * l) + vn);
    let y = if l > 8.0 { WHITE.1 * ((l + 16.0) / 116.0).powi(3) } else { WHITE.1 * l * (3.0f64 / 29.0).powi(3) };
    let x = y * 9.0 * u_prime / (4.0 * v_prime);
    let z = y * (12.0 - 3.0 * u_prime - 20.0 * v_prime) / (4.0 * v_prime);
    xyz_to_rgb(x, y, z)
}

/// Tektronix HVC (hue in degrees, value, chroma): polar L*u*v*, with the value as L*.
fn hvc_to_rgb(h: f64, v: f64, c: f64) -> Rgb {
    let angle = h.to_radians();
    luv_to_rgb(v, c * angle.cos(), c * angle.sin())
}

/// The X11 colour names a program is likely to use, matched without regard to case or spaces.
fn named_color(name: &str) -> Option<Rgb> {
    let key: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    if let Some(level) = key.strip_prefix("gray").or_else(|| key.strip_prefix("grey")) {
        let n: u32 = if level.is_empty() { 50 } else { level.parse().ok().filter(|n| *n <= 100)? };
        let v = ((n * 255 + 50) / 100) as u8;
        return Some(Rgb::new(v, v, v));
    }
    NAMES.iter().find(|(n, _)| *n == key).map(|(_, (r, g, b))| Rgb::new(*r, *g, *b))
}

/// X11 colour names and their values. X11 `green` is pure green, unlike the web colour of that name.
const NAMES: &[(&str, (u8, u8, u8))] = &[
    ("black", (0, 0, 0)),
    ("white", (255, 255, 255)),
    ("red", (255, 0, 0)),
    ("green", (0, 255, 0)),
    ("blue", (0, 0, 255)),
    ("yellow", (255, 255, 0)),
    ("cyan", (0, 255, 255)),
    ("magenta", (255, 0, 255)),
    ("orange", (255, 165, 0)),
    ("purple", (160, 32, 240)),
    ("brown", (165, 42, 42)),
    ("pink", (255, 192, 203)),
    ("violet", (238, 130, 238)),
    ("gold", (255, 215, 0)),
    ("navy", (0, 0, 128)),
    ("navyblue", (0, 0, 128)),
    ("maroon", (176, 48, 96)),
    ("turquoise", (64, 224, 208)),
    ("tan", (210, 180, 140)),
    ("salmon", (250, 128, 114)),
    ("khaki", (240, 230, 140)),
    ("coral", (255, 127, 80)),
    ("orchid", (218, 112, 214)),
    ("plum", (221, 160, 221)),
    ("beige", (245, 245, 220)),
    ("ivory", (255, 255, 240)),
    ("lavender", (230, 230, 250)),
    ("firebrick", (178, 34, 34)),
    ("forestgreen", (34, 139, 34)),
    ("darkgreen", (0, 100, 0)),
    ("darkblue", (0, 0, 139)),
    ("darkred", (139, 0, 0)),
    ("darkcyan", (0, 139, 139)),
    ("darkmagenta", (139, 0, 139)),
    ("lightblue", (173, 216, 230)),
    ("lightgreen", (144, 238, 144)),
    ("lightyellow", (255, 255, 224)),
    ("lightpink", (255, 182, 193)),
    ("skyblue", (135, 206, 235)),
    ("steelblue", (70, 130, 180)),
    ("royalblue", (65, 105, 225)),
    ("seagreen", (46, 139, 87)),
    ("limegreen", (50, 205, 50)),
    ("hotpink", (255, 105, 180)),
    ("deeppink", (255, 20, 147)),
    ("crimson", (220, 20, 60)),
    ("chocolate", (210, 105, 30)),
    ("sienna", (160, 82, 45)),
    ("wheat", (245, 222, 179)),
    ("snow", (255, 250, 250)),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `#` forms keep the high bits of a 16 bit value, as X11 does.
    #[test]
    fn hash_forms_pad_on_the_right() {
        assert_eq!(parse_color("#fff"), Some(Rgb::new(0xf0, 0xf0, 0xf0)));
        assert_eq!(parse_color("#f00f00f00"), Some(Rgb::new(0xf0, 0xf0, 0xf0)));
        assert_eq!(parse_color("#aaaabbbbcccc"), Some(Rgb::new(0xaa, 0xbb, 0xcc)));
    }

    /// `rgbi:` is a fraction of full intensity.
    #[test]
    fn rgbi_is_a_fraction() {
        assert_eq!(parse_color("rgbi:1/0/0.5"), Some(Rgb::new(255, 0, 128)));
        assert_eq!(parse_color("rgbi:1/0"), None);
    }

    /// The white point of each CIE space converts to white.
    #[test]
    fn cie_white_is_white() {
        assert_eq!(parse_color("CIELab:100/0/0"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_color("CIELuv:100/0/0"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_color("TekHVC:0/100/0"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_color("CIEXYZ:0.950456/1/1.088754"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_color("CIEuvY:0.19783/0.46832/1"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(parse_color("CIExyY:0.3127/0.3290/1"), Some(Rgb::new(255, 255, 255)));
    }

    /// Names are matched without regard to case and spaces, and `grayNN` is a level.
    #[test]
    fn names_resolve() {
        assert_eq!(parse_color("Navy Blue"), Some(Rgb::new(0, 0, 128)));
        assert_eq!(parse_color("gray50"), Some(Rgb::new(128, 128, 128)));
        assert_eq!(parse_color("nosuchcolour"), None);
    }
}
