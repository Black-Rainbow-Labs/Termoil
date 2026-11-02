//! I2: the kitty keyboard protocol. Expected bytes come from the published specification at
//! https://sw.kovidgoyal.net/kitty/keyboard-protocol/ . The functional key rows in tests/data are
//! generated from the spec's functional key table by a script that does not use this crate.
#![allow(clippy::type_complexity)]

mod common;

use common::*;
use terminus_input::{Key, KeyEvent, KeyEventKind as Kind, KeypadKey, Modifiers, NamedKey};

include!("data/kitty_functional.rs");

type Expect = Option<&'static [u8]>;

/// Every flag combination from 0 to 31 against keys whose press encoding depends only on flags 1 and 8.
#[test]
fn every_flag_combination_for_every_key_class() {
    use NamedKey::*;
    let kp5 = n(Kp(KeypadKey::Digit(5)));
    // key, mods, legacy bytes (flags without 1 or 8), disambiguate bytes (flag 1), all keys bytes (flag 8)
    let rows: Vec<(&str, Key, Modifiers, Expect, Expect, Expect)> = vec![
        ("a", c('a'), N, Some(b"a"), Some(b"a"), Some(b"\x1b[97u")),
        ("shift+a", c('a'), S, Some(b"A"), Some(b"A"), Some(b"\x1b[97;2u")),
        ("space", c(' '), N, Some(b" "), Some(b" "), Some(b"\x1b[32u")),
        ("ctrl+a", c('a'), C, Some(b"\x01"), Some(b"\x1b[97;5u"), Some(b"\x1b[97;5u")),
        ("alt+a", c('a'), A, Some(b"\x1ba"), Some(b"\x1b[97;3u"), Some(b"\x1b[97;3u")),
        ("ctrl+alt+a", c('a'), C | A, Some(b"\x1b\x01"), Some(b"\x1b[97;7u"), Some(b"\x1b[97;7u")),
        ("shift+alt+a", c('a'), S | A, Some(b"\x1bA"), Some(b"\x1b[97;4u"), Some(b"\x1b[97;4u")),
        ("escape", n(Escape), N, Some(b"\x1b"), Some(b"\x1b[27u"), Some(b"\x1b[27u")),
        ("enter", n(Enter), N, Some(b"\r"), Some(b"\r"), Some(b"\x1b[13u")),
        ("tab", n(Tab), N, Some(b"\t"), Some(b"\t"), Some(b"\x1b[9u")),
        ("backspace", n(Backspace), N, Some(b"\x7f"), Some(b"\x7f"), Some(b"\x1b[127u")),
        ("ctrl+enter", n(Enter), C, Some(b"\r"), Some(b"\x1b[13;5u"), Some(b"\x1b[13;5u")),
        ("shift+tab", n(Tab), S, Some(b"\x1b[Z"), Some(b"\x1b[9;2u"), Some(b"\x1b[9;2u")),
        ("ctrl+backspace", n(Backspace), C, Some(b"\x08"), Some(b"\x1b[127;5u"), Some(b"\x1b[127;5u")),
        ("up", n(Up), N, Some(b"\x1b[A"), Some(b"\x1b[A"), Some(b"\x1b[A")),
        ("ctrl+up", n(Up), C, Some(b"\x1b[1;5A"), Some(b"\x1b[1;5A"), Some(b"\x1b[1;5A")),
        ("home", n(Home), N, Some(b"\x1b[H"), Some(b"\x1b[H"), Some(b"\x1b[H")),
        ("delete", n(Delete), N, Some(b"\x1b[3~"), Some(b"\x1b[3~"), Some(b"\x1b[3~")),
        ("f5", n(F(5)), N, Some(b"\x1b[15~"), Some(b"\x1b[15~"), Some(b"\x1b[15~")),
        ("f13", n(F(13)), N, Some(b"\x1b[25~"), Some(b"\x1b[57376u"), Some(b"\x1b[57376u")),
        ("menu", n(Menu), N, Some(b"\x1b[29~"), Some(b"\x1b[57363u"), Some(b"\x1b[57363u")),
        ("keypad 5", kp5, N, Some(b"5"), Some(b"\x1b[57404u"), Some(b"\x1b[57404u")),
        ("caps lock key", n(CapsLock), N, None, Some(b"\x1b[57358u"), Some(b"\x1b[57358u")),
        ("left shift key", n(LeftShift), S, None, None, Some(b"\x1b[57441;2u")),
    ];
    for flags in 0u8..=31 {
        for (label, key, mods, legacy, disambiguate, all) in &rows {
            let expected = if flags & 8 != 0 {
                *all
            } else if flags & 1 != 0 {
                *disambiguate
            } else {
                *legacy
            };
            let mut event = ev(*key, *mods);
            event.shifted_key = None;
            if flags & 8 != 0 && flags & 16 != 0 {
                event.text = None;
            }
            check(&format!("{label} flags {flags}"), &event, &kitty(flags), expected);
        }
    }
}

/// Every key in the spec's functional key table: flag 8 plain, flag 8 with ctrl, flag 1 with shift+ctrl.
#[test]
fn every_functional_key_in_the_spec_table() {
    for (key, mods, flags, expected) in kitty_functional_rows() {
        check("functional", &ev(key, mods), &kitty(flags), expected);
    }
}

/// Press, repeat and release under flag 2 with the other flags that decide which keys are escape codes.
#[test]
fn press_repeat_and_release() {
    use NamedKey::*;
    let rep = |k: Key, m: Modifiers| ev(k, m).with_kind(Kind::Repeat);
    let rel = |k: Key, m: Modifiers| ev(k, m).with_kind(Kind::Release);
    let rows: Vec<(&str, KeyEvent, u8, Expect)> = vec![
        ("ctrl+a press", ev(c('a'), C), 3, Some(b"\x1b[97;5u")),
        ("ctrl+a repeat", rep(c('a'), C), 3, Some(b"\x1b[97;5:2u")),
        ("ctrl+a release", rel(c('a'), C), 3, Some(b"\x1b[97;5:3u")),
        ("a press", ev(c('a'), N), 3, Some(b"a")),
        ("a repeat", rep(c('a'), N), 3, Some(b"a")),
        ("a release", rel(c('a'), N), 3, None),
        ("shift+a release", rel(c('a'), S), 3, None),
        ("up press", ev(n(Up), N), 3, Some(b"\x1b[A")),
        ("up repeat", rep(n(Up), N), 3, Some(b"\x1b[1;1:2A")),
        ("up release", rel(n(Up), N), 3, Some(b"\x1b[1;1:3A")),
        ("shift+up release", rel(n(Up), S), 3, Some(b"\x1b[1;2:3A")),
        ("f5 repeat", rep(n(F(5)), N), 3, Some(b"\x1b[15;1:2~")),
        ("f5 release", rel(n(F(5)), N), 3, Some(b"\x1b[15;1:3~")),
        ("f1 release", rel(n(F(1)), C), 3, Some(b"\x1b[1;5:3P")),
        ("escape release", rel(n(Escape), N), 3, Some(b"\x1b[27;1:3u")),
        ("enter press", ev(n(Enter), N), 3, Some(b"\r")),
        ("enter repeat", rep(n(Enter), N), 3, Some(b"\r")),
        ("enter release", rel(n(Enter), N), 3, None),
        ("tab release", rel(n(Tab), N), 3, None),
        ("backspace release", rel(n(Backspace), N), 3, None),
        ("shift+enter release", rel(n(Enter), S), 3, None),
        ("a press all keys", ev(c('a'), N), 11, Some(b"\x1b[97u")),
        ("a repeat all keys", rep(c('a'), N), 11, Some(b"\x1b[97;1:2u")),
        ("a release all keys", rel(c('a'), N), 11, Some(b"\x1b[97;1:3u")),
        ("shift+a repeat all keys", rep(c('a'), S), 11, Some(b"\x1b[97;2:2u")),
        ("enter release all keys", rel(n(Enter), N), 11, Some(b"\x1b[13;1:3u")),
        ("tab release all keys", rel(n(Tab), N), 11, Some(b"\x1b[9;1:3u")),
        ("backspace release all keys", rel(n(Backspace), N), 11, Some(b"\x1b[127;1:3u")),
        ("left shift release all keys", rel(n(LeftShift), N), 11, Some(b"\x1b[57441;1:3u")),
        ("keypad 5 release", rel(n(Kp(KeypadKey::Digit(5))), N), 11, Some(b"\x1b[57404;1:3u")),
        ("a release flags 10", rel(c('a'), N), 10, Some(b"\x1b[97;1:3u")),
        ("a release flag 2 only", rel(c('a'), N), 2, None),
        ("a repeat flag 2 only", rep(c('a'), N), 2, Some(b"a")),
        ("ctrl+a release flag 2 only", rel(c('a'), C), 2, None),
        ("enter release flag 2 only", rel(n(Enter), N), 2, None),
        ("up release flag 2 only", rel(n(Up), N), 2, Some(b"\x1b[1;1:3A")),
        ("up repeat flag 2 only", rep(n(Up), N), 2, Some(b"\x1b[1;1:2A")),
        ("a release flag 0", rel(c('a'), N), 0, None),
        ("up release flag 0", rel(n(Up), N), 0, None),
        ("up repeat flag 0", rep(n(Up), N), 0, Some(b"\x1b[A")),
        ("a release flag 1", rel(c('a'), N), 1, None),
        ("ctrl+a release flag 1", rel(c('a'), C), 1, None),
        ("up release flag 1", rel(n(Up), N), 1, None),
        ("ctrl+a repeat flag 1", rep(c('a'), C), 1, Some(b"\x1b[97;5u")),
    ];
    for (label, event, flags, expected) in &rows {
        check(label, event, &kitty(*flags), *expected);
    }
}

/// Flag 4 adds the shifted key and the base layout key after the key code.
#[test]
fn alternate_keys() {
    let alt = |k: char, m: Modifiers, shifted: Option<char>, base: Option<char>| {
        KeyEvent::press(c(k), m).with_alternates(shifted, base)
    };
    let cyr_es = '\u{441}';
    let cyr_ef = '\u{444}';
    let rows: Vec<(&str, KeyEvent, u8, Expect)> = vec![
        ("ctrl+shift+a with shifted", alt('a', C | S, Some('A'), None), 5, Some(b"\x1b[97:65;6u")),
        ("base equal to key is dropped", alt('a', C | S, Some('A'), Some('a')), 5, Some(b"\x1b[97:65;6u")),
        ("base layout only", alt(cyr_es, C, None, Some('c')), 5, Some(b"\x1b[1089::99;5u")),
        ("shifted and base", alt(cyr_ef, C | S, Some('\u{424}'), Some('a')), 5, Some(b"\x1b[1092:1060:97;6u")),
        ("ctrl+shift+equal", alt('=', C | S, Some('+'), None), 5, Some(b"\x1b[61:43;6u")),
        ("ctrl+plus layout", alt('+', C, None, Some('=')), 5, Some(b"\x1b[43::61;5u")),
        ("shifted ignored without shift", alt('a', C, Some('A'), None), 5, Some(b"\x1b[97;5u")),
        ("no alternates present", alt('a', C, None, None), 5, Some(b"\x1b[97;5u")),
        ("flag 4 alone leaves text", alt('a', N, None, Some('c')), 4, Some(b"a")),
        ("flag 4 alone leaves ctrl legacy", alt('a', C, None, Some('c')), 4, Some(b"\x01")),
        ("flag 4 with all keys", alt(cyr_es, N, None, Some('c')), 12, Some(b"\x1b[1089::99u")),
        (
            "flag 4 with all keys and shift",
            alt(cyr_ef, S, Some('\u{424}'), Some('a')),
            12,
            Some(b"\x1b[1092:1060:97;2u"),
        ),
        ("flag 4 alone plain key", alt('a', N, None, None), 4, Some(b"a")),
        (
            "flag 4 with text and all keys",
            alt('a', S, Some('A'), Some('a')).with_text("A"),
            28,
            Some(b"\x1b[97:65;2;65u"),
        ),
    ];
    for (label, event, flags, expected) in &rows {
        check(label, event, &kitty(*flags), *expected);
    }
    let up = ev(n(NamedKey::Up), C).with_alternates(Some('x'), Some('y'));
    check("arrow ignores alternates", &up, &kitty(5), Some(b"\x1b[1;5A"));
}

/// Flag 16 adds the associated text as a third field, and only together with flag 8.
#[test]
fn associated_text() {
    use NamedKey::*;
    let txt = |k: Key, m: Modifiers, t: &str| KeyEvent::press(k, m).with_text(t);
    let rep = |e: KeyEvent| e.with_kind(Kind::Repeat);
    let rel = |e: KeyEvent| e.with_kind(Kind::Release);
    let rows: Vec<(&str, KeyEvent, u8, Expect)> = vec![
        ("shift+a text", txt(c('a'), S, "A"), 24, Some(b"\x1b[97;2;65u")),
        ("a text", txt(c('a'), N, "a"), 24, Some(b"\x1b[97;;97u")),
        ("text event without a key", txt(c('\0'), N, "\u{e5}"), 24, Some(b"\x1b[0;;229u")),
        ("two code points", txt(c('\0'), N, "ab"), 24, Some(b"\x1b[0;;97:98u")),
        ("three code points", txt(c('\0'), N, "a\u{301}b"), 24, Some(b"\x1b[0;;97:769:98u")),
        ("repeat keeps text", rep(txt(c('a'), N, "a")), 26, Some(b"\x1b[97;1:2;97u")),
        ("release drops text", rel(txt(c('a'), N, "a")), 26, Some(b"\x1b[97;1:3u")),
        ("control text dropped", txt(n(Enter), N, "\r"), 24, Some(b"\x1b[13u")),
        ("escape in text dropped", txt(c('['), C, "\x1b"), 24, Some(b"\x1b[91;5u")),
        ("no text no field", KeyEvent::press(c('a'), C), 24, Some(b"\x1b[97;5u")),
        ("flag 16 without 8 sends text", txt(c('a'), N, "a"), 17, Some(b"a")),
        ("flag 16 without 8 ctrl", KeyEvent::press(c('a'), C), 17, Some(b"\x1b[97;5u")),
        ("flag 16 alone", txt(c('a'), N, "a"), 16, Some(b"a")),
        ("flag 16 alone ctrl", KeyEvent::press(c('a'), C), 16, Some(b"\x01")),
        ("flags 4 8 16", txt(c('a'), S, "A").with_alternates(Some('A'), Some('a')), 28, Some(b"\x1b[97:65;2;65u")),
        ("keypad text", txt(n(Kp(KeypadKey::Digit(5))), N, "5"), 24, Some(b"\x1b[57404;;53u")),
        ("arrow has no text", txt(n(Up), N, "x"), 24, Some(b"\x1b[A")),
        ("text with modifiers", txt(c('a'), S | Modifiers::CAPS_LOCK, "a"), 24, Some(b"\x1b[97;66;97u")),
        ("all flags", txt(c('a'), S, "A").with_alternates(Some('A'), None), 31, Some(b"\x1b[97:65;2;65u")),
    ];
    for (label, event, flags, expected) in &rows {
        check(label, event, &kitty(*flags), *expected);
    }
}

/// Lock modifiers are left out for text producing keys unless all keys are reported.
#[test]
fn lock_modifiers() {
    use NamedKey::*;
    let caps = Modifiers::CAPS_LOCK;
    let num = Modifiers::NUM_LOCK;
    let rows: Vec<(&str, KeyEvent, u8, Expect)> = vec![
        ("text key with caps under flag 1", ev(c('a'), caps).with_text("A"), 1, Some(b"A")),
        ("text key with caps under flag 8", ev(c('a'), caps), 8, Some(b"\x1b[97;65u")),
        ("text key with num under flag 8", ev(c('a'), num), 8, Some(b"\x1b[97;129u")),
        ("text key with both under flag 8", ev(c('a'), caps | num), 8, Some(b"\x1b[97;193u")),
        ("ctrl+a with caps under flag 1", ev(c('a'), C | caps), 1, Some(b"\x1b[97;5u")),
        ("enter with caps under flag 1", ev(n(Enter), caps), 1, Some(b"\r")),
        ("enter with caps under flag 8", ev(n(Enter), caps), 8, Some(b"\x1b[13;65u")),
        ("up with caps under flag 1", ev(n(Up), caps), 1, Some(b"\x1b[1;65A")),
        ("up with num under flag 1", ev(n(Up), num), 1, Some(b"\x1b[1;129A")),
        ("f5 with caps under flag 1", ev(n(F(5)), caps), 1, Some(b"\x1b[15;65~")),
        ("escape with num under flag 1", ev(n(Escape), num), 1, Some(b"\x1b[27;129u")),
        ("lock key under flag 1", ev(n(CapsLock), caps), 1, Some(b"\x1b[57358;65u")),
        ("legacy ignores locks", ev(n(Up), caps | num), 0, Some(b"\x1b[A")),
    ];
    for (label, event, flags, expected) in &rows {
        check(label, event, &kitty(*flags), *expected);
    }
}

/// Examples printed on the specification page. Each row cites the passage it comes from.
#[test]
fn examples_from_the_spec() {
    use NamedKey::*;
    let txt = |k: Key, m: Modifiers, t: &str| KeyEvent::press(k, m).with_text(t);
    let rows: Vec<(&str, KeyEvent, u8, Expect)> = vec![
        // "Key codes": ctrl+shift+a is CSI 97;modifiers u, never CSI 65.
        ("key codes: ctrl+shift+a", ev(c('a'), C | S), 1, Some(b"\x1b[97;6u")),
        // "Modifiers": shift only is 1 + 1 = 2, ctrl+shift is 1 + 0b101 = 6.
        ("modifiers: shift+up", ev(n(Up), S), 1, Some(b"\x1b[1;2A")),
        ("modifiers: ctrl+shift+up", ev(n(Up), C | S), 1, Some(b"\x1b[1;6A")),
        ("modifiers: super", ev(n(Up), SU), 1, Some(b"\x1b[1;9A")),
        ("modifiers: hyper", ev(n(Up), Modifiers::HYPER), 1, Some(b"\x1b[1;17A")),
        ("modifiers: meta", ev(n(Up), Modifiers::META), 1, Some(b"\x1b[1;33A")),
        // "Text as code points": shift+a -> CSI 97 ; 2 ; 65 u
        ("text: shift+a", txt(c('a'), S, "A"), 24, Some(b"\x1b[97;2;65u")),
        // "Text as code points": alt+a -> CSI 0 ; ; 229 u (the OS consumed alt and produced a-ring)
        ("text: alt+a consumed by the OS", txt(c('\0'), N, "\u{e5}"), 24, Some(b"\x1b[0;;229u")),
        // "Event types": CSI key-code;modifier:1 / :2 / :3
        ("events: repeat", ev(c('a'), C).with_kind(Kind::Repeat), 3, Some(b"\x1b[97;5:2u")),
        ("events: release", ev(c('a'), C).with_kind(Kind::Release), 3, Some(b"\x1b[97;5:3u")),
        // "Event types": with no modifiers the modifier field must have the value 1.
        ("events: release without modifiers", ev(n(Escape), N).with_kind(Kind::Release), 3, Some(b"\x1b[27;1:3u")),
        // "Disambiguate escape codes": Esc becomes CSI 27 u, ctrl+c becomes CSI 99;5u.
        ("disambiguate: escape", ev(n(Escape), N), 1, Some(b"\x1b[27u")),
        ("disambiguate: ctrl+c", ev(c('c'), C), 1, Some(b"\x1b[99;5u")),
        ("disambiguate: alt+[", ev(c('['), A), 1, Some(b"\x1b[91;3u")),
        // "Disambiguate escape codes": Enter, Tab and Backspace keep their legacy bytes.
        ("disambiguate: enter", ev(n(Enter), N), 1, Some(b"\x0d")),
        ("disambiguate: tab", ev(n(Tab), N), 1, Some(b"\x09")),
        ("disambiguate: backspace", ev(n(Backspace), N), 1, Some(b"\x7f")),
        // "Bugs in fixterms": ctrl+shift+tab should be CSI 9 ; 6 u
        ("fixterms: ctrl+shift+tab", ev(n(Tab), C | S), 1, Some(b"\x1b[9;6u")),
        // "Functional key definitions": the CSI 1 letter forms omit the 1 without modifiers.
        ("functional: f3", ev(n(F(3)), N), 1, Some(b"\x1b[13~")),
        ("functional: f1", ev(n(F(1)), N), 1, Some(b"\x1b[P")),
        ("functional: f1 with ctrl", ev(n(F(1)), C), 1, Some(b"\x1b[1;5P")),
        ("functional: f4 with alt", ev(n(F(4)), A), 1, Some(b"\x1b[1;3S")),
        ("functional: keypad begin", ev(n(Kp(KeypadKey::Begin)), N), 1, Some(b"\x1b[E")),
        ("functional: keypad begin with shift", ev(n(Kp(KeypadKey::Begin)), S), 1, Some(b"\x1b[1;2E")),
        // "Report all keys as escape codes": modifier key events carry the modifier state including the event.
        ("all keys: left control press", ev(n(LeftControl), C), 8, Some(b"\x1b[57442;5u")),
        (
            "all keys: left control release",
            ev(n(LeftControl), N).with_kind(Kind::Release),
            10,
            Some(b"\x1b[57442;1:3u"),
        ),
        // "Quickstart" and "Progressive enhancement": a flag bit above 16 is not defined and selects legacy.
        ("flags above 31 are ignored", ev(c('a'), C), 32, Some(b"\x01")),
    ];
    for (label, event, flags, expected) in &rows {
        check(label, event, &kitty(*flags), *expected);
    }
}

/// The "Example encodings" table of the legacy text keys section, in legacy mode.
#[test]
fn legacy_text_key_example_encodings() {
    let m = modes();
    let columns: [(&str, Modifiers); 7] = [
        ("plain", N),
        ("shift", S),
        ("alt", A),
        ("ctrl", C),
        ("shift+alt", S | A),
        ("alt+ctrl", A | C),
        ("ctrl+shift", C | S),
    ];
    // Rows of the table: i, 3 and semicolon. One cell differs from the protocol document on
    // purpose: ctrl+shift+i is the control byte, as ctrl+i, because legacy mode follows xterm (where
    // Ctrl+Shift+R equals Ctrl+R; docs/research/testing-corpora.md and TEST-SUITE section 7, I1).
    // A shell at a prompt prints a CSI u form as text. Keys with no control byte keep CSI u.
    let table: [(char, [&[u8]; 7]); 3] = [
        ('i', [b"i", b"I", b"\x1bi", b"\t", b"\x1bI", b"\x1b\t", b"\t"]),
        ('3', [b"3", b"#", b"\x1b3", b"\x1b", b"\x1b#", b"\x1b\x1b", b"\x1b[51;6u"]),
        (';', [b";", b":", b"\x1b;", b";", b"\x1b:", b"\x1b;", b"\x1b[59;6u"]),
    ];
    for (ch, expected) in table {
        for ((name, mods), want) in columns.iter().zip(expected) {
            check(&format!("example {ch} {name}"), &ev(c(ch), *mods), &m, Some(want));
        }
    }
}

/// The "C0 controls" table of the legacy functional keys section, in legacy mode.
#[test]
fn legacy_c0_control_table() {
    use NamedKey::*;
    let m = modes();
    let columns: [(&str, Modifiers); 7] = [
        ("none", N),
        ("ctrl", C),
        ("alt", A),
        ("shift", S),
        ("ctrl+shift", C | S),
        ("alt+shift", A | S),
        ("ctrl+alt", C | A),
    ];
    let table: [(Key, [&[u8]; 7]); 5] = [
        (n(Enter), [b"\x0d", b"\x0d", b"\x1b\x0d", b"\x0d", b"\x0d", b"\x1b\x0d", b"\x1b\x0d"]),
        (n(Escape), [b"\x1b", b"\x1b", b"\x1b\x1b", b"\x1b", b"\x1b", b"\x1b\x1b", b"\x1b\x1b"]),
        (n(Backspace), [b"\x7f", b"\x08", b"\x1b\x7f", b"\x7f", b"\x08", b"\x1b\x7f", b"\x1b\x08"]),
        (n(Tab), [b"\x09", b"\x09", b"\x1b\x09", b"\x1b[Z", b"\x1b[Z", b"\x1b\x1b[Z", b"\x1b\x09"]),
        (c(' '), [b"\x20", b"\x00", b"\x1b\x20", b"\x20", b"\x00", b"\x1b\x20", b"\x1b\x00"]),
    ];
    for (key, expected) in table {
        for ((name, mods), want) in columns.iter().zip(expected) {
            check(&format!("c0 {key:?} {name}"), &ev(key, *mods), &m, Some(want));
        }
    }
}

/// The "Legacy functional encoding" table, including the application cursor key alternates.
#[test]
fn legacy_functional_encoding_table() {
    use NamedKey::*;
    let app = terminus_input::KeyboardModes { app_cursor: true, ..modes() };
    let plain: [(NamedKey, &[u8], &[u8]); 20] = [
        (Insert, b"\x1b[2~", b"\x1b[2~"),
        (Delete, b"\x1b[3~", b"\x1b[3~"),
        (PageUp, b"\x1b[5~", b"\x1b[5~"),
        (PageDown, b"\x1b[6~", b"\x1b[6~"),
        (Up, b"\x1b[A", b"\x1bOA"),
        (Down, b"\x1b[B", b"\x1bOB"),
        (Right, b"\x1b[C", b"\x1bOC"),
        (Left, b"\x1b[D", b"\x1bOD"),
        (Home, b"\x1b[H", b"\x1bOH"),
        (End, b"\x1b[F", b"\x1bOF"),
        (F(1), b"\x1bOP", b"\x1bOP"),
        (F(2), b"\x1bOQ", b"\x1bOQ"),
        (F(3), b"\x1bOR", b"\x1bOR"),
        (F(4), b"\x1bOS", b"\x1bOS"),
        (F(5), b"\x1b[15~", b"\x1b[15~"),
        (F(6), b"\x1b[17~", b"\x1b[17~"),
        (F(9), b"\x1b[20~", b"\x1b[20~"),
        (F(11), b"\x1b[23~", b"\x1b[23~"),
        (F(12), b"\x1b[24~", b"\x1b[24~"),
        (Menu, b"\x1b[29~", b"\x1b[29~"),
    ];
    for (key, normal, application) in plain {
        check("legacy table normal", &ev(n(key), N), &modes(), Some(normal));
        check("legacy table cursor mode", &ev(n(key), N), &app, Some(application));
        check("legacy table with modifiers in cursor mode", &ev(n(key), C), &app, ev_expect_modified(key));
    }
}

/// Returns the expected bytes of a key with ctrl held, which never uses SS3.
fn ev_expect_modified(key: NamedKey) -> Expect {
    use NamedKey::*;
    Some(match key {
        Insert => b"\x1b[2;5~",
        Delete => b"\x1b[3;5~",
        PageUp => b"\x1b[5;5~",
        PageDown => b"\x1b[6;5~",
        Up => b"\x1b[1;5A",
        Down => b"\x1b[1;5B",
        Right => b"\x1b[1;5C",
        Left => b"\x1b[1;5D",
        Home => b"\x1b[1;5H",
        End => b"\x1b[1;5F",
        F(1) => b"\x1b[1;5P",
        F(2) => b"\x1b[1;5Q",
        F(3) => b"\x1b[1;5R",
        F(4) => b"\x1b[1;5S",
        F(5) => b"\x1b[15;5~",
        F(6) => b"\x1b[17;5~",
        F(9) => b"\x1b[20;5~",
        F(11) => b"\x1b[23;5~",
        F(12) => b"\x1b[24;5~",
        _ => b"\x1b[29;5~",
    })
}

/// The "Legacy ctrl mapping of ASCII keys" table.
#[test]
fn legacy_ctrl_mapping_table() {
    let mut rows: Vec<(char, u8)> = vec![
        (' ', 0),
        ('/', 31),
        ('0', b'0'),
        ('1', b'1'),
        ('2', 0),
        ('3', 27),
        ('4', 28),
        ('5', 29),
        ('6', 30),
        ('7', 31),
        ('8', 127),
        ('9', b'9'),
    ];
    rows.extend([('?', 127), ('@', 0), ('[', 27), ('\\', 28), (']', 29), ('^', 30), ('_', 31), ('~', 30)]);
    rows.extend(('a'..='z').enumerate().map(|(i, ch)| (ch, i as u8 + 1)));
    for (ch, byte) in rows {
        check(&format!("ctrl mapping {ch:?}"), &ev(c(ch), C), &modes(), Some(&[byte]));
    }
}

/// Keys the spec lists that this encoder is expected to refuse, and out of range function keys.
#[test]
fn unsupported_and_invalid_keys() {
    use NamedKey::*;
    let rows: Vec<(&str, KeyEvent, u8, Expect)> = vec![
        ("f0 is invalid", ev(n(F(0)), N), 1, None),
        ("f36 is invalid", ev(n(F(36)), N), 1, None),
        ("f35 is valid", ev(n(F(35)), N), 1, Some(b"\x1b[57398u")),
        ("modifier key under flag 1", ev(n(LeftShift), S), 1, None),
        ("modifier key under flag 3", ev(n(LeftShift), S), 3, None),
        ("modifier key under flag 31 sends", ev(n(RightMeta), Modifiers::META), 31, Some(b"\x1b[57452;33u")),
        ("keypad with text under flag 1 sends text", ev(n(Kp(KeypadKey::Digit(7))), N).with_text("7"), 1, Some(b"7")),
        ("keypad enter has no text", ev(n(Kp(KeypadKey::Enter)), N), 1, Some(b"\x1b[57414u")),
        ("keypad with ctrl under flag 1", ev(n(Kp(KeypadKey::Digit(7))), C).with_text("7"), 1, Some(b"\x1b[57406;5u")),
        ("super+a under flag 1", ev(c('a'), SU), 1, Some(b"\x1b[97;9u")),
        ("hyper+a under flag 1", ev(c('a'), Modifiers::HYPER), 1, Some(b"\x1b[97;17u")),
        ("meta+a under flag 1", ev(c('a'), Modifiers::META), 1, Some(b"\x1b[97;33u")),
        ("media play under flag 1", ev(n(MediaPlay), N), 1, Some(b"\x1b[57428u")),
        ("uppercase key code is lowercased", ev(c('A'), C), 1, Some(b"\x1b[97;5u")),
        ("non ascii key code", ev(c('\u{e9}'), C), 1, Some(b"\x1b[233;5u")),
    ];
    for (label, event, flags, expected) in &rows {
        check(label, event, &kitty(*flags), *expected);
    }
}

/// Reports the number of rows this binary ran. Runs last when tests run one at a time.
#[test]
fn zz_report_row_count() {
    println!("I2 ROWS so far in this binary: {}", rows_run());
}
