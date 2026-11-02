//! Style reference counts after text overwrites cells that the same run of text wrote.

use terminus_vt::{Terminal, TerminalConfig};

/// Checks that every style's reference count equals the number of cells using it.
fn assert_counts_match(t: &Terminal) {
    let pages = &t.primary().pages;
    pages.for_rows(0..pages.total_rows(), |_, page, i| {
        if i + 1 != page.len() {
            return;
        }
        let mut counts = std::collections::HashMap::new();
        for r in 0..page.len() {
            for c in page.cells(r) {
                if c.style_id() != 0 {
                    *counts.entry(c.style_id()).or_insert(0u32) += 1;
                }
            }
        }
        for (id, refs) in page.style_ref_counts() {
            assert_eq!(counts.get(&id).copied().unwrap_or(0), refs, "style {id} count");
        }
    });
}

/// With autowrap off every character past the last column overwrites the last cell, which the
/// same run of text just wrote with the same style.
#[test]
fn autowrap_off_overwrite_of_a_cell_written_in_the_same_run_keeps_counts() {
    let mut t = Terminal::new(8, 4, TerminalConfig::default());
    t.advance(b"\x1b[?7l\x1b[1mA0,0 a0 , wide tail \xe4\xb8\xad\xe6\x96\x87 more");
    assert_counts_match(&t);
}

/// Overwriting the head of a wide character with narrow text, and then the column of its tail in
/// the same run, must give the same grid as feeding the bytes one at a time. The tail left behind
/// by the first overwrite used to be cleared by the second write together with the cell the
/// first write had just filled.
#[test]
fn narrow_text_over_a_wide_head_and_its_tail_in_one_run() {
    let input = "abcd\u{4e2d}\r\x1b[?7labcdA,".as_bytes();
    let mut whole = Terminal::new(6, 2, TerminalConfig::default());
    whole.advance(input);
    let mut bytewise = Terminal::new(6, 2, TerminalConfig::default());
    for b in input {
        bytewise.advance(std::slice::from_ref(b));
    }
    assert_eq!(terminus_vt::dump::grid_dump(&whole), terminus_vt::dump::grid_dump(&bytewise));
    assert_counts_match(&whole);
}
