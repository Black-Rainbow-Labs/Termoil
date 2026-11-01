//! Shared checks for the fuzz targets: the invariants from docs/TEST-SUITE.md section 8.

use terminus_vt::Terminal;
use terminus_vt::cell::Wide;

/// Panics if any invariant of the terminal's grid is broken.
#[allow(dead_code)]
pub fn check(t: &Terminal) {
    let s = t.screen();
    assert!(s.cursor.row < s.rows() && s.cursor.col < s.cols(), "cursor outside the grid");
    let pages = &s.pages;
    assert!(pages.total_rows() >= s.rows() as usize);
    pages.for_rows(0..pages.total_rows(), |_, page, i| {
        let cells = page.cells(i);
        for (c, cell) in cells.iter().enumerate() {
            match cell.wide() {
                Wide::Wide => assert!(c + 1 < cells.len() && cells[c + 1].wide() == Wide::SpacerTail, "wide cell without tail"),
                Wide::SpacerTail => assert!(c > 0 && cells[c - 1].wide() == Wide::Wide, "orphan tail"),
                Wide::SpacerHead => assert_eq!(c + 1, cells.len(), "spacer head not at the end"),
                Wide::Narrow => {}
            }
        }
    });
    assert!(s.kitty_keyboard.depth() <= 16);
    if t.is_alt_screen() {
        assert_eq!(s.pages.history_rows(), 0);
    }
}
