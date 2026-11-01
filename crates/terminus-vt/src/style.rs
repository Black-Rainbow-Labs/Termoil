//! Colours and text styles: what SGR sets and what a cell's style id points to.

/// A colour as the terminal stores it: the default, an entry in the 256 colour palette, or RGB.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Color {
    /// The default foreground or background (or, for underlines, the foreground).
    #[default]
    Default,
    /// An index into the 256 colour palette.
    Palette(u8),
    /// A 24 bit colour.
    Rgb(u8, u8, u8),
}

/// The underline styles SGR 4 can select.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(u8)]
pub enum Underline {
    /// No underline.
    #[default]
    None = 0,
    /// SGR 4 or 4:1.
    Single = 1,
    /// SGR 21 or 4:2.
    Double = 2,
    /// SGR 4:3, the undercurl.
    Curly = 3,
    /// SGR 4:4.
    Dotted = 4,
    /// SGR 4:5.
    Dashed = 5,
}

impl Underline {
    /// The underline style for a `4:n` sub parameter, or None for an unknown value.
    pub fn from_sub(n: u16) -> Option<Underline> {
        Some(match n {
            0 => Underline::None,
            1 => Underline::Single,
            2 => Underline::Double,
            3 => Underline::Curly,
            4 => Underline::Dotted,
            5 => Underline::Dashed,
            _ => return None,
        })
    }
}

/// Attribute bits in [`Style::attrs`].
pub mod attr {
    /// SGR 1.
    pub const BOLD: u16 = 1 << 0;
    /// SGR 2.
    pub const FAINT: u16 = 1 << 1;
    /// SGR 3.
    pub const ITALIC: u16 = 1 << 2;
    /// SGR 5 (and 6, rapid blink, which is drawn the same).
    pub const BLINK: u16 = 1 << 3;
    /// SGR 7.
    pub const INVERSE: u16 = 1 << 4;
    /// SGR 8.
    pub const INVISIBLE: u16 = 1 << 5;
    /// SGR 9.
    pub const STRIKE: u16 = 1 << 6;
    /// SGR 53.
    pub const OVERLINE: u16 = 1 << 7;
}

/// Everything about how a cell's text is drawn, apart from the character itself.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Style {
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Underline colour (SGR 58); Default means the foreground.
    pub underline_color: Color,
    /// Attribute bits from [`attr`].
    pub attrs: u16,
    /// Underline style.
    pub underline: Underline,
}

impl Style {
    /// True for the default style, which is style id 0 and never stored in a table.
    #[inline]
    pub fn is_default(&self) -> bool {
        *self == Style::default()
    }

    /// True when an attribute bit is set.
    #[inline]
    pub fn has(&self, bit: u16) -> bool {
        self.attrs & bit != 0
    }

    /// The style an erase leaves behind: only the background colour survives (xterm's
    /// background colour erase).
    #[inline]
    pub fn erase_style(&self) -> Style {
        Style { bg: self.bg, ..Style::default() }
    }
}

/// An RGB colour value used for the palette and the dynamic colours.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// Builds a colour from its components.
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }
}

/// The xterm default 256 colour palette: 16 ANSI colours, the 6x6x6 cube, and 24 greys.
pub fn default_palette() -> [Rgb; 256] {
    const ANSI: [Rgb; 16] = [
        Rgb::new(0x00, 0x00, 0x00),
        Rgb::new(0xcd, 0x00, 0x00),
        Rgb::new(0x00, 0xcd, 0x00),
        Rgb::new(0xcd, 0xcd, 0x00),
        Rgb::new(0x00, 0x00, 0xee),
        Rgb::new(0xcd, 0x00, 0xcd),
        Rgb::new(0x00, 0xcd, 0xcd),
        Rgb::new(0xe5, 0xe5, 0xe5),
        Rgb::new(0x7f, 0x7f, 0x7f),
        Rgb::new(0xff, 0x00, 0x00),
        Rgb::new(0x00, 0xff, 0x00),
        Rgb::new(0xff, 0xff, 0x00),
        Rgb::new(0x5c, 0x5c, 0xff),
        Rgb::new(0xff, 0x00, 0xff),
        Rgb::new(0x00, 0xff, 0xff),
        Rgb::new(0xff, 0xff, 0xff),
    ];
    let mut p = [Rgb::default(); 256];
    p[..16].copy_from_slice(&ANSI);
    let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
    for i in 0..216 {
        p[16 + i] = Rgb::new(level(i / 36), level((i / 6) % 6), level(i % 6));
    }
    for i in 0..24 {
        let v = (8 + i * 10) as u8;
        p[232 + i] = Rgb::new(v, v, v);
    }
    p
}
