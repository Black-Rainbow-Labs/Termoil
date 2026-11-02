//! Modifier key state as a small bit set, using the bit values the kitty keyboard protocol defines.

use std::ops::{BitAnd, BitOr, BitOrAssign};

/// A set of held modifier keys and lock states. The bit values are the ones the kitty keyboard
/// protocol puts on the wire (before the 1 is added).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const NONE: Modifiers = Modifiers(0);
    pub const SHIFT: Modifiers = Modifiers(1);
    pub const ALT: Modifiers = Modifiers(2);
    pub const CTRL: Modifiers = Modifiers(4);
    pub const SUPER: Modifiers = Modifiers(8);
    pub const HYPER: Modifiers = Modifiers(16);
    pub const META: Modifiers = Modifiers(32);
    pub const CAPS_LOCK: Modifiers = Modifiers(64);
    pub const NUM_LOCK: Modifiers = Modifiers(128);
    /// Caps lock and num lock together.
    pub const LOCKS: Modifiers = Modifiers(64 | 128);
    /// Super, hyper and meta together: the modifiers the legacy encoding has no byte for.
    pub const EXTENDED: Modifiers = Modifiers(8 | 16 | 32);

    /// The empty set.
    pub const fn empty() -> Modifiers {
        Modifiers(0)
    }

    /// The raw bit value.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// True when no bit is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when every bit of `other` is set in `self`.
    pub const fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when at least one bit of `other` is set in `self`.
    pub const fn intersects(self, other: Modifiers) -> bool {
        self.0 & other.0 != 0
    }

    /// The union of two sets, usable in constants.
    pub const fn union(self, other: Modifiers) -> Modifiers {
        Modifiers(self.0 | other.0)
    }

    /// `self` with every bit of `other` cleared.
    pub const fn without(self, other: Modifiers) -> Modifiers {
        Modifiers(self.0 & !other.0)
    }

    /// The protocol parameter value: 1 plus the bit field.
    pub const fn wire_value(self) -> u32 {
        1 + self.0 as u32
    }
}

impl BitOr for Modifiers {
    type Output = Modifiers;

    /// Union of two sets.
    fn bitor(self, rhs: Modifiers) -> Modifiers {
        Modifiers(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    /// Adds every bit of `rhs` to `self`.
    fn bitor_assign(&mut self, rhs: Modifiers) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Modifiers {
    type Output = Modifiers;

    /// Intersection of two sets.
    fn bitand(self, rhs: Modifiers) -> Modifiers {
        Modifiers(self.0 & rhs.0)
    }
}
