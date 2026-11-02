//! Termoil fonts: discovery, fallback, cell metrics, shaping with a run cache, and the sprite
//! characters drawn as geometry (`docs/TDD.md` section 4.6).
//!
//! A [`FontGrid`] holds the faces resolved for one configuration (regular, bold, italic, bold
//! italic, the emoji face and every fallback face found so far), the cell metrics, and the caches.
//! Windows that use the same configuration share one grid. Discovery and fallback go through
//! `fontique` (DirectWrite, Core Text or fontconfig), shaping through `harfrust`, and metrics and
//! outlines through `skrifa`. The three share one `read-fonts`, the same one `glifo` uses, so a face
//! is parsed by one library everywhere.

mod face;
mod metrics;
mod shape;
pub mod sprite;

use std::path::PathBuf;
use std::sync::Arc;

pub use face::{Face, FaceId, FontStyle, Presentation};
pub use fontique::Blob;
pub use metrics::CellMetrics;
pub use shape::{ShapedGlyph, ShapedRun};

/// What a [`FontGrid`] is built from.
#[derive(Clone, Debug)]
pub struct FontConfig {
    /// Family names in order of preference; the first one installed is the primary font.
    pub families: Vec<String>,
    /// Families tried, in order, for characters the primary font lacks, before the system fallback.
    pub fallback_families: Vec<String>,
    /// Font size in pixels (points times the display scale times 96 / 72).
    pub size_px: f32,
    /// Programming ligatures (the `calt` and `liga` features).
    pub ligatures: bool,
    /// Families drawn without ligatures even when `ligatures` is on (compared without case).
    pub ligatures_off: Vec<String>,
    /// Multiplier on the font's natural line height.
    pub line_height: f32,
    /// Font files loaded in addition to (or, without `system_fonts`, instead of) the system's.
    pub font_files: Vec<PathBuf>,
    /// Whether to enumerate the fonts installed on the system.
    pub system_fonts: bool,
    /// Shaped runs kept in the run cache before the oldest half is dropped.
    pub run_cache_entries: usize,
}

impl Default for FontConfig {
    /// The PRD's default: the first installed of Cascadia Code, JetBrains Mono, Fira Code, Menlo
    /// and DejaVu Sans Mono at 13 points (17.33 pixels at 96 dpi), ligatures on.
    fn default() -> Self {
        FontConfig {
            families: [
                "Cascadia Code",
                "JetBrains Mono",
                "Fira Code",
                "Menlo",
                "DejaVu Sans Mono",
                "Consolas",
                "Courier New",
            ]
            .map(String::from)
            .to_vec(),
            fallback_families: Vec::new(),
            size_px: 13.0 * 96.0 / 72.0,
            ligatures: true,
            ligatures_off: Vec::new(),
            line_height: 1.0,
            font_files: Vec::new(),
            system_fonts: true,
            run_cache_entries: 8192,
        }
    }
}

/// Why a grid could not be built.
#[derive(Debug)]
pub enum FontError {
    /// None of the configured families, nor any monospace system font, could be loaded.
    NoFont(String),
    /// A configured font file could not be read.
    Io(std::io::Error),
}

impl std::fmt::Display for FontError {
    /// A one line description.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::NoFont(s) => write!(f, "no usable font: {s}"),
            FontError::Io(e) => write!(f, "font file: {e}"),
        }
    }
}

impl std::error::Error for FontError {}

/// The faces, metrics and caches for one font configuration.
pub struct FontGrid {
    faces: face::FaceSet,
    metrics: CellMetrics,
    shaper: shape::RunCache,
    config: FontConfig,
}

impl FontGrid {
    /// Builds a grid: loads the fonts, picks the primary family, and measures the cell.
    pub fn new(config: FontConfig) -> Result<FontGrid, FontError> {
        let faces = face::FaceSet::load(&config)?;
        let metrics =
            metrics::measure(faces.face(faces.primary(FontStyle::Regular)), config.size_px, config.line_height);
        let shaper = shape::RunCache::new(config.run_cache_entries);
        Ok(FontGrid { faces, metrics, shaper, config })
    }

    /// The cell size and decoration positions in pixels.
    pub fn metrics(&self) -> &CellMetrics {
        &self.metrics
    }

    /// The configuration the grid was built from.
    pub fn config(&self) -> &FontConfig {
        &self.config
    }

    /// The font size in pixels.
    pub fn size_px(&self) -> f32 {
        self.config.size_px
    }

    /// A face by id.
    pub fn face(&self, id: FaceId) -> &Face {
        self.faces.face(id)
    }

    /// The face of the primary family for a style.
    pub fn primary(&self, style: FontStyle) -> FaceId {
        self.faces.primary(style)
    }

    /// The face that draws `c` in `style`. An emoji presentation is drawn by the colour emoji face when
    /// it has the character. Otherwise the primary face when it has the character, else a fallback face
    /// (the configured families, the system's fallback for the character's script, the symbol families,
    /// and the colour emoji face last). The answer is cached per character, style and presentation.
    pub fn face_for(&mut self, c: char, style: FontStyle, presentation: Presentation) -> FaceId {
        self.faces.face_for(c, style, presentation, &self.config.fallback_families)
    }

    /// Shapes `text` with face `face`, returning glyphs with their cluster offsets. Runs are cached
    /// by face and text, so a line that repeats costs one lookup.
    pub fn shape(&mut self, face: FaceId, text: &str) -> Arc<ShapedRun> {
        let f = self.faces.face(face);
        let ligatures =
            self.config.ligatures && !self.config.ligatures_off.iter().any(|n| n.eq_ignore_ascii_case(&f.family));
        self.shaper.shape(f, text, ligatures, self.config.size_px)
    }

    /// The glyph id for `c` in `face`, without shaping (for single characters that need none).
    pub fn glyph(&self, face: FaceId, c: char) -> Option<u32> {
        self.faces.face(face).glyph(c)
    }

    /// Number of faces loaded so far (primary styles plus fallbacks found).
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Run cache hits and misses since the grid was built.
    pub fn run_cache_stats(&self) -> (u64, u64) {
        self.shaper.stats()
    }
}
