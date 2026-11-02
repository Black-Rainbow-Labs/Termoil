//! Shared helpers for the table driven input tests. A "row" is one assertion of input to exact bytes.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use termoil_input::{
    Key, KeyEvent, KeyboardModes, Modifiers, MouseButton, MouseEvent, MouseEventKind, NamedKey, encode_key,
};

pub const N: Modifiers = Modifiers::NONE;
pub const S: Modifiers = Modifiers::SHIFT;
pub const A: Modifiers = Modifiers::ALT;
pub const C: Modifiers = Modifiers::CTRL;
pub const SU: Modifiers = Modifiers::SUPER;

static ROWS: AtomicUsize = AtomicUsize::new(0);

/// Counts one assertion row.
pub fn count_row() {
    ROWS.fetch_add(1, Ordering::SeqCst);
}

/// Returns how many rows have run so far in this test binary.
pub fn rows_run() -> usize {
    ROWS.load(Ordering::SeqCst)
}

/// A character key.
pub fn c(ch: char) -> Key {
    Key::Char(ch)
}

/// A named key.
pub fn n(key: NamedKey) -> Key {
    Key::Named(key)
}

/// Returns the US layout shifted form of an unshifted key.
fn shifted_of(ch: char) -> char {
    match ch {
        'a'..='z' => ch.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '`' => '~',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        other => other,
    }
}

/// Builds a press event the way the application does: text is the typed character unless a command modifier is held.
pub fn ev(key: Key, mods: Modifiers) -> KeyEvent {
    let mut event = KeyEvent::press(key, mods);
    if let Key::Char(ch) = key {
        let command = mods.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER);
        if !command {
            let typed = if mods.contains(Modifiers::SHIFT) { shifted_of(ch) } else { ch };
            event.text = Some(typed.to_string());
        }
        if mods.contains(Modifiers::SHIFT) && shifted_of(ch) != ch {
            event.shifted_key = Some(shifted_of(ch));
        }
    }
    event
}

/// The default terminal modes.
pub fn modes() -> KeyboardModes {
    KeyboardModes::default()
}

/// Modes with the kitty flags set.
pub fn kitty(flags: u8) -> KeyboardModes {
    KeyboardModes { kitty_flags: flags, ..KeyboardModes::default() }
}

/// Modes with modifyOtherKeys set.
pub fn mok(level: u8) -> KeyboardModes {
    KeyboardModes { modify_other_keys: level, ..KeyboardModes::default() }
}

/// Encodes an event and returns the bytes, or None when the encoder reports nothing sent.
pub fn encode(event: &KeyEvent, modes: &KeyboardModes) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let sent = encode_key(event, modes, &mut out);
    assert!(sent || out.is_empty(), "false returned but bytes were written: {}", out.escape_ascii());
    sent.then_some(out)
}

/// Asserts one row: the event under the modes encodes to exactly the expected bytes (None for nothing).
pub fn check(label: &str, event: &KeyEvent, modes: &KeyboardModes, expected: Option<&[u8]>) {
    count_row();
    let got = encode(event, modes);
    let show = |b: &Option<Vec<u8>>| b.as_ref().map(|v| v.escape_ascii().to_string());
    assert_eq!(
        show(&got),
        expected.map(|e| e.escape_ascii().to_string()),
        "{label}: key {:?} mods {:?} kind {:?} text {:?} modes {:?}",
        event.key,
        event.mods,
        event.kind,
        event.text,
        modes
    );
}

/// Builds a mouse event whose pixel position follows the cell, 8 by 16 pixels per cell.
pub fn me(kind: MouseEventKind, button: MouseButton, col: u32, row: u32, mods: Modifiers) -> MouseEvent {
    MouseEvent { kind, button, col, row, pixel_x: col * 8 + 3, pixel_y: row * 16 + 5, mods }
}
