//! A US keyboard as winit reports it (task-2589). The window tests press keys through this, so a
//! binding is pressed the way a person presses it and not the way the binding is written.
//!
//! `gui::input::raw_key` builds a `RawKey` from a winit `KeyEvent`. For a US layout winit gives:
//!
//! | Field | What winit gives | Example: Ctrl, Shift and `=` |
//! |---|---|---|
//! | `logical` | `key_without_modifiers`: the unshifted character on the key | `=` |
//! | `physical` | the US character at the key's position | `=` |
//! | `mods` | every modifier held, Shift included | Ctrl and Shift |
//! | `text` | `text_with_all_modifiers`: a control character for Ctrl and a letter on Windows and Linux, the character itself with Cmd on macOS, the typed character with no command modifier | none |
//! | `shifted` | `logical_key` when Shift is held: Shift applied, Ctrl and Cmd left out | `+` |
//!
//! The plus sign is therefore never a key of its own: it is Shift and `=`. A binding written as
//! `ctrl+plus` used to be pressed in the tests as the character `+`, which no US keyboard sends, and
//! that is how the font size keys stayed broken.

#![allow(dead_code)]

use termoil_app::config::{KeyChord, Platform};
use termoil_input::{Key, KeyEventKind, Modifiers, NamedKey, RawKey};

/// The unshifted key and Shift for a character a US keyboard types with Shift, or the character
/// itself when it has its own key.
pub fn us_base(c: char) -> (char, bool) {
    const SHIFTED: [(char, char); 21] = [
        ('!', '1'),
        ('@', '2'),
        ('#', '3'),
        ('$', '4'),
        ('%', '5'),
        ('^', '6'),
        ('&', '7'),
        ('*', '8'),
        ('(', '9'),
        (')', '0'),
        ('~', '`'),
        ('_', '-'),
        ('+', '='),
        ('{', '['),
        ('}', ']'),
        ('|', '\\'),
        (':', ';'),
        ('"', '\''),
        ('<', ','),
        ('>', '.'),
        ('?', '/'),
    ];
    if c.is_ascii_uppercase() {
        return (c.to_ascii_lowercase(), true);
    }
    SHIFTED.iter().find(|(s, _)| *s == c).map(|(_, b)| (*b, true)).unwrap_or((c, false))
}

/// The character a US keyboard types for a key with Shift held.
pub fn us_shifted(c: char) -> char {
    if c.is_ascii_lowercase() {
        return c.to_ascii_uppercase();
    }
    let pairs = "1!2@3#4$5%6^7&8*9(0)`~-_=+[{]}\\|;:'\",<.>/?";
    let chars: Vec<char> = pairs.chars().collect();
    chars.chunks(2).find(|p| p[0] == c).map(|p| p[1]).unwrap_or(c)
}

/// The named key of a binding key name, when it is one.
fn named(name: &str) -> Option<NamedKey> {
    Some(match name {
        "enter" => NamedKey::Enter,
        "escape" => NamedKey::Escape,
        "tab" => NamedKey::Tab,
        "backspace" => NamedKey::Backspace,
        "delete" => NamedKey::Delete,
        "insert" => NamedKey::Insert,
        "left" => NamedKey::Left,
        "right" => NamedKey::Right,
        "up" => NamedKey::Up,
        "down" => NamedKey::Down,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        f if f.starts_with('f') && f.len() > 1 => NamedKey::F(f[1..].parse().ok()?),
        _ => return None,
    })
}

/// The modifiers of a chord.
fn chord_mods(chord: &KeyChord) -> Modifiers {
    let mut m = Modifiers::NONE;
    for (on, bit) in [
        (chord.mods.shift, Modifiers::SHIFT),
        (chord.mods.alt, Modifiers::ALT),
        (chord.mods.ctrl, Modifiers::CTRL),
        (chord.mods.super_key, Modifiers::SUPER),
    ] {
        if on {
            m |= bit;
        }
    }
    m
}

/// What `text_with_all_modifiers` gives for a character key on `os`.
fn text_for(c: char, mods: Modifiers, os: Platform) -> Option<String> {
    let typed = if mods.contains(Modifiers::SHIFT) { us_shifted(c) } else { c };
    if mods.contains(Modifiers::SUPER) {
        return (os == Platform::MacOs).then(|| typed.to_string());
    }
    if mods.contains(Modifiers::CTRL) {
        let byte = c.is_ascii_lowercase().then(|| char::from(c as u8 - b'a' + 1));
        return byte.filter(|_| os != Platform::MacOs).map(String::from);
    }
    Some(typed.to_string())
}

/// The raw key a person makes on a US keyboard for a binding chord on `os`: `plus` becomes Shift
/// and `=`, `greater` becomes Shift and `.`, and a capital or a shifted symbol becomes Shift and its
/// key. None for a key the model does not know.
pub fn us_key(chord: &KeyChord, os: Platform) -> Option<RawKey> {
    let mut mods = chord_mods(chord);
    if let Some(n) = named(&chord.key) {
        return Some(RawKey::new(Key::Named(n), mods));
    }
    let written = match chord.key.as_str() {
        "plus" => '+',
        "greater" => '>',
        "space" => ' ',
        k => {
            let mut chars = k.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            c
        }
    };
    let (key, needs_shift) = us_base(written);
    if needs_shift {
        mods |= Modifiers::SHIFT;
    }
    let mut raw = RawKey::new(Key::Char(key), mods).with_physical(key);
    raw.text = text_for(key, mods, os);
    if mods.contains(Modifiers::SHIFT) {
        raw = raw.with_shifted(us_shifted(key));
    }
    Some(raw)
}

/// The same key let go: winit sends a release with the modifiers still held and no text.
pub fn release_of(raw: &RawKey) -> RawKey {
    let mut up = raw.clone().with_kind(KeyEventKind::Release);
    up.text = None;
    up
}

/// A key typed with no modifier, as the keyboard sends it: `+` is Shift and `=` with text `+`.
pub fn us_typed(c: char, os: Platform) -> RawKey {
    let (key, shift) = us_base(c);
    let mods = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
    let mut raw = RawKey::new(Key::Char(key), mods).with_physical(key);
    raw.text = text_for(key, mods, os);
    if shift {
        raw = raw.with_shifted(us_shifted(key));
    }
    raw
}
