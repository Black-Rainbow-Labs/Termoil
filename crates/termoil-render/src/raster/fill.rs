//! Filling a 0RGB framebuffer: blending one colour at a coverage, rectangles with antialiased
//! edges, coverage masks, and paths through a signed area accumulation rasterizer.
//!
//! Blending is done on the encoded sRGB values, as vello does, so text weight matches the vello
//! renderers.

use vello_common::kurbo::{BezPath, PathEl, Point, Rect};

use crate::frame::Rgba;

/// A framebuffer of 0RGB words and the rectangle drawing is clipped to.
pub struct Target<'a> {
    /// The pixels, row major.
    pub px: &'a mut [u32],
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Drawing is clipped to x0..x1, y0..y1.
    pub clip: (i32, i32, i32, i32),
}

/// A colour as a 0RGB word.
pub fn word(c: Rgba) -> u32 {
    (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

/// `x * a / 255`, rounded, for x and a in 0..=255.
#[inline]
fn mul255(x: u32, a: u32) -> u32 {
    let t = x * a + 128;
    (t + (t >> 8)) >> 8
}

/// `dst` with `src` (0RGB) laid over it at coverage `a` (0..=255).
#[inline]
pub fn blend(dst: u32, src: u32, a: u32) -> u32 {
    if a >= 255 {
        return src;
    }
    if a == 0 {
        return dst;
    }
    let inv = 255 - a;
    let ch = |s: u32| mul255((src >> s) & 0xff, a) + mul255((dst >> s) & 0xff, inv);
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// `dst` with a premultiplied pixel (alpha in the top byte) laid over it.
#[inline]
pub fn over_premultiplied(dst: u32, src: u32) -> u32 {
    let a = src >> 24;
    if a == 0 {
        return dst;
    }
    let inv = 255 - a;
    let ch = |s: u32| (((src >> s) & 0xff) + mul255((dst >> s) & 0xff, inv)).min(255);
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

impl Target<'_> {
    /// The clip intersected with the framebuffer: x0, y0, x1, y1.
    fn bounds(&self) -> (i32, i32, i32, i32) {
        let (x0, y0, x1, y1) = self.clip;
        (x0.max(0), y0.max(0), x1.min(self.width as i32), y1.min(self.height as i32))
    }

    /// Fills `r` with `color`, antialiasing fractional edges.
    pub fn fill_rect(&mut self, r: Rect, color: Rgba) {
        let (cx0, cy0, cx1, cy1) = self.bounds();
        let (x0, x1) = (r.x0.max(cx0 as f64), r.x1.min(cx1 as f64));
        let (y0, y1) = (r.y0.max(cy0 as f64), r.y1.min(cy1 as f64));
        if x0 >= x1 || y0 >= y1 || color.a == 0 {
            return;
        }
        let src = word(color);
        if color.a == 255 && [x0, x1, y0, y1].iter().all(|v| v.fract() == 0.0) {
            for y in y0 as usize..y1 as usize {
                self.px[y * self.width + x0 as usize..y * self.width + x1 as usize].fill(src);
            }
            return;
        }
        for y in y0.floor() as i32..y1.ceil() as i32 {
            let cov_y = (y1.min(y as f64 + 1.0) - y0.max(y as f64)).clamp(0.0, 1.0);
            let line = y as usize * self.width;
            for x in x0.floor() as i32..x1.ceil() as i32 {
                let cov_x = (x1.min(x as f64 + 1.0) - x0.max(x as f64)).clamp(0.0, 1.0);
                let a = (cov_x * cov_y * f64::from(color.a)).round() as u32;
                let p = &mut self.px[line + x as usize];
                *p = blend(*p, src, a);
            }
        }
    }

    /// Lays a coverage mask (`w` by `h`, one byte per pixel) at `(left, top)` in `color`.
    pub fn fill_mask(&mut self, mask: &[u8], w: usize, h: usize, left: i32, top: i32, color: Rgba) {
        let (cx0, cy0, cx1, cy1) = self.bounds();
        let (xs, xe) = ((cx0 - left).max(0), (cx1 - left).min(w as i32));
        let (ys, ye) = ((cy0 - top).max(0), (cy1 - top).min(h as i32));
        let src = word(color);
        let alpha = u32::from(color.a);
        for my in ys..ye {
            let row = &mask[my as usize * w..my as usize * w + w];
            let line = (top + my) as usize * self.width;
            for mx in xs..xe {
                let m = u32::from(row[mx as usize]);
                if m != 0 {
                    let p = &mut self.px[line + (left + mx) as usize];
                    *p = blend(*p, src, if alpha == 255 { m } else { mul255(m, alpha) });
                }
            }
        }
    }

    /// Lays premultiplied pixels (`w` by `h`, alpha in the top byte) at `(left, top)`.
    pub fn draw_premultiplied(&mut self, pixels: &[u32], w: usize, h: usize, left: i32, top: i32) {
        let (cx0, cy0, cx1, cy1) = self.bounds();
        let (xs, xe) = ((cx0 - left).max(0), (cx1 - left).min(w as i32));
        let (ys, ye) = ((cy0 - top).max(0), (cy1 - top).min(h as i32));
        for my in ys..ye {
            let line = (top + my) as usize * self.width;
            for mx in xs..xe {
                let p = &mut self.px[line + (left + mx) as usize];
                *p = over_premultiplied(*p, pixels[my as usize * w + mx as usize]);
            }
        }
    }

    /// Fills `path` (already in framebuffer pixels) with the non zero rule in `color`.
    pub fn fill_path(&mut self, path: &BezPath, color: Rgba) {
        let b = path_bounds(path);
        let (cx0, cy0, cx1, cy1) = self.bounds();
        let (x0, y0) = ((b.x0.floor() as i32).max(cx0), (b.y0.floor() as i32).max(cy0));
        let (x1, y1) = ((b.x1.ceil() as i32).min(cx1), (b.y1.ceil() as i32).min(cy1));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mask = rasterize(path, (f64::from(x0), f64::from(y0)), w, h);
        self.fill_mask(&mask, w, h, x0, y0, color);
    }
}

/// The bounding box of a path's points (control points included, which only makes it larger).
fn path_bounds(path: &BezPath) -> Rect {
    let mut b = Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for el in path.elements() {
        let pts: &[Point] = match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => std::slice::from_ref(p),
            PathEl::QuadTo(a, p) => &[*a, *p],
            PathEl::CurveTo(a, c, p) => &[*a, *c, *p],
            PathEl::ClosePath => &[],
        };
        for p in pts {
            b = Rect::new(b.x0.min(p.x), b.y0.min(p.y), b.x1.max(p.x), b.y1.max(p.y));
        }
    }
    b
}

/// The coverage of `path` over a `w` by `h` pixel area whose top left is `origin`.
pub fn rasterize(path: &BezPath, origin: (f64, f64), w: usize, h: usize) -> Vec<u8> {
    let mut acc = vec![0f32; w * h + 1];
    let mut first = Point::ZERO;
    let mut last = Point::ZERO;
    let local = |p: Point| Point::new(p.x - origin.0, p.y - origin.1);
    vello_common::kurbo::flatten(path.elements().iter().copied(), 0.1, |el| match el {
        PathEl::MoveTo(p) => {
            if last != first {
                line(&mut acc, w, h, last, first);
            }
            first = local(p);
            last = first;
        }
        PathEl::LineTo(p) => {
            let p = local(p);
            line(&mut acc, w, h, last, p);
            last = p;
        }
        PathEl::ClosePath => {
            line(&mut acc, w, h, last, first);
            last = first;
        }
        _ => {}
    });
    if last != first {
        line(&mut acc, w, h, last, first);
    }
    let mut sum = 0f32;
    acc[..w * h]
        .iter()
        .map(|a| {
            sum += a;
            (sum.abs().min(1.0) * 255.0 + 0.5) as u8
        })
        .collect()
}

/// Adds the signed area a line from `p0` to `p1` covers to each pixel's accumulator (the method of
/// font rasterizers such as font-rs: a running sum of the accumulators along the buffer is the
/// coverage). Points are clamped into the area.
fn line(acc: &mut [f32], w: usize, h: usize, p0: Point, p1: Point) {
    let clamp = |p: Point| Point::new(p.x.clamp(0.0, w as f64 - 0.001), p.y.clamp(0.0, h as f64));
    let (p0, p1) = (clamp(p0), clamp(p1));
    if p0.y == p1.y {
        return;
    }
    let (dir, p0, p1) = if p0.y < p1.y { (1.0f32, p0, p1) } else { (-1.0f32, p1, p0) };
    let dxdy = ((p1.x - p0.x) / (p1.y - p0.y)) as f32;
    let mut x = p0.x as f32;
    for y in p0.y as usize..(p1.y.ceil() as usize).min(h) {
        let dy = ((y + 1) as f32).min(p1.y as f32) - (y as f32).max(p0.y as f32);
        let xnext = x + dxdy * dy;
        cover_span(&mut acc[y * w..], x, xnext, dy * dir);
        x = xnext;
    }
}

/// Adds the area of one scanline's piece of a line, from `x` to `xnext`, with signed height `d`.
fn cover_span(acc: &mut [f32], x: f32, xnext: f32, d: f32) {
    let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
    let x0floor = x0.floor();
    let x0i = x0floor as usize;
    let x1ceil = x1.ceil();
    let x1i = x1ceil as usize;
    if x1i <= x0i + 1 {
        let xmf = 0.5 * (x + xnext) - x0floor;
        acc[x0i] += d - d * xmf;
        acc[x0i + 1] += d * xmf;
        return;
    }
    let s = (x1 - x0).recip();
    let x0f = x0 - x0floor;
    let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
    let x1f = x1 - x1ceil + 1.0;
    let am = 0.5 * s * x1f * x1f;
    acc[x0i] += d * a0;
    if x1i == x0i + 2 {
        acc[x0i + 1] += d * (1.0 - a0 - am);
    } else {
        let a1 = s * (1.5 - x0f);
        acc[x0i + 1] += d * (a1 - a0);
        for a in &mut acc[x0i + 2..x1i - 1] {
            *a += d * s;
        }
        let a2 = a1 + (x1i - x0i - 3) as f32 * s;
        acc[x1i - 1] += d * (1.0 - a2 - am);
    }
    acc[x1i] += d * am;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target over a buffer, clipped to the whole of it.
    fn target(px: &mut [u32], w: usize, h: usize) -> Target<'_> {
        Target { px, width: w, height: h, clip: (0, 0, w as i32, h as i32) }
    }

    /// A pixel aligned rectangle sets exactly its pixels.
    #[test]
    fn aligned_rect_fills_its_pixels() {
        let mut px = vec![0u32; 16];
        target(&mut px, 4, 4).fill_rect(Rect::new(1.0, 1.0, 3.0, 2.0), Rgba::rgb(255, 0, 0));
        let red: Vec<usize> = px.iter().enumerate().filter(|(_, p)| **p == 0xff0000).map(|(i, _)| i).collect();
        assert_eq!(red, vec![5, 6]);
    }

    /// A square path covers its interior fully and nothing outside it.
    #[test]
    fn square_path_covers_its_interior() {
        let mut p = BezPath::new();
        p.move_to((1.0, 1.0));
        p.line_to((3.0, 1.0));
        p.line_to((3.0, 3.0));
        p.line_to((1.0, 3.0));
        p.close_path();
        let m = rasterize(&p, (0.0, 0.0), 4, 4);
        assert_eq!(m, vec![0, 0, 0, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 0]);
    }

    /// Half coverage blends halfway, and the clip stops drawing outside it.
    #[test]
    fn half_coverage_and_clip() {
        let mut px = vec![0u32; 4];
        let mut t = target(&mut px, 4, 1);
        t.clip = (0, 0, 2, 1);
        t.fill_rect(Rect::new(0.5, 0.0, 4.0, 1.0), Rgba::rgb(254, 254, 254));
        assert_eq!(px, vec![0x7f7f7f, 0xfefefe, 0, 0]);
    }
}
