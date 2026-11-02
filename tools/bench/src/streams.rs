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

/// The words the generated streams are made of. "terminus" is the project's name before it became Termoil, and it
/// stays: the gate stream is pinned by length and checksum and every published throughput number was measured on it.
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

/// How many bytes each scenario is fed to: `mixed` to 1 GiB (the P1 gate file), the others to 256 MiB.
pub fn target_bytes(name: &str) -> u64 {
    if name == "mixed" { 1 << 30 } else { 256 << 20 }
}

/// How many times a block of `len` bytes is repeated to come nearest `target` bytes. Rounding to
/// the nearest count makes the `mixed` block (64 MiB and 82 bytes) sixteen copies, the gate file.
pub fn repeat_for(len: usize, target: u64) -> usize {
    let len = len.max(1) as u64;
    ((target + len / 2) / len).max(1) as usize
}

/// What a stream is made of, measured the way `docs/decisions/throughput.md` reports it.
#[derive(Debug, Clone, Copy)]
pub struct Composition {
    pub bytes: usize,
    pub lines: u64,
    pub bytes_per_line: f64,
    pub escapes_per_kb: f64,
    pub utf8_multibyte_per_kb: f64,
    pub fraction_in_escape: f64,
}

/// Counts lines, escape sequences, UTF-8 lead bytes and the bytes inside CSI sequences of `d`.
pub fn composition(d: &[u8]) -> Composition {
    let (mut lf, mut esc, mut multibyte, mut inside) = (0u64, 0u64, 0u64, 0u64);
    let mut i = 0;
    while i < d.len() {
        let b = d[i];
        lf += u64::from(b == b'\n');
        multibyte += u64::from(b >= 0xC0);
        if b == 0x1b {
            esc += 1;
            let start = i;
            i += 1;
            if i < d.len() && d[i] == b'[' {
                i += 1;
                while i < d.len() && !(0x40..=0x7e).contains(&d[i]) {
                    i += 1;
                }
                i += 1;
            }
            inside += (i.min(d.len()) - start) as u64;
            continue;
        }
        i += 1;
    }
    let n = d.len() as f64;
    Composition {
        bytes: d.len(),
        lines: lf,
        bytes_per_line: n / lf.max(1) as f64,
        escapes_per_kb: esc as f64 / (n / 1024.0),
        utf8_multibyte_per_kb: multibyte as f64 / (n / 1024.0),
        fraction_in_escape: inside as f64 / n,
    }
}

/// FNV-1a, 64 bit: a stable checksum written here so the pin needs no dependency.
pub fn fnv1a(d: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in d {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The length of the P1 gate block, `mixed(64 << 20)`. The 1 GiB gate file is this block sixteen times.
    const MIXED_LEN: usize = 67_108_946;
    /// FNV-1a 64 of the P1 gate block.
    const MIXED_FNV1A: u64 = 0xbcf2_8535_9a5a_6081;

    /// The P1 gate stream cannot change without this test failing. The composition is the one in
    /// `docs/decisions/throughput.md`: 94.8 bytes per line, 29.2 escape sequences per KB, 13.3 UTF-8
    /// multibyte sequences per KB, 16.4 percent of bytes inside escape sequences. A change to the
    /// stream is a change to the P1 gate and needs a decision record, not a new pin.
    #[test]
    fn mixed_gate_stream_is_pinned() {
        let d = mixed(64 << 20);
        assert_eq!(d.len(), MIXED_LEN);
        assert_eq!(fnv1a(&d), MIXED_FNV1A, "fnv1a {:#018x}", fnv1a(&d));
        let c = composition(&d);
        assert_eq!(format!("{:.1}", c.bytes_per_line), "94.8");
        assert_eq!(format!("{:.1}", c.escapes_per_kb), "29.2");
        assert_eq!(format!("{:.1}", c.utf8_multibyte_per_kb), "13.3");
        assert_eq!(format!("{:.1}", c.fraction_in_escape * 100.0), "16.4");
    }

    /// The gate file written by `write-stream mixed` is sixteen copies of the block.
    #[test]
    fn mixed_gate_file_is_sixteen_blocks() {
        assert_eq!(repeat_for(MIXED_LEN, target_bytes("mixed")), 16);
    }
}
