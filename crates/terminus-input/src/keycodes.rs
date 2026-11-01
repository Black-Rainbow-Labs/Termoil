//! The functional key tables: which CSI form each named key uses and its kitty code point.

use crate::key::{KeypadKey, NamedKey};

/// The three escape code forms the kitty keyboard protocol uses for a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// `CSI number ; mods ~`
    Tilde(u32),
    /// `CSI 1 ; mods letter`, with the `1;` left out when there are no modifiers.
    Letter(u8),
    /// `CSI code ; mods u`
    Unicode(u32),
}

/// The first private use code point of F13 in the kitty table.
const F13_CODE: u32 = 57376;

/// Returns the kitty code point of a keypad key, or None for Begin which has its own form.
fn keypad_code(kp: KeypadKey) -> Option<u32> {
    use KeypadKey::*;
    Some(match kp {
        Digit(d) => 57399 + u32::from(d.min(9)),
        Decimal => 57409,
        Divide => 57410,
        Multiply => 57411,
        Subtract => 57412,
        Add => 57413,
        Enter => 57414,
        Equal => 57415,
        Separator => 57416,
        Left => 57417,
        Right => 57418,
        Up => 57419,
        Down => 57420,
        PageUp => 57421,
        PageDown => 57422,
        Home => 57423,
        End => 57424,
        Insert => 57425,
        Delete => 57426,
        Begin => return None,
    })
}

/// Returns the code point of a media or modifier key in the private use area.
fn media_or_modifier_code(key: NamedKey) -> Option<u32> {
    use NamedKey::*;
    Some(match key {
        MediaPlay => 57428,
        MediaPause => 57429,
        MediaPlayPause => 57430,
        MediaReverse => 57431,
        MediaStop => 57432,
        MediaFastForward => 57433,
        MediaRewind => 57434,
        MediaTrackNext => 57435,
        MediaTrackPrevious => 57436,
        MediaRecord => 57437,
        LowerVolume => 57438,
        RaiseVolume => 57439,
        MuteVolume => 57440,
        LeftShift => 57441,
        LeftControl => 57442,
        LeftAlt => 57443,
        LeftSuper => 57444,
        LeftHyper => 57445,
        LeftMeta => 57446,
        RightShift => 57447,
        RightControl => 57448,
        RightAlt => 57449,
        RightSuper => 57450,
        RightHyper => 57451,
        RightMeta => 57452,
        IsoLevel3Shift => 57453,
        IsoLevel5Shift => 57454,
        _ => return None,
    })
}

/// Returns the tilde number of a function key F1 to F12 in the form the kitty table lists.
fn function_tilde(n: u8) -> Option<u32> {
    Some(match n {
        3 => 13,
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    })
}

/// Returns the kitty protocol form of a function key.
fn function_form(n: u8) -> Option<Form> {
    match n {
        1 => Some(Form::Letter(b'P')),
        2 => Some(Form::Letter(b'Q')),
        4 => Some(Form::Letter(b'S')),
        3 | 5..=12 => function_tilde(n).map(Form::Tilde),
        13..=35 => Some(Form::Unicode(F13_CODE + u32::from(n) - 13)),
        _ => None,
    }
}

/// Returns the form the kitty keyboard protocol uses for a named key, or None for an invalid key.
pub(crate) fn kitty_form(key: NamedKey) -> Option<Form> {
    use NamedKey::*;
    Some(match key {
        Escape => Form::Unicode(27),
        Enter => Form::Unicode(13),
        Tab => Form::Unicode(9),
        Backspace => Form::Unicode(127),
        Insert => Form::Tilde(2),
        Delete => Form::Tilde(3),
        Left => Form::Letter(b'D'),
        Right => Form::Letter(b'C'),
        Up => Form::Letter(b'A'),
        Down => Form::Letter(b'B'),
        PageUp => Form::Tilde(5),
        PageDown => Form::Tilde(6),
        Home => Form::Letter(b'H'),
        End => Form::Letter(b'F'),
        CapsLock => Form::Unicode(57358),
        ScrollLock => Form::Unicode(57359),
        NumLock => Form::Unicode(57360),
        PrintScreen => Form::Unicode(57361),
        Pause => Form::Unicode(57362),
        Menu => Form::Unicode(57363),
        F(n) => return function_form(n),
        Kp(KeypadKey::Begin) => Form::Letter(b'E'),
        Kp(kp) => Form::Unicode(keypad_code(kp)?),
        other => Form::Unicode(media_or_modifier_code(other)?),
    })
}

/// True for the named keys that use the same CSI or SS3 functional encoding in the legacy protocol and the kitty protocol.
pub(crate) fn is_cursor_or_function_key(key: NamedKey) -> bool {
    use NamedKey::*;
    matches!(
        key,
        Insert | Delete | Left | Right | Up | Down | PageUp | PageDown | Home | End | F(1..=12) | Kp(KeypadKey::Begin)
    )
}
