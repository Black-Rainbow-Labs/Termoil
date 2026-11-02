//! Termoil key and mouse encoders: legacy xterm, modifyOtherKeys, the kitty keyboard protocol and every mouse mode.

mod key;
mod keycodes;
mod kitty;
mod legacy;
mod modifiers;
mod mouse;
pub mod normalize;
mod paste;
mod seq;

pub use key::{Key, KeyEvent, KeyEventKind, KeyboardModes, KeypadKey, NamedKey};
pub use modifiers::Modifiers;
pub use mouse::{MouseButton, MouseEncoding, MouseEvent, MouseEventKind, MouseModes, MouseTracking, encode_mouse};
pub use normalize::{KeyNormalizer, RawKey};
pub use paste::{encode_focus, encode_paste};

/// Encodes a key event for the terminal modes in `modes` and appends the bytes to `out`. The kitty
/// keyboard protocol is used when any kitty flag is set, otherwise the legacy xterm encoding with
/// modifyOtherKeys. Returns false when the key sends nothing, such as a release without the event
/// type flag.
pub fn encode_key(event: &KeyEvent, modes: &KeyboardModes, out: &mut Vec<u8>) -> bool {
    if modes.kitty_flags & 0x1f != 0 { kitty::encode(event, modes, out) } else { legacy::encode(event, modes, out) }
}
