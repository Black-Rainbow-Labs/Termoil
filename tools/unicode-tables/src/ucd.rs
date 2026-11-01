//! Parsing of the vendored Unicode Character Database files into one value per code point.

use std::fs;
use std::path::Path;

/// Number of code points in the Unicode code space.
pub const CODE_POINTS: usize = 0x11_0000;

/// General_Category abbreviations in the order of their index; index 0 is Cn, the default.
pub const GC_NAMES: &[&str] = &[
    "Cn", "Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Mc", "Me", "Nd", "Nl", "No", "Pc", "Pd", "Ps", "Pe",
    "Pi", "Pf", "Po", "Sm", "Sc", "Sk", "So", "Zs", "Zl", "Zp", "Cc", "Cf", "Cs", "Co",
];
/// East_Asian_Width values in the order of their index; index 0 (N) is the default.
pub const EAW_NAMES: &[&str] = &["N", "A", "Na", "H", "W", "F"];
/// Grapheme_Cluster_Break values in the order of the `GraphemeBreak` enum; index 0 (Other) is the default.
pub const GBP_NAMES: &[&str] = &[
    "Other",
    "CR",
    "LF",
    "Control",
    "Extend",
    "ZWJ",
    "Regional_Indicator",
    "Prepend",
    "SpacingMark",
    "L",
    "V",
    "T",
    "LV",
    "LVT",
];
/// Indic_Conjunct_Break values in the order of the `IndicConjunctBreak` enum; index 0 (None) is the default.
pub const INCB_NAMES: &[&str] = &["None", "Linker", "Consonant", "Extend"];

/// The per code point properties the tables are built from.
pub struct Ucd {
    pub gc: Vec<u8>,
    pub eaw: Vec<u8>,
    pub gbp: Vec<u8>,
    pub incb: Vec<u8>,
    pub ext_pict: Vec<bool>,
    pub emoji_pres: Vec<bool>,
}

/// Reads every data file under `dir` and returns the properties of all code points.
pub fn load(dir: &Path) -> Ucd {
    let read = |name: &str| {
        fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("cannot read {name}: {e}"))
    };
    Ucd {
        gc: fill_values(&read("DerivedGeneralCategory.txt"), None, GC_NAMES),
        eaw: fill_values(&read("EastAsianWidth.txt"), None, EAW_NAMES),
        gbp: fill_values(&read("GraphemeBreakProperty.txt"), None, GBP_NAMES),
        incb: fill_values(&read("DerivedCoreProperties.txt"), Some("InCB"), INCB_NAMES),
        ext_pict: fill_flags(&read("emoji-data.txt"), "Extended_Pictographic"),
        emoji_pres: fill_flags(&read("emoji-data.txt"), "Emoji_Presentation"),
    }
}

/// Splits a data line into its trimmed fields, dropping the comment. Returns None for blank lines.
fn split_fields(line: &str) -> Option<Vec<&str>> {
    let body = line.split('#').next().unwrap_or("").trim();
    if body.is_empty() {
        return None;
    }
    Some(body.split(';').map(str::trim).collect())
}

/// Parses `XXXX` or `XXXX..YYYY` into an inclusive code point range.
fn parse_range(text: &str) -> (usize, usize) {
    let mut parts = text.split("..");
    let lo = usize::from_str_radix(parts.next().unwrap(), 16)
        .unwrap_or_else(|_| panic!("bad code point {text}"));
    let hi = parts
        .next()
        .map_or(lo, |h| usize::from_str_radix(h, 16).unwrap());
    (lo, hi)
}

/// Builds a per code point value index from a file whose lines are `range ; value` (or `range ; property ; value`
/// when `property` is given). A value missing from `names` is an error so that a new Unicode value is noticed.
fn fill_values(text: &str, property: Option<&str>, names: &[&str]) -> Vec<u8> {
    let mut out = vec![0u8; CODE_POINTS];
    for fields in text.lines().filter_map(split_fields) {
        let value = match property {
            Some(p) if fields.len() >= 3 && fields[1] == p => fields[2],
            Some(_) => continue,
            None => fields[1],
        };
        let index = names
            .iter()
            .position(|n| *n == value)
            .unwrap_or_else(|| panic!("unknown value {value}"));
        let (lo, hi) = parse_range(fields[0]);
        out[lo..=hi].fill(index as u8);
    }
    out
}

/// Builds a per code point flag from an emoji-data style file whose lines are `range ; property`.
fn fill_flags(text: &str, property: &str) -> Vec<bool> {
    let mut out = vec![false; CODE_POINTS];
    for fields in text.lines().filter_map(split_fields) {
        if fields.len() >= 2 && fields[1] == property {
            let (lo, hi) = parse_range(fields[0]);
            out[lo..=hi].fill(true);
        }
    }
    out
}
