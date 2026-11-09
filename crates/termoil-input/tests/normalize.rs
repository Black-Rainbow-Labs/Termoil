//! I3, part two: platform key normalisation (AltGr, dead keys, the Win key, the numeric keypad)
//! driven by tables over a fake key source, plus a few runs through the encoders.
#![allow(clippy::type_complexity)]

mod common;

use common::*;
use termoil_input::{
    Key, KeyEvent, KeyEventKind, KeyNormalizer, KeyboardModes, KeypadKey, Modifiers, NamedKey, RawKey, encode_key,
};

/// What a normalised event should look like: the key, the modifier bits and the text.
type Want = (Key, u8, Option<&'static str>);

/// Asserts that the events match the wanted keys, modifier bits and text, one row per event.
fn assert_events(label: &str, got: &[KeyEvent], want: &[Want]) {
    assert_eq!(got.len(), want.len(), "{label}: event count, got {got:?}");
    for (event, (key, bits, text)) in got.iter().zip(want) {
        count_row();
        assert_eq!(event.key, *key, "{label}: key");
        assert_eq!(event.mods.bits(), *bits, "{label}: modifier bits of {event:?}");
        assert_eq!(event.text.as_deref(), *text, "{label}: text of {event:?}");
    }
}

/// A raw press of a character key with text.
fn raw_text(ch: char, mods: Modifiers, text: &str) -> RawKey {
    RawKey::new(c(ch), mods).with_text(text)
}

/// A raw press of a dead key for the given accent.
fn dead(ch: char, accent: char) -> RawKey {
    RawKey::new(c(ch), N).with_dead(accent)
}

/// Feeds a sequence of raw keys through one normaliser and checks the events of every step.
fn run_sequence(label: &str, steps: Vec<(RawKey, Vec<Want>)>) {
    let mut normalizer = KeyNormalizer::new();
    for (index, (raw, want)) in steps.iter().enumerate() {
        let got = normalizer.feed(raw);
        assert_events(&format!("{label} step {index}"), &got, want);
    }
}

/// AltGr on Windows arrives as ctrl+alt (or with the AltGr flag) and types the layout's character.
#[test]
fn altgr_types_the_layout_character() {
    let rows: Vec<(&str, RawKey, Vec<Want>)> = vec![
        ("german altgr+q is at", raw_text('q', C | A, "@"), vec![(c('q'), 0, Some("@"))]),
        ("altgr flag with ctrl+alt", raw_text('q', C | A, "@").with_altgr(), vec![(c('q'), 0, Some("@"))]),
        ("altgr flag with alt only", raw_text('7', A, "{").with_altgr(), vec![(c('7'), 0, Some("{"))]),
        ("altgr flag with no modifiers", raw_text('7', N, "{").with_altgr(), vec![(c('7'), 0, Some("{"))]),
        ("altgr+e is euro", raw_text('e', C | A, "\u{20ac}"), vec![(c('e'), 0, Some("\u{20ac}"))]),
        ("altgr+shift keeps shift", raw_text('q', S | C | A, "Q"), vec![(c('q'), 1, Some("Q"))]),
        ("altgr+backslash", raw_text('\\', C | A, "\\"), vec![(c('\\'), 0, Some("\\"))]),
        ("real ctrl+alt+a has control text", raw_text('a', C | A, "\u{1}"), vec![(c('a'), 6, None)]),
        ("real ctrl+alt+a without text", RawKey::new(c('a'), C | A), vec![(c('a'), 6, None)]),
        ("altgr flag without text keeps ctrl+alt", RawKey::new(c('q'), C | A).with_altgr(), vec![(c('q'), 6, None)]),
        ("alt+a is a shortcut", raw_text('a', A, "a"), vec![(c('a'), 2, None)]),
        ("ctrl+c is a shortcut", raw_text('c', C, "\u{3}"), vec![(c('c'), 4, None)]),
        ("ctrl+shift+c", raw_text('c', C | S, "\u{3}"), vec![(c('c'), 5, None)]),
        ("plain a", raw_text('a', N, "a"), vec![(c('a'), 0, Some("a"))]),
        ("shift+a", raw_text('a', S, "A"), vec![(c('a'), 1, Some("A"))]),
        ("caps lock a", raw_text('a', Modifiers::CAPS_LOCK, "A"), vec![(c('a'), 64, Some("A"))]),
        (
            "enter has control text",
            RawKey::new(n(NamedKey::Enter), N).with_text("\r"),
            vec![(n(NamedKey::Enter), 0, None)],
        ),
        (
            "backspace has control text",
            RawKey::new(n(NamedKey::Backspace), N).with_text("\u{8}"),
            vec![(n(NamedKey::Backspace), 0, None)],
        ),
        ("arrow", RawKey::new(n(NamedKey::Up), S), vec![(n(NamedKey::Up), 1, None)]),
        ("f5 with ctrl", RawKey::new(n(NamedKey::F(5)), C), vec![(n(NamedKey::F(5)), 4, None)]),
    ];
    for (label, raw, want) in rows {
        let mut normalizer = KeyNormalizer::new();
        assert_events(label, &normalizer.feed(&raw), &want);
    }
}

/// The release of an AltGr key matches its press, and releases carry their kind through.
#[test]
fn altgr_release_and_kinds() {
    let mut normalizer = KeyNormalizer::new();
    let release = RawKey::new(c('q'), C | A).with_altgr().with_kind(KeyEventKind::Release);
    let got = normalizer.feed(&release);
    assert_events("altgr release", &got, &[(c('q'), 0, None)]);
    assert_eq!(got[0].kind, KeyEventKind::Release);
    let repeat = raw_text('a', N, "a").with_kind(KeyEventKind::Repeat);
    let got = normalizer.feed(&repeat);
    assert_events("repeat", &got, &[(c('a'), 0, Some("a"))]);
    assert_eq!(got[0].kind, KeyEventKind::Repeat);
    count_row();
}

/// Alternate keys are filled in from the physical key and the shifted text.
#[test]
fn alternate_keys_come_from_physical_key_and_text() {
    let mut normalizer = KeyNormalizer::new();
    let raw = raw_text('\u{441}', C, "").with_physical('c');
    let got = normalizer.feed(&raw);
    assert_eq!(got[0].base_layout_key, Some('c'));
    let shifted = raw_text('=', S, "+");
    let got = normalizer.feed(&shifted);
    assert_eq!(got[0].shifted_key, Some('+'));
    let same = raw_text('a', N, "a").with_physical('a');
    let got = normalizer.feed(&same);
    assert_eq!((got[0].base_layout_key, got[0].shifted_key), (None, None));
    for _ in 0..3 {
        count_row();
    }
}

/// With Ctrl or Cmd held the text is dropped, so the shifted key comes from the character the
/// layout reports for Shift (winit's `logical_key`). The text still wins when there is one, and the
/// layout's character is ignored without Shift (task-2589).
#[test]
fn shifted_key_comes_from_the_layout_when_a_command_key_drops_the_text() {
    let mut normalizer = KeyNormalizer::new();
    let rows: Vec<(&str, RawKey, Option<char>)> = vec![
        ("ctrl+shift+= on a US layout", RawKey::new(c('='), C | S).with_shifted('+'), Some('+')),
        ("cmd+shift+= on a US layout", RawKey::new(c('='), SU | S).with_shifted('+'), Some('+')),
        ("shift+= typing text", raw_text('=', S, "+").with_shifted('*'), Some('+')),
        ("ctrl+= without shift", RawKey::new(c('='), C).with_shifted('+'), None),
        ("shift+a reports itself", RawKey::new(c('a'), C | S).with_shifted('a'), None),
    ];
    for (label, raw, want) in rows {
        count_row();
        assert_eq!(normalizer.feed(&raw)[0].shifted_key, want, "{label}");
    }
}

/// Dead key then a letter it composes with: one row per accent and letter.
#[test]
fn dead_key_followed_by_a_composing_letter() {
    let cases: Vec<(char, char, &str, &str)> = vec![
        ('`', 'a', "a", "\u{e0}"),
        ('`', 'e', "e", "\u{e8}"),
        ('`', 'i', "i", "\u{ec}"),
        ('`', 'o', "o", "\u{f2}"),
        ('`', 'u', "u", "\u{f9}"),
        ('\u{b4}', 'a', "a", "\u{e1}"),
        ('\u{b4}', 'e', "e", "\u{e9}"),
        ('\u{b4}', 'i', "i", "\u{ed}"),
        ('\u{b4}', 'o', "o", "\u{f3}"),
        ('\u{b4}', 'u', "u", "\u{fa}"),
        ('\u{b4}', 'y', "y", "\u{fd}"),
        ('\'', 'e', "e", "\u{e9}"),
        ('^', 'a', "a", "\u{e2}"),
        ('^', 'e', "e", "\u{ea}"),
        ('^', 'i', "i", "\u{ee}"),
        ('^', 'o', "o", "\u{f4}"),
        ('^', 'u', "u", "\u{fb}"),
        ('~', 'a', "a", "\u{e3}"),
        ('~', 'n', "n", "\u{f1}"),
        ('~', 'o', "o", "\u{f5}"),
        ('\u{a8}', 'a', "a", "\u{e4}"),
        ('\u{a8}', 'e', "e", "\u{eb}"),
        ('\u{a8}', 'i', "i", "\u{ef}"),
        ('\u{a8}', 'o', "o", "\u{f6}"),
        ('\u{a8}', 'u', "u", "\u{fc}"),
        ('\u{a8}', 'y', "y", "\u{ff}"),
        ('"', 'u', "u", "\u{fc}"),
        ('\u{b8}', 'c', "c", "\u{e7}"),
        ('\u{b0}', 'a', "a", "\u{e5}"),
        ('\u{b0}', 'u', "u", "\u{16f}"),
        ('\u{300}', 'e', "e", "\u{e8}"),
        ('\u{301}', 'e', "e", "\u{e9}"),
        ('\u{302}', 'e', "e", "\u{ea}"),
        ('\u{303}', 'n', "n", "\u{f1}"),
        ('\u{308}', 'o', "o", "\u{f6}"),
        ('\u{327}', 'c', "c", "\u{e7}"),
        ('\u{30a}', 'a', "a", "\u{e5}"),
    ];
    for (accent, letter, text, composed) in cases {
        run_sequence(
            &format!("dead {accent:?} + {letter}"),
            vec![(dead('x', accent), vec![]), (raw_text(letter, N, text), vec![(c(letter), 0, Some(composed))])],
        );
    }
}

/// Dead key then a shifted letter composes the uppercase form.
#[test]
fn dead_key_followed_by_an_uppercase_letter() {
    let cases: Vec<(char, char, &str, &str)> = vec![
        ('`', 'a', "A", "\u{c0}"),
        ('\u{b4}', 'e', "E", "\u{c9}"),
        ('^', 'o', "O", "\u{d4}"),
        ('~', 'n', "N", "\u{d1}"),
        ('\u{a8}', 'u', "U", "\u{dc}"),
        ('\u{a8}', 'y', "Y", "\u{178}"),
        ('\u{b8}', 'c', "C", "\u{c7}"),
        ('\u{b0}', 'a', "A", "\u{c5}"),
    ];
    for (accent, letter, text, composed) in cases {
        run_sequence(
            &format!("dead {accent:?} + shift {letter}"),
            vec![(dead('x', accent), vec![]), (raw_text(letter, S, text), vec![(c(letter), 0, Some(composed))])],
        );
    }
}

/// Dead key then space gives the accent character, and a combining mark becomes its spacing form.
#[test]
fn dead_key_followed_by_space() {
    let cases: Vec<(char, &str)> = vec![
        ('`', "`"),
        ('\u{b4}', "\u{b4}"),
        ('\'', "'"),
        ('^', "^"),
        ('~', "~"),
        ('\u{a8}', "\u{a8}"),
        ('"', "\""),
        ('\u{b8}', "\u{b8}"),
        ('\u{b0}', "\u{b0}"),
        ('\u{300}', "`"),
        ('\u{301}', "\u{b4}"),
        ('\u{302}', "^"),
        ('\u{303}', "~"),
        ('\u{308}', "\u{a8}"),
        ('\u{327}', "\u{b8}"),
        ('\u{30a}', "\u{b0}"),
    ];
    for (accent, expected) in cases {
        run_sequence(
            &format!("dead {accent:?} + space"),
            vec![(dead('x', accent), vec![]), (raw_text(' ', N, " "), vec![(c(' '), 0, Some(expected))])],
        );
    }
}

/// The same dead key twice gives the accent character once.
#[test]
fn dead_key_twice() {
    let cases: Vec<(char, &str)> =
        vec![('`', "`"), ('\u{b4}', "\u{b4}"), ('^', "^"), ('~', "~"), ('\u{a8}', "\u{a8}"), ('\u{b8}', "\u{b8}")];
    for (accent, expected) in cases {
        run_sequence(
            &format!("dead {accent:?} twice"),
            vec![(dead('x', accent), vec![]), (dead('x', accent), vec![(c('x'), 0, Some(expected))])],
        );
    }
}

/// Two different dead keys give both accent characters, and a letter that does not compose gives the accent then the letter.
#[test]
fn dead_key_that_does_not_compose() {
    run_sequence(
        "two different dead keys",
        vec![
            (dead('x', '`'), vec![]),
            (dead('y', '\u{b4}'), vec![(c('\0'), 0, Some("`")), (c('y'), 0, Some("\u{b4}"))]),
        ],
    );
    run_sequence(
        "grave then x",
        vec![(dead('x', '`'), vec![]), (raw_text('x', N, "x"), vec![(c('\0'), 0, Some("`")), (c('x'), 0, Some("x"))])],
    );
    run_sequence(
        "acute then digit",
        vec![
            (dead('x', '\u{b4}'), vec![]),
            (raw_text('1', N, "1"), vec![(c('\0'), 0, Some("\u{b4}")), (c('1'), 0, Some("1"))]),
        ],
    );
    run_sequence(
        "cedilla then a",
        vec![
            (dead('x', '\u{b8}'), vec![]),
            (raw_text('a', N, "a"), vec![(c('\0'), 0, Some("\u{b8}")), (c('a'), 0, Some("a"))]),
        ],
    );
    run_sequence(
        "tilde then e",
        vec![(dead('x', '~'), vec![]), (raw_text('e', N, "e"), vec![(c('\0'), 0, Some("~")), (c('e'), 0, Some("e"))])],
    );
    run_sequence(
        "unknown dead accent then a",
        vec![
            (dead('x', '\u{2022}'), vec![]),
            (raw_text('a', N, "a"), vec![(c('\0'), 0, Some("\u{2022}")), (c('a'), 0, Some("a"))]),
        ],
    );
}

/// A dead key followed by a shortcut emits the shortcut and drops the pending accent.
#[test]
fn dead_key_followed_by_a_shortcut() {
    let after_a = vec![(c('a'), 0, Some("a"))];
    let cases: Vec<(&str, RawKey, Vec<Want>)> = vec![
        ("ctrl+c", raw_text('c', C, "\u{3}"), vec![(c('c'), 4, None)]),
        ("ctrl+shift+v", raw_text('v', C | S, "\u{16}"), vec![(c('v'), 5, None)]),
        ("alt+x", raw_text('x', A, "x"), vec![(c('x'), 2, None)]),
        ("super+e", RawKey::new(c('e'), SU), vec![(c('e'), 8, None)]),
        ("enter", RawKey::new(n(NamedKey::Enter), N), vec![(n(NamedKey::Enter), 0, None)]),
        ("escape", RawKey::new(n(NamedKey::Escape), N), vec![(n(NamedKey::Escape), 0, None)]),
        ("tab", RawKey::new(n(NamedKey::Tab), N), vec![(n(NamedKey::Tab), 0, None)]),
        ("backspace", RawKey::new(n(NamedKey::Backspace), N), vec![(n(NamedKey::Backspace), 0, None)]),
        ("left arrow", RawKey::new(n(NamedKey::Left), N), vec![(n(NamedKey::Left), 0, None)]),
        ("f5", RawKey::new(n(NamedKey::F(5)), N), vec![(n(NamedKey::F(5)), 0, None)]),
        ("key without text", RawKey::new(c('z'), N), vec![(c('z'), 0, None)]),
    ];
    for (label, shortcut, want) in cases {
        let mut normalizer = KeyNormalizer::new();
        assert_events(label, &normalizer.feed(&dead('x', '`')), &[]);
        assert!(normalizer.has_pending_dead());
        assert_events(label, &normalizer.feed(&shortcut), &want);
        assert!(!normalizer.has_pending_dead(), "{label}: pending dead key must be dropped");
        assert_events(label, &normalizer.feed(&raw_text('a', N, "a")), &after_a);
    }
}

/// Pressing shift between the dead key and the letter keeps the accent pending.
#[test]
fn modifier_press_keeps_the_dead_key_pending() {
    run_sequence(
        "dead, shift, letter",
        vec![
            (dead('x', '\u{b4}'), vec![]),
            (RawKey::new(n(NamedKey::LeftShift), S), vec![(n(NamedKey::LeftShift), 1, None)]),
            (raw_text('e', S, "E"), vec![(c('e'), 0, Some("\u{c9}"))]),
            (
                RawKey::new(n(NamedKey::LeftShift), N).with_kind(KeyEventKind::Release),
                vec![(n(NamedKey::LeftShift), 0, None)],
            ),
            (raw_text('e', N, "e"), vec![(c('e'), 0, Some("e"))]),
        ],
    );
    run_sequence(
        "dead, control press, letter",
        vec![
            (dead('x', '`'), vec![]),
            (RawKey::new(n(NamedKey::LeftControl), C), vec![(n(NamedKey::LeftControl), 4, None)]),
            (raw_text('a', C, "\u{1}"), vec![(c('a'), 4, None)]),
        ],
    );
}

/// Releases do not disturb a pending dead key, and the release of a dead key sends nothing.
#[test]
fn releases_around_dead_keys() {
    let mut normalizer = KeyNormalizer::new();
    assert_events("dead press", &normalizer.feed(&dead('x', '`')), &[]);
    let dead_release = dead('x', '`').with_kind(KeyEventKind::Release);
    assert_events("dead release", &normalizer.feed(&dead_release), &[]);
    assert!(normalizer.has_pending_dead());
    let other_release = RawKey::new(c('q'), N).with_kind(KeyEventKind::Release);
    assert_events("other release", &normalizer.feed(&other_release), &[(c('q'), 0, None)]);
    assert!(normalizer.has_pending_dead());
    assert_events("compose", &normalizer.feed(&raw_text('a', N, "a")), &[(c('a'), 0, Some("\u{e0}"))]);
    assert!(!normalizer.has_pending_dead());
    let letter_release = RawKey::new(c('a'), N).with_kind(KeyEventKind::Release);
    let got = normalizer.feed(&letter_release);
    assert_events("letter release", &got, &[(c('a'), 0, None)]);
    assert_eq!(got[0].kind, KeyEventKind::Release);
}

/// An AltGr character after a dead key does not compose, and the accent comes out first.
#[test]
fn dead_key_then_altgr_character() {
    run_sequence(
        "dead then altgr at",
        vec![
            (dead('x', '`'), vec![]),
            (raw_text('q', C | A, "@"), vec![(c('\0'), 0, Some("`")), (c('q'), 0, Some("@"))]),
        ],
    );
}

/// The Win and Cmd keys are the super modifier.
#[test]
fn win_key_is_super() {
    let rows: Vec<(&str, RawKey, Vec<Want>)> = vec![
        ("win+e", raw_text('e', SU, "e"), vec![(c('e'), 8, None)]),
        ("win+e without text", RawKey::new(c('e'), SU), vec![(c('e'), 8, None)]),
        ("win+shift+s", raw_text('s', SU | S, "S"), vec![(c('s'), 9, None)]),
        ("win+ctrl+left", RawKey::new(n(NamedKey::Left), SU | C), vec![(n(NamedKey::Left), 12, None)]),
        ("left win press", RawKey::new(n(NamedKey::LeftSuper), N), vec![(n(NamedKey::LeftSuper), 8, None)]),
        ("right win press", RawKey::new(n(NamedKey::RightSuper), N), vec![(n(NamedKey::RightSuper), 8, None)]),
        (
            "left win release",
            RawKey::new(n(NamedKey::LeftSuper), SU).with_kind(KeyEventKind::Release),
            vec![(n(NamedKey::LeftSuper), 0, None)],
        ),
        ("left shift press", RawKey::new(n(NamedKey::LeftShift), N), vec![(n(NamedKey::LeftShift), 1, None)]),
        ("left control press", RawKey::new(n(NamedKey::LeftControl), N), vec![(n(NamedKey::LeftControl), 4, None)]),
        ("right alt press", RawKey::new(n(NamedKey::RightAlt), N), vec![(n(NamedKey::RightAlt), 2, None)]),
        ("left hyper press", RawKey::new(n(NamedKey::LeftHyper), N), vec![(n(NamedKey::LeftHyper), 16, None)]),
        ("right meta press", RawKey::new(n(NamedKey::RightMeta), N), vec![(n(NamedKey::RightMeta), 32, None)]),
        (
            "left control release",
            RawKey::new(n(NamedKey::LeftControl), C).with_kind(KeyEventKind::Release),
            vec![(n(NamedKey::LeftControl), 0, None)],
        ),
        (
            "shift keeps other modifiers on release",
            RawKey::new(n(NamedKey::LeftShift), S | C).with_kind(KeyEventKind::Release),
            vec![(n(NamedKey::LeftShift), 4, None)],
        ),
    ];
    for (label, raw, want) in rows {
        let mut normalizer = KeyNormalizer::new();
        assert_events(label, &normalizer.feed(&raw), &want);
    }
}

/// Numpad digits with num lock on type text, and with num lock off act as navigation keys.
#[test]
fn numpad_with_num_lock_on_and_off() {
    let nav = [
        (0u8, KeypadKey::Insert),
        (1, KeypadKey::End),
        (2, KeypadKey::Down),
        (3, KeypadKey::PageDown),
        (4, KeypadKey::Left),
        (5, KeypadKey::Begin),
        (6, KeypadKey::Right),
        (7, KeypadKey::Home),
        (8, KeypadKey::Up),
        (9, KeypadKey::PageUp),
    ];
    let digit_text = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    for (digit, navigation) in nav {
        let key = n(NamedKey::Kp(KeypadKey::Digit(digit)));
        let mut normalizer = KeyNormalizer::new();
        let on = normalizer.feed(&RawKey::new(key, N).with_numlock(true));
        assert_events("numlock on", &on, &[(key, 128, Some(digit_text[usize::from(digit)]))]);
        let off = normalizer.feed(&RawKey::new(key, N).with_numlock(false));
        assert_events("numlock off", &off, &[(n(NamedKey::Kp(navigation)), 0, None)]);
    }
    let others: Vec<(KeypadKey, Option<&str>, Option<&str>)> = vec![
        (KeypadKey::Decimal, Some("."), None),
        (KeypadKey::Add, Some("+"), Some("+")),
        (KeypadKey::Subtract, Some("-"), Some("-")),
        (KeypadKey::Multiply, Some("*"), Some("*")),
        (KeypadKey::Divide, Some("/"), Some("/")),
        (KeypadKey::Equal, Some("="), Some("=")),
        (KeypadKey::Separator, Some(","), Some(",")),
        (KeypadKey::Enter, None, None),
    ];
    for (kp, on_text, off_text) in others {
        let key = n(NamedKey::Kp(kp));
        let mut normalizer = KeyNormalizer::new();
        let on = normalizer.feed(&RawKey::new(key, N).with_numlock(true));
        assert_events("operator numlock on", &on, &[(key, 128, on_text)]);
        let off_key = if kp == KeypadKey::Decimal { n(NamedKey::Kp(KeypadKey::Delete)) } else { key };
        let off = normalizer.feed(&RawKey::new(key, N).with_numlock(false));
        assert_events("operator numlock off", &off, &[(off_key, 0, off_text)]);
    }
    let mut normalizer = KeyNormalizer::new();
    let left = n(NamedKey::Kp(KeypadKey::Left));
    assert_events("dedicated keypad left", &normalizer.feed(&RawKey::new(left, N)), &[(left, 0, None)]);
    assert_events(
        "dedicated keypad left with numlock",
        &normalizer.feed(&RawKey::new(left, N).with_numlock(true)),
        &[(left, 128, None)],
    );
}

/// Normalised events run through the encoders produce the bytes a terminal program expects.
#[test]
fn normalised_events_encode_correctly() {
    let encode_all = |events: &[KeyEvent], modes: &KeyboardModes| -> Vec<u8> {
        let mut out = Vec::new();
        for event in events {
            encode_key(event, modes, &mut out);
        }
        out
    };
    let legacy = KeyboardModes::default();
    let app_cursor = KeyboardModes { app_cursor: true, ..legacy };
    let kitty_text = KeyboardModes { kitty_flags: 24, ..legacy };
    let kitty_disambiguate = KeyboardModes { kitty_flags: 1, ..legacy };
    let digit5 = n(NamedKey::Kp(KeypadKey::Digit(5)));
    let digit8 = n(NamedKey::Kp(KeypadKey::Digit(8)));
    let mut normalizer = KeyNormalizer::new();
    let rows: Vec<(&str, Vec<RawKey>, &KeyboardModes, &[u8])> = vec![
        ("altgr at", vec![raw_text('q', C | A, "@")], &legacy, b"@"),
        ("real ctrl+alt+a", vec![raw_text('a', C | A, "\u{1}")], &legacy, b"\x1b\x01"),
        ("dead acute e", vec![dead('x', '\u{b4}'), raw_text('e', N, "e")], &legacy, b"\xc3\xa9"),
        (
            "dead acute e as kitty text",
            vec![dead('x', '\u{b4}'), raw_text('e', N, "e")],
            &kitty_text,
            b"\x1b[101;;233u",
        ),
        ("dead grave x", vec![dead('x', '`'), raw_text('x', N, "x")], &legacy, b"`x"),
        // A legacy program cannot read Super, so an unbound Win or Cmd key sends nothing (task-2589).
        ("win+e", vec![raw_text('e', SU, "e")], &legacy, b""),
        ("win+e under flag 1", vec![raw_text('e', SU, "e")], &kitty_disambiguate, b"\x1b[101;9u"),
        ("cmd+= as a US keyboard sends it", vec![raw_text('=', SU, "=")], &legacy, b""),
        ("numpad 5 with numlock", vec![RawKey::new(digit5, N).with_numlock(true)], &legacy, b"5"),
        ("numpad 5 without numlock", vec![RawKey::new(digit5, N)], &legacy, b"\x1b[E"),
        ("numpad 8 without numlock", vec![RawKey::new(digit8, N)], &legacy, b"\x1b[A"),
        ("numpad 8 without numlock in cursor mode", vec![RawKey::new(digit8, N)], &app_cursor, b"\x1bOA"),
        (
            "numpad 5 with numlock under flag 1",
            vec![RawKey::new(digit5, N).with_numlock(true)],
            &kitty_disambiguate,
            b"5",
        ),
        (
            "numpad 5 with numlock and text",
            vec![RawKey::new(digit5, N).with_numlock(true)],
            &kitty_text,
            b"\x1b[57404;129;53u",
        ),
        ("numpad 5 without numlock under flag 1", vec![RawKey::new(digit5, N)], &kitty_disambiguate, b"\x1b[E"),
    ];
    for (label, raws, modes, expected) in rows {
        count_row();
        let mut events = Vec::new();
        for raw in &raws {
            events.extend(normalizer.feed(raw));
        }
        let got = encode_all(&events, modes);
        assert_eq!(got.escape_ascii().to_string(), expected.escape_ascii().to_string(), "{label}");
    }
}

/// Reports the number of rows this binary ran. Runs last when tests run one at a time.
#[test]
fn zz_report_row_count() {
    println!("I3 normalise ROWS so far in this binary: {}", rows_run());
}
