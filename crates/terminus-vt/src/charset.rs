//! Character sets selected by SCS (`ESC ( B`, `ESC ( 0` and the rest) and invoked by SI, SO,
//! SS2 and SS3.
//!
//! In UTF-8 the sets only change the 94 printable ASCII characters; anything outside ASCII is
//! printed as itself whatever set is active.

/// A 94 character set Terminus can designate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Charset {
    /// US ASCII (`B`).
    #[default]
    Ascii,
    /// DEC Special Graphics, the line drawing set (`0`).
    DecSpecial,
    /// United Kingdom: `#` is the pound sign (`A`).
    Uk,
    /// DEC Technical (`>`), mapped to ASCII except for the characters with Unicode equivalents.
    DecTechnical,
}

impl Charset {
    /// The set an SCS final byte designates, or None for a set Terminus does not have (which
    /// leaves the designation unchanged).
    pub fn from_final(byte: u8) -> Option<Charset> {
        Some(match byte {
            b'B' => Charset::Ascii,
            b'0' | b'2' => Charset::DecSpecial,
            b'A' => Charset::Uk,
            b'>' => Charset::DecTechnical,
            b'1' | b'<' | b'%' | b'4' | b'5' | b'C' | b'R' | b'f' | b'Q' | b'9' | b'K' | b'Y' | b'Z' | b'H' | b'7' | b'=' | b'6' | b'"' | b'`' | b'E' => Charset::Ascii,
            _ => return None,
        })
    }

    /// Translates one printable ASCII character through this set.
    #[inline]
    pub fn map(self, c: char) -> char {
        match self {
            Charset::Ascii | Charset::DecTechnical => c,
            Charset::Uk if c == '#' => '\u{a3}',
            Charset::Uk => c,
            Charset::DecSpecial => dec_special(c),
        }
    }
}

/// The DEC Special Graphics mapping for 0x5F to 0x7E.
fn dec_special(c: char) -> char {
    const TABLE: [char; 32] = [
        '\u{00a0}', '\u{25c6}', '\u{2592}', '\u{2409}', '\u{240c}', '\u{240d}', '\u{240a}', '\u{00b0}', '\u{00b1}', '\u{2424}', '\u{240b}', '\u{2518}', '\u{2510}', '\u{250c}', '\u{2514}', '\u{253c}', '\u{23ba}', '\u{23bb}', '\u{2500}', '\u{23bc}', '\u{23bd}', '\u{251c}', '\u{2524}', '\u{2534}', '\u{252c}', '\u{2502}', '\u{2264}', '\u{2265}', '\u{03c0}', '\u{2260}', '\u{00a3}', '\u{00b7}',
    ];
    match c as u32 {
        0x5f..=0x7e => TABLE[(c as u32 - 0x5f) as usize],
        _ => c,
    }
}

/// The four designations G0 to G3 and which ones are invoked into GL and GR.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Charsets {
    /// G0 to G3.
    pub g: [Charset; 4],
    /// The set invoked into GL (0 to 3).
    pub gl: u8,
    /// The set invoked into GR (0 to 3).
    pub gr: u8,
    /// A single shift (SS2 or SS3) waiting for the next character.
    pub single_shift: Option<u8>,
}

impl Charsets {
    /// True when printing ASCII needs no translation: GL is ASCII and no single shift is pending.
    #[inline]
    pub fn is_plain(&self) -> bool {
        self.single_shift.is_none() && self.g[self.gl as usize] == Charset::Ascii
    }

    /// Translates a printable character through the active set, consuming a single shift.
    #[inline]
    pub fn translate(&mut self, c: char) -> char {
        if !(' '..='~').contains(&c) {
            self.single_shift = None;
            return c;
        }
        let set = match self.single_shift.take() {
            Some(g) => self.g[g as usize],
            None => self.g[self.gl as usize],
        };
        set.map(c)
    }
}
