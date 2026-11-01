//! The packed cell and row header values that pages store.
//!
//! A cell is one 64 bit value: the first code point of what the cell shows, a tag saying whether
//! more code points follow in the page's grapheme store, the page local style id, the width
//! property, and flag bits. A row header holds the offset of the row's cells in the page and the
//! row flags. Flags may be true when nothing remains and are never false when something exists,
//! so a clear flag lets an operation skip work.

/// What a cell holds besides its code point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Wide {
    /// One column.
    Narrow = 0,
    /// The first column of a two column character.
    Wide = 1,
    /// The second column of a two column character; holds no text.
    SpacerTail = 2,
    /// The last column of a row whose wide character did not fit and moved to the next row.
    SpacerHead = 3,
}

/// The OSC 133 semantic content of a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Semantic {
    /// Command output (or anything not marked).
    #[default]
    Output = 0,
    /// Text the user typed after the prompt.
    Input = 1,
    /// The prompt itself.
    Prompt = 2,
}

const CP_MASK: u64 = 0x1f_ffff;
const CLUSTER_BIT: u64 = 1 << 21;
const STYLE_SHIFT: u32 = 24;
const STYLE_MASK: u64 = 0xffff << STYLE_SHIFT;
const WIDE_SHIFT: u32 = 40;
const WIDE_MASK: u64 = 0b11 << WIDE_SHIFT;
const PROTECTED_BIT: u64 = 1 << 42;
const HYPERLINK_BIT: u64 = 1 << 43;
const SEMANTIC_SHIFT: u32 = 44;
const SEMANTIC_MASK: u64 = 0b11 << SEMANTIC_SHIFT;

/// One grid cell, packed into 64 bits. Zero is a blank cell in the default style.
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(transparent)]
pub struct Cell(pub u64);

impl core::fmt::Debug for Cell {
    /// Prints the fields rather than the packed number.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Cell({:?} style={} {:?})", self.char(), self.style_id(), self.wide())
    }
}

impl Cell {
    /// A blank cell in the default style.
    pub const BLANK: Cell = Cell(0);

    /// A narrow cell holding `c` in style `style`.
    #[inline]
    pub fn new(c: char, style: u16) -> Cell {
        Cell(c as u64 | ((style as u64) << STYLE_SHIFT))
    }

    /// A blank cell in style `style` (an erase with a background colour).
    #[inline]
    pub fn blank(style: u16) -> Cell {
        Cell((style as u64) << STYLE_SHIFT)
    }

    /// The first code point, or `None` for a blank cell.
    #[inline]
    pub fn char(self) -> Option<char> {
        match (self.0 & CP_MASK) as u32 {
            0 => None,
            cp => char::from_u32(cp),
        }
    }

    /// The first code point as a number, 0 for blank.
    #[inline]
    pub fn codepoint(self) -> u32 {
        (self.0 & CP_MASK) as u32
    }

    /// True when the cell has no text.
    #[inline]
    pub fn is_blank(self) -> bool {
        self.0 & CP_MASK == 0
    }

    /// Replaces the code point, keeping everything else.
    #[inline]
    pub fn set_char(&mut self, c: char) {
        self.0 = (self.0 & !CP_MASK) | c as u64;
    }

    /// True when more code points follow in the page's grapheme store.
    #[inline]
    pub fn is_cluster(self) -> bool {
        self.0 & CLUSTER_BIT != 0
    }

    /// Marks the cell as having (or not having) a grapheme tail.
    #[inline]
    pub fn set_cluster(&mut self, on: bool) {
        self.set_bit(CLUSTER_BIT, on);
    }

    /// The page local style id; 0 is the default style.
    #[inline]
    pub fn style_id(self) -> u16 {
        ((self.0 & STYLE_MASK) >> STYLE_SHIFT) as u16
    }

    /// Sets the page local style id.
    #[inline]
    pub fn set_style_id(&mut self, id: u16) {
        self.0 = (self.0 & !STYLE_MASK) | ((id as u64) << STYLE_SHIFT);
    }

    /// The width property.
    #[inline]
    pub fn wide(self) -> Wide {
        match (self.0 & WIDE_MASK) >> WIDE_SHIFT {
            0 => Wide::Narrow,
            1 => Wide::Wide,
            2 => Wide::SpacerTail,
            _ => Wide::SpacerHead,
        }
    }

    /// Sets the width property.
    #[inline]
    pub fn set_wide(&mut self, w: Wide) {
        self.0 = (self.0 & !WIDE_MASK) | ((w as u64) << WIDE_SHIFT);
    }

    /// True when the cell is a spacer (head or tail) that holds no text of its own.
    #[inline]
    pub fn is_spacer(self) -> bool {
        matches!(self.wide(), Wide::SpacerTail | Wide::SpacerHead)
    }

    /// True when DECSCA marked the cell protected from selective erase.
    #[inline]
    pub fn protected(self) -> bool {
        self.0 & PROTECTED_BIT != 0
    }

    /// Sets the protection flag.
    #[inline]
    pub fn set_protected(&mut self, on: bool) {
        self.set_bit(PROTECTED_BIT, on);
    }

    /// True when the cell has an entry in the page's hyperlink map.
    #[inline]
    pub fn has_hyperlink(self) -> bool {
        self.0 & HYPERLINK_BIT != 0
    }

    /// Sets the hyperlink flag.
    #[inline]
    pub fn set_hyperlink(&mut self, on: bool) {
        self.set_bit(HYPERLINK_BIT, on);
    }

    /// The OSC 133 semantic content.
    #[inline]
    pub fn semantic(self) -> Semantic {
        match (self.0 & SEMANTIC_MASK) >> SEMANTIC_SHIFT {
            1 => Semantic::Input,
            2 => Semantic::Prompt,
            _ => Semantic::Output,
        }
    }

    /// Sets the OSC 133 semantic content.
    #[inline]
    pub fn set_semantic(&mut self, s: Semantic) {
        self.0 = (self.0 & !SEMANTIC_MASK) | ((s as u64) << SEMANTIC_SHIFT);
    }

    /// Sets or clears one bit.
    #[inline]
    fn set_bit(&mut self, bit: u64, on: bool) {
        if on {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }
}

/// Row flag bits.
pub mod row_flags {
    /// The row's last column wrapped onto the next row.
    pub const WRAP: u16 = 1 << 0;
    /// The row continues a row that wrapped.
    pub const WRAP_CONTINUATION: u16 = 1 << 1;
    /// Some cell may have a grapheme tail.
    pub const GRAPHEME: u16 = 1 << 2;
    /// Some cell may have a non default style.
    pub const STYLED: u16 = 1 << 3;
    /// Some cell may have a hyperlink.
    pub const HYPERLINK: u16 = 1 << 4;
    /// The row starts with an OSC 133 prompt mark.
    pub const SEMANTIC_PROMPT: u16 = 1 << 5;
    /// Some cell may hold a kitty Unicode placeholder.
    pub const KITTY_PLACEHOLDER: u16 = 1 << 6;
    /// The row changed since the renderer last copied it.
    pub const DIRTY: u16 = 1 << 7;
    /// The row is double width (DECDWL).
    pub const DOUBLE_WIDTH: u16 = 1 << 8;
    /// The row is the top half of double height (DECDHL top).
    pub const DOUBLE_HEIGHT_TOP: u16 = 1 << 9;
    /// The row is the bottom half of double height (DECDHL bottom).
    pub const DOUBLE_HEIGHT_BOTTOM: u16 = 1 << 10;
    /// The prompt mark on this row is a continuation prompt (OSC 133 A with k=s or P with k=c).
    pub const PROMPT_CONTINUATION: u16 = 1 << 11;
    /// Flags describing the line's size, cleared together.
    pub const LINE_SIZE: u16 = DOUBLE_WIDTH | DOUBLE_HEIGHT_TOP | DOUBLE_HEIGHT_BOTTOM;
    /// Flags that describe content and are cleared when the row is cleared.
    pub const CONTENT: u16 = GRAPHEME | STYLED | HYPERLINK | KITTY_PLACEHOLDER;
}

/// A row header: where the row's cells start in the page and its flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Row {
    /// Index of the row's first cell in the page's cell array.
    pub offset: u32,
    /// Bits from [`row_flags`].
    pub flags: u16,
}

impl Row {
    /// True when a flag bit is set.
    #[inline]
    pub fn has(&self, bit: u16) -> bool {
        self.flags & bit != 0
    }

    /// Sets or clears flag bits.
    #[inline]
    pub fn set(&mut self, bit: u16, on: bool) {
        if on {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }
}
