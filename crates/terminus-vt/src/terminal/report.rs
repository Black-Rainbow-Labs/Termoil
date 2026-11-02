//! Reports: device attributes, status reports, mode reports, XTVERSION, DECRQSS and XTGETTCAP.
//! Every answer is byte exact and is tested in `corpus/vt/reports.test`.

use super::{CursorShape, Event, Terminal};
use crate::modes::{Mode, ModeKind};
use crate::parser::CsiSeq;
use crate::style::{Color, Underline, attr};

/// The terminfo capabilities XTGETTCAP answers, which are also the ones the shipped
/// `xterm-terminus` entry defines. `shell-integration/terminfo` is generated from this table.
pub const TERMINFO_CAPS: &[(&str, &str)] = &[
    ("TN", "xterm-terminus"),
    ("name", "xterm-terminus"),
    ("Co", "256"),
    ("colors", "256"),
    ("RGB", "8/8/8"),
    ("Tc", ""),
    ("Su", ""),
    ("Smulx", "\x1b[4:%p1%dm"),
    ("Setulc", "\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%dm"),
    ("Ss", "\x1b[%p1%d q"),
    ("Se", "\x1b[2 q"),
    ("Sync", "\x1b[?2026%?%p1%{1}%-%tl%eh%;"),
    ("BD", "\x1b[?2004l"),
    ("BE", "\x1b[?2004h"),
    ("PE", "\x1b[201~"),
    ("PS", "\x1b[200~"),
    ("fsl", "\x07"),
    ("tsl", "\x1b]2;"),
    ("hs", ""),
    ("kbs", "\x7f"),
    ("kcuu1", "\x1bOA"),
    ("kcud1", "\x1bOB"),
    ("kcuf1", "\x1bOC"),
    ("kcub1", "\x1bOD"),
    ("khome", "\x1bOH"),
    ("kend", "\x1bOF"),
    ("kdch1", "\x1b[3~"),
    ("kich1", "\x1b[2~"),
    ("knp", "\x1b[6~"),
    ("kpp", "\x1b[5~"),
    ("kf1", "\x1bOP"),
    ("kf2", "\x1bOQ"),
    ("kf3", "\x1bOR"),
    ("kf4", "\x1bOS"),
    ("kf5", "\x1b[15~"),
    ("kf6", "\x1b[17~"),
    ("kf7", "\x1b[18~"),
    ("kf8", "\x1b[19~"),
    ("kf9", "\x1b[20~"),
    ("kf10", "\x1b[21~"),
    ("kf11", "\x1b[23~"),
    ("kf12", "\x1b[24~"),
    ("smkx", "\x1b[?1h\x1b="),
    ("rmkx", "\x1b[?1l\x1b>"),
    ("smcup", "\x1b[?1049h"),
    ("rmcup", "\x1b[?1049l"),
    ("civis", "\x1b[?25l"),
    ("cnorm", "\x1b[?12l\x1b[?25h"),
    ("bel", "\x07"),
    ("cols", "80"),
    ("lines", "24"),
];

/// The version number DA2 reports. xterm reports its patch number, and programs that look at it
/// expect a value from 314 up.
const DA2_VERSION: u16 = 400;

/// The widest sixel image the decoder accepts, and the tallest it accepts before the pixel cap.
const SIXEL_MAX_DIMENSION: u64 = 16_384;

impl Terminal {
    /// CSI sequences that answer with a report, plus anything unrecognised (ignored).
    pub(super) fn report_dispatch(&mut self, seq: &CsiSeq) {
        let p = &seq.params;
        match (seq.private, seq.intermediates(), seq.final_byte) {
            (0, [], b'c') if p.get(0) == 0 => self.reply_da1(),
            (b'>', [], b'c') if p.get(0) == 0 => self.reply_da2(),
            (b'=', [], b'c') if p.get(0) == 0 => self.reply(b"\x1bP!|5445524d\x1b\\"),
            (b'?', [], b'S') => self.xtsmgraphics(p.get(0), p.get(1)),
            (0, [], b'n') => self.dsr(p.get(0)),
            (b'?', [], b'n') => self.dec_dsr(p.get(0), p.get(1)),
            (0, [b'$'], b'p') if self.vt_level >= 3 => self.decrqm(ModeKind::Ansi, p.get(0)),
            (b'?', [b'$'], b'p') if self.vt_level >= 3 => self.decrqm(ModeKind::Dec, p.get(0)),
            (b'>', [], b'q') => {
                let v = self.config.version_string.clone();
                self.reply(format!("\x1bP>|{v}\x1b\\").as_bytes());
            }
            _ => {}
        }
    }

    /// DA1 (and DECID): the conformance level, then the features Terminus implements: 132 columns
    /// (1), sixel graphics (4), selective erase (6), horizontal scrolling (21), ANSI colour (22),
    /// rectangular editing (28) and clipboard access (52). xterm also lists a printer, ReGIS, NRCS, the technical
    /// character set, a locator and user windows; Terminus leaves out what it does not have, because
    /// programs read this list to decide what to send. Level 1 (VT100) has no feature list.
    pub(super) fn reply_da1(&mut self) {
        let reply = match self.vt_level {
            1 => "[?1;2c",
            2 => "[?62;1;4;6;22;52c",
            3 => "[?63;1;4;6;22;52c",
            4 => "[?64;1;4;6;21;22;28;52c",
            _ => "[?65;1;4;6;21;22;28;52c",
        };
        self.reply(reply.as_bytes());
    }

    /// XTSMGRAPHICS (`CSI ? Pi ; Pa ; Pv S`): answers `CSI ? Pi ; Ps ; Pv... S`. Pi 1 is the number of
    /// sixel colour registers (always 256) and Pi 2 the sixel geometry in pixels. Read (Pa 1) and
    /// maximum (Pa 4) succeed, and reset (Pa 2) answers the default, which is the same value. A set
    /// request (Pa 3) fails with Ps 3, because the sizes are fixed. Pi 3 (ReGIS) is not supported.
    fn xtsmgraphics(&mut self, item: u16, action: u16) {
        if !(1..=2).contains(&item) {
            self.reply(format!("[?{item};1S").as_bytes());
            return;
        }
        if !(1..=4).contains(&action) {
            self.reply(format!("[?{item};2S").as_bytes());
            return;
        }
        if action == 3 {
            self.reply(format!("[?{item};3S").as_bytes());
            return;
        }
        let value = if item == 1 { "256".to_string() } else { self.sixel_geometry(action == 4) };
        self.reply(format!("[?{item};0;{value}S").as_bytes());
    }

    /// The `width;height` XTSMGRAPHICS reports for sixel: the maximum an image may be (16384
    /// wide, and as tall as the pixel cap allows), or the size of the window in pixels.
    fn sixel_geometry(&self, maximum: bool) -> String {
        if maximum {
            let height = (self.graphics.max_pixels() / SIXEL_MAX_DIMENSION).clamp(1, SIXEL_MAX_DIMENSION);
            return format!("{SIXEL_MAX_DIMENSION};{height}");
        }
        let (cw, ch) = self.cell_pixel;
        format!("{};{}", self.cols() as u32 * cw as u32, self.rows() as u32 * ch as u32)
    }

    /// DA2: the terminal type for the conformance level (as xterm reports it), a version number
    /// in the range xterm's patch numbers use, and ROM cartridge 0.
    fn reply_da2(&mut self) {
        let kind = match self.vt_level {
            1 => 0,
            2 => 1,
            3 => 24,
            4 => 41,
            _ => 64,
        };
        self.reply(format!("\x1b[>{kind};{DA2_VERSION};0c").as_bytes());
    }

    /// DECSCL: `CSI 61..65 ; Ps " p` sets the conformance level and soft resets the terminal.
    pub(super) fn set_conformance_level(&mut self, seq: &CsiSeq) {
        let ps = seq.params.get(0);
        if !(61..=65).contains(&ps) {
            return;
        }
        self.vt_level = (ps - 60) as u8;
        self.c1_8bit = ps > 61 && seq.params.get(1) != 1;
        self.soft_reset();
    }

    /// DSR: 5 is the status (always OK), 6 the cursor position.
    fn dsr(&mut self, ps: u16) {
        match ps {
            5 => self.reply(b"\x1b[0n"),
            6 => {
                let (row, col) = self.cursor_report_position();
                self.reply(format!("\x1b[{row};{col}R").as_bytes());
            }
            _ => {}
        }
    }

    /// DEC DSR: DECXCPR, printer, UDK, keyboard, locator, macro space and checksum, data
    /// integrity, multiple sessions, and the colour scheme query.
    fn dec_dsr(&mut self, ps: u16, pid: u16) {
        match ps {
            6 => {
                let (row, col) = self.cursor_report_position();
                self.reply(format!("\x1b[?{row};{col};1R").as_bytes());
            }
            15 => self.reply(b"\x1b[?13n"),
            25 => self.reply(b"\x1b[?21n"),
            26 => self.reply(b"\x1b[?27;1;0;0n"),
            55 => self.reply(b"\x1b[?50n"),
            56 => self.reply(b"\x1b[?57;0n"),
            62 => self.reply(b"\x1b[0*{"),
            63 => self.reply(format!("\x1bP{pid}!~0000\x1b\\").as_bytes()),
            75 => self.reply(b"\x1b[?70n"),
            85 => self.reply(b"\x1b[?83n"),
            996 => self.event(Event::ColorSchemeQuery),
            _ => {}
        }
    }

    /// The 1 based cursor position for CPR, relative to the margins in origin mode.
    fn cursor_report_position(&self) -> (u16, u16) {
        let s = self.screen();
        let origin = self.modes.get(Mode::Origin);
        let row = if origin { s.cursor.row.saturating_sub(s.top) } else { s.cursor.row };
        let col = if origin && self.modes.get(Mode::LeftRightMargins) {
            s.cursor.col.saturating_sub(s.left)
        } else {
            s.cursor.col
        };
        (row + 1, col + 1)
    }

    /// DECRQM: reports one mode.
    fn decrqm(&mut self, kind: ModeKind, number: u16) {
        let value = self.modes.report(kind, number);
        let q = if kind == ModeKind::Dec { "?" } else { "" };
        self.reply(format!("\x1b[{q}{number};{value}$y").as_bytes());
    }

    /// Reports the size in cells and pixels for mode 2048.
    pub(super) fn report_size_in_band(&mut self) {
        let (w, h) = self.cell_pixel;
        let (cols, rows) = (self.cols(), self.rows());
        let reply = format!("\x1b[48;{rows};{cols};{};{}t", rows as u32 * h as u32, cols as u32 * w as u32);
        self.reply(reply.as_bytes());
    }

    /// DECRQSS: reports a setting as the sequence that would set it.
    pub(super) fn decrqss(&mut self, request: &[u8]) {
        let s = self.screen();
        let answer = match request {
            b"m" => Some(format!("{}m", sgr_string(&s.cursor.style))),
            b"r" => Some(format!("{};{}r", s.top + 1, s.bottom + 1)),
            b"s" => Some(format!("{};{}s", s.left + 1, s.right + 1)),
            b" q" => Some(format!("{} q", cursor_shape_number(self.cursor_shape))),
            b"\"q" => Some(format!("{}\"q", if s.cursor.protected { 1 } else { 0 })),
            b"\"p" => Some(format!("{};{}\"p", 60 + self.vt_level, if self.c1_8bit { 0 } else { 1 })),
            b"$}" => Some(format!("{}$}}", self.sasd)),
            b"*x" => Some(format!("{}*x", self.sace)),
            b"$~" => Some(format!("{}$~", self.status_line_type)),
            b"t" => Some(format!("{}t", s.rows())),
            b"$|" => Some(format!("{}$|", s.cols())),
            b"*|" => Some(format!("{}*|", s.rows())),
            _ => None,
        };
        match answer {
            Some(a) => self.reply(format!("\x1bP1$r{a}\x1b\\").as_bytes()),
            None => self.reply(b"\x1bP0$r\x1b\\"),
        }
    }

    /// XTGETTCAP: answers terminfo capabilities named in hex, `;` separated.
    pub(super) fn xtgettcap(&mut self, request: &[u8]) {
        for name_hex in request.split(|&b| b == b';') {
            let name = hex_decode(name_hex).and_then(|b| String::from_utf8(b).ok());
            let value = name.as_deref().and_then(|n| TERMINFO_CAPS.iter().find(|(k, _)| *k == n)).map(|(_, v)| *v);
            match value {
                Some(v) => {
                    let hex_name = String::from_utf8_lossy(name_hex).to_ascii_uppercase();
                    let reply = if v.is_empty() {
                        format!("\x1bP1+r{hex_name}\x1b\\")
                    } else {
                        format!("\x1bP1+r{hex_name}={}\x1b\\", hex_encode(v.as_bytes()))
                    };
                    self.reply(reply.as_bytes());
                }
                None => {
                    let reply = format!("\x1bP0+r{}\x1b\\", String::from_utf8_lossy(name_hex));
                    self.reply(reply.as_bytes());
                    break;
                }
            }
        }
    }
}

/// The DECSCUSR number for a shape.
fn cursor_shape_number(shape: CursorShape) -> u8 {
    match shape {
        CursorShape::Default => 0,
        CursorShape::Block { blink } => {
            if blink {
                1
            } else {
                2
            }
        }
        CursorShape::Underline { blink } => {
            if blink {
                3
            } else {
                4
            }
        }
        CursorShape::Bar { blink } => {
            if blink {
                5
            } else {
                6
            }
        }
    }
}

/// The SGR parameters that recreate a style, starting with 0.
pub fn sgr_string(style: &crate::style::Style) -> String {
    let mut parts = vec!["0".to_string()];
    let flags = [
        (attr::BOLD, "1"),
        (attr::FAINT, "2"),
        (attr::ITALIC, "3"),
        (attr::BLINK, "5"),
        (attr::INVERSE, "7"),
        (attr::INVISIBLE, "8"),
        (attr::STRIKE, "9"),
        (attr::OVERLINE, "53"),
    ];
    for (bit, code) in flags {
        if style.has(bit) {
            parts.push(code.to_string());
        }
    }
    match style.underline {
        Underline::None => {}
        Underline::Single => parts.push("4".to_string()),
        Underline::Double => parts.push("4:2".to_string()),
        u => parts.push(format!("4:{}", u as u8)),
    }
    push_color(&mut parts, style.fg, 30, 90, 38);
    push_color(&mut parts, style.bg, 40, 100, 48);
    if style.underline_color != Color::Default {
        push_color(&mut parts, style.underline_color, 0, 0, 58);
    }
    parts.join(";")
}

/// Adds the SGR for a colour: the short codes for the 16 colours where they exist.
fn push_color(parts: &mut Vec<String>, c: Color, base: u16, bright: u16, extended: u16) {
    match c {
        Color::Default => {}
        Color::Palette(i) if i < 8 && base != 0 => parts.push((base + i as u16).to_string()),
        Color::Palette(i) if i < 16 && bright != 0 => parts.push((bright + i as u16 - 8).to_string()),
        Color::Palette(i) => parts.push(format!("{extended}:5:{i}")),
        Color::Rgb(r, g, b) => parts.push(format!("{extended}:2::{r}:{g}:{b}")),
    }
}

/// Decodes a hex string.
fn hex_decode(hex: &[u8]) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.chunks(2).map(|p| u8::from_str_radix(core::str::from_utf8(p).ok()?, 16).ok()).collect()
}

/// Encodes bytes as upper case hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}
