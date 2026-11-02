//! Shaping runs of cell text with harfrust, and the run cache.
//!
//! A run is the text of consecutive cells with one face and style. Its shaped glyphs carry the
//! byte offset of the cluster they came from, which the frame builder maps back to cells; a
//! ligature that spans cells is one glyph whose cluster is its first cell. Runs repeat a lot in a
//! terminal (prompts, borders, the same log line), so results are cached by face and text.

use std::collections::HashMap;
use std::sync::Arc;

use harfrust::{Feature, ShaperData, ShaperInstance, Tag, UnicodeBuffer};

use crate::face::Face;

/// One shaped glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Glyph id in the face.
    pub id: u32,
    /// Byte offset in the run's text of the first character this glyph draws.
    pub cluster: u32,
    /// Horizontal advance in pixels.
    pub advance: f32,
    /// Horizontal offset from the pen position, in pixels.
    pub x_offset: f32,
    /// Vertical offset from the baseline, in pixels (up is positive, as in fonts).
    pub y_offset: f32,
}

/// The glyphs of one run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    /// Glyphs in visual order.
    pub glyphs: Vec<ShapedGlyph>,
}

/// A face's identity (`Face::key`): the file, the face in it and its variation coordinates.
type FaceKey = (usize, u32, u64);

/// A cached run: the face, ligatures on or off, the size in pixels as bits, and the text.
type RunKey = (FaceKey, bool, u32, String);

/// The run cache.
pub struct RunCache {
    entries: HashMap<RunKey, Arc<ShapedRun>>,
    limit: usize,
    hits: u64,
    misses: u64,
    data: HashMap<(usize, u32), Arc<ShaperData>>,
    instances: HashMap<FaceKey, Arc<ShaperInstance>>,
}

impl RunCache {
    /// An empty cache holding up to `limit` runs.
    pub fn new(limit: usize) -> RunCache {
        RunCache {
            entries: HashMap::new(),
            limit: limit.max(16),
            hits: 0,
            misses: 0,
            data: HashMap::new(),
            instances: HashMap::new(),
        }
    }

    /// Hits and misses so far.
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// The shaped glyphs of `text` in `face`, from the cache when possible.
    pub fn shape(&mut self, face: &Face, text: &str, ligatures: bool, size_px: f32) -> Arc<ShapedRun> {
        let key = (face.key(), ligatures, size_px.to_bits(), text.to_string());
        if let Some(run) = self.entries.get(&key) {
            self.hits += 1;
            return run.clone();
        }
        self.misses += 1;
        let run = Arc::new(self.shape_uncached(face, text, ligatures, size_px));
        if self.entries.len() >= self.limit {
            // Dropping everything is simpler than tracking age and costs one reshape per run.
            self.entries.clear();
        }
        self.entries.insert(key, run.clone());
        run
    }

    /// Shapes without the cache.
    fn shape_uncached(&mut self, face: &Face, text: &str, ligatures: bool, size_px: f32) -> ShapedRun {
        let Some(font) = face.font_ref() else { return ShapedRun::default() };
        let data_key = (face.blob.as_ref().as_ptr() as usize, face.index);
        let data = self.data.entry(data_key).or_insert_with(|| Arc::new(ShaperData::new(&font))).clone();
        let instance = (!face.coords.is_empty()).then(|| {
            let coords = face.coords.iter().copied();
            self.instances
                .entry(face.key())
                .or_insert_with(|| Arc::new(ShaperInstance::from_coords(&font, coords)))
                .clone()
        });
        let shaper = data.shaper(&font).instance(instance.as_deref()).build();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let features = run_features(ligatures);
        let output = shaper.shape(buffer, harfrust::ShapeOptions::new().features(&features));
        let upem = shaper.units_per_em().max(1) as f32;
        let scale = size_px / upem;
        let glyphs = output
            .glyph_infos()
            .iter()
            .zip(output.glyph_positions())
            .map(|(info, pos)| ShapedGlyph {
                id: info.glyph_id,
                cluster: info.cluster,
                advance: pos.x_advance as f32 * scale,
                x_offset: pos.x_offset as f32 * scale,
                y_offset: pos.y_offset as f32 * scale,
            })
            .collect();
        ShapedRun { glyphs }
    }
}

/// The OpenType features for a run: ligatures on or off.
fn run_features(ligatures: bool) -> Vec<Feature> {
    let value = u32::from(ligatures);
    [b"liga", b"calt", b"clig"].iter().map(|t| Feature::new(Tag::new(t), value, ..)).collect()
}
