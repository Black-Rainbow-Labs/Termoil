//! A screen: the page list plus the cursor, tab stops, margins and character sets that go with
//! it. The terminal has two, the primary screen and the alternate screen.

use crate::cell::Semantic;
use crate::charset::Charsets;
use crate::pagelist::{PageList, ScrollbackLimit};
use crate::style::Style;
use crate::unicode::GraphemeBreaker;

/// The cursor: position, pending wrap, and the pen used for new text.
#[derive(Clone, Debug, Default)]
pub struct Cursor {
    /// Active row, 0 based.
    pub row: u16,
    /// Column, 0 based.
    pub col: u16,
    /// The last column was written and the next printable character wraps first.
    pub pending_wrap: bool,
    /// The SGR pen.
    pub style: Style,
    /// DECSCA: new cells are protected from selective erase.
    pub protected: bool,
    /// The OSC 8 hyperlink new cells get.
    pub hyperlink: Option<u32>,
    /// The OSC 133 content type new cells get.
    pub semantic: Semantic,
    /// Cached page identity and style id for `style`, so printing does not intern per cell.
    pub style_cache: Option<(u64, u16)>,
    /// The last few (page identity, style, id) the pen used, so switching back and forth between
    /// a colour and the default does not look the style up in the page's table each time.
    pub recent_styles: [Option<(u64, Style, u16)>; 4],
    /// Which slot of `recent_styles` the next new entry replaces.
    pub recent_next: u8,
}

/// What DECSC saves and DECRC restores.
#[derive(Clone, Debug, Default)]
pub struct SavedCursor {
    /// Row.
    pub row: u16,
    /// Column.
    pub col: u16,
    /// Pending wrap.
    pub pending_wrap: bool,
    /// The pen.
    pub style: Style,
    /// DECSCA state.
    pub protected: bool,
    /// Origin mode.
    pub origin: bool,
    /// Character sets.
    pub charsets: Charsets,
}

/// The last cell text was printed into, so a following combining character or grapheme
/// extender can join it.
#[derive(Clone, Copy, Debug)]
pub struct LastPrint {
    /// Active row.
    pub row: u16,
    /// Column of the cell (the first column of a wide character).
    pub col: u16,
    /// Grapheme break state after the cell's last code point; None for a plain ASCII cell,
    /// whose state is built from `ascii` only when something needs it.
    pub state: Option<GraphemeBreaker>,
    /// The cell's character when `state` is None.
    pub ascii: u8,
    /// Cursor position right after printing; any other position means the cursor moved.
    pub after: (u16, u16, bool),
}

impl LastPrint {
    /// The grapheme break state after the cell.
    pub fn breaker(&self) -> GraphemeBreaker {
        self.state.unwrap_or_else(|| GraphemeBreaker::new(self.ascii as char))
    }
}

/// The kitty keyboard protocol flag stack for one screen, bounded at 16 entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KittyKeyboard {
    stack: Vec<u8>,
    current: u8,
}

/// Depth of the kitty keyboard flag stack.
pub const KITTY_STACK_DEPTH: usize = 16;

impl KittyKeyboard {
    /// The flags in effect.
    pub fn flags(&self) -> u8 {
        self.current
    }

    /// Entries on the stack.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// `CSI > flags u`: pushes the current flags and sets new ones. A full stack drops its oldest.
    pub fn push(&mut self, flags: u8) {
        if self.stack.len() == KITTY_STACK_DEPTH {
            self.stack.remove(0);
        }
        self.stack.push(self.current);
        self.current = flags & 0x1f;
    }

    /// `CSI < n u`: pops `n` entries. Popping past empty resets the flags to zero.
    pub fn pop(&mut self, n: usize) {
        for _ in 0..n.max(1) {
            match self.stack.pop() {
                Some(f) => self.current = f,
                None => {
                    self.current = 0;
                    break;
                }
            }
        }
    }

    /// `CSI = flags ; mode u`: 1 sets, 2 adds bits, 3 removes bits.
    pub fn set(&mut self, flags: u8, mode: u16) {
        let flags = flags & 0x1f;
        self.current = match mode {
            2 => self.current | flags,
            3 => self.current & !flags,
            _ => flags,
        };
    }

    /// Clears the stack and the flags.
    pub fn reset(&mut self) {
        self.stack.clear();
        self.current = 0;
    }
}

/// One screen.
#[derive(Debug)]
pub struct Screen {
    /// Every row.
    pub pages: PageList,
    /// The cursor.
    pub cursor: Cursor,
    /// DECSC storage.
    pub saved: Option<SavedCursor>,
    /// Tab stops by column.
    pub tabs: Vec<bool>,
    /// Scrolling region top, inclusive.
    pub top: u16,
    /// Scrolling region bottom, inclusive.
    pub bottom: u16,
    /// Left margin, inclusive (used when DECLRMM is on).
    pub left: u16,
    /// Right margin, inclusive.
    pub right: u16,
    /// Character sets.
    pub charsets: Charsets,
    /// Kitty keyboard flags for this screen.
    pub kitty_keyboard: KittyKeyboard,
    /// Where the last character went, for joining clusters.
    pub last_print: Option<LastPrint>,
}

impl Screen {
    /// A blank screen. `history` is false for the alternate screen.
    pub fn new(cols: u16, rows: u16, limit: ScrollbackLimit, history: bool) -> Screen {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Screen {
            pages: PageList::new(cols, rows, limit, history),
            cursor: Cursor::default(),
            saved: None,
            tabs: default_tabs(cols),
            top: 0,
            bottom: rows - 1,
            left: 0,
            right: cols - 1,
            charsets: Charsets::default(),
            kitty_keyboard: KittyKeyboard::default(),
            last_print: None,
        }
    }

    /// Columns.
    #[inline]
    pub fn cols(&self) -> u16 {
        self.pages.cols()
    }

    /// Rows.
    #[inline]
    pub fn rows(&self) -> u16 {
        self.pages.rows()
    }

    /// Resets the scrolling region and margins to the whole screen.
    pub fn reset_margins(&mut self) {
        self.top = 0;
        self.bottom = self.rows() - 1;
        self.left = 0;
        self.right = self.cols() - 1;
    }

    /// Scrolls rows `0..=bottom` up by one with the top row going into history; rows below the
    /// region stay where they are.
    pub fn push_top_line_to_history(&mut self, bottom: u16) {
        let rows = self.rows();
        self.pages.grow();
        if bottom + 1 < rows {
            self.pages.scroll_down(bottom, rows - 1, 1, None);
        }
    }

    /// True when the scrolling region covers every row and column.
    #[inline]
    pub fn full_region(&self) -> bool {
        self.top == 0 && self.bottom == self.rows() - 1 && self.left == 0 && self.right == self.cols() - 1
    }
}

/// Tab stops every eight columns.
pub fn default_tabs(cols: u16) -> Vec<bool> {
    (0..cols).map(|c| c % 8 == 0 && c != 0).collect()
}
