//! Deterministic generators for the benchmark streams.
//!
//! `mixed` is the P1 gate stream: what a terminal running agents and builds actually receives.
//! Most lines are plain words; about one in eight carries SGR colours; about one in twenty has
//! UTF-8 (accents, CJK, an emoji); one in sixteen is a progress bar redrawn with CR; some lines
//! are long enough to wrap. The other scenarios follow vtebench's names and stress one thing each.

/// A small fast generator so streams are the same on every run.
struct Rng(u64);

impl Rng {
    /// The next pseudo random number.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const WORDS: &[&str] = &[
    "the",
    "compiling",
    "terminus",
    "warning:",
    "error",
    "src/main.rs",
    "Finished",
    "release",
    "target",
    "in",
    "1.23s",
    "test",
    "ok",
    "passed",
    "--> crates/vt/src/page.rs:42:7",
    "fn",
    "let",
    "mut",
    "self",
    "=",
    "{",
    "}",
    "0x1f",
    "Downloaded",
    "serde",
    "v1.0.229",
    "|",
    "^^^^",
    "note:",
    "#[derive(Debug)]",
];

const UNICODE: &[&str] =
    &["café", "naïve", "über", "中文字符", "日本語のテキスト", "한국어", "✓", "→", "😀", "Ωmega", "αβγ"];

/// One line of the mixed stream.
fn mixed_line(r: &mut Rng, out: &mut Vec<u8>, k: Knobs) {
    let kind = r.below(16);
    if kind == 0 && k.progress {
        let pct = r.below(101);
        let filled = (pct / 5) as usize;
        out.extend_from_slice(format!("\r[{}{}] {pct:>3}%", "#".repeat(filled), " ".repeat(20 - filled)).as_bytes());
        return;
    }
    let long = r.below(10) == 0;
    let len = 3 + r.below(if long { 40 } else { 14 });
    for i in 0..len {
        if i > 0 {
            out.push(b' ');
        }
        if r.below(8) == 0 && k.sgr {
            out.extend_from_slice(
                format!("\x1b[{};{}m", 30 + r.below(8), if r.below(2) == 0 { 1 } else { 22 }).as_bytes(),
            );
        }
        if r.below(20) == 0 && k.unicode {
            out.extend_from_slice(UNICODE[r.below(UNICODE.len() as u64) as usize].as_bytes());
        } else {
            out.extend_from_slice(WORDS[r.below(WORDS.len() as u64) as usize].as_bytes());
        }
        if r.below(8) == 0 && k.sgr {
            out.extend_from_slice(b"\x1b[0m");
        }
    }
    out.extend_from_slice(if r.below(4) == 0 { b"\r\n" } else { b"\n" });
}

/// Which parts of the mixed stream are on, for finding where time goes.
#[derive(Clone, Copy)]
pub struct Knobs {
    sgr: bool,
    unicode: bool,
    progress: bool,
}

/// The mixed stream, about `size` bytes.
pub fn mixed(size: usize) -> Vec<u8> {
    mixed_with(size, Knobs { sgr: true, unicode: true, progress: true })
}

/// The mixed stream with some parts switched off.
pub fn mixed_with(size: usize, k: Knobs) -> Vec<u8> {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let mut out = Vec::with_capacity(size + 512);
    out.extend_from_slice(b"\x1b[20h");
    while out.len() < size {
        mixed_line(&mut r, &mut out, k);
    }
    out
}

/// Plain ASCII lines scrolling the screen.
fn scrolling(size: usize) -> Vec<u8> {
    let mut r = Rng(7);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        let n = 20 + r.below(150) as usize;
        for _ in 0..n {
            out.push(b'a' + r.below(26) as u8);
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Every cell with its own colours, redrawn from the home position.
fn dense_cells(size: usize) -> Vec<u8> {
    let mut r = Rng(11);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        out.extend_from_slice(b"\x1b[H");
        for _ in 0..200 * 60 {
            out.extend_from_slice(
                format!("\x1b[38;5;{};48;5;{};1;4m{}", r.below(256), r.below(256), (b'A' + r.below(26) as u8) as char)
                    .as_bytes(),
            );
        }
    }
    out
}

/// Text scrolling inside a scrolling region.
fn scrolling_in_region(size: usize) -> Vec<u8> {
    let mut out = b"\x1b[5;55r\x1b[55;1H".to_vec();
    out.extend(scrolling(size));
    out
}

/// Wide and combining characters.
fn unicode(size: usize) -> Vec<u8> {
    let mut r = Rng(13);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        for _ in 0..30 {
            out.extend_from_slice(UNICODE[r.below(UNICODE.len() as u64) as usize].as_bytes());
            out.push(b' ');
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Cursor movement to random cells with one character each.
fn cursor_motion(size: usize) -> Vec<u8> {
    let mut r = Rng(17);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        out.extend_from_slice(
            format!("\x1b[{};{}H{}", 1 + r.below(60), 1 + r.below(200), (b'a' + r.below(26) as u8) as char).as_bytes(),
        );
    }
    out
}

/// Erase and redraw of whole lines, as full screen programs do.
fn erase_redraw(size: usize) -> Vec<u8> {
    let mut r = Rng(19);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        out.extend_from_slice(format!("\x1b[{};1H\x1b[2K", 1 + r.below(60)).as_bytes());
        for _ in 0..120 {
            out.push(b'a' + r.below(26) as u8);
        }
    }
    out
}

/// Every scenario with its stream.
pub fn scenarios() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("mixed", mixed(64 << 20)),
        ("scrolling", scrolling(16 << 20)),
        ("dense_cells", dense_cells(16 << 20)),
        ("scrolling_in_region", scrolling_in_region(16 << 20)),
        ("unicode", unicode(16 << 20)),
        ("cursor_motion", cursor_motion(16 << 20)),
        ("erase_redraw", erase_redraw(16 << 20)),
        ("mixed_plain", mixed_with(32 << 20, Knobs { sgr: false, unicode: false, progress: false })),
        ("mixed_sgr", mixed_with(32 << 20, Knobs { sgr: true, unicode: false, progress: false })),
        ("mixed_unicode", mixed_with(32 << 20, Knobs { sgr: false, unicode: true, progress: false })),
        ("mixed_progress", mixed_with(32 << 20, Knobs { sgr: false, unicode: false, progress: true })),
        (
            "micro_line60_lf",
            [
                b"x".repeat(60),
                b"
"
                .to_vec(),
            ]
            .concat()
            .repeat(256 << 10),
        ),
        (
            "micro_line60_lnm",
            [
                b"[20h".to_vec(),
                [
                    b"x".repeat(60),
                    b"
"
                    .to_vec(),
                ]
                .concat()
                .repeat(256 << 10),
            ]
            .concat(),
        ),
        (
            "micro_words_lf",
            b"the compiling terminus warning: error src/main.rs Finished x
"
            .repeat(256 << 10),
        ),
        ("micro_sgr_pair", b"[31mab[0mcd".repeat(1 << 20)),
        ("micro_sgr_same", b"[31mab[31mcd".repeat(1 << 20)),
        ("micro_print_only", b"abcd".repeat(4 << 20)),
        ("micro_lf", b"\n".repeat(16 << 20)),
        ("micro_short_lines", b"abc\r\n".repeat(4 << 20)),
        ("micro_wrap_ascii", b"abcdefghij".repeat(1600 << 10)),
        ("micro_sgr", b"\x1b[31m".repeat(3 << 20)),
        ("micro_crlf", b"\r\n".repeat(8 << 20)),
        ("micro_cr", b"\r".repeat(16 << 20)),
        ("micro_print_cr", b"abc\r".repeat(4 << 20)),
        ("micro_print_home", b"abc\x1b[H".repeat(3 << 20)),
    ]
}
