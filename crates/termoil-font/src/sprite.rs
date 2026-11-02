//! Sprite characters drawn as geometry instead of from a font.
//!
//! Box drawing (U+2500 to U+257F), block elements (U+2580 to U+259F), braille (U+2800 to U+28FF),
//! sextants (U+1FB00 to U+1FB3B) and the Powerline separators (U+E0B0 to U+E0BF) are computed from
//! the cell size so that neighbouring cells join without gaps or overlaps at any size.
//!
//! Conventions:
//! * Coordinates are pixels with the cell's top left at (0, 0). Every shape lies inside the cell.
//! * Line positions and thicknesses are whole pixels. A line is centred with the extra pixel (when
//!   the cell size minus the thickness is odd) going to the bottom or right side.
//! * Fractions of a cell (halves, eighths, thirds) are rounded half up, so an upper half block is
//!   `(height + 1) / 2` rows tall and the lower half block takes the remaining rows. Halves of the
//!   same cell never overlap.
//! * Heavy lines are twice the light thickness. A double line is two light strokes with a gap of one
//!   light thickness, so its total span is three light thicknesses.
//!
//! Octants (U+1CD00) are not drawn by this module.

use kurbo::{Affine, BezPath, Point, Rect};

/// One filled shape of a sprite, in pixels, with the cell's top left at (0, 0).
#[derive(Clone, Debug, PartialEq)]
pub enum SpriteShape {
    /// An axis aligned rectangle.
    Rect(Rect),
    /// A closed path filled with the non zero rule.
    Path(BezPath),
    /// A rectangle drawn at this fraction of the foreground colour (the shade characters).
    Shade { rect: Rect, alpha: f32 },
}

/// Control points factor for approximating a quarter ellipse with one cubic Bezier curve.
const KAPPA: f64 = 0.552_284_749_830_793_4;
/// Largest polygon the clipper handles (a four point band clipped by four planes has at most 8).
const CLIP_CAP: usize = 16;

const NONE: u8 = 0;
const LIGHT: u8 = 1;
const HEAVY: u8 = 2;
const DOUBLE: u8 = 3;

/// Builds one row of the arm table: the line style of the left, right, up and down arms.
const fn a(left: u8, right: u8, up: u8, down: u8) -> [u8; 4] {
    [left, right, up, down]
}

const Z: [u8; 4] = a(NONE, NONE, NONE, NONE);

/// Arms (left, right, up, down) of U+2500 to U+257F that are drawn from straight arms. Dashes,
/// rounded corners and diagonals are zero rows because they are drawn by their own functions.
#[rustfmt::skip]
const ARMS: [[u8; 4]; 128] = [
    a(1,1,0,0), a(2,2,0,0), a(0,0,1,1), a(0,0,2,2), Z, Z, Z, Z, Z, Z, Z, Z,
    a(0,1,0,1), a(0,2,0,1), a(0,1,0,2), a(0,2,0,2),
    a(1,0,0,1), a(2,0,0,1), a(1,0,0,2), a(2,0,0,2),
    a(0,1,1,0), a(0,2,1,0), a(0,1,2,0), a(0,2,2,0),
    a(1,0,1,0), a(2,0,1,0), a(1,0,2,0), a(2,0,2,0),
    a(0,1,1,1), a(0,2,1,1), a(0,1,2,1), a(0,1,1,2), a(0,1,2,2), a(0,2,2,1), a(0,2,1,2), a(0,2,2,2),
    a(1,0,1,1), a(2,0,1,1), a(1,0,2,1), a(1,0,1,2), a(1,0,2,2), a(2,0,2,1), a(2,0,1,2), a(2,0,2,2),
    a(1,1,0,1), a(2,1,0,1), a(1,2,0,1), a(2,2,0,1), a(1,1,0,2), a(2,1,0,2), a(1,2,0,2), a(2,2,0,2),
    a(1,1,1,0), a(2,1,1,0), a(1,2,1,0), a(2,2,1,0), a(1,1,2,0), a(2,1,2,0), a(1,2,2,0), a(2,2,2,0),
    a(1,1,1,1), a(2,1,1,1), a(1,2,1,1), a(2,2,1,1), a(1,1,2,1), a(1,1,1,2), a(1,1,2,2),
    a(2,1,2,1), a(1,2,2,1), a(2,1,1,2), a(1,2,1,2),
    a(2,2,2,1), a(2,2,1,2), a(2,1,2,2), a(1,2,2,2), a(2,2,2,2),
    Z, Z, Z, Z,
    a(3,3,0,0), a(0,0,3,3),
    a(0,3,0,1), a(0,1,0,3), a(0,3,0,3), a(3,0,0,1), a(1,0,0,3), a(3,0,0,3),
    a(0,3,1,0), a(0,1,3,0), a(0,3,3,0), a(3,0,1,0), a(1,0,3,0), a(3,0,3,0),
    a(0,3,1,1), a(0,1,3,3), a(0,3,3,3), a(3,0,1,1), a(1,0,3,3), a(3,0,3,3),
    a(3,3,0,1), a(1,1,0,3), a(3,3,0,3), a(3,3,1,0), a(1,1,3,0), a(3,3,3,0),
    a(3,3,1,1), a(1,1,3,3), a(3,3,3,3),
    Z, Z, Z, Z, Z, Z, Z,
    a(1,0,0,0), a(0,0,1,0), a(0,1,0,0), a(0,0,0,1),
    a(2,0,0,0), a(0,0,2,0), a(0,2,0,0), a(0,0,0,2),
    a(1,2,0,0), a(0,0,1,2), a(2,1,0,0), a(0,0,2,1),
];

/// Quadrant masks for U+2596 to U+259F (bit 0 upper left, 1 upper right, 2 lower left, 3 lower right).
const QUADRANTS: [u8; 10] = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14];

/// True when `c` is drawn by this module instead of from a font.
pub fn is_sprite(c: char) -> bool {
    matches!(c as u32, 0x2500..=0x259F | 0x2800..=0x28FF | 0x1FB00..=0x1FB3B | 0xE0B0..=0xE0BF)
}

/// The shapes for `c` in a cell of `width` by `height` pixels with line thickness `light` pixels.
/// Heavy lines are twice `light` and double lines are two light lines one light thickness apart.
/// Returns None when `c` is not a sprite. Sizes below 1 are treated as 1.
pub fn sprite(c: char, width: u32, height: u32, light: u32) -> Option<Vec<SpriteShape>> {
    let (w, h, light) = (width.max(1), height.max(1), light.max(1));
    let cp = c as u32;
    Some(match cp {
        0x2500..=0x257F => box_drawing(cp, w, h, light),
        0x2580..=0x259F => block_element(cp, w, h),
        0x2800..=0x28FF => braille(cp, w, h),
        0x1FB00..=0x1FB3B => sextant(cp, w, h),
        0xE0B0..=0xE0BF => powerline(cp, w, h, light),
        _ => return None,
    })
}

/// Converts a pixel count or coordinate to f64.
fn f(v: u32) -> f64 {
    f64::from(v)
}

/// Builds a rectangle from pixel coordinates.
fn rect(x0: u32, y0: u32, x1: u32, y1: u32) -> Rect {
    Rect::new(f(x0), f(y0), f(x1), f(y1))
}

/// Adds a rectangle to `out` unless it is empty.
fn push_rect(out: &mut Vec<SpriteShape>, x0: u32, y0: u32, x1: u32, y1: u32) {
    if x1 > x0 && y1 > y0 {
        out.push(SpriteShape::Rect(rect(x0, y0, x1, y1)));
    }
}

/// Adds a rectangle given along the main axis and across it; `horizontal` means main is x.
fn push_oriented(out: &mut Vec<SpriteShape>, horizontal: bool, main: (u32, u32), cross: (u32, u32)) {
    if horizontal {
        push_rect(out, main.0, cross.0, main.1, cross.1);
    } else {
        push_rect(out, cross.0, main.0, cross.1, main.1);
    }
}

/// Returns `k / parts` of `size` rounded half up, in whole pixels.
fn edge(k: u32, size: u32, parts: u32) -> u32 {
    (2 * k * size + parts) / (2 * parts)
}

/// Returns the extent of `n` eighths of `size` measured from the start, at least one pixel.
fn eighths(n: u32, size: u32) -> u32 {
    edge(n, size, 8).clamp(1, size)
}

/// Returns where `n` eighths measured from the end of `size` begin, leaving at least one pixel.
fn eighths_from_end(n: u32, size: u32) -> u32 {
    edge(8 - n, size, 8).min(size - 1)
}

/// Returns the start and end of a band of `span` pixels centred in `size`.
fn centered(span: u32, size: u32) -> (u32, u32) {
    let span = span.min(size);
    let lo = (size - span) / 2;
    (lo, lo + span)
}

/// Returns the thickness in pixels of one stroke of a line style when measured across `size`.
fn unit(style: u8, light: u32, size: u32) -> u32 {
    match style {
        NONE => 0,
        LIGHT => light.min(size),
        HEAVY => (2 * light).min(size),
        _ => light.min((size / 3).max(1)),
    }
}

/// Returns the total thickness of a line style across `size` (three strokes' worth for a double).
fn span(style: u8, light: u32, size: u32) -> u32 {
    let u = unit(style, light, size);
    if style == DOUBLE { (3 * u).min(size) } else { u }
}

/// Draws the box drawing character `cp` (U+2500 to U+257F).
fn box_drawing(cp: u32, w: u32, h: u32, light: u32) -> Vec<SpriteShape> {
    let mut out = Vec::new();
    match cp {
        0x2504..=0x250B => {
            dashes(&mut out, ((cp - 0x2504) / 2).is_multiple_of(2), (3 + (cp - 0x2504) / 4, cp % 2 == 1), (w, h), light)
        }
        0x254C..=0x254F => dashes(&mut out, cp < 0x254E, (2, cp % 2 == 1), (w, h), light),
        0x256D..=0x2570 => out.push(rounded_corner(cp, w, h, light)),
        0x2571..=0x2573 => diagonals(&mut out, cp, w, h, light),
        _ => straight_arms(&mut out, ARMS[(cp - 0x2500) as usize], w, h, light),
    }
    out
}

/// Draws a dashed line. `dash` is (count, heavy) and `size` is (width, height) of the cell.
fn dashes(out: &mut Vec<SpriteShape>, horizontal: bool, dash: (u32, bool), size: (u32, u32), light: u32) {
    let (n, heavy) = dash;
    let (main, cross) = if horizontal { size } else { (size.1, size.0) };
    let thick = if heavy { 2 * light } else { light };
    let band = centered(thick, cross);
    for i in 0..n {
        let (p0, p1) = (edge(i, main, n), edge(i + 1, main, n));
        let gap = if p1 - p0 >= 3 { ((p1 - p0) / 4).max(1) } else { 0 };
        let (a0, a1) = if p1 - p0 > 2 * gap { (p0 + gap, p1 - gap) } else { (p0, p1) };
        push_oriented(out, horizontal, (a0, a1), band);
    }
}

/// The styles of the four arms of a junction and where the arm being drawn sits.
struct ArmInput {
    own: u8,
    opposite: u8,
    side_a: u8,
    side_b: u8,
    is_high: bool,
}

/// Draws every arm of a junction from the left, right, up, down styles in `arms`.
fn straight_arms(out: &mut Vec<SpriteShape>, arms: [u8; 4], w: u32, h: u32, light: u32) {
    let [l, r, u, d] = arms;
    let horizontal = [(l, r, false), (r, l, true)];
    for (own, opposite, is_high) in horizontal {
        if own != NONE {
            let input = ArmInput { own, opposite, side_a: u, side_b: d, is_high };
            emit_arm(out, true, &input, (w, h), light);
        }
    }
    let vertical = [(u, d, false), (d, u, true)];
    for (own, opposite, is_high) in vertical {
        if own != NONE {
            let input = ArmInput { own, opposite, side_a: l, side_b: r, is_high };
            emit_arm(out, false, &input, (h, w), light);
        }
    }
}

/// Returns where an arm stops (low arm) or starts (high arm) along its axis when it meets the
/// perpendicular arm of style `p`, or None when there is no such arm.
fn meet_point(is_high: bool, p: u8, light: u32, main: u32) -> Option<u32> {
    if p == NONE {
        return None;
    }
    let (lo, hi) = centered(span(p, light, main), main);
    let u = unit(p, light, main);
    Some(match (p == DOUBLE, is_high) {
        (true, false) => lo + u,
        (true, true) => hi - u,
        (false, false) => hi,
        (false, true) => lo,
    })
}

/// Draws one arm: a single stroke for light and heavy lines, two strokes for a double line.
/// `size` is (length along the arm's axis, length across it).
fn emit_arm(out: &mut Vec<SpriteShape>, horizontal: bool, input: &ArmInput, size: (u32, u32), light: u32) {
    let (main, cross) = size;
    let perp_span = span(input.side_a, light, main).max(span(input.side_b, light, main));
    let (plo, phi) = centered(perp_span, main);
    let centre = if input.is_high { main / 2 } else { main.div_ceil(2) };
    let perp_edge = match (perp_span > 0, input.is_high) {
        (false, _) => centre,
        (true, false) => phi,
        (true, true) => plo,
    };
    let along = |p: u32| if input.is_high { (p, main) } else { (0, p) };
    if input.own != DOUBLE {
        let band = centered(span(input.own, light, cross), cross);
        push_oriented(out, horizontal, along(perp_edge), band);
        return;
    }
    let u = unit(DOUBLE, light, cross);
    let (clo, _) = centered(3 * u, cross);
    let fallback = if input.opposite != NONE { centre } else { perp_edge };
    for (side, p_style) in [input.side_a, input.side_b].into_iter().enumerate() {
        let p = meet_point(input.is_high, p_style, light, main).unwrap_or(fallback);
        let c0 = clo + 2 * u * side as u32;
        push_oriented(out, horizontal, along(p), (c0.min(cross), (c0 + u).min(cross)));
    }
}

/// Describes a quarter of an ellipse: its centre, the direction it extends in and its radii.
struct Quarter {
    cx: f64,
    cy: f64,
    dx: f64,
    dy: f64,
    rx: f64,
    ry: f64,
}

/// Appends the cubic curve for `q`. Forward runs from (cx, cy + dy*ry) to (cx + dx*rx, cy) and
/// backward runs the other way. The path's current point must already be the start.
fn push_quarter(path: &mut BezPath, q: &Quarter, forward: bool) {
    let a = Point::new(q.cx, q.cy + q.dy * q.ry);
    let b = Point::new(q.cx + q.dx * q.rx, q.cy);
    let c1 = Point::new(q.cx + q.dx * KAPPA * q.rx, q.cy + q.dy * q.ry);
    let c2 = Point::new(q.cx + q.dx * q.rx, q.cy + q.dy * KAPPA * q.ry);
    if forward {
        path.curve_to(c1, c2, b);
    } else {
        path.curve_to(c2, c1, a);
    }
}

/// Draws a rounded corner (U+256D to U+2570) as a quarter circle band that meets the edge pixels
/// of the straight lines exactly.
fn rounded_corner(cp: u32, w: u32, h: u32, light: u32) -> SpriteShape {
    let (right, down) = match cp {
        0x256D => (true, true),
        0x256E => (false, true),
        0x256F => (false, false),
        _ => (true, false),
    };
    let (x0, x1) = centered(light.min(w), w);
    let (y0, y1) = centered(light.min(h), h);
    let (cx, cy) = (if right { f(w) } else { 0.0 }, if down { f(h) } else { 0.0 });
    let (dx, dy) = (if right { -1.0 } else { 1.0 }, if down { -1.0 } else { 1.0 });
    let (xa, xb) = ((cx - f(x0)).abs(), (cx - f(x1)).abs());
    let (ya, yb) = ((cy - f(y0)).abs(), (cy - f(y1)).abs());
    let outer = Quarter { cx, cy, dx, dy, rx: xa.max(xb), ry: ya.max(yb) };
    let inner = Quarter { cx, cy, dx, dy, rx: xa.min(xb), ry: ya.min(yb) };
    let mut path = BezPath::new();
    path.move_to((cx, cy + dy * outer.ry));
    push_quarter(&mut path, &outer, true);
    path.line_to((cx + dx * inner.rx, cy));
    push_quarter(&mut path, &inner, false);
    path.close_path();
    SpriteShape::Path(path)
}

/// Draws U+2571 (rising), U+2572 (falling) and U+2573 (both) diagonals.
fn diagonals(out: &mut Vec<SpriteShape>, cp: u32, w: u32, h: u32, light: u32) {
    let (wf, hf, t) = (f(w), f(h), f(light));
    if cp != 0x2572 {
        out.extend(diagonal_band(Point::new(0.0, hf), Point::new(wf, 0.0), t, (wf, hf)));
    }
    if cp != 0x2571 {
        out.extend(diagonal_band(Point::new(0.0, 0.0), Point::new(wf, hf), t, (wf, hf)));
    }
}

/// Builds a line of thickness `t` from `p0` to `p1`, extended past both ends and clipped to the cell.
fn diagonal_band(p0: Point, p1: Point, t: f64, cell: (f64, f64)) -> Option<SpriteShape> {
    let d = p1 - p0;
    let len = d.hypot();
    if len == 0.0 {
        return None;
    }
    let u = d / len;
    let n = kurbo::Vec2::new(-u.y, u.x) * (t / 2.0);
    let (s, e) = (p0 - u * t, p1 + u * t);
    clipped_polygon(&[s + n, e + n, e - n, s - n], cell)
}

/// Clips a polygon to the cell rectangle and returns it as a path, or None when it has no area.
fn clipped_polygon(points: &[Point], cell: (f64, f64)) -> Option<SpriteShape> {
    let (buf, n) = clip_polygon(points, cell);
    (n >= 3).then(|| SpriteShape::Path(polygon_path(&buf[..n])))
}

/// Builds a closed path through `points`.
fn polygon_path(points: &[Point]) -> BezPath {
    let mut path = BezPath::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            path.move_to(*p);
        } else {
            path.line_to(*p);
        }
    }
    path.close_path();
    path
}

/// Clips a polygon against the four sides of the cell (Sutherland and Hodgman).
fn clip_polygon(points: &[Point], cell: (f64, f64)) -> ([Point; CLIP_CAP], usize) {
    let planes = [(-1.0, 0.0, 0.0), (1.0, 0.0, cell.0), (0.0, -1.0, 0.0), (0.0, 1.0, cell.1)];
    let mut cur = [Point::ZERO; CLIP_CAP];
    let mut n = points.len().min(CLIP_CAP);
    cur[..n].copy_from_slice(&points[..n]);
    for plane in planes {
        (cur, n) = clip_plane(&cur[..n], plane);
    }
    for p in &mut cur[..n] {
        p.x = p.x.clamp(0.0, cell.0);
        p.y = p.y.clamp(0.0, cell.1);
    }
    (cur, n)
}

/// Keeps the part of a polygon where `a*x + b*y - c <= 0`.
fn clip_plane(input: &[Point], plane: (f64, f64, f64)) -> ([Point; CLIP_CAP], usize) {
    let (a, b, c) = plane;
    let mut out = [Point::ZERO; CLIP_CAP];
    let mut m = 0;
    for (i, &p) in input.iter().enumerate() {
        let q = input[(i + 1) % input.len()];
        let (fp, fq) = (a * p.x + b * p.y - c, a * q.x + b * q.y - c);
        if fp <= 0.0 && m < CLIP_CAP {
            out[m] = p;
            m += 1;
        }
        if (fp < 0.0 && fq > 0.0 || fp > 0.0 && fq < 0.0) && m < CLIP_CAP {
            out[m] = p + (q - p) * (fp / (fp - fq));
            m += 1;
        }
    }
    (out, m)
}

/// Draws the block element `cp` (U+2580 to U+259F).
fn block_element(cp: u32, w: u32, h: u32) -> Vec<SpriteShape> {
    let mut out = Vec::new();
    match cp {
        0x2580 => push_rect(&mut out, 0, 0, w, eighths(4, h)),
        0x2581..=0x2587 => push_rect(&mut out, 0, eighths_from_end(cp - 0x2580, h), w, h),
        0x2588 => push_rect(&mut out, 0, 0, w, h),
        0x2589..=0x258F => push_rect(&mut out, 0, 0, eighths(0x2590 - cp, w), h),
        0x2590 => push_rect(&mut out, eighths_from_end(4, w), 0, w, h),
        0x2591..=0x2593 => {
            let alpha = 0.25 * (cp - 0x2590) as f32;
            out.push(SpriteShape::Shade { rect: rect(0, 0, w, h), alpha });
        }
        0x2594 => push_rect(&mut out, 0, 0, w, eighths(1, h)),
        0x2595 => push_rect(&mut out, eighths_from_end(1, w), 0, w, h),
        _ => quadrants(&mut out, QUADRANTS[(cp - 0x2596) as usize], w, h),
    }
    out
}

/// Draws the quadrants set in `mask` (bit 0 upper left, 1 upper right, 2 lower left, 3 lower right).
fn quadrants(out: &mut Vec<SpriteShape>, mask: u8, w: u32, h: u32) {
    let (xm, ym) = (edge(1, w, 2), edge(1, h, 2));
    for bit in 0..4 {
        if mask >> bit & 1 == 1 {
            let (x0, x1) = if bit % 2 == 0 { (0, xm) } else { (xm, w) };
            let (y0, y1) = if bit < 2 { (0, ym) } else { (ym, h) };
            push_rect(out, x0, y0, x1, y1);
        }
    }
}

/// Draws the braille pattern `cp` (U+2800 to U+28FF): one small square for each set dot.
fn braille(cp: u32, w: u32, h: u32) -> Vec<SpriteShape> {
    let mut out = Vec::new();
    for bit in 0..8u32 {
        if (cp - 0x2800) >> bit & 1 == 1 {
            let (col, row) = match bit {
                0..=2 => (0, bit),
                3..=5 => (1, bit - 3),
                6 => (0, 3),
                _ => (1, 3),
            };
            out.push(SpriteShape::Rect(braille_dot(col, row, w, h)));
        }
    }
    out
}

/// Returns the square for the braille dot in column `col` (0 or 1) and row `row` (0 to 3).
fn braille_dot(col: u32, row: u32, w: u32, h: u32) -> Rect {
    let (wf, hf) = (f(w), f(h));
    let side = ((wf / 2.0).min(hf / 4.0) * 0.55).round().clamp(1.0, wf.min(hf));
    let cx = wf * f(2 * col + 1) / 4.0;
    let cy = hf * f(2 * row + 1) / 8.0;
    let x0 = (cx - side / 2.0).round().clamp(0.0, wf - side);
    let y0 = (cy - side / 2.0).round().clamp(0.0, hf - side);
    Rect::new(x0, y0, x0 + side, y0 + side)
}

/// Draws the sextant `cp` (U+1FB00 to U+1FB3B), a 2 by 3 block pattern.
fn sextant(cp: u32, w: u32, h: u32) -> Vec<SpriteShape> {
    let mut pattern = cp - 0x1FB00 + 1;
    if pattern >= 21 {
        pattern += 1;
    }
    if pattern >= 42 {
        pattern += 1;
    }
    let xm = edge(1, w, 2);
    let ys = [0, edge(1, h, 3), edge(2, h, 3), h];
    let mut out = Vec::new();
    for bit in 0..6usize {
        if pattern >> bit & 1 == 1 {
            let (x0, x1) = if bit % 2 == 0 { (0, xm) } else { (xm, w) };
            push_rect(&mut out, x0, ys[bit / 2], x1, ys[bit / 2 + 1]);
        }
    }
    out
}

/// Draws the Powerline separator `cp` (U+E0B0 to U+E0BF).
fn powerline(cp: u32, w: u32, h: u32, light: u32) -> Vec<SpriteShape> {
    let (wf, hf, t) = (f(w), f(h), f(light.min(w).min(h)));
    if cp >= 0xE0B8 {
        return diagonal_separator(cp - 0xE0B8, (wf, hf), t);
    }
    let mut out = match cp & !2 {
        0xE0B0 => vec![SpriteShape::Path(polygon_path(&[
            Point::new(0.0, 0.0),
            Point::new(wf, hf / 2.0),
            Point::new(0.0, hf),
        ]))],
        0xE0B1 => chevron(wf, hf, t),
        0xE0B4 => vec![SpriteShape::Path(half_ellipse(wf, hf, t, true))],
        _ => vec![SpriteShape::Path(half_ellipse(wf, hf, t, false))],
    };
    if cp & 2 != 0 {
        let mirror = Affine::new([-1.0, 0.0, 0.0, 1.0, wf, 0.0]);
        for shape in &mut out {
            if let SpriteShape::Path(p) = shape {
                p.apply_affine(mirror);
            }
        }
    }
    out
}

/// Draws the outline of a right pointing triangle as two lines meeting at the tip.
fn chevron(wf: f64, hf: f64, t: f64) -> Vec<SpriteShape> {
    let tip = Point::new(wf, hf / 2.0);
    let cell = (wf, hf);
    let upper = diagonal_band(Point::new(0.0, 0.0), tip, t, cell);
    let lower = diagonal_band(tip, Point::new(0.0, hf), t, cell);
    upper.into_iter().chain(lower).collect()
}

/// Draws a half ellipse bulging to the right, filled or as an outline of thickness `t`.
fn half_ellipse(wf: f64, hf: f64, t: f64, filled: bool) -> BezPath {
    let cy = hf / 2.0;
    let quarter = |dy: f64, rx: f64, ry: f64| Quarter { cx: 0.0, cy, dx: 1.0, dy, rx, ry };
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    push_quarter(&mut path, &quarter(-1.0, wf, cy), true);
    push_quarter(&mut path, &quarter(1.0, wf, cy), false);
    if !filled {
        let (rx, ry) = ((wf - t).max(0.0), (cy - t).max(0.0));
        path.line_to((0.0, cy + ry));
        push_quarter(&mut path, &quarter(1.0, rx, ry), true);
        push_quarter(&mut path, &quarter(-1.0, rx, ry), false);
    }
    path.close_path();
    path
}

/// Draws the diagonal separators U+E0B8 to U+E0BF, given their index from U+E0B8: even indexes
/// are filled corner triangles and odd indexes are diagonal lines.
fn diagonal_separator(index: u32, cell: (f64, f64), t: f64) -> Vec<SpriteShape> {
    let (wf, hf) = cell;
    let (tl, tr, bl, br) = (Point::new(0.0, 0.0), Point::new(wf, 0.0), Point::new(0.0, hf), Point::new(wf, hf));
    let triangle = |pts: [Point; 3]| vec![SpriteShape::Path(polygon_path(&pts))];
    match index {
        0 => triangle([tl, bl, br]),
        2 => triangle([tr, br, bl]),
        4 => triangle([tl, tr, bl]),
        6 => triangle([tl, tr, br]),
        1 | 7 => diagonal_band(tl, br, t, cell).into_iter().collect(),
        _ => diagonal_band(bl, tr, t, cell).into_iter().collect(),
    }
}
