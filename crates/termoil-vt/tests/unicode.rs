//! U3: Unicode conformance tests against the vendored Unicode 17.0.0 data files.

use std::fs;
use termoil_vt::unicode::{GraphemeBreaker, char_width, cluster_width, props};

/// Reads a vendored Unicode data file.
fn data(name: &str) -> String {
    let path = format!("{}/../../corpus/unicode/17.0.0/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

/// Parses `XXXX` or `XXXX..YYYY` into an inclusive range of code points.
fn parse_range(text: &str) -> (u32, u32) {
    let mut parts = text.split("..");
    let lo = u32::from_str_radix(parts.next().unwrap(), 16).unwrap();
    let hi = parts.next().map_or(lo, |h| u32::from_str_radix(h, 16).unwrap());
    (lo, hi)
}

/// Returns the `(first, second)` fields of every `range ; value` line of a data file, comments removed.
fn property_lines(name: &str) -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    for line in data(name).lines() {
        let body = line.split('#').next().unwrap().trim();
        let Some((range, value)) = body.split_once(';') else {
            continue;
        };
        let (lo, hi) = parse_range(range.trim());
        out.push((lo, hi, value.trim().to_string()));
    }
    out
}

/// Splits a string into chars from a list of hex code points.
fn chars_of(hex: &[&str]) -> Vec<char> {
    hex.iter().map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap()).collect()
}

/// Splits a sequence into clusters with `GraphemeBreaker`.
fn clusters_of(chars: &[char]) -> Vec<Vec<char>> {
    let mut clusters = vec![vec![chars[0]]];
    let mut breaker = GraphemeBreaker::new(chars[0]);
    for &c in &chars[1..] {
        if breaker.should_break(c) {
            clusters.push(Vec::new());
        }
        clusters.last_mut().unwrap().push(c);
    }
    clusters
}

/// Every line of GraphemeBreakTest.txt must produce exactly the listed boundaries.
#[test]
fn grapheme_break_test_file() {
    let mut failures = Vec::new();
    let mut count = 0;
    for (number, line) in data("GraphemeBreakTest.txt").lines().enumerate() {
        let body = line.split('#').next().unwrap().trim();
        if body.is_empty() {
            continue;
        }
        let mut chars = Vec::new();
        let mut expected = Vec::new();
        for token in body.split_whitespace() {
            match token {
                "\u{f7}" => expected.push(true),
                "\u{d7}" => expected.push(false),
                hex => chars.extend(chars_of(&[hex])),
            }
        }
        let expected = &expected[1..expected.len() - 1];
        let mut breaker = GraphemeBreaker::new(chars[0]);
        let actual: Vec<bool> = chars[1..].iter().map(|&c| breaker.should_break(c)).collect();
        count += 1;
        if actual != expected {
            failures.push(format!("line {}: {line}", number + 1));
        }
    }
    assert!(failures.is_empty(), "{} of {count} lines failed:\n{}", failures.len(), failures.join("\n"));
    assert!(count > 700, "only {count} test lines were read");
}

/// Collects the code points whose General_Category is Mn, Me or Cf.
fn zero_width_categories() -> Vec<(u32, u32)> {
    let wanted = ["Mn", "Me", "Cf"];
    property_lines("DerivedGeneralCategory.txt")
        .into_iter()
        .filter(|(_, _, v)| wanted.contains(&v.as_str()))
        .map(|(l, h, _)| (l, h))
        .collect()
}

/// True when the code point is one of the width 0 groups described in the module documentation.
fn is_zero_width_by_rule(cp: u32, categories: &[(u32, u32)]) -> bool {
    let controls = cp < 0x20 || (0x7F..=0x9F).contains(&cp);
    let hangul = (0x1160..=0x11FF).contains(&cp) || (0xD7B0..=0xD7FF).contains(&cp);
    let combining = categories.iter().any(|&(lo, hi)| (lo..=hi).contains(&cp));
    (controls || hangul || combining) && cp != 0xAD
}

/// East Asian Width W and F code points are 2 (except combining and format characters, which are 0), and
/// Na, H and N code points are 1 unless they are emoji presentation, zero width or control code points.
#[test]
fn east_asian_width_file() {
    let categories = zero_width_categories();
    let mut failures = Vec::new();
    for (lo, hi, class) in property_lines("EastAsianWidth.txt") {
        for cp in lo..=hi {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let zero = is_zero_width_by_rule(cp, &categories);
            let expected = match class.as_str() {
                "W" | "F" if zero => 0,
                "W" | "F" => 2,
                "Na" | "H" | "N" if zero => 0,
                "Na" | "H" | "N" if props(c).emoji_presentation && props(c).width == 2 => 2,
                "Na" | "H" | "N" if (0x1F1E6..=0x1F1FF).contains(&cp) => 1,
                "Na" | "H" | "N" => 1,
                _ => continue,
            };
            if char_width(c) != expected || props(c).width != expected {
                failures.push(format!("U+{cp:04X} class {class}: width {} expected {expected}", char_width(c)));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures, first ones:\n{}",
        failures.len(),
        failures.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Every emoji with Emoji_Presentation=Yes except regional indicators is width 2 on its own.
#[test]
fn emoji_presentation_code_points_are_wide() {
    for (lo, hi, prop) in property_lines("emoji-data.txt") {
        if prop != "Emoji_Presentation" {
            continue;
        }
        for cp in (lo..=hi).filter(|cp| !(0x1F1E6..=0x1F1FF).contains(cp)) {
            assert_eq!(char_width(char::from_u32(cp).unwrap()), 2, "U+{cp:04X}");
        }
    }
}

/// Every fully qualified sequence in emoji-test.txt is one cluster and 2 cells wide.
#[test]
fn emoji_test_fully_qualified_sequences() {
    let mut count = 0;
    let mut failures = Vec::new();
    for line in data("emoji-test.txt").lines() {
        let body = line.split('#').next().unwrap();
        let Some((codes, status)) = body.split_once(';') else {
            continue;
        };
        if status.trim() != "fully-qualified" {
            continue;
        }
        let chars = chars_of(&codes.split_whitespace().collect::<Vec<_>>());
        let clusters = clusters_of(&chars);
        let width = cluster_width(&chars);
        count += 1;
        if clusters.len() != 1 || width != 2 {
            failures.push(format!("{}: {} clusters, width {width}", codes.trim(), clusters.len()));
        }
    }
    assert!(count > 3000, "only {count} sequences were read");
    assert!(
        failures.is_empty(),
        "{} failures, first ones:\n{}",
        failures.len(),
        failures.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// An emoji presentation sequence (VS16) is wide.
#[test]
fn emoji_presentation_sequence_is_wide() {
    assert_eq!(cluster_width(&['\u{2764}', '\u{FE0F}']), 2);
    assert_eq!(cluster_width(&['\u{1F600}']), 2);
}

/// A text presentation sequence (VS15) is narrow, even on an emoji that is wide by default.
#[test]
fn text_presentation_sequence_is_narrow() {
    assert_eq!(cluster_width(&['\u{2764}', '\u{FE0E}']), 1);
    assert_eq!(cluster_width(&['\u{2764}']), 1);
    assert_eq!(cluster_width(&['\u{1F600}', '\u{FE0E}']), 1);
}

/// A ZWJ sequence such as a family is one cluster and one wide cell.
#[test]
fn zwj_sequence_is_one_wide_cell() {
    let family = ['\u{1F468}', '\u{200D}', '\u{1F469}', '\u{200D}', '\u{1F467}', '\u{200D}', '\u{1F466}'];
    assert_eq!(clusters_of(&family).len(), 1);
    assert_eq!(cluster_width(&family), 2);
}

/// A pair of regional indicators is one wide cell, a lone one is narrow, and three form a pair plus a single.
#[test]
fn regional_indicator_pairs_are_one_wide_cell() {
    let flag = ['\u{1F1FA}', '\u{1F1F8}'];
    assert_eq!(clusters_of(&flag).len(), 1);
    assert_eq!(cluster_width(&flag), 2);
    assert_eq!(cluster_width(&flag[..1]), 1);
    assert_eq!(clusters_of(&['\u{1F1FA}', '\u{1F1F8}', '\u{1F1E9}']).len(), 2);
}

/// A combining mark with nothing before it is its own narrow cell.
#[test]
fn lone_combining_mark_is_one_narrow_cell() {
    assert_eq!(char_width('\u{0301}'), 0);
    assert_eq!(cluster_width(&['\u{0301}']), 1);
    assert_eq!(cluster_width(&['e', '\u{0301}']), 1);
    assert_eq!(cluster_width(&['\u{4E2D}', '\u{0301}']), 2);
}

/// Spot checks of the legacy width rules for ASCII, controls, soft hyphen, zero width space and Hangul jamo.
#[test]
fn legacy_widths() {
    assert_eq!(char_width('a'), 1);
    assert_eq!(char_width('\0'), 0);
    assert_eq!(char_width('\u{7F}'), 0);
    assert_eq!(char_width('\u{85}'), 0);
    assert_eq!(char_width('\u{AD}'), 1);
    assert_eq!(char_width('\u{200B}'), 0);
    assert_eq!(char_width('\u{1161}'), 0);
    assert_eq!(char_width('\u{AC00}'), 2);
    assert_eq!(char_width('\u{FF21}'), 2);
    assert_eq!(char_width('\u{00A1}'), 1);
}

/// The generated tables report the same Unicode version as `UNICODE_VERSION`.
#[test]
fn table_version_matches_constant() {
    let (major, minor, patch) = termoil_vt::unicode::UNICODE_VERSION;
    assert_eq!(termoil_vt::unicode::TABLES_DATA_VERSION, format!("{major}.{minor}.{patch}"));
}
