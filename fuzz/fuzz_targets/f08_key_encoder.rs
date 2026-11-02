//! F8: the key encoder over every mode combination a few bytes can select.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_input::{Key, KeyEvent, KeyEventKind, KeyboardModes, Modifiers, NamedKey, encode_key};

fuzz_target!(|data: &[u8]| {
    if data.len() < 6 {
        return;
    }
    let named = [NamedKey::Escape, NamedKey::Enter, NamedKey::Tab, NamedKey::Backspace, NamedKey::Insert, NamedKey::Delete, NamedKey::Left, NamedKey::Right, NamedKey::Up, NamedKey::Down, NamedKey::PageUp, NamedKey::PageDown, NamedKey::F(1 + data[1] % 35)];
    let key = if data[0] & 1 == 0 { Key::Char(char::from_u32(u32::from_le_bytes([data[1], data[2], 0, 0])).unwrap_or('a')) } else { Key::Named(named[data[1] as usize % named.len()]) };
    let kind = match data[3] % 3 {
        0 => KeyEventKind::Press,
        1 => KeyEventKind::Repeat,
        _ => KeyEventKind::Release,
    };
    let text = (data[4] & 1 == 1).then(|| String::from_utf8_lossy(&data[6..]).into_owned());
    let event = KeyEvent { key, mods: Modifiers(data[5]), kind, text, shifted_key: None, base_layout_key: None };
    let modes = KeyboardModes { app_cursor: data[4] & 2 != 0, app_keypad: data[4] & 4 != 0, modify_other_keys: data[4] >> 6, kitty_flags: data[3] >> 3, backarrow_is_bs: data[4] & 8 != 0, alt_sends_escape: data[4] & 16 != 0 };
    let mut out = Vec::new();
    let _ = encode_key(&event, &modes, &mut out);
});
