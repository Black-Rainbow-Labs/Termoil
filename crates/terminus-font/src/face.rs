//! Faces: loading the configured families, choosing the primary face per style, and fallback for
//! characters the primary face does not have.

use std::collections::HashMap;

use fontique::{
    Attributes, Blob, Collection, CollectionOptions, FallbackKey, FontStyle as FqStyle, FontWeight, FontWidth,
    QueryStatus, Script, SourceCache,
};
use skrifa::{FontRef, MetadataProvider};
use unicode_script::UnicodeScript;

use crate::{FontConfig, FontError};

/// Index of a face in a grid.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FaceId(pub u16);

/// The four styles a terminal asks for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FontStyle {
    /// Upright, normal weight.
    Regular,
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Bold italic.
    BoldItalic,
}

impl FontStyle {
    /// The style for the bold and italic flags.
    pub fn from_flags(bold: bool, italic: bool) -> FontStyle {
        match (bold, italic) {
            (false, false) => FontStyle::Regular,
            (true, false) => FontStyle::Bold,
            (false, true) => FontStyle::Italic,
            (true, true) => FontStyle::BoldItalic,
        }
    }

    /// The fontique attributes asked for in this style.
    fn attributes(self) -> Attributes {
        let weight =
            if matches!(self, FontStyle::Bold | FontStyle::BoldItalic) { FontWeight::BOLD } else { FontWeight::NORMAL };
        let style =
            if matches!(self, FontStyle::Italic | FontStyle::BoldItalic) { FqStyle::Italic } else { FqStyle::Normal };
        Attributes::new(FontWidth::NORMAL, style, weight)
    }
}

/// How a cluster asks to be drawn: as text, or as a colour emoji (Unicode's Emoji_Presentation, or
/// a VS16 selector after the character).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Presentation {
    /// A text glyph. The colour emoji face is the last fallback.
    Text,
    /// A colour emoji. The colour emoji face is tried before the primary face.
    Emoji,
}

/// One loaded face.
#[derive(Clone, Debug)]
pub struct Face {
    /// The font file's bytes.
    pub blob: Blob<u8>,
    /// Index of the face in a collection file.
    pub index: u32,
    /// The family name the face was found under.
    pub family: String,
    /// The style was asked for but the family has no such face: draw bold by emboldening.
    pub synthetic_bold: bool,
    /// The style was asked for but the family has no such face: draw italic by skewing.
    pub synthetic_italic: bool,
    /// The face has colour glyphs (COLR, CBDT or sbix), so its glyphs are drawn in their own colours.
    pub color: bool,
}

impl Face {
    /// The parsed font.
    pub fn font_ref(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(self.blob.as_ref(), self.index).ok()
    }

    /// The glyph id for `c`, or None when the face lacks it.
    pub fn glyph(&self, c: char) -> Option<u32> {
        let font = self.font_ref()?;
        font.charmap().map(c).map(|g| g.to_u32()).filter(|&g| g != 0)
    }
}

/// Every face a grid has loaded, with the fallback cache.
pub struct FaceSet {
    collection: Collection,
    sources: SourceCache,
    faces: Vec<Face>,
    primary: [FaceId; 4],
    emoji: Option<FaceId>,
    fallback: HashMap<(char, FontStyle, Presentation), FaceId>,
    by_blob: HashMap<(usize, u32), FaceId>,
}

/// Emoji families tried, in order, for emoji presentation characters.
const EMOJI_FAMILIES: &[&str] =
    &["Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", "Twemoji Mozilla", "Noto Emoji"];

/// Families with wide symbol coverage, tried after the system fallback for a character no other face
/// has. The system fallback for symbols and combining marks (scripts Common and Inherited) does not
/// reach these on Windows, which left ✢, ⏵, ⧉ and U+0310 as missing glyph boxes (task-2468).
const SYMBOL_FAMILIES: &[&str] = &[
    "Segoe UI Symbol",
    "Cambria Math",
    "Segoe UI",
    "Segoe UI Historic",
    "Apple Symbols",
    "STIX Two Math",
    "Noto Sans Symbols 2",
    "Noto Sans Symbols",
    "Noto Sans Math",
    "DejaVu Sans",
    "Symbola",
    "Arial Unicode MS",
];

impl FaceSet {
    /// Loads the configured font files and families and picks the primary face for each style.
    pub fn load(config: &FontConfig) -> Result<FaceSet, FontError> {
        let mut collection = Collection::new(CollectionOptions { shared: false, system_fonts: config.system_fonts });
        for path in &config.font_files {
            let bytes = std::fs::read(path).map_err(FontError::Io)?;
            collection.register_fonts(Blob::from(bytes), None);
        }
        let mut set = FaceSet {
            collection,
            sources: SourceCache::default(),
            faces: Vec::new(),
            primary: [FaceId(0); 4],
            emoji: None,
            fallback: HashMap::new(),
            by_blob: HashMap::new(),
        };
        let family =
            set.first_installed(&config.families).ok_or_else(|| FontError::NoFont(config.families.join(", ")))?;
        for (k, style) in
            [FontStyle::Regular, FontStyle::Bold, FontStyle::Italic, FontStyle::BoldItalic].into_iter().enumerate()
        {
            let id = set.query_family(&family, style).ok_or_else(|| FontError::NoFont(family.clone()))?;
            set.primary[k] = id;
        }
        set.emoji = EMOJI_FAMILIES.iter().find_map(|f| set.query_family(f, FontStyle::Regular));
        if let Some(e) = set.emoji {
            set.faces[e.0 as usize].color = true;
        }
        Ok(set)
    }

    /// Number of faces loaded.
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// A face by id.
    pub fn face(&self, id: FaceId) -> &Face {
        &self.faces[id.0 as usize]
    }

    /// The primary face for a style.
    pub fn primary(&self, style: FontStyle) -> FaceId {
        self.primary[style as usize]
    }

    /// The first family name in `names` that the collection has.
    fn first_installed(&mut self, names: &[String]) -> Option<String> {
        names.iter().find(|n| self.collection.family_by_name(n).is_some()).cloned().or_else(|| {
            let mono = self.collection.generic_families(fontique::GenericFamily::Monospace).next()?;
            self.collection.family_name(mono).map(str::to_string)
        })
    }

    /// Loads the face of `family` closest to `style`, noting when the style must be synthesised.
    fn query_family(&mut self, family: &str, style: FontStyle) -> Option<FaceId> {
        let id = self.collection.family_id(family)?;
        let font = {
            let mut query = self.collection.query(&mut self.sources);
            query.set_families([fontique::QueryFamily::Id(id)]);
            query.set_attributes(style.attributes());
            query.first_available_font()?
        };
        let synth_bold = font.synthesis.embolden();
        let synth_italic = font.synthesis.skew().is_some();
        Some(self.intern(font.blob, font.index, family, synth_bold, synth_italic))
    }

    /// Adds a face once, returning the existing id for a face already loaded.
    fn intern(
        &mut self, blob: Blob<u8>, index: u32, family: &str, synthetic_bold: bool, synthetic_italic: bool,
    ) -> FaceId {
        let key = (blob.as_ref().as_ptr() as usize, index);
        if let Some(&id) = self.by_blob.get(&key) {
            return id;
        }
        let id = FaceId(self.faces.len() as u16);
        self.faces.push(Face {
            blob,
            index,
            family: family.to_string(),
            synthetic_bold,
            synthetic_italic,
            color: false,
        });
        self.by_blob.insert(key, id);
        id
    }

    /// The face that draws `c` in `style` and `presentation`, finding and caching a fallback when
    /// needed. An emoji presentation goes to the colour emoji face first; text goes to the primary face,
    /// then the fallbacks, and reaches the colour emoji face only when nothing else has the character.
    pub fn face_for(&mut self, c: char, style: FontStyle, presentation: Presentation, configured: &[String]) -> FaceId {
        let primary = self.primary(style);
        let emoji = self.emoji.filter(|&e| self.face(e).glyph(c).is_some());
        if let (Presentation::Emoji, Some(e)) = (presentation, emoji) {
            return e;
        }
        if self.face(primary).glyph(c).is_some() {
            return primary;
        }
        if let Some(&id) = self.fallback.get(&(c, style, presentation)) {
            return id;
        }
        let found = self.find_fallback(c, style, configured).or(emoji).unwrap_or(primary);
        self.fallback.insert((c, style, presentation), found);
        found
    }

    /// Searches the configured families, the system fallback and the symbol families for a text face
    /// that has `c`. Colour faces are passed over, so a text symbol is never drawn as an emoji when a
    /// text face has it.
    fn find_fallback(&mut self, c: char, style: FontStyle, configured: &[String]) -> Option<FaceId> {
        for name in configured {
            if let Some(id) = self.query_family(name, style).filter(|&id| self.face(id).glyph(c).is_some()) {
                return Some(id);
            }
        }
        if let Some(id) = self.system_fallback(c, style).filter(|&id| Some(id) != self.emoji) {
            return Some(id);
        }
        SYMBOL_FAMILIES.iter().find_map(|name| {
            let id = self.query_family(name, style)?;
            (Some(id) != self.emoji && self.face(id).glyph(c).is_some()).then_some(id)
        })
    }

    /// Asks the platform's fallback list for the character's script for a face that has `c`.
    fn system_fallback(&mut self, c: char, style: FontStyle) -> Option<FaceId> {
        let script = script_of(c);
        let mut hit = None;
        {
            let mut query = self.collection.query(&mut self.sources);
            query.set_attributes(style.attributes());
            query.set_fallbacks(FallbackKey::new(script, None));
            query.matches_with(|font| {
                if font.charmap().is_some_and(|m| m.map(c).is_some_and(|g| g != 0)) {
                    hit = Some((font.blob.clone(), font.index, font.family.0));
                    QueryStatus::Stop
                } else {
                    QueryStatus::Continue
                }
            });
        }
        let (blob, index, family) = hit?;
        let name = self.collection.family_name(family).unwrap_or("fallback").to_string();
        Some(self.intern(blob, index, &name, false, false))
    }
}

/// The fontique script of a character, by its Unicode script property. Characters common to
/// several scripts (punctuation, symbols) use Zyyy.
fn script_of(c: char) -> Script {
    let name = c.script().short_name();
    let mut tag = [b'Z', b'y', b'y', b'y'];
    for (k, b) in name.bytes().take(4).enumerate() {
        tag[k] = b;
    }
    Script::from_bytes(tag)
}
