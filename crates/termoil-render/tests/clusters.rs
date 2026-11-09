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

/// A grid on the test font whose only ligature draws f and i as one glyph one cell wide, the way Menlo's
/// fi ligature does (`corpus/fonts/make_fi_ligature_font.py`).
fn fi_ligature_grid(ligatures: bool) -> FontGrid {
    let font =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fonts/FiLigatureTest-Regular.ttf");
    let config = FontConfig {
        families: vec!["Fi Ligature Test".into()],
        font_files: vec![font],
        system_fonts: false,
        size_px: 20.0,
        ligatures,
        ..FontConfig::default()
    };
    FontGrid::new(config).expect("the fi ligature test font loads")
}

/// The x position of every glyph in a one row frame of `text`.
fn glyph_xs(g: &mut FontGrid, text: &str) -> Vec<f32> {
    let mut t = Terminal::new(20, 1, TerminalConfig::default());
    t.advance(text.as_bytes());
    let mut state = RenderState::new(20, 1, Colors::dark_default());
    state.update(&mut t, 0);
    let frame = build_frame(&state, g, &BuildOptions::default());
    frame.rows.iter().flat_map(|r| r.runs.iter()).flat_map(|r| r.glyphs.iter().map(|x| x.x)).collect()
}

#[test]
fn a_ligature_narrower_than_its_cells_is_not_drawn() {
    // task-2584: Menlo joins f and i into one glyph one cell wide, so the palette drew "confi g" with the
    // i cell left empty. Such a run is shaped again without ligatures: one glyph per cell, none missing.
    let mut g = fi_ligature_grid(true);
    let face = g.primary(termoil_font::FontStyle::Regular);
    assert_eq!(g.shape(face, "config").glyphs.len(), 5, "the test font joins f and i");
    let xs = glyph_xs(&mut g, "show-config");
    let plain = glyph_xs(&mut fi_ligature_grid(false), "show-config");
    assert_eq!(xs.len(), 11, "one glyph per letter: {xs:?}");
    assert_eq!(xs, plain, "placed as with ligatures off");
    let w = g.metrics().width as f32;
    for (i, x) in xs.iter().enumerate() {
        assert!((x - xs[0] - i as f32 * w).abs() < 0.01, "glyph {i} sits in its own cell: {xs:?}");
    }
}
