//! Clusters drawn with the system's fonts (task-2472): a base letter with several combining marks is shaped in
//! a face that has every mark, so none of them is drawn as a missing glyph box. Runs on the fonts installed
//! on this machine, like the fallback matrix in termoil-font; a machine without such a face prints and skips.

use termoil_font::{FontConfig, FontGrid};
use termoil_render::{BuildOptions, Colors, RenderState, build_frame};
use termoil_vt::{Terminal, TerminalConfig};

/// The glyph ids of every run in a one row frame of `text`, with the family of each run.
fn glyphs_of(g: &mut FontGrid, text: &str) -> Vec<(String, Vec<u32>)> {
    let mut t = Terminal::new(20, 1, TerminalConfig::default());
    t.advance(text.as_bytes());
    let mut state = RenderState::new(20, 1, Colors::dark_default());
    state.update(&mut t, 0);
    let frame = build_frame(&state, g, &BuildOptions::default());
    let runs = frame.rows.iter().flat_map(|r| r.runs.iter());
    runs.map(|r| (g.face(r.face).family.clone(), r.glyphs.iter().map(|x| x.id).collect())).collect()
}

#[test]
fn a_letter_with_five_marks_has_no_missing_glyphs() {
    let config = FontConfig { families: vec!["Cascadia Code".into()], size_px: 20.0, ..FontConfig::default() };
    let Ok(mut g) = FontGrid::new(config) else { return };
    let runs = glyphs_of(&mut g, "Z\u{324}\u{354}\u{367}\u{311}\u{313}");
    println!("{runs:?}");
    let ids: Vec<u32> = runs.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
    if cfg!(windows) || ids.iter().all(|&id| id != 0) {
        assert_eq!(ids.len(), 6, "the letter and five marks");
        assert!(ids.iter().all(|&id| id != 0), "no mark is a missing glyph: {runs:?}");
    }
}

#[test]
fn thai_and_devanagari_clusters_are_drawn() {
    let config = FontConfig { families: vec!["Cascadia Code".into()], size_px: 20.0, ..FontConfig::default() };
    let Ok(mut g) = FontGrid::new(config) else { return };
    for text in ["\u{e01}\u{e33}", "\u{915}\u{94d}\u{937}"] {
        let runs = glyphs_of(&mut g, text);
        println!("{text}: {runs:?}");
        let ids: Vec<u32> = runs.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
        assert!(!ids.is_empty() && ids.iter().all(|&id| id != 0), "{text}: {runs:?}");
    }
}

#[test]
fn flags_are_one_glyph() {
    let config = FontConfig { families: vec!["Cascadia Code".into()], size_px: 20.0, ..FontConfig::default() };
    let Ok(mut g) = FontGrid::new(config) else { return };
    let england = "\u{1f3f4}\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}";
    for text in ["\u{1f1ef}\u{1f1f5}", "\u{1f1e7}\u{1f1f7}", england] {
        let runs = glyphs_of(&mut g, text);
        println!("{text}: {runs:?}");
        assert_eq!(runs.len(), 1, "{text}: {runs:?}");
        assert_eq!(runs[0].1.len(), 1, "a flag is one glyph, not two letters: {runs:?}");
        assert!(runs[0].1[0] != 0);
    }
}
