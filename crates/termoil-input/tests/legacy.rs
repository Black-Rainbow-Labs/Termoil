//! I1: the legacy xterm encoding, application cursor and keypad modes, and modifyOtherKeys.
//! The grid rows in tests/data are generated from the published tables by a script that does not use this crate.
#![allow(clippy::type_complexity)]

mod common;

use common::*;
use termoil_input::{Key, KeyEvent, KeyEventKind, KeyboardModes, KeypadKey, Modifiers, NamedKey};

include!("data/legacy_printable.rs");
include!("data/legacy_named.rs");
include!("data/legacy_named_app_cursor.rs");
include!("data/legacy_keypad.rs");

/// Every printable ASCII key with none, shift, ctrl, alt, ctrl+alt, shift+alt and ctrl+shift.
#[test]
fn printable_keys_with_modifier_combinations() {
    for (key, mods, expected) in legacy_printable_rows() {
        check("printable", &ev(key, mods), &modes(), expected);
    }
}

/// Every named key with all eight combinations of shift, alt and ctrl.
#[test]
fn named_keys_with_all_modifier_combinations() {
    for (key, mods, expected) in legacy_named_rows() {
        check("named", &ev(key, mods), &modes(), expected);
    }
}

/// Cursor keys, Home and End in application cursor mode with all eight modifier combinations.
#[test]
fn application_cursor_mode() {
    let app = KeyboardModes { app_cursor: true, ..modes() };
    for (key, mods, expected) in legacy_app_cursor_rows() {
        check("app cursor", &ev(key, mods), &app, expected);
    }
}

/// Application cursor mode does not change keys that are not cursor keys.
#[test]
fn application_cursor_mode_leaves_other_keys_alone() {
    let app = KeyboardModes { app_cursor: true, ..modes() };
    let rows: &[(Key, &[u8])] = &[
        (n(NamedKey::Insert), b"\x1b[2~"),
        (n(NamedKey::Delete), b"\x1b[3~"),
        (n(NamedKey::PageUp), b"\x1b[5~"),
        (n(NamedKey::PageDown), b"\x1b[6~"),
        (n(NamedKey::F(5)), b"\x1b[15~"),
        (n(NamedKey::F(1)), b"\x1bOP"),
        (n(NamedKey::Enter), b"\r"),
        (c('a'), b"a"),
    ];
    for &(key, expected) in rows {
        check("app cursor other", &ev(key, N), &app, Some(expected));
    }
}

/// Every keypad key with application keypad off and on, and with application cursor mode on.
#[test]
fn keypad_keys_in_both_keypad_modes() {
    let off = modes();
    let on = KeyboardModes { app_keypad: true, ..modes() };
    let on_cursor = KeyboardModes { app_keypad: true, app_cursor: true, ..modes() };
    for (key, mods, plain, app) in legacy_keypad_rows() {
        check("keypad off", &ev(key, mods), &off, plain);
        check("keypad on", &ev(key, mods), &on, app);
        let nav = matches!(
            key,
            Key::Named(NamedKey::Kp(
                KeypadKey::Left
                    | KeypadKey::Right
                    | KeypadKey::Up
                    | KeypadKey::Down
                    | KeypadKey::Home
                    | KeypadKey::End
                    | KeypadKey::Begin
            ))
        );
        if !nav {
            check("keypad on with app cursor", &ev(key, mods), &on_cursor, app);
        }
    }
}

/// Keypad navigation keys follow application cursor mode like the main block keys.
#[test]
fn keypad_navigation_follows_application_cursor() {
    let cursor = KeyboardModes { app_cursor: true, ..modes() };
    let rows: &[(KeypadKey, &[u8])] = &[
        (KeypadKey::Up, b"\x1bOA"),
        (KeypadKey::Down, b"\x1bOB"),
        (KeypadKey::Right, b"\x1bOC"),
        (KeypadKey::Left, b"\x1bOD"),
        (KeypadKey::Home, b"\x1bOH"),
        (KeypadKey::End, b"\x1bOF"),
        (KeypadKey::Begin, b"\x1bOE"),
    ];
    for &(kp, expected) in rows {
        check("keypad nav", &ev(n(NamedKey::Kp(kp)), N), &cursor, Some(expected));
    }
}

/// Modified keypad keys: plain mode types the character with the usual modifier handling, application mode adds the parameter.
#[test]
fn keypad_keys_with_modifiers() {
    let on = KeyboardModes { app_keypad: true, ..modes() };
    let off = modes();
    let kp = |k| n(NamedKey::Kp(k));
    let rows: &[(Key, Modifiers, &KeyboardModes, &[u8])] = &[
        (kp(KeypadKey::Digit(5)), S, &on, b"\x1bO2u"),
        (kp(KeypadKey::Enter), A, &on, b"\x1bO3M"),
        (kp(KeypadKey::Add), C, &on, b"\x1bO5k"),
        (kp(KeypadKey::Digit(0)), S | C, &on, b"\x1bO6p"),
        (kp(KeypadKey::Up), C, &on, b"\x1b[1;5A"),
        (kp(KeypadKey::Delete), S, &on, b"\x1b[3;2~"),
        (kp(KeypadKey::Digit(5)), A, &off, b"\x1b5"),
        (kp(KeypadKey::Enter), A, &off, b"\x1b\r"),
        (kp(KeypadKey::Multiply), C, &off, b"*"),
        (kp(KeypadKey::Digit(1)), N, &off, b"1"),
    ];
    for &(key, mods, m, expected) in rows {
        check("keypad modified", &ev(key, mods), m, Some(expected));
    }
}

/// The named special cases from the protocol notes and xterm.
#[test]
fn named_special_cases() {
    let rows: &[(&str, Key, Modifiers, &[u8])] = &[
        ("Ctrl+I is Tab", c('i'), C, b"\t"),
        ("Ctrl+M is Enter", c('m'), C, b"\r"),
        ("Ctrl+Space is NUL", c(' '), C, b"\0"),
        ("Ctrl+@ is NUL", c('@'), C, b"\0"),
        ("Ctrl+[ is ESC", c('['), C, b"\x1b"),
        ("Ctrl+backslash", c('\\'), C, b"\x1c"),
        ("Ctrl+]", c(']'), C, b"\x1d"),
        ("Ctrl+^", c('^'), C, b"\x1e"),
        ("Ctrl+~", c('~'), C, b"\x1e"),
        ("Ctrl+_", c('_'), C, b"\x1f"),
        ("Ctrl+/", c('/'), C, b"\x1f"),
        ("Ctrl+?", c('?'), C, b"\x7f"),
        ("Ctrl+2", c('2'), C, b"\0"),
        ("Ctrl+3", c('3'), C, b"\x1b"),
        ("Ctrl+4", c('4'), C, b"\x1c"),
        ("Ctrl+5", c('5'), C, b"\x1d"),
        ("Ctrl+6", c('6'), C, b"\x1e"),
        ("Ctrl+7", c('7'), C, b"\x1f"),
        ("Ctrl+8", c('8'), C, b"\x7f"),
        ("Ctrl+0 untouched", c('0'), C, b"0"),
        ("Ctrl+1 untouched", c('1'), C, b"1"),
        ("Ctrl+9 untouched", c('9'), C, b"9"),
        ("Ctrl+minus untouched", c('-'), C, b"-"),
        ("Ctrl+semicolon untouched", c(';'), C, b";"),
        ("Alt+[ is the CSI introducer", c('['), A, b"\x1b["),
        ("Alt+Shift+semicolon", c(';'), A | S, b"\x1b:"),
        ("Ctrl+Alt+[", c('['), C | A, b"\x1b\x1b"),
        ("Shift+Tab is CSI Z", n(NamedKey::Tab), S, b"\x1b[Z"),
        ("Alt+Shift+Tab", n(NamedKey::Tab), A | S, b"\x1b\x1b[Z"),
        ("Ctrl+Tab", n(NamedKey::Tab), C, b"\t"),
        ("Alt+Backspace", n(NamedKey::Backspace), A, b"\x1b\x7f"),
        ("Ctrl+Backspace is BS", n(NamedKey::Backspace), C, b"\x08"),
        ("Ctrl+Alt+Backspace", n(NamedKey::Backspace), C | A, b"\x1b\x08"),
        ("Shift+Backspace", n(NamedKey::Backspace), S, b"\x7f"),
        ("Alt+Enter", n(NamedKey::Enter), A, b"\x1b\r"),
        ("Ctrl+Enter is CR", n(NamedKey::Enter), C, b"\r"),
        ("Alt+Escape", n(NamedKey::Escape), A, b"\x1b\x1b"),
        ("Menu is CSI 29 ~", n(NamedKey::Menu), N, b"\x1b[29~"),
        ("F21 has no legacy form", n(NamedKey::F(21)), N, b"\x1b[57384u"),
        ("Shift+F13", n(NamedKey::F(13)), S, b"\x1b[25;2~"),
    ];
    for &(label, key, mods, expected) in rows {
        check(label, &ev(key, mods), &modes(), Some(expected));
    }
}

/// Backspace follows DECBKM and Ctrl+Backspace sends the opposite byte.
#[test]
fn backspace_follows_backarrow_mode() {
    let bs = KeyboardModes { backarrow_is_bs: true, ..modes() };
    let del = modes();
    let rows: &[(Modifiers, &KeyboardModes, &[u8])] = &[
        (N, &del, b"\x7f"),
        (C, &del, b"\x08"),
        (N, &bs, b"\x08"),
        (C, &bs, b"\x7f"),
        (A, &bs, b"\x1b\x08"),
        (A | C, &bs, b"\x1b\x7f"),
        (S, &bs, b"\x08"),
        (S | C, &del, b"\x08"),
    ];
    for &(mods, m, expected) in rows {
        check("backspace", &ev(n(NamedKey::Backspace), mods), m, Some(expected));
    }
}

/// With alt_sends_escape off, alt sets the eighth bit of an ASCII key and the result is UTF-8 coded.
#[test]
fn alt_without_escape_sets_the_eighth_bit() {
    let meta = KeyboardModes { alt_sends_escape: false, ..modes() };
    let rows: &[(Key, Modifiers, &[u8])] =
        &[(c('a'), A, b"\xc3\xa1"), (c('a'), A | S, b"\xc3\x81"), (c('a'), A | C, b"\xc2\x81"), (c('a'), N, b"a")];
    for &(key, mods, expected) in rows {
        check("alt eighth bit", &ev(key, mods), &meta, Some(expected));
    }
}

/// Super, hyper and meta have no legacy byte and use the CSI u form.
#[test]
fn extended_modifiers_use_csi_u() {
    let rows: &[(Key, Modifiers, &[u8])] = &[
        (c('a'), SU, b"\x1b[97;9u"),
        (c('a'), Modifiers::HYPER, b"\x1b[97;17u"),
        (c('a'), Modifiers::META, b"\x1b[97;33u"),
        (c('a'), SU | C, b"\x1b[97;13u"),
        (n(NamedKey::Enter), SU, b"\x1b[13;9u"),
        (n(NamedKey::Escape), SU, b"\x1b[27;9u"),
        (n(NamedKey::Up), SU, b"\x1b[1;9A"),
        (n(NamedKey::F(5)), SU, b"\x1b[15;9~"),
    ];
    for &(key, mods, expected) in rows {
        check("extended modifiers", &ev(key, mods), &modes(), Some(expected));
    }
}

/// Lock modifiers are not encoded in legacy mode, and keys without a legacy encoding send nothing.
#[test]
fn lock_modifiers_and_keys_without_legacy_encoding() {
    let caps = Modifiers::CAPS_LOCK;
    let num = Modifiers::NUM_LOCK;
    let rows: &[(Key, Modifiers, Option<&[u8]>)] = &[
        (n(NamedKey::Up), num, Some(b"\x1b[A")),
        (n(NamedKey::Up), caps | num | C, Some(b"\x1b[1;5A")),
        (c('a'), caps, Some(b"a")),
        (c('a'), caps | C, Some(b"\x01")),
        (n(NamedKey::CapsLock), N, None),
        (n(NamedKey::NumLock), N, None),
        (n(NamedKey::ScrollLock), N, None),
        (n(NamedKey::PrintScreen), N, None),
        (n(NamedKey::Pause), N, None),
        (n(NamedKey::LeftShift), S, None),
        (n(NamedKey::MediaPlay), N, None),
        (n(NamedKey::F(36)), N, None),
    ];
    for &(key, mods, expected) in rows {
        check("locks and unmapped", &ev(key, mods), &modes(), expected);
    }
}

/// Releases send nothing, repeats send what a press sends, and text from the layout is used as given.
#[test]
fn event_kinds_and_layout_text() {
    let release = ev(c('a'), N).with_kind(KeyEventKind::Release);
    check("release", &release, &modes(), None);
    let release_up = ev(n(NamedKey::Up), N).with_kind(KeyEventKind::Release);
    check("release up", &release_up, &modes(), None);
    let repeat = ev(c('a'), N).with_kind(KeyEventKind::Repeat);
    check("repeat", &repeat, &modes(), Some(b"a"));
    let repeat_up = ev(n(NamedKey::Up), C).with_kind(KeyEventKind::Repeat);
    check("repeat up", &repeat_up, &modes(), Some(b"\x1b[1;5A"));
    check("accent text", &ev(c('e'), N).with_text("\u{e9}"), &modes(), Some(b"\xc3\xa9"));
    check("caps text", &ev(c('a'), Modifiers::CAPS_LOCK).with_text("A"), &modes(), Some(b"A"));
    check("pure text event", &ev(c('\0'), N).with_text("\u{e5}"), &modes(), Some(b"\xc3\xa5"));
    check("pure text without text", &KeyEvent::press(c('\0'), N), &modes(), None);
    check("german shift", &ev(c('z'), S).with_text("Z"), &modes(), Some(b"Z"));
    check("alt ignores layout text", &ev(c('a'), A).with_text("\u{e5}"), &modes(), Some(b"\x1ba"));
    check("non ascii with ctrl", &ev(c('\u{441}'), C), &modes(), Some(b"\x1b[1089;5u"));
    check("non ascii with alt", &ev(c('\u{441}'), A), &modes(), Some(b"\x1b\xd1\x81"));
}

/// modifyOtherKeys level 2: every key with a modifier other than shift alone uses `CSI 27;m;code~`.
#[test]
fn modify_other_keys_level_2() {
    let m = mok(2);
    let rows: &[(Key, Modifiers, &[u8])] = &[
        (c('a'), N, b"a"),
        (c('a'), S, b"A"),
        (c('a'), C, b"\x1b[27;5;97~"),
        (c('a'), C | S, b"\x1b[27;6;65~"),
        (c('a'), A, b"\x1b[27;3;97~"),
        (c('a'), A | S, b"\x1b[27;4;65~"),
        (c('a'), C | A, b"\x1b[27;7;97~"),
        (c('a'), SU, b"\x1b[27;9;97~"),
        (c(' '), C, b"\x1b[27;5;32~"),
        (c('i'), C, b"\x1b[27;5;105~"),
        (c('m'), C, b"\x1b[27;5;109~"),
        (c('['), C, b"\x1b[27;5;91~"),
        (c('1'), C, b"\x1b[27;5;49~"),
        (c(';'), C, b"\x1b[27;5;59~"),
        (c('1'), C | S, b"\x1b[27;6;33~"),
        (n(NamedKey::Enter), N, b"\r"),
        (n(NamedKey::Enter), S, b"\x1b[27;2;13~"),
        (n(NamedKey::Enter), C, b"\x1b[27;5;13~"),
        (n(NamedKey::Enter), A, b"\x1b[27;3;13~"),
        (n(NamedKey::Tab), N, b"\t"),
        (n(NamedKey::Tab), S, b"\x1b[Z"),
        (n(NamedKey::Tab), C, b"\x1b[27;5;9~"),
        (n(NamedKey::Tab), C | S, b"\x1b[27;6;9~"),
        (n(NamedKey::Backspace), N, b"\x7f"),
        (n(NamedKey::Backspace), C, b"\x1b[27;5;127~"),
        (n(NamedKey::Backspace), A, b"\x1b[27;3;127~"),
        (n(NamedKey::Escape), N, b"\x1b"),
        (n(NamedKey::Escape), S, b"\x1b[27;2;27~"),
        (n(NamedKey::Up), C, b"\x1b[1;5A"),
        (n(NamedKey::F(5)), C, b"\x1b[15;5~"),
    ];
    for &(key, mods, expected) in rows {
        check("mok 2", &ev(key, mods), &m, Some(expected));
    }
}

/// modifyOtherKeys level 1: only combinations that have no distinct control byte use `CSI 27;m;code~`.
#[test]
fn modify_other_keys_level_1() {
    let m = mok(1);
    let rows: &[(Key, Modifiers, &[u8])] = &[
        (c('a'), N, b"a"),
        (c('a'), S, b"A"),
        (c('a'), C, b"\x01"),
        (c('a'), A, b"\x1ba"),
        (c('a'), C | A, b"\x1b\x01"),
        (c('a'), C | S, b"\x1b[27;6;65~"),
        (c('a'), SU, b"\x1b[27;9;97~"),
        (c('i'), C, b"\t"),
        (c('m'), C, b"\r"),
        (c(' '), C, b"\0"),
        (c('['), C, b"\x1b"),
        (c('2'), C, b"\0"),
        (c('2'), C | S, b"\x1b[27;6;64~"),
        (c('0'), C, b"\x1b[27;5;48~"),
        (c('1'), C, b"\x1b[27;5;49~"),
        (c('9'), C, b"\x1b[27;5;57~"),
        (c('-'), C, b"\x1b[27;5;45~"),
        (c(';'), C, b"\x1b[27;5;59~"),
        (c(','), C, b"\x1b[27;5;44~"),
        (c('.'), C, b"\x1b[27;5;46~"),
        (c('\''), C, b"\x1b[27;5;39~"),
        (n(NamedKey::Enter), S, b"\x1b[27;2;13~"),
        (n(NamedKey::Enter), C, b"\x1b[27;5;13~"),
        (n(NamedKey::Enter), A, b"\x1b\r"),
        (n(NamedKey::Tab), S, b"\x1b[Z"),
        (n(NamedKey::Tab), C, b"\x1b[27;5;9~"),
        (n(NamedKey::Tab), C | S, b"\x1b[27;6;9~"),
        (n(NamedKey::Backspace), C, b"\x08"),
        (n(NamedKey::Escape), S, b"\x1b"),
        (n(NamedKey::Up), C, b"\x1b[1;5A"),
    ];
    for &(key, mods, expected) in rows {
        check("mok 1", &ev(key, mods), &m, Some(expected));
    }
}

/// Reports the number of rows this binary ran. Runs last when tests run one at a time.
#[test]
fn zz_report_row_count() {
    println!("I1 ROWS so far in this binary: {}", rows_run());
}
