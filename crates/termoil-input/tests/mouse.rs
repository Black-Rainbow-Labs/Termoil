//! I3, part one: mouse reports in every tracking mode and encoding, focus reports and paste wrapping.
//! The grid rows in tests/data are generated from the xterm control sequence rules by a script that does not use this crate.
#![allow(clippy::type_complexity)]

mod common;

use MouseButton as B;
use MouseEncoding as E;
use MouseEventKind as K;
use MouseTracking as T;
use common::*;
use termoil_input::{
    Modifiers, MouseButton, MouseEncoding, MouseEvent, MouseEventKind, MouseModes, MouseTracking, encode_focus,
    encode_mouse, encode_paste,
};

include!("data/mouse_grid.rs");

/// Asserts one mouse row.
fn check_mouse(
    label: &str, event: &MouseEvent, tracking: MouseTracking, encoding: MouseEncoding, last: Option<(u32, u32)>,
    expected: Option<&[u8]>,
) {
    count_row();
    let mut out = Vec::new();
    let sent = encode_mouse(event, &MouseModes { tracking, encoding }, last, &mut out);
    assert!(sent || out.is_empty(), "{label}: false returned but bytes were written");
    let got = sent.then(|| out.escape_ascii().to_string());
    let want = expected.map(|e| e.escape_ascii().to_string());
    assert_eq!(got, want, "{label}: {event:?} {tracking:?} {encoding:?} last {last:?}");
}

/// Five tracking modes by five encodings by fifteen events, including the coordinate limits.
#[test]
fn mouse_grid_over_every_tracking_mode_and_encoding() {
    for (tracking, encoding, event, last, expected) in mouse_grid_rows() {
        check_mouse("grid", &event, tracking, encoding, last, expected);
    }
}

/// Every button in the SGR encoding: presses use the button number and releases use the final byte m.
#[test]
fn sgr_button_numbers() {
    let rows: &[(MouseButton, &[u8], Option<&[u8]>)] = &[
        (B::Left, b"\x1b[<0;1;1M", Some(b"\x1b[<0;1;1m")),
        (B::Middle, b"\x1b[<1;1;1M", Some(b"\x1b[<1;1;1m")),
        (B::Right, b"\x1b[<2;1;1M", Some(b"\x1b[<2;1;1m")),
        (B::WheelUp, b"\x1b[<64;1;1M", None),
        (B::WheelDown, b"\x1b[<65;1;1M", None),
        (B::WheelLeft, b"\x1b[<66;1;1M", None),
        (B::WheelRight, b"\x1b[<67;1;1M", None),
        (B::Back, b"\x1b[<128;1;1M", Some(b"\x1b[<128;1;1m")),
        (B::Forward, b"\x1b[<129;1;1M", Some(b"\x1b[<129;1;1m")),
        (B::Button10, b"\x1b[<130;1;1M", Some(b"\x1b[<130;1;1m")),
        (B::Button11, b"\x1b[<131;1;1M", Some(b"\x1b[<131;1;1m")),
    ];
    for &(button, press, release) in rows {
        check_mouse("sgr press", &me(K::Press, button, 0, 0, N), T::Normal, E::Sgr, None, Some(press));
        check_mouse("sgr release", &me(K::Release, button, 0, 0, N), T::Normal, E::Sgr, None, release);
    }
}

/// Every button in the default encoding: releases always report button 3.
#[test]
fn default_encoding_button_bytes() {
    let rows: &[(MouseButton, &[u8], Option<&[u8]>)] = &[
        (B::Left, b"\x1b[M !!", Some(b"\x1b[M#!!")),
        (B::Middle, b"\x1b[M!!!", Some(b"\x1b[M#!!")),
        (B::Right, b"\x1b[M\"!!", Some(b"\x1b[M#!!")),
        (B::WheelUp, b"\x1b[M`!!", None),
        (B::WheelDown, b"\x1b[Ma!!", None),
        (B::WheelLeft, b"\x1b[Mb!!", None),
        (B::WheelRight, b"\x1b[Mc!!", None),
        (B::Back, b"\x1b[M\xa0!!", Some(b"\x1b[M#!!")),
        (B::Forward, b"\x1b[M\xa1!!", Some(b"\x1b[M#!!")),
        (B::Button10, b"\x1b[M\xa2!!", Some(b"\x1b[M#!!")),
        (B::Button11, b"\x1b[M\xa3!!", Some(b"\x1b[M#!!")),
    ];
    for &(button, press, release) in rows {
        check_mouse("default press", &me(K::Press, button, 0, 0, N), T::Normal, E::Default, None, Some(press));
        check_mouse("default release", &me(K::Release, button, 0, 0, N), T::Normal, E::Default, None, release);
    }
}

/// Modifier bits: shift 4, alt 8, ctrl 16. Super, hyper and meta are not reported.
#[test]
fn mouse_modifiers() {
    let rows: &[(Modifiers, &[u8], &[u8], &[u8])] = &[
        (N, b"\x1b[<0;1;1M", b"\x1b[M !!", b"\x1b[32;1;1M"),
        (S, b"\x1b[<4;1;1M", b"\x1b[M$!!", b"\x1b[36;1;1M"),
        (A, b"\x1b[<8;1;1M", b"\x1b[M(!!", b"\x1b[40;1;1M"),
        (C, b"\x1b[<16;1;1M", b"\x1b[M0!!", b"\x1b[48;1;1M"),
        (S | A, b"\x1b[<12;1;1M", b"\x1b[M,!!", b"\x1b[44;1;1M"),
        (S | C, b"\x1b[<20;1;1M", b"\x1b[M4!!", b"\x1b[52;1;1M"),
        (A | C, b"\x1b[<24;1;1M", b"\x1b[M8!!", b"\x1b[56;1;1M"),
        (S | A | C, b"\x1b[<28;1;1M", b"\x1b[M<!!", b"\x1b[60;1;1M"),
        (SU, b"\x1b[<0;1;1M", b"\x1b[M !!", b"\x1b[32;1;1M"),
        (SU | C, b"\x1b[<16;1;1M", b"\x1b[M0!!", b"\x1b[48;1;1M"),
        (Modifiers::CAPS_LOCK | Modifiers::NUM_LOCK, b"\x1b[<0;1;1M", b"\x1b[M !!", b"\x1b[32;1;1M"),
    ];
    for &(mods, sgr, default, urxvt) in rows {
        let event = me(K::Press, B::Left, 0, 0, mods);
        check_mouse("mods sgr", &event, T::Normal, E::Sgr, None, Some(sgr));
        check_mouse("mods default", &event, T::Normal, E::Default, None, Some(default));
        check_mouse("mods urxvt", &event, T::Normal, E::Urxvt, None, Some(urxvt));
    }
    check_mouse(
        "release with shift sgr",
        &me(K::Release, B::Left, 0, 0, S),
        T::Normal,
        E::Sgr,
        None,
        Some(b"\x1b[<4;1;1m"),
    );
    check_mouse(
        "release with ctrl default",
        &me(K::Release, B::Left, 0, 0, C),
        T::Normal,
        E::Default,
        None,
        Some(b"\x1b[M3!!"),
    );
    check_mouse(
        "wheel with ctrl sgr",
        &me(K::Press, B::WheelUp, 0, 0, C),
        T::Normal,
        E::Sgr,
        None,
        Some(b"\x1b[<80;1;1M"),
    );
}

/// X10 mode reports presses only and never modifiers.
#[test]
fn x10_mode() {
    let rows: &[(&str, MouseEvent, Option<&[u8]>)] = &[
        ("press", me(K::Press, B::Left, 0, 0, N), Some(b"\x1b[M !!")),
        ("press with ctrl", me(K::Press, B::Left, 0, 0, C | S), Some(b"\x1b[M !!")),
        ("right press", me(K::Press, B::Right, 4, 2, N), Some(b"\x1b[M\"%#")),
        ("release", me(K::Release, B::Left, 0, 0, N), None),
        ("motion", me(K::Motion, B::Left, 1, 1, N), None),
        ("motion without button", me(K::Motion, B::None, 1, 1, N), None),
        ("wheel up press", me(K::Press, B::WheelUp, 0, 0, N), Some(b"\x1b[M`!!")),
    ];
    for (label, event, expected) in rows {
        check_mouse(label, event, T::X10, E::Default, None, *expected);
    }
}

/// Motion reporting by mode, with and without a held button and with and without a cell change.
#[test]
fn motion_reporting() {
    let rows: &[(&str, MouseTracking, MouseEvent, Option<(u32, u32)>, Option<&[u8]>)] = &[
        (
            "any: no button, new cell",
            T::AnyEvent,
            me(K::Motion, B::None, 6, 7, N),
            Some((0, 0)),
            Some(b"\x1b[<35;7;8M"),
        ),
        ("any: no button, first report", T::AnyEvent, me(K::Motion, B::None, 6, 7, N), None, Some(b"\x1b[<35;7;8M")),
        ("any: no button, same cell", T::AnyEvent, me(K::Motion, B::None, 6, 7, N), Some((6, 7)), None),
        ("any: left held", T::AnyEvent, me(K::Motion, B::Left, 6, 7, N), Some((0, 0)), Some(b"\x1b[<32;7;8M")),
        (
            "any: right held with shift",
            T::AnyEvent,
            me(K::Motion, B::Right, 6, 7, S),
            Some((0, 0)),
            Some(b"\x1b[<38;7;8M"),
        ),
        (
            "any: middle held with ctrl and alt",
            T::AnyEvent,
            me(K::Motion, B::Middle, 6, 7, C | A),
            Some((0, 0)),
            Some(b"\x1b[<57;7;8M"),
        ),
        (
            "any: only the column changed",
            T::AnyEvent,
            me(K::Motion, B::Left, 6, 7, N),
            Some((5, 7)),
            Some(b"\x1b[<32;7;8M"),
        ),
        (
            "any: only the row changed",
            T::AnyEvent,
            me(K::Motion, B::Left, 6, 7, N),
            Some((6, 6)),
            Some(b"\x1b[<32;7;8M"),
        ),
        ("any: wheel motion", T::AnyEvent, me(K::Motion, B::WheelUp, 6, 7, N), None, None),
        (
            "button event: left held",
            T::ButtonEvent,
            me(K::Motion, B::Left, 6, 7, N),
            Some((0, 0)),
            Some(b"\x1b[<32;7;8M"),
        ),
        ("button event: no button", T::ButtonEvent, me(K::Motion, B::None, 6, 7, N), Some((0, 0)), None),
        ("button event: same cell", T::ButtonEvent, me(K::Motion, B::Left, 6, 7, N), Some((6, 7)), None),
        ("normal: motion is not reported", T::Normal, me(K::Motion, B::Left, 6, 7, N), Some((0, 0)), None),
        ("normal: motion without button", T::Normal, me(K::Motion, B::None, 6, 7, N), Some((0, 0)), None),
        ("off: nothing", T::Off, me(K::Press, B::Left, 0, 0, N), None, None),
        ("press ignores the last cell", T::Normal, me(K::Press, B::Left, 6, 7, N), Some((6, 7)), Some(b"\x1b[<0;7;8M")),
        (
            "release ignores the last cell",
            T::Normal,
            me(K::Release, B::Left, 6, 7, N),
            Some((6, 7)),
            Some(b"\x1b[<0;7;8m"),
        ),
        ("press without a button", T::Normal, me(K::Press, B::None, 0, 0, N), None, None),
        ("release without a button", T::Normal, me(K::Release, B::None, 0, 0, N), None, None),
        ("back button motion", T::AnyEvent, me(K::Motion, B::Back, 1, 1, N), Some((0, 0)), Some(b"\x1b[<160;2;2M")),
    ];
    for (label, tracking, event, last, expected) in rows {
        check_mouse(label, event, *tracking, E::Sgr, *last, *expected);
    }
}

/// Coordinate limits of the default and UTF-8 forms and the unlimited decimal forms.
#[test]
fn coordinate_limits() {
    let p = |col, row| me(K::Press, B::Left, col, row, N);
    let rows: &[(&str, MouseEncoding, MouseEvent, Option<&[u8]>)] = &[
        ("default: column 222 is the last", E::Default, p(222, 0), Some(b"\x1b[M \xff!")),
        ("default: column 223 does not fit", E::Default, p(223, 0), None),
        ("default: row 222 is the last", E::Default, p(0, 222), Some(b"\x1b[M !\xff")),
        ("default: row 223 does not fit", E::Default, p(0, 223), None),
        ("default: both beyond", E::Default, p(300, 300), None),
        ("default: column 93", E::Default, p(93, 0), Some(b"\x1b[M ~!")),
        ("default: column 94", E::Default, p(94, 0), Some(b"\x1b[M \x7f!")),
        ("default: column 95", E::Default, p(95, 0), Some(b"\x1b[M \x80!")),
        ("utf8: column 93 is one byte", E::Utf8, p(93, 0), Some(b"\x1b[M ~!")),
        ("utf8: column 94 is one byte", E::Utf8, p(94, 0), Some(b"\x1b[M \x7f!")),
        ("utf8: column 95 is two bytes", E::Utf8, p(95, 0), Some(b"\x1b[M \xc2\x80!")),
        ("utf8: column 222", E::Utf8, p(222, 0), Some(b"\x1b[M \xc3\xbf!")),
        ("utf8: column 223", E::Utf8, p(223, 0), Some(b"\x1b[M \xc4\x80!")),
        ("utf8: column 2014 is the last", E::Utf8, p(2014, 0), Some(b"\x1b[M \xdf\xbf!")),
        ("utf8: column 2015 does not fit", E::Utf8, p(2015, 0), None),
        ("utf8: row 2014 is the last", E::Utf8, p(0, 2014), Some(b"\x1b[M !\xdf\xbf")),
        ("utf8: row 2015 does not fit", E::Utf8, p(0, 2015), None),
        ("urxvt: large column", E::Urxvt, p(3000, 4000), Some(b"\x1b[32;3001;4001M")),
        ("sgr: large column", E::Sgr, p(3000, 4000), Some(b"\x1b[<0;3001;4001M")),
        ("sgr: release large column", E::Sgr, me(K::Release, B::Left, 3000, 4000, N), Some(b"\x1b[<0;3001;4001m")),
        ("sgr pixels: large column", E::SgrPixels, p(3000, 4000), Some(b"\x1b[<0;24004;64006M")),
    ];
    for (label, encoding, event, expected) in rows {
        check_mouse(label, event, T::Normal, *encoding, None, *expected);
    }
}

/// Pixel coordinates in mode 1016: motion inside one cell is still reported.
#[test]
fn sgr_pixels() {
    let mut moved = me(K::Motion, B::Left, 2, 3, N);
    moved.pixel_x = 20;
    moved.pixel_y = 50;
    check_mouse("pixel motion same cell", &moved, T::ButtonEvent, E::SgrPixels, Some((2, 3)), Some(b"\x1b[<32;21;51M"));
    check_mouse("pixel press", &me(K::Press, B::Left, 0, 0, N), T::Normal, E::SgrPixels, None, Some(b"\x1b[<0;4;6M"));
    check_mouse(
        "pixel release",
        &me(K::Release, B::Left, 0, 0, N),
        T::Normal,
        E::SgrPixels,
        None,
        Some(b"\x1b[<0;4;6m"),
    );
    check_mouse(
        "pixel wheel",
        &me(K::Press, B::WheelDown, 1, 1, N),
        T::Normal,
        E::SgrPixels,
        None,
        Some(b"\x1b[<65;12;22M"),
    );
    check_mouse("pixel motion without button", &moved, T::ButtonEvent, E::SgrPixels, None, Some(b"\x1b[<32;21;51M"));
    let mut none = moved;
    none.button = B::None;
    check_mouse("pixel motion no button any", &none, T::AnyEvent, E::SgrPixels, Some((2, 3)), Some(b"\x1b[<35;21;51M"));
    check_mouse("pixel motion no button button event", &none, T::ButtonEvent, E::SgrPixels, None, None);
}

/// Focus in and focus out reports.
#[test]
fn focus_reports() {
    for (focused, expected) in [(true, &b"\x1b[I"[..]), (false, &b"\x1b[O"[..])] {
        count_row();
        let mut out = Vec::new();
        encode_focus(focused, &mut out);
        assert_eq!(out, expected);
    }
    count_row();
    let mut out = b"x".to_vec();
    encode_focus(true, &mut out);
    assert_eq!(out, b"x\x1b[I", "focus appends to the output");
}

/// Bracketed paste wraps the text and removes marker sequences inside it. Unbracketed paste converts line endings.
#[test]
fn paste_encoding() {
    let rows: &[(&str, &str, bool, &[u8])] = &[
        ("bracketed plain", "hello", true, b"\x1b[200~hello\x1b[201~"),
        ("bracketed empty", "", true, b"\x1b[200~\x1b[201~"),
        ("bracketed keeps newlines", "a\nb", true, b"\x1b[200~a\nb\x1b[201~"),
        ("bracketed keeps crlf", "a\r\nb", true, b"\x1b[200~a\r\nb\x1b[201~"),
        ("bracketed keeps unicode", "h\u{e9}llo \u{1f600}", true, b"\x1b[200~h\xc3\xa9llo \xf0\x9f\x98\x80\x1b[201~"),
        ("bracketed removes the end marker", "a\x1b[201~b", true, b"\x1b[200~ab\x1b[201~"),
        ("bracketed removes the start marker", "a\x1b[200~b", true, b"\x1b[200~ab\x1b[201~"),
        ("bracketed removes both", "\x1b[200~a\x1b[201~", true, b"\x1b[200~a\x1b[201~"),
        ("bracketed removes repeated markers", "a\x1b[201~\x1b[201~b\x1b[201~", true, b"\x1b[200~ab\x1b[201~"),
        ("bracketed removes a rebuilt marker", "\x1b[2\x1b[201~01~", true, b"\x1b[200~\x1b[201~"),
        ("bracketed removes a rebuilt start marker", "\x1b[20\x1b[200~0~x", true, b"\x1b[200~x\x1b[201~"),
        ("bracketed keeps other escapes", "\x1b[31mred", true, b"\x1b[200~\x1b[31mred\x1b[201~"),
        ("bracketed marker only", "\x1b[201~", true, b"\x1b[200~\x1b[201~"),
        ("unbracketed plain", "hello", false, b"hello"),
        ("unbracketed lf", "a\nb", false, b"a\rb"),
        ("unbracketed crlf", "a\r\nb", false, b"a\rb"),
        ("unbracketed cr", "a\rb", false, b"a\rb"),
        ("unbracketed blank line", "a\n\nb", false, b"a\r\rb"),
        ("unbracketed trailing newline", "a\n", false, b"a\r"),
        ("unbracketed crlf twice", "\r\n\r\n", false, b"\r\r"),
        ("unbracketed empty", "", false, b""),
        ("unbracketed unicode", "\u{e9}\n", false, b"\xc3\xa9\r"),
        ("unbracketed leaves markers", "a\x1b[201~b", false, b"a\x1b[201~b"),
    ];
    for (label, text, bracketed, expected) in rows {
        count_row();
        let mut out = Vec::new();
        encode_paste(text, *bracketed, &mut out);
        assert_eq!(out.escape_ascii().to_string(), expected.escape_ascii().to_string(), "{label}");
    }
}

/// A paste can never contain the bytes that end the bracket early, however it is built.
#[test]
fn bracketed_paste_cannot_end_the_bracket() {
    let pieces = ["\x1b", "[", "2", "0", "1", "~", "a", "\x1b[201~", "\x1b[200~"];
    for a in pieces {
        for b in pieces {
            for d in pieces {
                count_row();
                let text = format!("{a}{b}{d}");
                let mut out = Vec::new();
                encode_paste(&text, true, &mut out);
                let inner = &out[6..out.len() - 6];
                assert!(
                    !inner.windows(6).any(|w| w == b"\x1b[201~" || w == b"\x1b[200~"),
                    "marker survived in {text:?}"
                );
            }
        }
    }
}

/// Reports the number of rows this binary ran. Runs last when tests run one at a time.
#[test]
fn zz_report_row_count() {
    println!("I3 mouse ROWS so far in this binary: {}", rows_run());
}
