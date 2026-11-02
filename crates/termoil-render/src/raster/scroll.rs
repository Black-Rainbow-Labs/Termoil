//! Scroll detection for the raster renderer: when every row still on screen shows what the row k
//! below it showed last frame, the pixels are moved up k rows and only the k new rows are drawn.
//! Streaming output scrolls on almost every frame, so this is most of what a frame costs then.

use crate::frame::{Frame, GlyphRun, RowFrame};

/// Whether `a` draws exactly what `b` drew, `dy` pixels lower (`dy` is negative for a move up).
fn same_moved(a: &RowFrame, b: &RowFrame, dy: f32) -> bool {
    a.backgrounds == b.backgrounds
        && a.runs.len() == b.runs.len()
        && a.sprites.len() == b.sprites.len()
        && a.decorations.len() == b.decorations.len()
        && a.y == b.y + dy
        && a.runs.iter().zip(&b.runs).all(|(r, s)| same_run_moved(r, s, dy))
        && a.sprites
            .iter()
            .zip(&b.sprites)
            .all(|(s, t)| s.ch == t.ch && s.x == t.x && s.color == t.color && s.y == t.y + dy)
        && a.decorations.iter().zip(&b.decorations).all(|(d, e)| {
            d.kind == e.kind
                && d.x0 == e.x0
                && d.x1 == e.x1
                && d.thickness == e.thickness
                && d.color == e.color
                && d.y == e.y + dy
        })
}

/// Whether run `a` is run `b` moved down by `dy`.
fn same_run_moved(a: &GlyphRun, b: &GlyphRun, dy: f32) -> bool {
    a.face == b.face
        && a.size == b.size
        && a.color == b.color
        && a.embolden == b.embolden
        && a.skew == b.skew
        && a.glyphs.len() == b.glyphs.len()
        && a.glyphs.iter().zip(&b.glyphs).all(|(g, h)| g.id == h.id && g.x == h.x && g.y == h.y + dy)
}

/// How many rows the frame's content moved up since `previous` was drawn: the smallest k for which
/// every row i below the last k shows what row i + k showed. Zero when there is no such k.
pub fn scrolled_rows(f: &Frame, previous: &[RowFrame]) -> usize {
    let n = f.rows.len();
    if n < 2 || previous.len() != n {
        return 0;
    }
    let ch = f.cell_height as f32;
    (1..n)
        .find(|&k| {
            let dy = -(k as f32) * ch;
            (0..n - k).all(|i| same_moved(&f.rows[i], &previous[i + k], dy))
        })
        .unwrap_or(0)
}

/// Moves the pixels of `x0..x1` by `rows` lines from `src_top` to `dst_top`, for `lines` lines, in a
/// framebuffer `width` pixels wide. The source is below the destination.
pub fn move_up(px: &mut [u32], width: usize, (x0, x1): (usize, usize), src_top: usize, dst_top: usize, lines: usize) {
    for i in 0..lines {
        let (s, d) = ((src_top + i) * width, (dst_top + i) * width);
        px.copy_within(s + x0..s + x1, d + x0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{GlyphPos, Rgba};
    use termoil_font::FaceId;

    /// A row at `y` with one glyph `id`.
    fn row(y: f32, id: u32) -> RowFrame {
        let glyphs = vec![GlyphPos { id, x: 2.0, y: y + 12.0 }];
        let run =
            GlyphRun { face: FaceId(0), size: 13.0, color: Rgba::rgb(1, 2, 3), embolden: false, skew: false, glyphs };
        RowFrame { row: 0, y, runs: vec![run], ..RowFrame::default() }
    }

    /// Rows that moved up two lines are found as a scroll of two; changed rows are not a scroll.
    #[test]
    fn a_two_line_scroll_is_found() {
        let before: Vec<RowFrame> = (0..5).map(|i| row(i as f32 * 16.0, i)).collect();
        let after: Vec<RowFrame> = (0..5).map(|i| row(i as f32 * 16.0, i + 2)).collect();
        let f = Frame { cell_height: 16, rows: after, ..Frame::default() };
        assert_eq!(scrolled_rows(&f, &before), 2);
        let g =
            Frame { cell_height: 16, rows: (0..5).map(|i| row(i as f32 * 16.0, 9 + i)).collect(), ..Frame::default() };
        assert_eq!(scrolled_rows(&g, &before), 0);
    }
}
