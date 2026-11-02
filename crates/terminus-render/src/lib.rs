//! Terminus rendering: the render state snapshot, the frame builder, the `Renderer` trait and the
//! two vello renderers behind it (`docs/TDD.md` section 4.7).
//!
//! The pipeline is: [`RenderState::update`] copies what changed under the terminal lock;
//! [`build_frame`] turns it into a [`Frame`] of plain records outside the lock; a [`Renderer`]
//! draws the frame. [`VelloGpuRenderer`] is the window's renderer and [`VelloCpuRenderer`] the
//! fallback with no GPU and the pixel tests' reference. The frame builder knows nothing about
//! either, so a third renderer (the atlas contingency) would only add a drawing backend.

mod build;
mod cpu;
mod draw;
mod frame;
pub mod golden;
mod gpu;
mod readback;
mod state;

use std::time::Duration;

pub use build::{BuildOptions, FrameBuilder, build_frame, contrast};
pub use cpu::VelloCpuRenderer;
pub use draw::Layer;
pub use frame::{
    BgSpan, CursorFrame, CursorStyle, Decoration, DecorationKind, Frame, GlyphPos, GlyphRun, Rgba, RowFrame, SpriteCell,
};
pub use gpu::{GpuContext, VelloGpuRenderer, request_device};
pub use state::{CellState, CellText, CellWidth, Colors, CursorState, RenderState, RowState};

/// An RGBA8 image (straight alpha), what [`Renderer::read_pixels`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels, row major, four bytes each.
    pub rgba: Vec<u8>,
}

/// Timing of one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Time to encode the scene (CPU).
    pub encode: Duration,
    /// Time until the frame was complete, including rasterising (CPU) or the GPU finishing.
    pub total: Duration,
    /// Glyphs drawn.
    pub glyphs: usize,
}

/// Why a frame could not be drawn.
#[derive(Debug)]
pub enum RenderError {
    /// The GPU reported an error (a lost device among them); the app recreates the renderer.
    Gpu(String),
}

impl std::fmt::Display for RenderError {
    /// A one line description.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Gpu(s) => write!(f, "GPU: {s}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// What every renderer does.
pub trait Renderer {
    /// Changes the size of what is drawn.
    fn resize(&mut self, width: u32, height: u32);
    /// Draws one frame filling the target.
    fn render(&mut self, frame: &Frame, grid: &terminus_font::FontGrid) -> Result<FrameStats, RenderError> {
        self.render_layers(frame.background, &[Layer { x: 0.0, y: 0.0, frame }], grid)
    }
    /// Fills the target with `background` and draws the layers at their offsets: the panes of a
    /// split, the tab bar and overlays.
    fn render_layers(
        &mut self, background: Rgba, layers: &[Layer<'_>], grid: &terminus_font::FontGrid,
    ) -> Result<FrameStats, RenderError>;
    /// The last frame's pixels.
    fn read_pixels(&mut self) -> Result<Image, RenderError>;
    /// A name for logs and test output, including the adapter for the GPU renderer.
    fn name(&self) -> String;
}
