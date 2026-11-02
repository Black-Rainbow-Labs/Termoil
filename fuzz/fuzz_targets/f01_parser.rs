//! F1: the parser alone, SIMD and scalar paths, which must agree.
#![no_main]
use libfuzzer_sys::fuzz_target;
use termoil_vt::parser::{Action, Parser};

fuzz_target!(|data: &[u8]| {
    let run = |simd: bool| {
        let mut p = Parser::default();
        p.set_simd(simd);
        let mut out = Vec::new();
        p.advance(data, &mut |a: Action<'_>| out.push(format!("{a:?}")));
        out
    };
    assert_eq!(run(true), run(false));
});
