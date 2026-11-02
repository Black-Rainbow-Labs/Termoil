//! The window's own text: the tab bar's titles, the configuration error banner and the IME preedit. Each is
//! a small `Terminal` written with escape sequences and built into a frame like a pane, so it gets
//! the same shaping, font fallback and wide character handling as terminal text.

use termoil_font::FontGrid;
use termoil_render::{BuildOptions, Colors, Frame, FrameBuilder, RenderState};
use termoil_vt::style::Rgb;
use termoil_vt::{Terminal, TerminalConfig};

/// A small terminal used to draw one strip of the window.
pub struct Strip {
    term: Terminal,
    state: RenderState,
    builder: FrameBuilder,
    last: Vec<u8>,
    frame: Option<Frame>,
}

impl Strip {
    /// A strip of `rows` rows.
    pub fn new(rows: u16, colors: Colors) -> Strip {
        let config = TerminalConfig {
            scrollback: termoil_vt::pagelist::ScrollbackLimit { bytes: 0, lines: Some(0) },
            ..TerminalConfig::default()
        };
        Strip {
            term: Terminal::new(1, rows, config),
            state: RenderState::new(1, rows, colors),
            builder: FrameBuilder::new(),
            last: Vec::new(),
            frame: None,
        }
    }

    /// The frame for `content` at `cols` columns and `rows` rows, rebuilt only when the content, size
    /// or colours changed.
    pub fn frame(
        &mut self, cols: u16, rows: u16, content: &[u8], colors: &Colors, grid: &mut FontGrid, opts: &BuildOptions,
    ) -> &Frame {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let changed = self.frame.is_none()
            || self.last != content
            || self.term.cols() != cols
            || self.term.rows() != rows
            || self.state.defaults != *colors;
        if changed {
            if self.term.cols() != cols || self.term.rows() != rows {
                self.term.resize(cols, rows);
            }
            self.state.defaults = colors.clone();
            self.state.needs_full = true;
            self.term.advance(b"\x1bc\x1b[?25l\x1b[?7l");
            self.term.advance(content);
            self.state.update(&mut self.term, 0);
            self.frame = Some(self.builder.build(&self.state, grid, opts));
            self.last = content.to_vec();
        }
        self.frame.as_ref().expect("built above")
    }

    /// Sets what the text cannot carry on the frame built last: its full size in pixels, its
    /// background, and the shapes drawn under the text (the tab bar's tabs and buttons). Called after
    /// every `frame`, since the shapes change with the pointer while the text does not.
    pub fn decorate(
        &mut self, width: u32, height: u32, background: termoil_render::Rgba, shapes: Vec<termoil_render::Shape>,
    ) {
        if let Some(f) = self.frame.as_mut() {
            f.width = width;
            f.height = height;
            f.background = background;
            f.shapes = shapes;
        }
    }

    /// The frame built last, if any.
    pub fn frame_ref(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }
}

/// An SGR sequence setting a truecolor foreground and background.
pub fn sgr_colors(fg: Rgb, bg: Rgb) -> String {
    format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg.r, fg.g, fg.b, bg.r, bg.g, bg.b)
}

/// The banner shown while the configuration file has an error.
pub fn banner(message: &str) -> Vec<u8> {
    let fg = Rgb::new(0xff, 0xff, 0xff);
    let bg = Rgb::new(0x9b, 0x1c, 0x1c);
    format!(
        "{}\x1b[K Configuration error, previous settings kept: {}",
        sgr_colors(fg, bg),
        message.replace(['\r', '\n'], " ")
    )
    .into_bytes()
}

/// The banner for a shader that did not compile or could not be read: the window draws without it.
pub fn shader_banner(message: &str) -> Vec<u8> {
    let fg = Rgb::new(0xff, 0xff, 0xff);
    let bg = Rgb::new(0x9b, 0x1c, 0x1c);
    format!("{}\x1b[K Shader error, drawing without it: {}", sgr_colors(fg, bg), message.replace(['\r', '\n'], " "))
        .into_bytes()
}

/// The IME preedit: the composing text, underlined, in the pane colours.
pub fn preedit(text: &str, colors: &Colors) -> Vec<u8> {
    format!("{}\x1b[4m{}", sgr_colors(colors.foreground, colors.background), text).into_bytes()
}
