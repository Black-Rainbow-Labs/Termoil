//! Translation of a hotkey chord (`config::KeyChord`) into what each platform's global hotkey API
//! asks for: macOS virtual key codes and Carbon modifier flags, X11 keysyms and modifier masks, and
//! the trigger text of the XDG GlobalShortcuts portal. These are plain functions of the chord, so
//! they are compiled and tested on every platform even though each is used on one.
#![allow(dead_code)]

use crate::config::{KeyChord, Mods};

/// Carbon modifier flag cmdKey.
pub const MAC_CMD: u32 = 1 << 8;
/// Carbon modifier flag shiftKey.
pub const MAC_SHIFT: u32 = 1 << 9;
/// Carbon modifier flag optionKey.
pub const MAC_OPTION: u32 = 1 << 11;
/// Carbon modifier flag controlKey.
pub const MAC_CONTROL: u32 = 1 << 12;

/// X11 modifier mask bit Shift.
pub const X_SHIFT: u16 = 1 << 0;
/// X11 modifier mask bit Control.
pub const X_CONTROL: u16 = 1 << 2;
/// X11 modifier mask bit Mod1, which is Alt.
pub const X_ALT: u16 = 1 << 3;
/// X11 modifier mask bit Mod4, which is the Super key.
pub const X_SUPER: u16 = 1 << 6;

/// The single character a key name is, when it is one.
fn single_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// The number in a function key name such as `f12`, when it is within 1 to `max`.
fn function_number(key: &str, max: u32) -> Option<u32> {
    key.strip_prefix('f')?.parse().ok().filter(|n| (1..=max).contains(n))
}

/// The macOS virtual key code (the kVK_ANSI_* and kVK_* values) of a chord's key.
pub fn mac_key_code(key: &str) -> Option<u32> {
    if let Some(c) = single_char(key) {
        return mac_character_code(c);
    }
    match key {
        "space" => Some(49),
        "enter" => Some(36),
        "tab" => Some(48),
        "escape" => Some(53),
        "plus" => Some(24),
        k => mac_function_code(k),
    }
}

/// The virtual key code of a single character key on an ANSI keyboard.
fn mac_character_code(c: char) -> Option<u32> {
    const LETTERS: [u32; 26] =
        [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6];
    const DIGITS: [u32; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    match c {
        'a'..='z' => Some(LETTERS[(c as u8 - b'a') as usize]),
        '0'..='9' => Some(DIGITS[(c as u8 - b'0') as usize]),
        '`' => Some(50),
        '-' => Some(27),
        '=' | '+' => Some(24),
        '[' => Some(33),
        ']' => Some(30),
        '\\' => Some(42),
        ';' => Some(41),
        '\'' => Some(39),
        ',' => Some(43),
        '.' => Some(47),
        '/' => Some(44),
        _ => None,
    }
}

/// The virtual key code of F1 to F20.
fn mac_function_code(key: &str) -> Option<u32> {
    const F: [u32; 20] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90];
    function_number(key, 20).map(|n| F[n as usize - 1])
}

/// The Carbon modifier flags of a chord's modifiers.
pub fn mac_modifiers(m: Mods) -> u32 {
    let mut flags = 0;
    for (on, flag) in [(m.ctrl, MAC_CONTROL), (m.alt, MAC_OPTION), (m.shift, MAC_SHIFT), (m.super_key, MAC_CMD)] {
        if on {
            flags |= flag;
        }
    }
    flags
}

/// The X11 keysym of a chord's key. Printable ASCII keysyms equal the character code.
pub fn x11_keysym(key: &str) -> Option<u32> {
    if let Some(c) = single_char(key) {
        return c.is_ascii_graphic().then_some(c as u32);
    }
    match key {
        "space" => Some(0x20),
        "plus" => Some(0x2b),
        "enter" => Some(0xff0d),
        "tab" => Some(0xff09),
        "escape" => Some(0xff1b),
        k => function_number(k, 24).map(|n| 0xffbd + n),
    }
}

/// The X11 modifier mask of a chord's modifiers.
pub fn x11_modifiers(m: Mods) -> u16 {
    let mut mask = 0;
    for (on, bit) in [(m.ctrl, X_CONTROL), (m.alt, X_ALT), (m.shift, X_SHIFT), (m.super_key, X_SUPER)] {
        if on {
            mask |= bit;
        }
    }
    mask
}

/// The first keycode whose keysym row holds `keysym`, given the server's keyboard mapping (`table`
/// has `per` keysyms for each keycode starting at `min_keycode`). The uppercase form of a letter
/// counts too, since a chord names the key and not its shifted symbol.
pub fn keycode_for_keysym(keysym: u32, min_keycode: u8, per: usize, table: &[u32]) -> Option<u8> {
    if per == 0 {
        return None;
    }
    let upper = if (0x61..=0x7a).contains(&keysym) { keysym - 0x20 } else { keysym };
    let index = table.chunks(per).position(|row| row.iter().any(|&s| s == keysym || s == upper))?;
    u8::try_from(min_keycode as usize + index).ok()
}

/// The XKB keysym name the portal's trigger text uses for a chord's key.
fn portal_key_name(key: &str) -> Option<String> {
    if let Some(c) = single_char(key) {
        let name = match c {
            'a'..='z' | '0'..='9' => c.to_string(),
            '`' => "grave".into(),
            '-' => "minus".into(),
            '=' => "equal".into(),
            '+' => "plus".into(),
            '[' => "bracketleft".into(),
            ']' => "bracketright".into(),
            '\\' => "backslash".into(),
            ';' => "semicolon".into(),
            '\'' => "apostrophe".into(),
            ',' => "comma".into(),
            '.' => "period".into(),
            '/' => "slash".into(),
            _ => return None,
        };
        return Some(name);
    }
    match key {
        "space" | "plus" => Some(key.into()),
        "enter" => Some("Return".into()),
        "tab" => Some("Tab".into()),
        "escape" => Some("Escape".into()),
        k => function_number(k, 24).map(|n| format!("F{n}")),
    }
}

/// The preferred trigger text of the XDG GlobalShortcuts portal, for example `CTRL+grave`.
pub fn portal_trigger(chord: &KeyChord) -> Option<String> {
    let m = chord.mods;
    let mut parts: Vec<String> = Vec::new();
    for (on, name) in [(m.ctrl, "CTRL"), (m.alt, "ALT"), (m.shift, "SHIFT"), (m.super_key, "LOGO")] {
        if on {
            parts.push(name.into());
        }
    }
    parts.push(portal_key_name(&chord.key)?);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> KeyChord {
        crate::keymap::parse_chord(text).unwrap()
    }

    #[test]
    fn mac_codes_match_the_ansi_layout() {
        assert_eq!(mac_key_code("a"), Some(0));
        assert_eq!(mac_key_code("z"), Some(6));
        assert_eq!(mac_key_code("`"), Some(50));
        assert_eq!(mac_key_code("space"), Some(49));
        assert_eq!(mac_key_code("1"), Some(18));
        assert_eq!(mac_key_code("0"), Some(29));
        assert_eq!(mac_key_code("f1"), Some(122));
        assert_eq!(mac_key_code("f12"), Some(111));
        assert_eq!(mac_key_code("f21"), None);
        assert_eq!(mac_key_code("pageup"), None);
    }

    #[test]
    fn mac_modifiers_use_carbon_flags() {
        let c = chord("ctrl+alt+shift+super+x");
        assert_eq!(mac_modifiers(c.mods), 256 | 512 | 2048 | 4096);
        assert_eq!(mac_modifiers(chord("ctrl+`").mods), 4096);
    }

    #[test]
    fn x11_keysyms_and_masks() {
        assert_eq!(x11_keysym("`"), Some(0x60));
        assert_eq!(x11_keysym("a"), Some(0x61));
        assert_eq!(x11_keysym("f1"), Some(0xffbe));
        assert_eq!(x11_keysym("f12"), Some(0xffc9));
        assert_eq!(x11_keysym("enter"), Some(0xff0d));
        assert_eq!(x11_keysym("home"), None);
        assert_eq!(x11_modifiers(chord("ctrl+alt+shift+super+x").mods), 1 | 4 | 8 | 64);
    }

    #[test]
    fn keycode_lookup_reads_the_server_table() {
        // Keycodes 8 to 10 with two keysyms each: a and A, grave and asciitilde, b and B.
        let table = [0x61, 0x41, 0x60, 0x7e, 0x62, 0x42];
        assert_eq!(keycode_for_keysym(0x60, 8, 2, &table), Some(9));
        assert_eq!(keycode_for_keysym(0x62, 8, 2, &table), Some(10));
        assert_eq!(keycode_for_keysym(0x7a, 8, 2, &table), None);
        assert_eq!(keycode_for_keysym(0x61, 8, 0, &table), None);
    }

    #[test]
    fn portal_trigger_names_the_chord() {
        assert_eq!(portal_trigger(&chord("ctrl+`")).as_deref(), Some("CTRL+grave"));
        assert_eq!(portal_trigger(&chord("ctrl+shift+super+f9")).as_deref(), Some("CTRL+SHIFT+LOGO+F9"));
        assert_eq!(portal_trigger(&chord("alt+space")).as_deref(), Some("ALT+space"));
    }
}
