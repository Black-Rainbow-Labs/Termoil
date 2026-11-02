//! Tests for the procedurally drawn sprite characters.

use kurbo::{Rect, Shape};
use termoil_font::sprite::{SpriteShape, is_sprite, sprite};

const SIZES: [(u32, u32); 5] = [(7, 15), (8, 16), (9, 19), (10, 22), (13, 27)];

/// Rasterises shapes into a grid of booleans, sampling each pixel centre. Row major, `h` rows.
fn raster(shapes: &[SpriteShape], w: u32, h: u32) -> Vec<Vec<bool>> {
    let mut grid = vec![vec![false; w as usize]; h as usize];
    for (y, row) in grid.iter_mut().enumerate() {
        for (x, lit) in row.iter_mut().enumerate() {
            let p = kurbo::Point::new(x as f64 + 0.5, y as f64 + 0.5);
            *lit = shapes.iter().any(|s| match s {
                SpriteShape::Rect(r) => r.contains(p),
                SpriteShape::Path(path) => path.contains(p),
                SpriteShape::Shade { rect, .. } => rect.contains(p),
            });
        }
    }
    grid
}

/// Rasterises the character at the given cell size.
fn draw(c: char, w: u32, h: u32, light: u32) -> Vec<Vec<bool>> {
    raster(&sprite(c, w, h, light).expect("sprite"), w, h)
}

/// Counts lit pixels in column `x`.
fn column_count(g: &[Vec<bool>], x: usize) -> usize {
    g.iter().filter(|row| row[x]).count()
}

/// Counts lit pixels in row `y`.
fn row_count(g: &[Vec<bool>], y: usize) -> usize {
    g[y].iter().filter(|&&b| b).count()
}

/// Counts 4 connected groups of lit pixels.
fn components(g: &[Vec<bool>]) -> usize {
    let (h, w) = (g.len(), g[0].len());
    let mut seen = vec![vec![false; w]; h];
    let mut count = 0;
    for y in 0..h {
        for x in 0..w {
            if g[y][x] && !seen[y][x] {
                count += 1;
                let mut stack = vec![(x, y)];
                seen[y][x] = true;
                while let Some((cx, cy)) = stack.pop() {
                    let near = [(cx.wrapping_sub(1), cy), (cx + 1, cy), (cx, cy.wrapping_sub(1)), (cx, cy + 1)];
                    for (nx, ny) in near {
                        if nx < w && ny < h && g[ny][nx] && !seen[ny][nx] {
                            seen[ny][nx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
    }
    count
}

/// Every code point in the covered ranges is a sprite and some outside them are not.
#[test]
fn coverage_ranges() {
    let ranges = [0x2500..=0x259F, 0x2800..=0x28FF, 0x1FB00..=0x1FB3B, 0xE0B0..=0xE0BF];
    for range in ranges {
        for cp in range {
            let c = char::from_u32(cp).unwrap();
            assert!(is_sprite(c), "{cp:X}");
            let shapes = sprite(c, 8, 16, 1).unwrap_or_else(|| panic!("{cp:X} is None"));
            assert!(!shapes.is_empty() || cp == 0x2800, "{cp:X} has no shapes");
        }
    }
    for cp in [0x41, 0x24FF, 0x25A0, 0x27FF, 0x2900, 0x1FAFF, 0x1FB3C, 0xE0A0, 0xE0A3, 0xE0AF, 0xE0C0, 0x1CD00] {
        let c = char::from_u32(cp).unwrap();
        assert!(!is_sprite(c), "{cp:X}");
        assert!(sprite(c, 8, 16, 1).is_none(), "{cp:X}");
    }
}

/// Horizontal lines reach both side edges and vertical lines reach top and bottom.
#[test]
fn lines_touch_edges() {
    for (w, h) in SIZES {
        for c in ['─', '━', '═'] {
            let g = draw(c, w, h, 1);
            let rows: Vec<usize> = (0..h as usize).filter(|&y| g[y][0]).collect();
            assert!(!rows.is_empty());
            for y in rows {
                assert_eq!(row_count(&g, y), w as usize, "{c} {w}x{h}");
            }
        }
        for c in ['│', '┃', '║'] {
            let g = draw(c, w, h, 1);
            let cols: Vec<usize> = (0..w as usize).filter(|&x| g[0][x]).collect();
            assert!(!cols.is_empty());
            for x in cols {
                assert_eq!(column_count(&g, x), h as usize, "{c} {w}x{h}");
            }
        }
    }
}

/// A row of horizontal line cells and a column of vertical line cells have no gaps at the seams.
#[test]
fn neighbours_join() {
    for (w, h) in SIZES {
        let g = draw('─', w, h, 2);
        let lit: Vec<usize> = (0..h as usize).filter(|&y| g[y][0]).collect();
        assert!(!lit.is_empty());
        assert!(lit.iter().all(|&y| g[y][w as usize - 1]));
        let g = draw('│', w, h, 2);
        let lit: Vec<usize> = (0..w as usize).filter(|&x| g[0][x]).collect();
        assert!(!lit.is_empty());
        assert!(lit.iter().all(|&x| g[h as usize - 1][x]));
    }
}

/// Light lines are exactly `light` pixels thick and heavy lines are within one pixel of twice that.
#[test]
fn line_thickness() {
    for (w, h) in SIZES {
        for light in [1u32, 2] {
            let cy = h as usize / 2;
            assert_eq!(column_count(&draw('─', w, h, light), 1), light as usize);
            assert_eq!(row_count(&draw('│', w, h, light), cy), light as usize);
            let heavy_h = column_count(&draw('━', w, h, light), 1) as u32;
            let heavy_v = row_count(&draw('┃', w, h, light), cy) as u32;
            for t in [heavy_h, heavy_v] {
                assert!((2 * light - 1..=2 * light + 1).contains(&t), "{w}x{h} light {light} heavy {t}");
            }
        }
    }
}

/// The full block covers every pixel and the upper half block covers the top (height + 1) / 2 rows.
#[test]
fn blocks() {
    for (w, h) in SIZES {
        let g = draw('█', w, h, 1);
        assert!(g.iter().all(|row| row.iter().all(|&b| b)));
        let g = draw('▀', w, h, 1);
        let top = h.div_ceil(2) as usize;
        for (y, row) in g.iter().enumerate() {
            assert_eq!(row.iter().all(|&b| b), y < top, "{w}x{h} row {y}");
            assert_eq!(row.iter().any(|&b| b), y < top);
        }
        let lower = draw('▄', w, h, 1);
        for y in 0..h as usize {
            assert_eq!(lower[y][0], !g[y][0], "halves must not overlap or leave a gap");
        }
    }
}

/// Shades are full cell rectangles with the quarter alpha steps.
#[test]
fn shades() {
    for (c, alpha) in [('░', 0.25f32), ('▒', 0.5), ('▓', 0.75)] {
        match sprite(c, 8, 16, 1).unwrap().as_slice() {
            [SpriteShape::Shade { rect, alpha: a }] => {
                assert_eq!(*rect, Rect::new(0.0, 0.0, 8.0, 16.0));
                assert_eq!(*a, alpha);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Braille U+28FF has eight separate dots, U+2801 only the top left dot, U+2800 nothing.
#[test]
fn braille_dots() {
    for (w, h) in SIZES {
        assert_eq!(components(&draw('\u{28FF}', w, h, 1)), 8, "{w}x{h}");
        let g = draw('\u{2801}', w, h, 1);
        assert_eq!(components(&g), 1);
        let lit: Vec<(usize, usize)> =
            (0..h as usize).flat_map(|y| (0..w as usize).map(move |x| (x, y))).filter(|&(x, y)| g[y][x]).collect();
        assert!(lit.iter().all(|&(x, y)| x < w as usize / 2 && y < h as usize / 4));
        assert!(draw('\u{2800}', w, h, 1).iter().all(|r| r.iter().all(|&b| !b)));
        let right_column = draw('\u{2808}', w, h, 1);
        assert!(right_column.iter().all(|r| r[..w as usize / 2].iter().all(|&b| !b)), "dot 4 is top right");
    }
}

/// Sextants U+1FB00 (upper left) and U+1FB3B (all but the upper left) have the expected pixels.
#[test]
fn sextants() {
    let g = draw('\u{1FB00}', 8, 15, 1);
    assert!(g[0][0] && !g[0][7] && !g[14][0] && !g[5][0]);
    let g = draw('\u{1FB3B}', 8, 15, 1);
    assert!(!g[0][0] && g[0][7] && g[14][0] && g[14][7] && g[8][0]);
}

/// Rounded corners join the straight lines of neighbouring cells at the edge pixels.
#[test]
fn rounded_corners_join() {
    for (w, h) in SIZES {
        let g = draw('╭', w, h, 1);
        let edge_row: Vec<usize> = (0..h as usize).filter(|&y| g[y][w as usize - 1]).collect();
        let straight: Vec<usize> = (0..h as usize).filter(|&y| draw('─', w, h, 1)[y][0]).collect();
        assert_eq!(edge_row, straight, "╭ right edge {w}x{h}");
        let edge_col: Vec<usize> = (0..w as usize).filter(|&x| g[h as usize - 1][x]).collect();
        let straight: Vec<usize> = (0..w as usize).filter(|&x| draw('│', w, h, 1)[0][x]).collect();
        assert_eq!(edge_col, straight, "╭ bottom edge {w}x{h}");
    }
}

/// Dashed lines have gaps inside the cell and still line up at the cell edges.
#[test]
fn dashes_have_gaps() {
    let g = draw('┄', 12, 16, 1);
    let y = (0..16).find(|&y| g[y].iter().any(|&b| b)).unwrap();
    let runs = g[y].windows(2).filter(|p| !p[0] && p[1]).count() + usize::from(g[y][0]);
    assert_eq!(runs, 3);
}

/// Every shape lies inside the cell rectangle for every sprite at several sizes.
#[test]
fn shapes_inside_cell() {
    let ranges = [0x2500..=0x259F, 0x2800..=0x28FF, 0x1FB00..=0x1FB3B, 0xE0B0..=0xE0BF];
    for (w, h) in [(1, 1), (1, 2), (2, 1), (7, 15), (8, 16), (13, 27)] {
        let cell = Rect::new(-1e-9, -1e-9, w as f64 + 1e-9, h as f64 + 1e-9);
        for range in ranges.clone() {
            for cp in range {
                let c = char::from_u32(cp).unwrap();
                for light in [1, 2, 5] {
                    for shape in sprite(c, w, h, light).unwrap() {
                        let b = match &shape {
                            SpriteShape::Rect(r) => *r,
                            SpriteShape::Path(p) => p.bounding_box(),
                            SpriteShape::Shade { rect, .. } => *rect,
                        };
                        assert!(cell.contains_rect(b), "{cp:X} {w}x{h} light {light}: {b:?}");
                    }
                }
            }
        }
    }
}

/// Tiny cells never panic.
#[test]
fn tiny_cells_do_not_panic() {
    let ranges = [0x2500..=0x259F, 0x2800..=0x28FF, 0x1FB00..=0x1FB3B, 0xE0B0..=0xE0BF];
    for (w, h) in [(1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (1, 3)] {
        for range in ranges.clone() {
            for cp in range {
                let _ = sprite(char::from_u32(cp).unwrap(), w, h, 1);
                let _ = sprite(char::from_u32(cp).unwrap(), w, h, 3);
            }
        }
    }
    assert!(sprite('─', 0, 0, 0).is_some());
}

/// Powerline triangles fill the expected side of the cell and the lines are inside the cell.
#[test]
fn powerline_shapes() {
    let g = draw('\u{E0B0}', 10, 20, 1);
    assert!(g[1][0] && g[18][0] && g[10][8] && !g[0][9] && !g[19][9]);
    let g = draw('\u{E0B2}', 10, 20, 1);
    assert!(g[1][9] && g[10][1] && !g[0][0]);
    let g = draw('\u{E0B4}', 10, 20, 1);
    assert!(g[0][0] && g[10][9] && !g[0][9]);
    let g = draw('\u{E0B8}', 10, 20, 1);
    assert!(g[19][9] && g[19][0] && !g[0][9]);
}

/// Double lines are two strokes with a gap between them.
#[test]
fn double_lines() {
    for (w, h) in SIZES {
        let g = draw('═', w, h, 1);
        let rows: Vec<usize> = (0..h as usize).filter(|&y| g[y][0]).collect();
        assert_eq!(rows.len(), 2, "{w}x{h}");
        assert_eq!(rows[1] - rows[0], 2);
    }
}
