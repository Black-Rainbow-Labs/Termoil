//! termoil-font: metrics and shaping on the vendored JetBrains Mono, and the system fallback
//! matrix (docs/TDD.md section 10, question 4) on this machine's installed fonts.

use std::path::PathBuf;

use termoil_font::{FontConfig, FontGrid, FontStyle, Presentation};

/// The vendored JetBrains Mono files.
fn vendored() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fonts");
    ["Regular", "Bold", "Italic", "BoldItalic"].iter().map(|s| dir.join(format!("JetBrainsMono-{s}.ttf"))).collect()
}

/// A grid on JetBrains Mono only, so results do not depend on the machine.
fn jbm(size_px: f32, ligatures: bool) -> FontGrid {
    let config = FontConfig {
        families: vec!["JetBrains Mono".into()],
        font_files: vendored(),
        system_fonts: false,
        size_px,
        ligatures,
        ..FontConfig::default()
    };
    FontGrid::new(config).expect("JetBrains Mono loads")
}

#[test]
fn metrics_are_whole_pixels_and_sane() {
    for size in [10.0, 13.0, 17.33, 24.0, 32.0] {
        let g = jbm(size, true);
        let m = g.metrics();
        assert!(m.width >= 1 && m.height > m.width, "size {size}: {m:?}");
        assert!((m.width as f32 - size * 0.6).abs() <= 1.0, "JetBrains Mono advance is 600 units per 1000: {m:?}");
        assert!(m.baseline > 0.0 && m.baseline < m.height as f32);
        assert!(m.underline_position > m.baseline && m.underline_position < m.height as f32 + 1.0);
        assert!(m.strike_position < m.baseline);
        assert!(m.underline_thickness >= 1.0 && m.line_thickness >= 1);
    }
}

#[test]
fn styles_resolve_to_four_faces() {
    let g = jbm(16.0, true);
    let ids: Vec<_> = [FontStyle::Regular, FontStyle::Bold, FontStyle::Italic, FontStyle::BoldItalic]
        .iter()
        .map(|&s| g.primary(s))
        .collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 4, "{ids:?}");
    assert!(!g.face(g.primary(FontStyle::Bold)).synthetic_bold);
}

#[test]
fn ligatures_shape_to_fewer_glyph_ids_when_on() {
    let mut on = jbm(16.0, true);
    let mut off = jbm(16.0, false);
    let face = on.primary(FontStyle::Regular);
    let a = on.shape(face, "=> != ===");
    let b = off.shape(face, "=> != ===");
    assert_eq!(a.glyphs.len(), b.glyphs.len(), "JetBrains Mono ligatures keep one glyph per cell");
    let changed = a.glyphs.iter().zip(&b.glyphs).filter(|(x, y)| x.id != y.id).count();
    assert!(changed > 0, "with ligatures on some glyphs are replaced");
    for g in a.glyphs.iter() {
        assert!((g.advance - a.glyphs[0].advance).abs() < 0.01, "monospace advances");
    }
}

#[test]
fn run_cache_hits_on_repeat() {
    let mut g = jbm(16.0, true);
    let face = g.primary(FontStyle::Regular);
    let first = g.shape(face, "cargo build --release");
    let second = g.shape(face, "cargo build --release");
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(g.run_cache_stats(), (1, 1));
    assert_eq!(first.glyphs.len(), "cargo build --release".len());
    assert!(first.glyphs.windows(2).all(|w| w[0].cluster < w[1].cluster));
}

#[test]
fn missing_glyph_without_system_fonts_stays_on_primary() {
    let mut g = jbm(16.0, true);
    let face = g.face_for('中', FontStyle::Regular, Presentation::Text);
    assert_eq!(face, g.primary(FontStyle::Regular));
    assert!(g.glyph(face, '中').is_none());
}

/// The fallback matrix: with the system's fonts, every sample finds a face that has it. The
/// result is printed so the decision record can quote it.
#[test]
fn system_fallback_matrix() {
    let config =
        FontConfig { families: vec!["JetBrains Mono".into()], font_files: vendored(), ..FontConfig::default() };
    let mut g = FontGrid::new(config).expect("system collection loads");
    let samples = [
        ('中', "Han"),
        ('あ', "Hiragana"),
        ('한', "Hangul"),
        ('Ж', "Cyrillic"),
        ('α', "Greek"),
        ('ע', "Hebrew"),
        ('ع', "Arabic"),
        ('ก', "Thai"),
        ('क', "Devanagari"),
        ('😀', "Emoji"),
        ('✓', "Symbol"),
        ('⌘', "Technical"),
    ];
    let mut missing = Vec::new();
    for (c, name) in samples {
        let presentation = if name == "Emoji" { Presentation::Emoji } else { Presentation::Text };
        let face = g.face_for(c, FontStyle::Regular, presentation);
        let found = g.glyph(face, c).is_some();
        eprintln!("{name:<11} {c} -> {} ({})", g.face(face).family, if found { "found" } else { "MISSING" });
        if !found {
            missing.push(name);
        }
    }
    if std::env::var("CI").is_err() {
        assert!(missing.is_empty(), "no fallback face on this machine for {missing:?}");
    }
}

/// The symbols Claude Code draws its screen with (task-2468): every one is found in a text face, never
/// the colour emoji face. Before, ⏺ and ✳ came out as colour emoji, and ✢, ⏵, ⧉ and the combining
/// U+0310 as missing glyph boxes, because the emoji face was tried before the system fallback and the
/// system fallback for symbols did not reach Segoe UI Symbol or Cambria Math.
#[test]
fn claude_code_symbols_are_drawn_as_text() {
    let config =
        FontConfig { families: vec!["JetBrains Mono".into()], font_files: vendored(), ..FontConfig::default() };
    let mut g = FontGrid::new(config).expect("system collection loads");
    let mut wrong = Vec::new();
    for c in ['⏺', '⎿', '✻', '✶', '✳', '✢', '⏵', '⧉', '↯', '\u{0310}'] {
        let face = g.face_for(c, FontStyle::Regular, Presentation::Text);
        let f = g.face(face);
        eprintln!("U+{:04X} -> {} (colour {})", c as u32, f.family, f.color);
        if g.glyph(face, c).is_none() || f.color {
            wrong.push(format!("U+{:04X} on {} (colour {})", c as u32, f.family, f.color));
        }
    }
    if std::env::var("CI").is_err() {
        assert!(wrong.is_empty(), "not drawn as text: {wrong:?}");
    }
    let emoji = g.face_for('😀', FontStyle::Regular, Presentation::Emoji);
    if std::env::var("CI").is_err() {
        assert!(g.face(emoji).color, "an emoji presentation is drawn by the colour emoji face");
    }
}
