//! Packing of the properties into a three stage lookup table and rendering of the generated Rust source.

use crate::ucd::{CODE_POINTS, Ucd};
use std::collections::HashMap;

/// Code points per second stage block, as a power of two.
pub const BLOCK_SHIFT: usize = 7;
const BLOCK_SIZE: usize = 1 << BLOCK_SHIFT;

/// Indices into `ucd::GC_NAMES`, `ucd::EAW_NAMES` and `ucd::GBP_NAMES` that the width rules use.
const GC_MN: u8 = 6;
const GC_ME: u8 = 8;
const GC_CF: u8 = 27;
const EAW_W: u8 = 4;
const EAW_F: u8 = 5;
const GBP_REGIONAL_INDICATOR: u8 = 6;

/// Terminal width of one code point: 0, 1 or 2, following the rules in the Terminus design document.
pub fn width_of(cp: usize, u: &Ucd) -> u8 {
    if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
        return 0;
    }
    if cp == 0xAD {
        return 1;
    }
    let gc = u.gc[cp];
    if gc == GC_MN || gc == GC_ME || gc == GC_CF {
        return 0;
    }
    if (0x1160..=0x11FF).contains(&cp) || (0xD7B0..=0xD7FF).contains(&cp) {
        return 0;
    }
    if u.gbp[cp] == GBP_REGIONAL_INDICATOR {
        return 1;
    }
    if u.eaw[cp] == EAW_W || u.eaw[cp] == EAW_F || u.emoji_pres[cp] {
        return 2;
    }
    1
}

/// Packs the properties of one code point into 10 bits: width, grapheme break, InCB, Extended_Pictographic, Emoji_Presentation.
pub fn pack(cp: usize, u: &Ucd) -> u16 {
    u16::from(width_of(cp, u))
        | u16::from(u.gbp[cp]) << 2
        | u16::from(u.incb[cp]) << 6
        | u16::from(u.ext_pict[cp]) << 8
        | u16::from(u.emoji_pres[cp]) << 9
}

/// The three stages: block ids per block of code points, deduplicated blocks of value indexes, and the distinct packed values.
pub struct Tables {
    pub stage1: Vec<u32>,
    pub stage2: Vec<u32>,
    pub values: Vec<u32>,
    pub ascii: Vec<u32>,
}

/// Builds the lookup stages for all code points, numbering blocks and values in order of first appearance.
pub fn build(u: &Ucd) -> Tables {
    let mut value_ids: HashMap<u16, u32> = HashMap::new();
    let mut values: Vec<u32> = Vec::new();
    let mut block_ids: HashMap<Vec<u32>, u32> = HashMap::new();
    let (mut stage1, mut stage2) = (Vec::new(), Vec::new());
    for start in (0..CODE_POINTS).step_by(BLOCK_SIZE) {
        let block: Vec<u32> = (start..start + BLOCK_SIZE)
            .map(|cp| {
                let packed = pack(cp, u);
                *value_ids.entry(packed).or_insert_with(|| {
                    values.push(u32::from(packed));
                    values.len() as u32 - 1
                })
            })
            .collect();
        let next_id = block_ids.len() as u32;
        let id = *block_ids.entry(block.clone()).or_insert_with(|| {
            stage2.extend_from_slice(&block);
            next_id
        });
        stage1.push(id);
    }
    assert!(
        values.len() <= 256,
        "more than 256 distinct property values"
    );
    let ascii = (0..0x80).map(|cp| u32::from(pack(cp, u))).collect();
    Tables {
        stage1,
        stage2,
        values,
        ascii,
    }
}

/// Smallest unsigned integer type name that holds every entry.
fn int_type(entries: &[u32]) -> &'static str {
    let max = entries.iter().copied().max().unwrap_or(0);
    if max < 256 { "u8" } else { "u16" }
}

/// Renders one static array with entries wrapped at about 110 columns. `ty` forces the element type when given.
fn render_array(name: &str, entries: &[u32], ty: Option<&'static str>) -> String {
    let ty = ty.unwrap_or_else(|| int_type(entries));
    let mut out = format!(
        "#[rustfmt::skip]\npub(super) static {name}: [{ty}; {}] = [\n",
        entries.len()
    );
    let mut line = String::new();
    for e in entries {
        let item = format!("{e},");
        if line.len() + item.len() > 110 {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        line.push_str(&item);
    }
    out.push_str(&line);
    out.push_str("\n];\n");
    out
}

/// Renders the whole generated source file for the given tables and Unicode version text.
pub fn render(t: &Tables, version: &str) -> String {
    let mut out = String::new();
    out.push_str("// Generated file. Do not edit by hand.\n");
    out.push_str("// Regenerate with: cargo run -p unicode-tables -- generate\n");
    out.push_str(&format!(
        "// Source data: corpus/unicode/{version}/ (Unicode {version}).\n"
    ));
    out.push_str("// Packed value: bits 0..1 width, 2..5 grapheme break, 6..7 InCB, 8 Extended_Pictographic, 9 Emoji_Presentation.\n\n");
    out.push_str(&format!(
        "pub(super) const DATA_VERSION: &str = \"{version}\";\n"
    ));
    out.push_str(&format!(
        "pub(super) const BLOCK_SHIFT: usize = {BLOCK_SHIFT};\n\n"
    ));
    let arrays = [
        ("STAGE1", &t.stage1, None),
        ("STAGE2", &t.stage2, Some("u8")),
        ("VALUES", &t.values, Some("u16")),
        ("ASCII", &t.ascii, Some("u16")),
    ];
    for (name, entries, ty) in arrays {
        out.push_str(&render_array(name, entries, ty));
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}
