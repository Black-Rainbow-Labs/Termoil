//! Platform key normalisation: turns raw operating system key events into the platform independent
//! `KeyEvent` the encoders take. It resolves AltGr, dead keys, the Win key and the numeric keypad.

use crate::key::{Key, KeyEvent, KeyEventKind, KeypadKey, NamedKey};
use crate::modifiers::Modifiers;
use crate::seq::has_control;

/// A key event as the operating system reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawKey {
    /// The logical key: the unshifted character on the active layout, or a named key.
    pub logical: Key,
    /// The key at the same physical position on a standard PC 101 layout.
    pub physical: Option<char>,
    /// Modifiers as the OS reports them. On Windows AltGr arrives as ctrl plus alt.
    pub mods: Modifiers,
    /// True when the OS says the right alt key is AltGr.
    pub altgr: bool,
    /// The accent when this press is a dead key.
    pub dead: Option<char>,
    /// The num lock state.
    pub numlock: bool,
    /// The text the OS produced for the press.
    pub text: Option<String>,
    /// The character the layout types for this key with Shift held, when Shift is held. The OS
    /// reports it even when Ctrl or Cmd make the text empty, so a binding written as `ctrl+plus`
    /// can match Ctrl, Shift and the `=` key.
    pub shifted: Option<char>,
    pub kind: KeyEventKind,
}

impl RawKey {
    /// Builds a press with the given logical key and modifiers, num lock off and no text.
    pub fn new(logical: Key, mods: Modifiers) -> RawKey {
        RawKey {
            logical,
            physical: None,
            mods,
            altgr: false,
            dead: None,
            numlock: false,
            text: None,
            shifted: None,
            kind: KeyEventKind::Press,
        }
    }

    /// Returns the raw key with its text set.
    pub fn with_text(mut self, text: &str) -> RawKey {
        self.text = Some(text.to_string());
        self
    }

    /// Returns the raw key marked as a dead key press for the given accent.
    pub fn with_dead(mut self, accent: char) -> RawKey {
        self.dead = Some(accent);
        self
    }

    /// Returns the raw key with the AltGr flag set.
    pub fn with_altgr(mut self) -> RawKey {
        self.altgr = true;
        self
    }

    /// Returns the raw key with the num lock state set.
    pub fn with_numlock(mut self, on: bool) -> RawKey {
        self.numlock = on;
        self
    }

    /// Returns the raw key with its physical (base layout) key set.
    pub fn with_physical(mut self, physical: char) -> RawKey {
        self.physical = Some(physical);
        self
    }

    /// Returns the raw key with the character the layout types for it with Shift held.
    pub fn with_shifted(mut self, shifted: char) -> RawKey {
        self.shifted = Some(shifted);
        self
    }

    /// Returns the raw key with its event kind replaced.
    pub fn with_kind(mut self, kind: KeyEventKind) -> RawKey {
        self.kind = kind;
        self
    }
}

/// The state machine that normalises raw keys. It remembers a dead key until the next key arrives.
#[derive(Clone, Debug, Default)]
pub struct KeyNormalizer {
    pending_dead: Option<char>,
}

impl KeyNormalizer {
    /// Creates a normaliser with no pending dead key.
    pub fn new() -> KeyNormalizer {
        KeyNormalizer { pending_dead: None }
    }

    /// True while a dead key is waiting for the next key.
    pub fn has_pending_dead(&self) -> bool {
        self.pending_dead.is_some()
    }

    /// Feeds one raw key and returns the events it produces: none for a dead key that starts a
    /// composition, one normally, two when a pending accent did not combine with the next key.
    pub fn feed(&mut self, raw: &RawKey) -> Vec<KeyEvent> {
        let (mods, text) = resolve_altgr(raw);
        if raw.kind == KeyEventKind::Release {
            return if raw.dead.is_some() { Vec::new() } else { vec![to_event(raw, mods, text)] };
        }
        if let Some(accent) = raw.dead {
            return self.feed_dead_key(accent, raw, mods);
        }
        let Some(pending) = self.pending_dead else { return vec![to_event(raw, mods, text)] };
        if matches!(raw.logical, Key::Named(k) if k.is_modifier_key()) {
            return vec![to_event(raw, mods, text)];
        }
        self.pending_dead = None;
        if is_shortcut(raw, mods, &text) {
            return vec![to_event(raw, mods, text)];
        }
        compose_pending(pending, raw, mods, text)
    }

    /// Handles a dead key press: start a composition, or finish one when a second dead key follows.
    fn feed_dead_key(&mut self, accent: char, raw: &RawKey, mods: Modifiers) -> Vec<KeyEvent> {
        match self.pending_dead.take() {
            None => {
                self.pending_dead = Some(accent);
                Vec::new()
            }
            Some(first) if first == accent => vec![text_event(raw.logical, spacing_accent(accent), mods)],
            Some(first) => vec![
                text_event(Key::Char('\0'), spacing_accent(first), Modifiers::NONE),
                text_event(raw.logical, spacing_accent(accent), mods),
            ],
        }
    }
}

/// Resolves AltGr: when the OS reports ctrl plus alt (or the AltGr flag) and the key typed a printable
/// character, the typed character is the key and ctrl and alt are removed. The release of an AltGr
/// key also loses ctrl and alt so that it matches its press.
fn resolve_altgr(raw: &RawKey) -> (Modifiers, Option<String>) {
    let printable = raw.text.clone().filter(|t| !t.is_empty() && !has_control(t));
    let ctrl_alt = raw.mods.contains(Modifiers::CTRL | Modifiers::ALT);
    let altgr_release = raw.altgr && raw.kind == KeyEventKind::Release;
    if (raw.altgr || ctrl_alt) && (printable.is_some() || altgr_release) {
        return (raw.mods.without(Modifiers::CTRL | Modifiers::ALT), printable);
    }
    (raw.mods, raw.text.clone())
}

/// True when the key is a command rather than typed text: a named key, or ctrl, alt or super held.
fn is_shortcut(raw: &RawKey, mods: Modifiers, text: &Option<String>) -> bool {
    let printable = text.as_ref().is_some_and(|t| !t.is_empty() && !has_control(t));
    matches!(raw.logical, Key::Named(_))
        || mods.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::EXTENDED)
        || !printable
}

/// Combines a pending accent with the key that followed it.
fn compose_pending(accent: char, raw: &RawKey, mods: Modifiers, text: Option<String>) -> Vec<KeyEvent> {
    let typed = text.clone().unwrap_or_default();
    let mut chars = typed.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    if single == Some(' ') {
        return vec![text_event(raw.logical, spacing_accent(accent), mods)];
    }
    if let Some(composed) = single.and_then(|c| compose(accent, c)) {
        return vec![text_event(raw.logical, composed, mods)];
    }
    vec![text_event(Key::Char('\0'), spacing_accent(accent), Modifiers::NONE), to_event(raw, mods, text)]
}

/// Builds a press event that types one character.
fn text_event(key: Key, ch: char, mods: Modifiers) -> KeyEvent {
    let mut event = KeyEvent::press(key, mods.without(Modifiers::SHIFT));
    event.text = Some(ch.to_string());
    event
}

/// Returns the modifier bit a modifier key sets, or None for any other key.
fn modifier_bit(key: NamedKey) -> Option<Modifiers> {
    use NamedKey::*;
    Some(match key {
        LeftShift | RightShift => Modifiers::SHIFT,
        LeftControl | RightControl => Modifiers::CTRL,
        LeftAlt | RightAlt => Modifiers::ALT,
        LeftSuper | RightSuper => Modifiers::SUPER,
        LeftHyper | RightHyper => Modifiers::HYPER,
        LeftMeta | RightMeta => Modifiers::META,
        _ => return None,
    })
}

/// Returns the character a keypad key types with num lock on.
fn keypad_text(kp: KeypadKey) -> Option<char> {
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

/// Returns the navigation key a numeric keypad digit or decimal point acts as with num lock off.
fn keypad_navigation(kp: KeypadKey) -> KeypadKey {
    match kp {
        KeypadKey::Digit(0) => KeypadKey::Insert,
        KeypadKey::Digit(1) => KeypadKey::End,
        KeypadKey::Digit(2) => KeypadKey::Down,
        KeypadKey::Digit(3) => KeypadKey::PageDown,
        KeypadKey::Digit(4) => KeypadKey::Left,
        KeypadKey::Digit(5) => KeypadKey::Begin,
        KeypadKey::Digit(6) => KeypadKey::Right,
        KeypadKey::Digit(7) => KeypadKey::Home,
        KeypadKey::Digit(8) => KeypadKey::Up,
        KeypadKey::Digit(9) => KeypadKey::PageUp,
        KeypadKey::Decimal => KeypadKey::Delete,
        other => other,
    }
}

/// Applies the keypad rule: num lock on gives text, num lock off turns digits and the decimal point into navigation keys.
fn resolve_keypad(raw: &RawKey, text: Option<String>) -> (Key, Option<String>) {
    let Key::Named(NamedKey::Kp(kp)) = raw.logical else { return (raw.logical, text) };
    let navigation = keypad_navigation(kp);
    if !raw.numlock && navigation != kp {
        return (Key::Named(NamedKey::Kp(navigation)), None);
    }
    (raw.logical, keypad_text(kp).map(String::from).or(text))
}

/// Converts a raw key into a key event using the resolved modifiers and text.
fn to_event(raw: &RawKey, mods: Modifiers, text: Option<String>) -> KeyEvent {
    let (key, text) = resolve_keypad(raw, text);
    let mut mods = mods;
    if raw.numlock && matches!(raw.logical, Key::Named(NamedKey::Kp(_))) {
        mods |= Modifiers::NUM_LOCK;
    }
    if let Key::Named(named) = key
        && let Some(bit) = modifier_bit(named)
    {
        mods = if raw.kind == KeyEventKind::Release { mods.without(bit) } else { mods | bit };
    }
    let command = mods.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER);
    let text = text.filter(|t| !t.is_empty() && !has_control(t) && !command);
    let shifted_key = shifted_from_text(key, mods, &text).or_else(|| shifted_from_layout(raw, key, mods));
    let base_layout_key = match key {
        Key::Char(c) => raw.physical.filter(|p| *p != c),
        Key::Named(_) => None,
    };
    KeyEvent { key, mods, kind: raw.kind, text, shifted_key, base_layout_key }
}

/// Returns the shifted character of a key when shift is held and the text is one different character.
fn shifted_from_text(key: Key, mods: Modifiers, text: &Option<String>) -> Option<char> {
    let Key::Char(c) = key else { return None };
    if !mods.contains(Modifiers::SHIFT) {
        return None;
    }
    let mut chars = text.as_deref()?.chars();
    match (chars.next(), chars.next()) {
        (Some(s), None) if s != c => Some(s),
        _ => None,
    }
}

/// Returns the shifted character the OS reported for a character key when Shift is held and the
/// character differs from the key.
fn shifted_from_layout(raw: &RawKey, key: Key, mods: Modifiers) -> Option<char> {
    let Key::Char(c) = key else { return None };
    raw.shifted.filter(|s| mods.contains(Modifiers::SHIFT) && *s != c)
}

/// Returns the accent group of a dead key character: 0 grave, 1 acute, 2 circumflex, 3 tilde, 4 diaeresis, 5 cedilla, 6 ring.
fn accent_group(accent: char) -> Option<usize> {
    Some(match accent {
        '`' | '\u{300}' => 0,
        '\u{b4}' | '\'' | '\u{301}' => 1,
        '^' | '\u{302}' => 2,
        '~' | '\u{303}' | '\u{2dc}' => 3,
        '\u{a8}' | '"' | '\u{308}' => 4,
        '\u{b8}' | '\u{327}' => 5,
        '\u{b0}' | '\u{2da}' | '\u{30a}' => 6,
        _ => return None,
    })
}

/// The letters each accent combines with and the composed results, in the same order.
const COMPOSITIONS: [(&str, &str); 7] = [
    ("aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
    ("aeiouyAEIOUY", "áéíóúýÁÉÍÓÚÝ"),
    ("aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
    ("anoANO", "ãñõÃÑÕ"),
    ("aeiouyAEIOUY", "äëïöüÿÄËÏÖÜŸ"),
    ("cC", "çÇ"),
    ("aAuU", "åÅůŮ"),
];

/// Returns the character an accent and a base letter compose into, when the table has one.
pub(crate) fn compose(accent: char, base: char) -> Option<char> {
    let (bases, results) = COMPOSITIONS[accent_group(accent)?];
    let index = bases.chars().position(|b| b == base)?;
    results.chars().nth(index)
}

/// Returns the spacing form of an accent: combining marks become the plain accent character.
fn spacing_accent(accent: char) -> char {
    match accent {
        '\u{300}' => '`',
        '\u{301}' => '\u{b4}',
        '\u{302}' => '^',
        '\u{303}' => '~',
        '\u{308}' => '\u{a8}',
        '\u{327}' => '\u{b8}',
        '\u{30a}' => '\u{b0}',
        other => other,
    }
}
