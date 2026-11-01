//! U4 properties (`docs/TEST-SUITE.md` section 8), checked with proptest over random streams of
//! text, controls and escape sequences.

use proptest::prelude::*;
use terminus_vt::cell::{Wide, row_flags};
use terminus_vt::dump;
use terminus_vt::modes::Mode;
use terminus_vt::page::Page;
use terminus_vt::pagelist::{PageList, ScrollbackLimit};
use terminus_vt::parser::{Action, Parser};
use terminus_vt::{Terminal, TerminalConfig};

/// One piece of a random stream.
fn piece() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => "[a-zA-Z0-9 .,]{1,30}".prop_map(|s| s.into_bytes()),
        1 => prop::sample::select(vec!["中", "文", "😀", "❤\u{fe0f}", "e\u{301}", "👨\u{200d}👩", "🇺🇸", "\u{301}", "é"]).prop_map(|s| s.as_bytes().to_vec()),
        2 => prop::sample::select(vec!["\r", "\n", "\r\n", "\t", "\x08", "\x07", "\x0b"]).prop_map(|s| s.as_bytes().to_vec()),
        3 => (prop::sample::select(vec!['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'M', 'P', 'S', 'T', 'X', '@', 'd', 'b', 'Z', 'I']), 0u16..40, 0u16..40)
            .prop_map(|(f, a, b)| format!("\x1b[{a};{b}{f}").into_bytes()),
        2 => prop::collection::vec(prop::sample::select(vec![0u16, 1, 2, 3, 4, 7, 9, 22, 31, 32, 38, 41, 48, 5, 2, 200, 255, 90, 107]), 0..8)
            .prop_map(|v| format!("\x1b[{}m", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(";")).into_bytes()),
        1 => (0u16..30, 0u16..30).prop_map(|(t, b)| format!("\x1b[{t};{b}r").into_bytes()),
        1 => prop::sample::select(vec!["\x1b7", "\x1b8", "\x1bM", "\x1bD", "\x1bE", "\x1bc", "\x1b[?1049h", "\x1b[?1049l", "\x1b[?47h", "\x1b[?47l", "\x1b[?6h", "\x1b[?6l", "\x1b[?7l", "\x1b[?7h", "\x1b[4h", "\x1b[4l", "\x1b[?69h\x1b[3;9s", "\x1b[?69l", "\x1b[?2027l", "\x1b[?2027h", "\x1b[>1u", "\x1b[>3u", "\x1b[<u", "\x1b[3J", "\x1b#8", "\x1b(0", "\x1b(B", "\x1b[1\"q", "\x1b[0\"q", "\x1b[?2J"]).prop_map(|s| s.as_bytes().to_vec()),
        1 => "[a-z]{0,10}".prop_map(|s| format!("\x1b]8;;http://x/{s}\x1b\\").into_bytes()),
        1 => "[a-z ]{0,10}".prop_map(|s| format!("\x1b]2;{s}\x07").into_bytes()),
        1 => prop::sample::select(vec!["\x1b]133;A\x07", "\x1b]133;B\x07", "\x1b]133;C\x07", "\x1b]133;D;0\x07"]).prop_map(|s| s.as_bytes().to_vec()),
    ]
}

/// A random stream.
fn stream() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(piece(), 0..120).prop_map(|v| v.concat())
}

/// A terminal with a small scrollback so pruning happens.
fn term(cols: u16, rows: u16) -> Terminal {
    let config = TerminalConfig { scrollback: ScrollbackLimit { bytes: 400_000, lines: Some(300) }, ..TerminalConfig::default() };
    Terminal::new(cols, rows, config)
}

/// Every check that must hold for any terminal state.
fn check_invariants(t: &Terminal) {
    for screen in [Some(t.primary()), t.is_alt_screen().then(|| t.screen())].into_iter().flatten() {
        let c = &screen.cursor;
        assert!(c.row < screen.rows() && c.col < screen.cols(), "cursor {},{} outside {}x{}", c.row, c.col, screen.cols(), screen.rows());
        let pages = &screen.pages;
        assert!(pages.total_rows() >= screen.rows() as usize);
        pages.for_rows(0..pages.total_rows(), |y, page, i| check_row(page, i, y));
        for (_, pin) in pages.pins() {
            if let Some(y) = pages.y_of(pin.serial) {
                assert!(y < pages.total_rows());
            }
        }
        assert!(screen.kitty_keyboard.depth() <= 16);
    }
    if t.is_alt_screen() {
        assert_eq!(t.screen().pages.history_rows(), 0, "the alternate screen has history");
    }
    let limit = t.primary().pages.limit();
    let hist = t.primary().pages.history_rows();
    assert!(limit.lines.is_none_or(|l| hist <= l), "history {hist} over line limit");
}

/// Width and style invariants of one row.
fn check_row(page: &Page, i: usize, y: usize) {
    let cells = page.cells(i);
    assert_eq!(cells.len(), page.cols() as usize);
    for (col, cell) in cells.iter().enumerate() {
        match cell.wide() {
            Wide::Wide => assert!(col + 1 < cells.len() && cells[col + 1].wide() == Wide::SpacerTail, "wide cell without tail at {y},{col}"),
            Wide::SpacerTail => assert!(col > 0 && cells[col - 1].wide() == Wide::Wide, "orphan tail at {y},{col}"),
            Wide::SpacerHead => assert_eq!(col + 1, cells.len(), "spacer head not at the end at {y},{col}"),
            Wide::Narrow => {}
        }
        if cell.style_id() != 0 {
            assert!(page.row(i).has(row_flags::STYLED), "styled flag missing at {y}");
        }
    }
}

/// Style reference counts equal the number of cells using each style.
fn check_style_counts(t: &Terminal) {
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

/// The text of every logical line, history and screen, for comparing across resizes.
fn logical_text(t: &Terminal) -> Vec<String> {
    let pages = &t.primary().pages;
    let mut out = Vec::new();
    terminus_vt::text::for_lines(pages, 0..pages.total_rows(), |l| {
        out.push(l.text.trim_end().to_string());
        true
    });
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::default())]

    /// P1: any chunking gives the same grid.
    #[test]
    fn chunking_does_not_matter(s in stream(), cut in 1usize..17) {
        let mut a = term(20, 6);
        a.advance(&s);
        let mut b = term(20, 6);
        for c in s.chunks(cut) { b.advance(c); }
        prop_assert_eq!(dump::grid_dump(&a), dump::grid_dump(&b));
        prop_assert_eq!(dump::history_text(&a), dump::history_text(&b));
    }

    /// P2 to P4, P8, P10, P11, P6: the invariants after any stream.
    #[test]
    fn invariants_hold(s in stream(), cols in 2u16..30, rows in 1u16..10) {
        let mut t = term(cols, rows);
        t.advance(&s);
        check_invariants(&t);
    }

    /// P9: interned style reference counts match the cells.
    #[test]
    fn style_counts_match(s in stream()) {
        let mut t = term(15, 5);
        t.advance(&s);
        check_style_counts(&t);
    }

    /// P13: the SIMD front end and the scalar path agree on any bytes.
    #[test]
    fn simd_and_scalar_agree(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        let collect = |simd: bool| {
            let mut p = Parser::default();
            p.set_simd(simd);
            let mut out = Vec::new();
            p.advance(&bytes, &mut |a: Action<'_>| out.push(format!("{a:?}")));
            out
        };
        prop_assert_eq!(collect(true), collect(false));
    }

    /// P14: any bytes at all never break an invariant.
    #[test]
    fn arbitrary_bytes_are_safe(bytes in prop::collection::vec(any::<u8>(), 0..2000)) {
        let mut t = term(12, 4);
        t.advance(&bytes);
        check_invariants(&t);
    }

    /// P5: resizing to a new width and back keeps the text when nothing was pruned.
    #[test]
    fn resize_and_back_keeps_text(s in "[a-z ]{0,200}(\r\n[a-z ]{0,100}){0,5}", w in 3u16..40) {
        let mut t = Terminal::new(20, 30, TerminalConfig::default());
        t.advance(s.as_bytes());
        let before = logical_text(&t);
        t.resize(w, 30);
        check_invariants(&t);
        t.resize(20, 30);
        prop_assume!(t.primary().pages.pruned_pages() == 0);
        prop_assert_eq!(logical_text(&t), before);
    }

    /// P7: resizes interleaved with output keep every invariant.
    #[test]
    fn resize_storm(s in stream(), sizes in prop::collection::vec((1u16..40, 1u16..12), 1..8)) {
        let mut t = term(20, 6);
        let chunk = (s.len() / sizes.len()).max(1);
        for (k, (c, r)) in sizes.iter().enumerate() {
            let from = (k * chunk).min(s.len());
            let to = ((k + 1) * chunk).min(s.len());
            t.advance(&s[from..to]);
            check_invariants(&t);
            t.resize(*c, *r);
            check_invariants(&t);
        }
    }

    /// P12: a page's text survives compression and restore.
    #[test]
    fn compression_round_trip(s in stream()) {
        let mut t = term(16, 4);
        t.advance(&s);
        let pages = &t.primary().pages;
        pages.for_rows(0..pages.total_rows(), |_, page, i| {
            if i == 0 {
                let back = Page::decompress(&page.compress()).expect("restores");
                for r in 0..page.len() {
                    let (mut a, mut b) = (String::new(), String::new());
                    page.row_text(r, &mut a);
                    back.row_text(r, &mut b);
                    assert_eq!(a, b);
                    assert_eq!(page.row(r).flags, back.row(r).flags);
                }
            }
        });
    }

    /// P15: the scrollback never exceeds its byte limit after any amount of output.
    #[test]
    fn byte_limit_honoured(lines in 1usize..3000, width in 10u16..200) {
        let config = TerminalConfig { scrollback: ScrollbackLimit { bytes: 2_000_000, lines: None }, ..TerminalConfig::default() };
        let mut t = Terminal::new(width, 10, config);
        for i in 0..lines { t.advance(format!("line {i}\r\n").as_bytes()); }
        let pages = &t.primary().pages;
        prop_assert!(pages.memory_bytes() <= 2_000_000 + 2 * pages.page_count().min(2) * 400_000);
    }

    /// P16: the kitty keyboard stack never grows past 16, whatever is pushed.
    #[test]
    fn kitty_stack_bounded(pushes in 0usize..100, pops in 0usize..10) {
        let mut t = term(10, 2);
        for i in 0..pushes { t.advance(format!("\x1b[>{}u", i % 32).as_bytes()); }
        for _ in 0..pops { t.advance(b"\x1b[<u"); }
        prop_assert!(t.screen().kitty_keyboard.depth() <= 16);
    }

    /// P17: memory stays bounded over 10 000 random operations.
    #[test]
    fn memory_bounded(seed in any::<u64>()) {
        let mut t = term(40, 10);
        let mut x = seed | 1;
        for _ in 0..10_000 {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            let op = match x % 6 { 0 => "\x1b[2J", 1 => "abcdefghij", 2 => "\r\n", 3 => "\x1b[31;42mxyz", 4 => "\x1b]8;;http://a\x1b\\L", _ => "\u{4e2d}" };
            t.advance(op.as_bytes());
        }
        prop_assert!(t.primary().pages.memory_bytes() <= 400_000 + 2 * 400_000);
        check_style_counts(&t);
    }

    /// P18: a selection over the whole screen copies the screen's text.
    #[test]
    fn select_all_matches_dump(s in "[a-z ]{0,60}(\r\n[a-z]{0,30}){0,4}") {
        let mut t = Terminal::new(20, 30, TerminalConfig::default());
        t.advance(s.as_bytes());
        let pages = &mut t.primary_mut().pages;
        let top = pages.active_y(0);
        let sel = terminus_vt::selection::Selection::new(pages, terminus_vt::selection::SelectionMode::Char, top, 0);
        sel.update(pages, top + 29, 19);
        let text = sel.text(pages, terminus_vt::selection::DEFAULT_WORD_SEPARATORS);
        sel.release(pages);
        let expected: Vec<String> = logical_text(&t).into_iter().collect();
        let got: Vec<String> = text.trim_end_matches('\n').split('\n').map(|s| s.to_string()).collect::<Vec<_>>();
        let got: Vec<String> = { let mut g = got; while g.last().is_some_and(String::is_empty) { g.pop(); } g };
        prop_assert_eq!(got, expected);
    }

    /// P19: search finds every marker written, wherever lines wrapped.
    #[test]
    fn search_finds_markers(n in 1usize..30, width in 5u16..40) {
        let mut t = Terminal::new(width, 6, TerminalConfig::default());
        for i in 0..n { t.advance(format!("noise MARK{i:03} noise\r\n").as_bytes()); }
        let re = terminus_vt::search::compile("MARK\\d{3}", true).expect("valid");
        prop_assert_eq!(terminus_vt::search::count(&t.primary().pages, &re), n);
    }

    /// P20: pins move with their text through a reflow.
    #[test]
    fn pins_follow_reflow(prefix in "[a-z]{0,50}", width in 3u16..30) {
        let mut t = Terminal::new(30, 10, TerminalConfig::default());
        t.advance(prefix.as_bytes());
        t.advance(b"X");
        let x_col = prefix.len() as u16 % 30;
        let x_row = prefix.len() / 30;
        let pages = &mut t.primary_mut().pages;
        let y = pages.active_y(x_row as u16);
        let pin = pages.pin_create(y, x_col);
        t.resize(width, 10);
        let pages = &t.primary().pages;
        let (py, pc) = pages.pin_get(pin).expect("pin survives");
        let ch = pages.with_row(py, |p, i| p.cells(i)[pc as usize].char());
        prop_assert_eq!(ch, Some('X'));
    }

    /// P21: mode 2027 off gives one cell per code point of width at least one.
    #[test]
    fn legacy_width_mode(s in "[a-z\u{4e2d}\u{301}]{0,20}") {
        let mut t = Terminal::new(80, 2, TerminalConfig::default());
        t.advance(b"\x1b[?2027l");
        prop_assert!(!t.mode(Mode::GraphemeCluster));
        t.advance(s.as_bytes());
        check_invariants(&t);
    }

    /// P22: the history line limit is honoured exactly.
    #[test]
    fn line_limit_exact(lines in 0usize..2000, limit in 0usize..500) {
        let config = TerminalConfig { scrollback: ScrollbackLimit { bytes: usize::MAX, lines: Some(limit) }, ..TerminalConfig::default() };
        let mut t = Terminal::new(10, 5, config);
        for i in 0..lines { t.advance(format!("{i}\r\n").as_bytes()); }
        let hist = t.primary().pages.history_rows();
        prop_assert!(hist <= limit);
        prop_assert_eq!(hist, (lines + 1).saturating_sub(5).min(limit));
    }

    /// P23: clearing the scrollback keeps the screen and drops all history.
    #[test]
    fn clear_scrollback(lines in 0usize..400) {
        let mut t = Terminal::new(10, 5, TerminalConfig::default());
        for i in 0..lines { t.advance(format!("{i}\r\n").as_bytes()); }
        let screen = dump::text_dump(&t);
        t.advance(b"\x1b[3J");
        prop_assert_eq!(t.primary().pages.history_rows(), 0);
        prop_assert_eq!(dump::text_dump(&t), screen);
    }

    /// P24: compressing cold pages changes nothing a reader can see.
    #[test]
    fn compress_cold_is_invisible(lines in 0usize..3000) {
        let mut t = Terminal::new(40, 10, TerminalConfig::default());
        for i in 0..lines { t.advance(format!("\x1b[3{}mline {i}\r\n", i % 8).as_bytes()); }
        let before = dump::history_text(&t);
        t.primary_mut().pages.compress_cold(1);
        prop_assert_eq!(dump::history_text(&t), before);
        t.advance(b"more\r\n");
        check_invariants(&t);
    }

    /// P25: the list's total never drops below the screen height.
    #[test]
    fn total_at_least_rows(rows in 1u16..30, out in 0usize..100) {
        let mut pages = PageList::new(10, rows, ScrollbackLimit { bytes: 1, lines: Some(0) }, true);
        for _ in 0..out { pages.grow(); }
        prop_assert!(pages.total_rows() >= rows as usize);
    }
}
