//! winit key events to the platform independent `RawKey` the normaliser takes (`docs/TDD.md`
//! section 4.8): the logical key without modifiers, the US layout key at the same position, the
//! text, the modifiers and the keypad.

use terminus_input::{Key, KeyEventKind, KeypadKey, Modifiers, NamedKey, RawKey};
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key as WKey, KeyCode, KeyLocation, ModifiersState, NamedKey as WNamed, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

/// Converts winit modifiers.
pub fn modifiers(m: ModifiersState) -> Modifiers {
    let mut out = Modifiers::NONE;
    for (on, bit) in [
        (m.shift_key(), Modifiers::SHIFT),
        (m.alt_key(), Modifiers::ALT),
        (m.control_key(), Modifiers::CTRL),
        (m.super_key(), Modifiers::SUPER),
    ] {
        if on {
            out |= bit;
        }
    }
    out
}

/// Converts a winit key event, or None for a key Terminus has no name for.
pub fn raw_key(ev: &KeyEvent, mods: ModifiersState) -> Option<RawKey> {
    let kind = match (ev.state, ev.repeat) {
        (ElementState::Released, _) => KeyEventKind::Release,
        (ElementState::Pressed, true) => KeyEventKind::Repeat,
        (ElementState::Pressed, false) => KeyEventKind::Press,
    };
    let (logical, dead, numlock) = logical_key(ev)?;
    let mut raw = RawKey::new(logical, modifiers(mods));
    raw.kind = kind;
    raw.dead = dead;
    raw.numlock = numlock;
    raw.physical = physical_char(ev.physical_key);
    raw.altgr = mods.control_key()
        && mods.alt_key()
        && matches!(ev.text.as_deref(), Some(t) if !t.is_empty() && !t.chars().any(char::is_control));
    raw.text = ev.text_with_all_modifiers().map(str::to_string).filter(|t| !t.is_empty());
    Some(raw)
}

/// The logical key, the dead key accent, and whether num lock is on (for keypad keys).
fn logical_key(ev: &KeyEvent) -> Option<(Key, Option<char>, bool)> {
    if ev.location == KeyLocation::Numpad
        && let Some(kp) = keypad(ev)
    {
        let numlock = matches!(ev.logical_key, WKey::Character(_));
        return Some((Key::Named(NamedKey::Kp(kp)), None, numlock));
    }
    match ev.key_without_modifiers() {
        WKey::Character(s) => {
            let c = s.chars().next()?;
            Some((Key::Char(c.to_lowercase().next().unwrap_or(c)), None, false))
        }
        WKey::Dead(accent) => Some((Key::Char('\0'), Some(accent.unwrap_or('\u{b4}')), false)),
        WKey::Named(WNamed::Space) => Some((Key::Char(' '), None, false)),
        WKey::Named(n) => named(n).map(|k| (Key::Named(k), None, false)),
        _ => None,
    }
}

/// The keypad key at a numeric keypad position.
fn keypad(ev: &KeyEvent) -> Option<KeypadKey> {
    let PhysicalKey::Code(code) = ev.physical_key else { return None };
    Some(match code {
        KeyCode::Numpad0 => KeypadKey::Digit(0),
        KeyCode::Numpad1 => KeypadKey::Digit(1),
        KeyCode::Numpad2 => KeypadKey::Digit(2),
        KeyCode::Numpad3 => KeypadKey::Digit(3),
        KeyCode::Numpad4 => KeypadKey::Digit(4),
        KeyCode::Numpad5 => KeypadKey::Digit(5),
        KeyCode::Numpad6 => KeypadKey::Digit(6),
        KeyCode::Numpad7 => KeypadKey::Digit(7),
        KeyCode::Numpad8 => KeypadKey::Digit(8),
        KeyCode::Numpad9 => KeypadKey::Digit(9),
        KeyCode::NumpadDecimal => KeypadKey::Decimal,
        KeyCode::NumpadDivide => KeypadKey::Divide,
        KeyCode::NumpadMultiply => KeypadKey::Multiply,
        KeyCode::NumpadSubtract => KeypadKey::Subtract,
        KeyCode::NumpadAdd => KeypadKey::Add,
        KeyCode::NumpadEnter => KeypadKey::Enter,
        KeyCode::NumpadEqual => KeypadKey::Equal,
        KeyCode::NumpadComma => KeypadKey::Separator,
        _ => return None,
    })
}

/// Maps a winit named key.
fn named(n: WNamed) -> Option<NamedKey> {
    Some(match n {
        WNamed::Escape => NamedKey::Escape,
        WNamed::Enter => NamedKey::Enter,
        WNamed::Tab => NamedKey::Tab,
        WNamed::Backspace => NamedKey::Backspace,
        WNamed::Insert => NamedKey::Insert,
        WNamed::Delete => NamedKey::Delete,
        WNamed::ArrowLeft => NamedKey::Left,
        WNamed::ArrowRight => NamedKey::Right,
        WNamed::ArrowUp => NamedKey::Up,
        WNamed::ArrowDown => NamedKey::Down,
        WNamed::PageUp => NamedKey::PageUp,
        WNamed::PageDown => NamedKey::PageDown,
        WNamed::Home => NamedKey::Home,
        WNamed::End => NamedKey::End,
        WNamed::CapsLock => NamedKey::CapsLock,
        WNamed::ScrollLock => NamedKey::ScrollLock,
        WNamed::NumLock => NamedKey::NumLock,
        WNamed::PrintScreen => NamedKey::PrintScreen,
        WNamed::Pause => NamedKey::Pause,
        WNamed::ContextMenu => NamedKey::Menu,
        WNamed::Shift => NamedKey::LeftShift,
        WNamed::Control => NamedKey::LeftControl,
        WNamed::Alt => NamedKey::LeftAlt,
        WNamed::Super | WNamed::Meta => NamedKey::LeftSuper,
        WNamed::Hyper => NamedKey::LeftHyper,
        WNamed::AltGraph => NamedKey::IsoLevel3Shift,
        WNamed::MediaPlay => NamedKey::MediaPlay,
        WNamed::MediaPause => NamedKey::MediaPause,
        WNamed::MediaPlayPause => NamedKey::MediaPlayPause,
        WNamed::MediaStop => NamedKey::MediaStop,
        WNamed::MediaFastForward => NamedKey::MediaFastForward,
        WNamed::MediaRewind => NamedKey::MediaRewind,
        WNamed::MediaTrackNext => NamedKey::MediaTrackNext,
        WNamed::MediaTrackPrevious => NamedKey::MediaTrackPrevious,
        WNamed::MediaRecord => NamedKey::MediaRecord,
        WNamed::AudioVolumeDown => NamedKey::LowerVolume,
        WNamed::AudioVolumeUp => NamedKey::RaiseVolume,
        WNamed::AudioVolumeMute => NamedKey::MuteVolume,
        f => return function_key(f),
    })
}

/// F1 to F35.
fn function_key(n: WNamed) -> Option<NamedKey> {
    use WNamed::*;
    let all = [
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
        F25, F26, F27, F28, F29, F30, F31, F32, F33, F34, F35,
    ];
    all.iter().position(|k| *k == n).map(|i| NamedKey::F(i as u8 + 1))
}

/// The character the key at this position types on a US layout, for the kitty protocol's base
/// layout key.
fn physical_char(p: PhysicalKey) -> Option<char> {
    let PhysicalKey::Code(c) = p else { return None };
    let letters = [
        (KeyCode::KeyA, 'a'),
        (KeyCode::KeyB, 'b'),
        (KeyCode::KeyC, 'c'),
        (KeyCode::KeyD, 'd'),
        (KeyCode::KeyE, 'e'),
        (KeyCode::KeyF, 'f'),
        (KeyCode::KeyG, 'g'),
        (KeyCode::KeyH, 'h'),
        (KeyCode::KeyI, 'i'),
        (KeyCode::KeyJ, 'j'),
        (KeyCode::KeyK, 'k'),
        (KeyCode::KeyL, 'l'),
        (KeyCode::KeyM, 'm'),
        (KeyCode::KeyN, 'n'),
        (KeyCode::KeyO, 'o'),
        (KeyCode::KeyP, 'p'),
        (KeyCode::KeyQ, 'q'),
        (KeyCode::KeyR, 'r'),
        (KeyCode::KeyS, 's'),
        (KeyCode::KeyT, 't'),
        (KeyCode::KeyU, 'u'),
        (KeyCode::KeyV, 'v'),
        (KeyCode::KeyW, 'w'),
        (KeyCode::KeyX, 'x'),
        (KeyCode::KeyY, 'y'),
        (KeyCode::KeyZ, 'z'),
        (KeyCode::Digit0, '0'),
        (KeyCode::Digit1, '1'),
        (KeyCode::Digit2, '2'),
        (KeyCode::Digit3, '3'),
        (KeyCode::Digit4, '4'),
        (KeyCode::Digit5, '5'),
        (KeyCode::Digit6, '6'),
        (KeyCode::Digit7, '7'),
        (KeyCode::Digit8, '8'),
        (KeyCode::Digit9, '9'),
        (KeyCode::Minus, '-'),
        (KeyCode::Equal, '='),
        (KeyCode::BracketLeft, '['),
        (KeyCode::BracketRight, ']'),
        (KeyCode::Backslash, '\\'),
        (KeyCode::Semicolon, ';'),
        (KeyCode::Quote, '\''),
        (KeyCode::Backquote, '`'),
        (KeyCode::Comma, ','),
        (KeyCode::Period, '.'),
        (KeyCode::Slash, '/'),
        (KeyCode::Space, ' '),
    ];
    letters.iter().find(|(k, _)| *k == c).map(|(_, ch)| *ch)
}
