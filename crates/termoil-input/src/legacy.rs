//! The legacy xterm key encoding, including cursor and keypad application modes and modifyOtherKeys.

use crate::key::{Key, KeyEvent, KeyEventKind, KeyboardModes, KeypadKey, NamedKey};
use crate::keycodes::{Form, kitty_form};
use crate::modifiers::Modifiers;
use crate::seq::{ESC, has_control, push_char, push_csi, push_csi_u, push_num, unshifted, us_shifted};

/// Encodes a key event with the legacy encoding. Returns false when the key sends nothing.
pub(crate) fn encode(event: &KeyEvent, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    if event.kind == KeyEventKind::Release {
        return false;
    }
    let mods = event.mods.without(Modifiers::LOCKS);
    match event.key {
        Key::Char(c) => encode_char(event, c, mods, modes, out),
        Key::Named(k) => encode_named(event, k, mods, modes, out),
    }
}

/// Returns the C0 control byte the ctrl key turns an ASCII key into, or None when ctrl leaves the key alone.
pub(crate) fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c {
        ' ' | '@' | '2' => 0,
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '[' | '3' => 27,
        '\\' | '4' => 28,
        ']' | '5' => 29,
        '^' | '6' | '~' => 30,
        '_' | '7' | '/' => 31,
        '8' | '?' => 127,
        _ => return None,
    })
}

/// Appends the bytes with an ESC prefix (or the eighth bit set) when alt is held.
fn push_with_alt(out: &mut Vec<u8>, bytes: &[u8], alt: bool, modes: &KeyboardModes) {
    if !alt {
        out.extend_from_slice(bytes);
    } else if modes.alt_sends_escape {
        out.push(ESC);
        out.extend_from_slice(bytes);
    } else if bytes.len() == 1 && bytes[0].is_ascii() {
        push_char(out, char::from(bytes[0] | 0x80));
    } else {
        out.extend_from_slice(bytes);
    }
}

/// Returns the code point of the key for the CSI u and modifyOtherKeys forms.
fn key_code(c: char) -> u32 {
    unshifted(c) as u32
}

/// Writes the event text, used for pure text events.
fn push_text(event: &KeyEvent, out: &mut Vec<u8>) -> bool {
    match event.text.as_ref().filter(|t| !t.is_empty()) {
        Some(t) => {
            out.extend_from_slice(t.as_bytes());
            true
        }
        None => false,
    }
}

/// Writes `CSI 27 ; mods ; code ~`, the xterm modifyOtherKeys form.
fn push_other_keys(out: &mut Vec<u8>, wire_mods: u32, code: u32) {
    push_csi(out);
    out.extend_from_slice(b"27;");
    push_num(out, wire_mods);
    out.push(b';');
    push_num(out, code);
    out.push(b'~');
}

/// Decides whether modifyOtherKeys sends a character key as `CSI 27;m;code~` and returns the code and modifier value.
fn other_keys_for_char(event: &KeyEvent, c: char, mods: Modifiers, modes: &KeyboardModes) -> Option<(u32, u32)> {
    let level = modes.modify_other_keys;
    let held = mods.intersects(Modifiers::ALT | Modifiers::CTRL | Modifiers::EXTENDED);
    if level == 0 || !held {
        return None;
    }
    let shift = mods.contains(Modifiers::SHIFT);
    let ctrl = mods.contains(Modifiers::CTRL);
    let distinct = c.is_ascii() && ctrl_byte(c).is_some() && !shift;
    let send = level >= 2 || mods.intersects(Modifiers::EXTENDED) || (ctrl && !distinct);
    if !send {
        return None;
    }
    let ch = if shift { event.shifted_key.unwrap_or_else(|| us_shifted(c)) } else { c };
    Some((ch as u32, mods.wire_value()))
}

/// Encodes a character key following the kitty legacy text key algorithm.
fn encode_char(event: &KeyEvent, c: char, mods: Modifiers, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    if c == '\0' {
        return push_text(event, out);
    }
    if let Some((code, m)) = other_keys_for_char(event, c, mods, modes) {
        push_other_keys(out, m, code);
        return true;
    }
    let (shift, ctrl, alt) =
        (mods.contains(Modifiers::SHIFT), mods.contains(Modifiers::CTRL), mods.contains(Modifiers::ALT));
    // Ctrl+Shift+letter is Ctrl+letter, as in xterm: shift does not change a control byte. Other
    // keys with Ctrl and Shift have no control byte of their own and keep the CSI u form.
    let letter = c.is_ascii_alphabetic();
    let needs_csi_u =
        mods.intersects(Modifiers::EXTENDED) || (ctrl && shift && c != ' ' && !letter) || (ctrl && !c.is_ascii());
    if needs_csi_u {
        push_csi_u(out, key_code(c), mods.wire_value());
        return true;
    }
    if ctrl {
        let byte = ctrl_byte(c).unwrap_or(c as u8);
        push_with_alt(out, &[byte], alt, modes);
        return true;
    }
    let text = typed_text(event, c, shift, alt);
    push_with_alt(out, text.as_bytes(), alt, modes);
    true
}

/// Returns the text a printable key sends. With alt held the OS text is ignored because alt changes it on some platforms.
fn typed_text(event: &KeyEvent, c: char, shift: bool, alt: bool) -> String {
    if !alt && let Some(t) = event.text.as_ref().filter(|t| !t.is_empty() && !has_control(t)) {
        return t.clone();
    }
    let ch = if shift { event.shifted_key.unwrap_or_else(|| us_shifted(c)) } else { c };
    ch.to_string()
}

/// Encodes a named key with the legacy encoding.
fn encode_named(event: &KeyEvent, key: NamedKey, mods: Modifiers, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    use NamedKey::*;
    match key {
        Escape | Enter | Tab | Backspace => encode_c0_key(key, mods, modes, out),
        Kp(kp) => encode_keypad(event, kp, mods, modes, out),
        Insert => push_tilde(out, 2, mods),
        Delete => push_tilde(out, 3, mods),
        PageUp => push_tilde(out, 5, mods),
        PageDown => push_tilde(out, 6, mods),
        Menu => push_tilde(out, 29, mods),
        Up => push_cursor(out, b'A', mods, modes),
        Down => push_cursor(out, b'B', mods, modes),
        Right => push_cursor(out, b'C', mods, modes),
        Left => push_cursor(out, b'D', mods, modes),
        Home => push_cursor(out, b'H', mods, modes),
        End => push_cursor(out, b'F', mods, modes),
        F(n) => encode_function(n, mods, out),
        _ => false,
    }
}

/// Writes `CSI n ~` or `CSI n ; mods ~`.
fn push_tilde(out: &mut Vec<u8>, n: u32, mods: Modifiers) -> bool {
    push_csi(out);
    push_num(out, n);
    if mods.wire_value() > 1 {
        out.push(b';');
        push_num(out, mods.wire_value());
    }
    out.push(b'~');
    true
}

/// Writes a cursor key: `CSI letter`, `SS3 letter` in application cursor mode, or `CSI 1 ; mods letter`.
fn push_cursor(out: &mut Vec<u8>, letter: u8, mods: Modifiers, modes: &KeyboardModes) -> bool {
    if mods.wire_value() > 1 {
        push_csi(out);
        out.extend_from_slice(b"1;");
        push_num(out, mods.wire_value());
    } else if modes.app_cursor {
        out.extend_from_slice(&[ESC, b'O']);
    } else {
        push_csi(out);
    }
    out.push(letter);
    true
}

/// Encodes F1 to F35: SS3 for F1 to F4, tilde forms for F5 to F20, CSI u beyond that.
fn encode_function(n: u8, mods: Modifiers, out: &mut Vec<u8>) -> bool {
    const TILDE: [u32; 16] = [15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34];
    match n {
        1..=4 => {
            let letter = b'P' + (n - 1);
            if mods.wire_value() > 1 {
                return push_cursor(out, letter, mods, &KeyboardModes::default());
            }
            out.extend_from_slice(&[ESC, b'O', letter]);
            true
        }
        5..=20 => push_tilde(out, TILDE[usize::from(n - 5)], mods),
        21..=35 => match kitty_form(NamedKey::F(n)) {
            Some(Form::Unicode(code)) => {
                push_csi_u(out, code, mods.wire_value());
                true
            }
            _ => false,
        },
        _ => false,
    }
}

/// Returns the modifyOtherKeys modifier value for Escape, Enter, Tab and Backspace, or None when the legacy bytes are used.
fn other_keys_for_c0(key: NamedKey, mods: Modifiers, level: u8) -> Option<u32> {
    let (shift, ctrl) = (mods.contains(Modifiers::SHIFT), mods.contains(Modifiers::CTRL));
    let any = mods.intersects(Modifiers::SHIFT | Modifiers::ALT | Modifiers::CTRL);
    let send = match level {
        0 => false,
        1 => (key == NamedKey::Enter && (shift || ctrl)) || (key == NamedKey::Tab && ctrl),
        _ => any && !(key == NamedKey::Tab && mods == Modifiers::SHIFT),
    };
    send.then(|| mods.wire_value())
}

/// Encodes Escape, Enter, Tab and Backspace following the kitty legacy C0 table.
fn encode_c0_key(key: NamedKey, mods: Modifiers, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    let code = match key {
        NamedKey::Escape => 27,
        NamedKey::Enter => 13,
        NamedKey::Tab => 9,
        _ => 127,
    };
    if mods.intersects(Modifiers::EXTENDED) {
        push_csi_u(out, code, mods.wire_value());
        return true;
    }
    if let Some(m) = other_keys_for_c0(key, mods, modes.modify_other_keys) {
        push_other_keys(out, m, code);
        return true;
    }
    let (shift, ctrl, alt) =
        (mods.contains(Modifiers::SHIFT), mods.contains(Modifiers::CTRL), mods.contains(Modifiers::ALT));
    if alt {
        out.push(ESC);
    }
    if key == NamedKey::Tab && shift {
        out.extend_from_slice(b"\x1b[Z");
        return true;
    }
    let backspace = if modes.backarrow_is_bs != ctrl { 0x08 } else { 0x7f };
    out.push(match key {
        NamedKey::Escape => 0x1b,
        NamedKey::Enter => 0x0d,
        NamedKey::Tab => 0x09,
        _ => backspace,
    });
    true
}

/// Returns the character a keypad key types when the keypad is not in application mode.
fn keypad_char(kp: KeypadKey) -> Option<char> {
    use KeypadKey::*;
    Some(match kp {
        Digit(d) if d <= 9 => char::from(b'0' + d),
        Decimal => '.',
        Divide => '/',
        Multiply => '*',
        Subtract => '-',
        Add => '+',
        Equal => '=',
        Separator => ',',
        _ => return None,
    })
}

/// Returns the final byte of the SS3 sequence a keypad key sends in application keypad mode.
fn keypad_ss3_final(kp: KeypadKey) -> Option<u8> {
    use KeypadKey::*;
    Some(match kp {
        Digit(d) if d <= 9 => b'p' + d,
        Multiply => b'j',
        Add => b'k',
        Separator => b'l',
        Subtract => b'm',
        Decimal => b'n',
        Divide => b'o',
        Enter => b'M',
        Equal => b'X',
        _ => return None,
    })
}

/// Returns the plain named key a keypad navigation key stands for.
fn keypad_nav_equivalent(kp: KeypadKey) -> Option<NamedKey> {
    use KeypadKey as K;
    Some(match kp {
        K::Left => NamedKey::Left,
        K::Right => NamedKey::Right,
        K::Up => NamedKey::Up,
        K::Down => NamedKey::Down,
        K::PageUp => NamedKey::PageUp,
        K::PageDown => NamedKey::PageDown,
        K::Home => NamedKey::Home,
        K::End => NamedKey::End,
        K::Insert => NamedKey::Insert,
        K::Delete => NamedKey::Delete,
        _ => return None,
    })
}

/// Encodes a keypad key: navigation keys act like the main block keys, the rest follow DECKPAM.
fn encode_keypad(event: &KeyEvent, kp: KeypadKey, mods: Modifiers, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    if let Some(named) = keypad_nav_equivalent(kp) {
        return encode_named(event, named, mods, modes, out);
    }
    if kp == KeypadKey::Begin {
        return push_cursor(out, b'E', mods, modes);
    }
    if modes.app_keypad {
        let Some(final_byte) = keypad_ss3_final(kp) else { return false };
        out.extend_from_slice(&[ESC, b'O']);
        if mods.wire_value() > 1 {
            push_num(out, mods.wire_value());
        }
        out.push(final_byte);
        return true;
    }
    if kp == KeypadKey::Enter {
        return encode_c0_key(NamedKey::Enter, mods, modes, out);
    }
    let Some(ch) = keypad_char(kp) else { return false };
    let plain = KeyEvent::press(Key::Char(ch), mods);
    encode_char(&plain, ch, mods, modes, out)
}
