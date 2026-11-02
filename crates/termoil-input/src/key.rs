//! Platform independent key event types. The application converts window system events into these.

use crate::modifiers::Modifiers;

/// Whether a key went down, auto repeated or came up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

/// A key on the numeric keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeypadKey {
    Digit(u8),
    Decimal,
    Divide,
    Multiply,
    Subtract,
    Add,
    Enter,
    Equal,
    Separator,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    Begin,
}

/// A key that has no Unicode character of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Escape,
    Enter,
    Tab,
    Backspace,
    Insert,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    /// A function key, 1 to 35.
    F(u8),
    Kp(KeypadKey),
    MediaPlay,
    MediaPause,
    MediaPlayPause,
    MediaReverse,
    MediaStop,
    MediaFastForward,
    MediaRewind,
    MediaTrackNext,
    MediaTrackPrevious,
    MediaRecord,
    LowerVolume,
    RaiseVolume,
    MuteVolume,
    LeftShift,
    LeftControl,
    LeftAlt,
    LeftSuper,
    LeftHyper,
    LeftMeta,
    RightShift,
    RightControl,
    RightAlt,
    RightSuper,
    RightHyper,
    RightMeta,
    IsoLevel3Shift,
    IsoLevel5Shift,
}

impl NamedKey {
    /// True for the keys that are modifiers themselves (shift, control, alt, super, hyper, meta, ISO shifts).
    pub fn is_modifier_key(self) -> bool {
        use NamedKey::*;
        matches!(
            self,
            LeftShift
                | LeftControl
                | LeftAlt
                | LeftSuper
                | LeftHyper
                | LeftMeta
                | RightShift
                | RightControl
                | RightAlt
                | RightSuper
                | RightHyper
                | RightMeta
                | IsoLevel3Shift
                | IsoLevel5Shift
        )
    }
}

/// The key that was pressed. `Char` holds the unshifted, lowercase character of the key on the
/// active layout. `Char('\0')` means a pure text event with no key identity (text from an IME or
/// from a modifier the operating system consumed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Named(NamedKey),
}

/// One key event ready to be encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub mods: Modifiers,
    pub kind: KeyEventKind,
    /// The text the key produces with the current layout and modifiers, for example "A" for shift+a.
    pub text: Option<String>,
    /// The shifted form of the key on the active layout, when the layout has one.
    pub shifted_key: Option<char>,
    /// The key at the same physical position on a standard PC 101 layout.
    pub base_layout_key: Option<char>,
}

impl KeyEvent {
    /// Builds a press event with no text and no alternate keys.
    pub fn press(key: Key, mods: Modifiers) -> KeyEvent {
        KeyEvent { key, mods, kind: KeyEventKind::Press, text: None, shifted_key: None, base_layout_key: None }
    }

    /// Returns the event with its kind replaced.
    pub fn with_kind(mut self, kind: KeyEventKind) -> KeyEvent {
        self.kind = kind;
        self
    }

    /// Returns the event with its text replaced.
    pub fn with_text(mut self, text: &str) -> KeyEvent {
        self.text = Some(text.to_string());
        self
    }

    /// Returns the event with a shifted key and a base layout key set.
    pub fn with_alternates(mut self, shifted: Option<char>, base: Option<char>) -> KeyEvent {
        self.shifted_key = shifted;
        self.base_layout_key = base;
        self
    }
}

/// The terminal modes that decide how keys are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardModes {
    /// DECCKM: cursor keys send SS3 instead of CSI.
    pub app_cursor: bool,
    /// DECKPAM: keypad keys send SS3 sequences.
    pub app_keypad: bool,
    /// xterm modifyOtherKeys level: 0, 1 or 2.
    pub modify_other_keys: u8,
    /// Kitty keyboard protocol flags: 1 disambiguate, 2 event types, 4 alternate keys, 8 all keys as escape codes, 16 associated text.
    pub kitty_flags: u8,
    /// DECBKM: true makes Backspace send BS (0x08), false sends DEL (0x7f).
    pub backarrow_is_bs: bool,
    /// Alt prefixes the key with ESC (true) or sets the eighth bit (false).
    pub alt_sends_escape: bool,
}

impl Default for KeyboardModes {
    /// The power on state: no application modes, legacy encoding, Backspace sends DEL, Alt sends ESC.
    fn default() -> KeyboardModes {
        KeyboardModes {
            app_cursor: false,
            app_keypad: false,
            modify_other_keys: 0,
            kitty_flags: 0,
            backarrow_is_bs: false,
            alt_sends_escape: true,
        }
    }
}
