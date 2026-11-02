//! Stores the program can grow by sending more data stay bounded: the OSC 8 hyperlink table, the
//! tmux passthrough queue and the OSC 99 notifications still being assembled. Each test feeds far
//! more than the limit and checks the newest data survived.

use terminus_vt::{Event, Terminal, TerminalConfig};

/// A terminal of 20 by 2 cells with the given settings.
fn terminal(config: TerminalConfig) -> Terminal {
    Terminal::new(20, 2, config)
}

/// Prints the character `x` inside an OSC 8 hyperlink to `uri`.
fn print_link(term: &mut Terminal, uri: &str) {
    term.advance(format!("\x1b]8;;{uri}\x07x\x1b]8;;\x07").as_bytes());
}

/// The URI of the hyperlink the cell at row 0, column `col` of the shown screen carries.
fn link_at(term: &Terminal, col: usize) -> Option<String> {
    let pages = &term.screen().pages;
    let y = pages.active_y(0);
    let id = pages.with_row(y, |page, i| page.hyperlink(page.cell_index(i, col)))?;
    term.hyperlink(id).map(|h| h.uri.clone())
}

#[test]
fn hyperlinks_stay_under_the_limit_even_while_cells_refer_to_all_of_them() {
    let config = TerminalConfig { max_hyperlinks: 64, ..TerminalConfig::default() };
    let mut term = terminal(config);
    for i in 0..2000 {
        print_link(&mut term, &format!("http://example.test/{i}"));
        term.advance(b"\r\n");
        assert!(term.hyperlink_count() <= 64);
    }
    assert!(term.hyperlink_count() > 56);
    assert!(term.hyperlink(1999).is_some_and(|h| h.uri == "http://example.test/1999"));
}

#[test]
fn hyperlinks_no_cell_refers_to_are_dropped_and_the_newest_link_survives() {
    let config = TerminalConfig { max_hyperlinks: 64, ..TerminalConfig::default() };
    let mut term = terminal(config);
    for i in 0..2000 {
        print_link(&mut term, &format!("http://example.test/{i}"));
        term.advance(b"\r\n");
    }
    term.advance(b"\x1b[3J\x1b[2J\x1b[H");
    for i in 0..2000 {
        print_link(&mut term, &format!("http://later.test/{i}"));
        term.advance(b"\x1b[H");
        assert!(term.hyperlink_count() <= 64);
    }
    assert_eq!(link_at(&term, 0).as_deref(), Some("http://later.test/1999"));
}

#[test]
fn hyperlink_table_stays_under_the_limit_when_scrollback_prunes_old_rows() {
    let scrollback = terminus_vt::pagelist::ScrollbackLimit { bytes: 64 << 10, lines: None };
    let config = TerminalConfig { max_hyperlinks: 256, scrollback, ..TerminalConfig::default() };
    let mut term = terminal(config);
    for i in 0..20_000 {
        print_link(&mut term, &format!("http://example.test/{i}"));
        term.advance(
            b"

",
        );
        assert!(term.hyperlink_count() <= 256);
    }
    print_link(&mut term, "http://newest.test/");
    term.advance(b"[H");
    print_link(&mut term, "http://newest.test/");
    assert!(term.hyperlink_count() <= 256);
    assert_eq!(link_at(&term, 0).as_deref(), Some("http://newest.test/"));
}

#[test]
fn tmux_passthrough_drops_the_oldest_sequences_past_the_byte_limit() {
    let config = TerminalConfig { max_passthrough_bytes: 1000, ..TerminalConfig::default() };
    let mut term = terminal(config);
    for i in 0..200 {
        let inner = format!("msg-{i:04}-{}", "a".repeat(40));
        term.advance(format!("\x1bPtmux;{inner}\x1b\\").as_bytes());
    }
    let taken = term.take_passthrough();
    let total: usize = taken.iter().map(Vec::len).sum();
    assert!(total <= 1000, "kept {total} bytes");
    assert!(taken.len() >= 10);
    assert!(taken.last().unwrap().starts_with(b"msg-0199-"));
    assert!(taken.windows(2).all(|w| w[0] < w[1]), "order is kept");
    assert!(term.take_passthrough().is_empty());
}

#[test]
fn tmux_passthrough_keeps_one_sequence_larger_than_the_limit() {
    let config = TerminalConfig { max_passthrough_bytes: 10, ..TerminalConfig::default() };
    let mut term = terminal(config);
    term.advance(b"\x1bPtmux;0123456789abcdef\x1b\\");
    assert_eq!(term.take_passthrough(), vec![b"0123456789abcdef".to_vec()]);
}

#[test]
fn only_the_newest_pending_notifications_survive_to_be_sent() {
    let config = TerminalConfig { max_pending_notifications: 4, ..TerminalConfig::default() };
    let mut term = terminal(config);
    for i in 0..100 {
        term.advance(format!("\x1b]99;i=n{i}:d=0:p=title;T{i}\x1b\\").as_bytes());
    }
    for i in 0..100 {
        term.advance(format!("\x1b]99;i=n{i}:p=icon;\x1b\\").as_bytes());
    }
    let sent: Vec<String> = term
        .take_events()
        .into_iter()
        .filter_map(|e| if let Event::Notification { title, .. } = e { Some(title) } else { None })
        .collect();
    assert_eq!(sent, vec!["T96", "T97", "T98", "T99"]);
}

#[test]
fn a_notification_title_and_body_are_cut_at_the_byte_limit() {
    let config = TerminalConfig { max_notification_bytes: 10, ..TerminalConfig::default() };
    let mut term = terminal(config);
    for _ in 0..50 {
        term.advance(b"\x1b]99;i=big:d=0;0123456789\x1b\\");
    }
    term.advance(b"\x1b]99;i=big:p=body;abcdefghijklmnop\x1b\\");
    let events = term.take_events();
    assert_eq!(events, vec![Event::Notification { title: "0123456789".into(), body: "abcdefghij".into() }]);
}
