//! F11: the page allocator: cell writes, styles, grapheme tails, hyperlinks, row moves and the
//! compressed round trip.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_vt::cell::Cell;
use termoil_vt::page::Page;
use termoil_vt::style::{Color, Style};

fuzz_target!(|data: &[u8]| {
    let mut page = Page::new(8, 6);
    for _ in 0..6 {
        page.push_row();
    }
    for op in data.chunks(4) {
        let get = |k: usize| op.get(k).copied().unwrap_or(0);
        let (a, b, c, d) = (get(0), get(1), get(2), get(3));
        let (row, col) = (b as usize % 6, c as usize % 8);
        match a % 7 {
            0 => {
                let id = page.intern_style(&Style { fg: Color::Palette(d), ..Style::default() });
                page.write_cell(row, col, Cell::new(char::from(b'a' + d % 26), id));
            }
            1 => page.clear_cells(row, col, col + d as usize % 4, Cell::BLANK, d & 1 == 1),
            2 => {
                let _ = page.append_grapheme(row, col, '\u{301}', 64);
            }
            3 => page.set_hyperlink(row, col, d as u32),
            4 => page.rotate_up(0, 5, 1 + d as usize % 3),
            5 => page.rotate_down(0, 5, 1 + d as usize % 3),
            _ => page.clear_row(row),
        }
    }
    let back = Page::decompress(&page.compress()).expect("round trip");
    for r in 0..6 {
        let (mut x, mut y) = (String::new(), String::new());
        page.row_text(r, &mut x);
        back.row_text(r, &mut y);
        assert_eq!(x, y);
    }
});
