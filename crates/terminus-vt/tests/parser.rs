//! U1: the parser state machine, table driven.
//!
//! Each case is a byte sequence and the list of actions it must produce, written as short strings.
//! Every case runs through the SIMD front end and the scalar path, each fed whole, one byte at a
//! time, and in seeded random chunks, and all six runs must give the same actions.

use terminus_vt::parser::{Action, Parser, ParserLimits, Terminator};

/// Renders one action as the short string the tables use.
fn render(action: Action<'_>) -> String {
    match action {
        Action::Print(s) | Action::PrintAscii(s) => format!("P({s})"),
        Action::Execute(b) => format!("X({b:02x})"),
        Action::Esc { intermediates, final_byte } => {
            format!("E({}{})", String::from_utf8_lossy(intermediates), final_byte as char)
        }
        Action::Csi(seq) => format!(
            "C({}{:?}{}{})",
            private(seq.private),
            seq.params,
            String::from_utf8_lossy(seq.intermediates()),
            seq.final_byte as char
        ),
        Action::Osc(data, term) => {
            format!("O({}{})", String::from_utf8_lossy(data), if term == Terminator::Bel { ",bel" } else { "" })
        }
        Action::Dcs(seq, data) => format!(
            "D({}{:?}{}{}|{})",
            private(seq.private),
            seq.params,
            String::from_utf8_lossy(seq.intermediates()),
            seq.final_byte as char,
            String::from_utf8_lossy(data)
        ),
        Action::Apc(data) => format!("A({})", String::from_utf8_lossy(data)),
    }
}

/// The private marker as a string, empty when there is none.
fn private(b: u8) -> String {
    if b == 0 { String::new() } else { (b as char).to_string() }
}

/// Feeds `chunks` through a parser and returns the rendered actions with adjacent prints merged.
fn run(chunks: &[&[u8]], simd: bool, limits: ParserLimits) -> Vec<String> {
    let mut parser = Parser::new(limits);
    parser.set_simd(simd);
    let mut out: Vec<String> = Vec::new();
    for chunk in chunks {
        parser.advance(chunk, &mut |a: Action<'_>| {
            let s = render(a);
            if let (Some(last), true) = (out.last_mut(), s.starts_with("P("))
                && last.starts_with("P(")
            {
                last.pop();
                last.push_str(&s[2..]);
                return;
            }
            out.push(s);
        });
    }
    out
}

/// Splits `input` at pseudo random points from a fixed seed.
fn random_chunks(input: &[u8], seed: u64) -> Vec<&[u8]> {
    let mut state = seed | 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let n = 1 + (state % 7) as usize;
        let end = (i + n).min(input.len());
        out.push(&input[i..end]);
        i = end;
    }
    out
}

/// Runs one case every way and checks each way gives `expected`.
fn check_with(input: &[u8], expected: &[&str], limits: ParserLimits) {
    for simd in [true, false] {
        let whole = run(&[input], simd, limits);
        assert_eq!(whole, expected, "whole, simd={simd}, input {:?}", String::from_utf8_lossy(input));
        let bytes: Vec<&[u8]> = input.chunks(1).collect();
        assert_eq!(
            run(&bytes, simd, limits),
            expected,
            "byte at a time, simd={simd}, input {:?}",
            String::from_utf8_lossy(input)
        );
        for seed in [1u64, 7, 42] {
            let chunks = random_chunks(input, seed);
            assert_eq!(
                run(&chunks, simd, limits),
                expected,
                "chunks seed {seed}, simd={simd}, input {:?}",
                String::from_utf8_lossy(input)
            );
        }
    }
}

/// Runs one case with the default limits.
fn check(input: &[u8], expected: &[&str]) {
    check_with(input, expected, ParserLimits::default());
}

#[test]
fn ground_text_and_controls() {
    check(b"hello", &["P(hello)"]);
    check(b"a\nb\rc", &["P(a)", "X(0a)", "P(b)", "X(0d)", "P(c)"]);
    check(b"\x07\x08\x09\x0b\x0c\x0e\x0f", &["X(07)", "X(08)", "X(09)", "X(0b)", "X(0c)", "X(0e)", "X(0f)"]);
    check(b"a\x7fb", &["P(ab)"]);
    check(b"", &[]);
    check("h\u{e9}llo w\u{f6}rld \u{1F600}".as_bytes(), &["P(h\u{e9}llo w\u{f6}rld \u{1F600})"]);
}

#[test]
fn every_c0_in_ground_executes() {
    for b in 0u8..0x20 {
        if b == 0x1b {
            continue;
        }
        let input = [b'a', b, b'b'];
        let exec = format!("X({b:02x})");
        check(&input, &["P(a)", &exec, "P(b)"]);
    }
}

#[test]
fn escape_sequences() {
    check(b"\x1bc", &["E(c)"]);
    check(b"\x1b7\x1b8", &["E(7)", "E(8)"]);
    check(b"\x1b(0", &["E((0)"]);
    check(b"\x1b#8", &["E(#8)"]);
    check(b"\x1b %G", &["E( %G)"]);
    check(b"\x1b !\"#x", &[]);
    check(b"\x1b\x1bc", &["E(c)"]);
    check(b"\x1b\nc", &["X(0a)", "E(c)"]);
    for f in 0x30u8..=0x7e {
        if matches!(f, b'[' | b']' | b'P' | b'X' | b'^' | b'_') {
            continue;
        }
        let input = [0x1b, f];
        let exp = format!("E({})", f as char);
        check(&input, &[&exp]);
    }
}

#[test]
fn csi_basic() {
    check(b"\x1b[m", &["C([]m)"]);
    check(b"\x1b[0m", &["C([[0]]m)"]);
    check(b"\x1b[1;2H", &["C([[1], [2]]H)"]);
    check(b"\x1b[;H", &["C([[0], [0]]H)"]);
    check(b"\x1b[?1049h", &["C(?[[1049]]h)"]);
    check(b"\x1b[>c", &["C(>[]c)"]);
    check(b"\x1b[=c", &["C(=[]c)"]);
    check(b"\x1b[<0;1;2M", &["C(<[[0], [1], [2]]M)"]);
    check(b"\x1b[ q", &["C([] q)"]);
    check(b"\x1b[2 q", &["C([[2]] q)"]);
    check(b"\x1b[!p", &["C([]!p)"]);
    check(b"\x1b[?$p", &["C(?[]$p)"]);
    check(b"\x1b[38:2::255:0:0m", &["C([[38, 2, 0, 255, 0, 0]]m)"]);
    check(b"\x1b[4:3m", &["C([[4, 3]]m)"]);
    check(b"\x1b[38;5;196m", &["C([[38], [5], [196]]m)"]);
}

#[test]
fn csi_every_final_byte() {
    for f in 0x40u8..=0x7e {
        let input = [0x1b, b'[', b'5', f];
        let exp = format!("C([[5]]{})", f as char);
        check(&input, &[&exp]);
    }
}

#[test]
fn csi_c0_executes_inside() {
    check(b"\x1b[1\x0a;2H", &["X(0a)", "C([[1], [2]]H)"]);
    check(b"\x1b[\x08A", &["X(08)", "C([]A)"]);
    for b in [0x07u8, 0x08, 0x09, 0x0a, 0x0d] {
        let input = [0x1b, b'[', b'3', b, b'C'];
        let exec = format!("X({b:02x})");
        check(&input, &[&exec, "C([[3]]C)"]);
    }
}

#[test]
fn csi_cancel_and_restart() {
    check(b"\x1b[12\x18A", &["X(18)", "P(A)"]);
    check(b"\x1b[12\x1aA", &["X(1a)", "P(A)"]);
    check(b"\x1b[12\x1b[3A", &["C([[3]]A)"]);
    check(b"\x1b[1\x7f2A", &["C([[12]]A)"]);
}

#[test]
fn csi_ignore_paths() {
    check(b"\x1b[1?2hX", &["P(X)"]);
    check(b"\x1b[ 1hX", &["P(X)"]);
    check(b"\x1b[!!!pX", &["P(X)"]);
    check(b"\x1b[1;2;?mok", &["P(ok)"]);
}

#[test]
fn csi_limits_clamp() {
    let mut long = b"\x1b[".to_vec();
    for i in 1..=20 {
        long.extend_from_slice(format!("{i};").as_bytes());
    }
    long.push(b'm');
    let expected: Vec<String> = (1..=16).map(|i| format!("[{i}]")).collect();
    check(&long, &[&format!("C([{}]m)", expected.join(", "))]);
    check(b"\x1b[99999999H", &["C([[65535]]H)"]);
    check(b"\x1b[1:2:3:4:5:6:7:8m", &["C([[1, 2, 3, 4, 5, 6]]m)"]);
}

#[test]
fn eight_bit_c1() {
    check(b"\x9b1;2H", &["C([[1], [2]]H)"]);
    check(b"\x9d0;t\x9c", &["O(0;t)"]);
    check(b"\x9d0;t\x07", &["O(0;t,bel)"]);
    check(b"\x84\x85\x88\x8d", &["E(D)", "E(E)", "E(H)", "E(M)"]);
    check(b"\x90qdata\x9c", &["D([]q|data)"]);
    check(b"\x9fGa=q\x9c", &["A(Ga=q)"]);
    // Inside a valid UTF-8 sequence, 0x80 to 0x9F are continuation bytes and therefore text.
    check("\u{100}\u{11b}\u{24c}".as_bytes(), &["P(\u{100}\u{11b}\u{24c})"]);
    check("\u{2500}\u{2502}".as_bytes(), &["P(\u{2500}\u{2502})"]);
}

#[test]
fn osc_terminators() {
    check(b"\x1b]0;title\x07", &["O(0;title,bel)"]);
    check(b"\x1b]0;title\x1b\\", &["O(0;title)"]);
    check(b"\x1b]0;title\x9c", &["O(0;title)"]);
    check("\x1b]2;\u{442}\u{438}\u{442}\u{43b}\x07".as_bytes(), &["O(2;\u{442}\u{438}\u{442}\u{43b},bel)"]);
    // U+045C encodes as D1 9C: the 0x9C is a continuation byte, not ST.
    check("\x1b]2;\u{45c}x\x07".as_bytes(), &["O(2;\u{45c}x,bel)"]);
    check(b"\x1b]8;id=1;http://x\x1b\\link\x1b]8;;\x1b\\", &["O(8;id=1;http://x)", "P(link)", "O(8;;)"]);
    check(b"\x1b]0;a\x18b", &["X(18)", "P(b)"]);
    check(b"\x1b]0;a\nb\x07", &["O(0;ab,bel)"]);
    check(b"\x1b]\x07", &["O(,bel)"]);
}

#[test]
fn osc_over_limit_is_dropped_and_counted() {
    let limits = ParserLimits { osc_bytes: 16, ..ParserLimits::default() };
    check_with(b"\x1b]0;short\x07ok", &["O(0;short,bel)", "P(ok)"], limits);
    check_with(b"\x1b]0;this payload is too long\x07ok", &["P(ok)"], limits);
    let mut p = Parser::new(limits);
    p.advance(b"\x1b]0;this payload is too long\x07", &mut |_a: Action<'_>| {});
    assert_eq!(p.stats().osc_dropped, 1);
}

#[test]
fn dcs_sequences() {
    check(b"\x1bP1$r0m\x1b\\", &["D([[1]]$r|0m)"]);
    check(b"\x1bP+q544e\x1b\\", &["D([]+q|544e)"]);
    check(b"\x1bPq#0;2;0;0;0#0~~\x1b\\", &["D([]q|#0;2;0;0;0#0~~)"]);
    check(b"\x1bPtmux;\x1b\x1b]0;x\x07\x1b\\", &["D([]t|mux;\x1b]0;x\x07)"]);
    check(b"\x1bP>|x\x1b\\", &["D(>[]||x)"]);
    check(b"\x1bP1;2;3qpayload\x9c", &["D([[1], [2], [3]]q|payload)"]);
    check(b"\x1bP1:2?qz\x1b\\X", &["P(X)"]);
    check(b"\x1bPqab\x18c", &["X(18)", "P(c)"]);
}

#[test]
fn apc_sos_pm() {
    check(b"\x1b_Gf=100,a=T;AAAA\x1b\\", &["A(Gf=100,a=T;AAAA)"]);
    check(b"\x1bXsos text\x1b\\ok", &["P(ok)"]);
    check(b"\x1b^pm text\x1b\\ok", &["P(ok)"]);
    check(b"\x98sos\x9cok", &["P(ok)"]);
    check(b"\x9epm\x9cok", &["P(ok)"]);
    let big = format!("\x1b_G{}\x1b\\", "A".repeat(100_000));
    check(big.as_bytes(), &[&format!("A(G{})", "A".repeat(100_000))]);
}

#[test]
fn utf8_errors_become_replacement_characters() {
    check(b"a\xffb", &["P(a\u{FFFD}b)"]);
    check(b"a\xc0\xafb", &["P(a\u{FFFD}\u{FFFD}b)"]);
    check(b"a\xe0\x80\xafb", &["P(a\u{FFFD})", "E(@)", "P(\u{FFFD}b)"]);
    check(b"a\xed\xa0\xa0b", &["P(a\u{FFFD}\u{FFFD}\u{FFFD}b)"]);
    check(b"a\xf5b", &["P(a\u{FFFD}b)"]);
    check(b"a\xf4\xa0\xa0\xa0b", &["P(a\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}b)"]);
    check(b"a\xe2\x82b", &["P(a\u{FFFD}b)"]);
    check(b"a\xe2\x82\nb", &["P(a\u{FFFD})", "X(0a)", "P(b)"]);
    check(b"a\xe2\x82\x1b[mb", &["P(a\u{FFFD})", "C([]m)", "P(b)"]);
    check(b"\xe2\x82\xac", &["P(\u{20ac})"]);
    check(b"\xf0\x9f\x98\x80x", &["P(\u{1F600}x)"]);
}

#[test]
fn split_utf8_across_reads() {
    let mut p = Parser::default();
    let mut out = String::new();
    for chunk in [&b"\xf0"[..], b"\x9f", b"\x98", b"\x80!"] {
        p.advance(chunk, &mut |a: Action<'_>| {
            if let Action::Print(s) | Action::PrintAscii(s) = a {
                out.push_str(s)
            }
        });
    }
    assert_eq!(out, "\u{1F600}!");
    assert!(p.is_ground());
}

#[test]
fn sequences_split_at_every_boundary() {
    let input: &[u8] = b"ab\x1b[1;31mred\x1b[0m\x1b]0;t\x07\x1bP1$rm\x1b\\\x1b_Gi=1\x1b\\\xe2\x82\xac\x9b2J";
    let expected = run(&[input], true, ParserLimits::default());
    for cut in 0..=input.len() {
        let (a, b) = input.split_at(cut);
        assert_eq!(run(&[a, b], true, ParserLimits::default()), expected, "cut at {cut}");
        assert_eq!(run(&[a, b], false, ParserLimits::default()), expected, "cut at {cut} scalar");
    }
}
