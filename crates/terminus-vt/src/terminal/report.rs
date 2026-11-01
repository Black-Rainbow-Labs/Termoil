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

impl Terminal {
    /// CSI sequences that answer with a report, plus anything unrecognised (ignored).
    pub(super) fn report_dispatch(&mut self, seq: &CsiSeq) {
        let p = &seq.params;
        match (seq.private, seq.intermediates(), seq.final_byte) {
            (0, [], b'c') if p.get(0) == 0 => self.reply(b"\x1b[?62;1;4;6;22;28;52c"),
            (b'>', [], b'c') if p.get(0) == 0 => self.reply_da2(),
            (b'=', [], b'c') if p.get(0) == 0 => self.reply(b"\x1bP!|5445524d\x1b\\"),
            (0, [], b'n') => self.dsr(p.get(0)),
            (b'?', [], b'n') => self.dec_dsr(p.get(0)),
            (0, [b'$'], b'p') => self.decrqm(ModeKind::Ansi, p.get(0)),
            (b'?', [b'$'], b'p') => self.decrqm(ModeKind::Dec, p.get(0)),
            (b'>', [], b'q') => {
                let v = self.config.version_string.clone();
                self.reply(format!("\x1bP>|{v}\x1b\\").as_bytes());
            }
            _ => {}
        }
    }

    /// DA2: terminal type 1, the version as a number, ROM 0.
    fn reply_da2(&mut self) {
        let mut parts = env!("CARGO_PKG_VERSION").split('.').map(|s| s.parse::<u32>().unwrap_or(0));
        let (a, b, c) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        self.reply(format!("\x1b[>1;{};0c", a * 10000 + b * 100 + c).as_bytes());
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

    /// DEC DSR: DECXCPR, printer, UDK, keyboard, and the colour scheme query.
    fn dec_dsr(&mut self, ps: u16) {
        match ps {
            6 => {
                let (row, col) = self.cursor_report_position();
                self.reply(format!("\x1b[?{row};{col};1R").as_bytes());
            }
            15 => self.reply(b"\x1b[?13n"),
            25 => self.reply(b"\x1b[?21n"),
            26 => self.reply(b"\x1b[?27;1;0;0n"),
            996 => self.event(Event::ColorSchemeQuery),
            _ => {}
        }
    }

    /// The 1 based cursor position for CPR, relative to the margins in origin mode.
    fn cursor_report_position(&self) -> (u16, u16) {
        let s = self.screen();
        let origin = self.modes.get(Mode::Origin);
        let row = if origin { s.cursor.row.saturating_sub(s.top) } else { s.cursor.row };
        let col = if origin && self.modes.get(Mode::LeftRightMargins) { s.cursor.col.saturating_sub(s.left) } else { s.cursor.col };
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
            b"\"p" => Some("64;1\"p".to_string()),
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
                    let reply = if v.is_empty() { format!("\x1bP1+r{hex_name}\x1b\\") } else { format!("\x1bP1+r{hex_name}={}\x1b\\", hex_encode(v.as_bytes())) };
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
        CursorShape::Block { blink } => if blink { 1 } else { 2 },
        CursorShape::Underline { blink } => if blink { 3 } else { 4 },
        CursorShape::Bar { blink } => if blink { 5 } else { 6 },
    }
}

/// The SGR parameters that recreate a style, starting with 0.
pub fn sgr_string(style: &crate::style::Style) -> String {
    let mut parts = vec!["0".to_string()];
    let flags = [(attr::BOLD, "1"), (attr::FAINT, "2"), (attr::ITALIC, "3"), (attr::BLINK, "5"), (attr::INVERSE, "7"), (attr::INVISIBLE, "8"), (attr::STRIKE, "9"), (attr::OVERLINE, "53")];
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
    if hex.len() % 2 != 0 {
        return None;
    }
    hex.chunks(2).map(|p| u8::from_str_radix(core::str::from_utf8(p).ok()?, 16).ok()).collect()
}

/// Encodes bytes as upper case hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}
