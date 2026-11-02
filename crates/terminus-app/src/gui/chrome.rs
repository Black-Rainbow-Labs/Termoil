//! The window's own text: the tab bar, the configuration error banner and the IME preedit. Each is
//! a small `Terminal` written with escape sequences and built into a frame like a pane, so it gets
//! the same shaping, font fallback and wide character handling as terminal text.

use terminus_font::FontGrid;
use terminus_render::{BuildOptions, Colors, Frame, FrameBuilder, RenderState};
use terminus_vt::style::Rgb;
use terminus_vt::{Terminal, TerminalConfig};

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
            scrollback: terminus_vt::pagelist::ScrollbackLimit { bytes: 0, lines: Some(0) },
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

    /// The frame built last, if any.
    pub fn frame_ref(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }
}

/// An SGR sequence setting a truecolor foreground and background.
pub fn sgr_colors(fg: Rgb, bg: Rgb) -> String {
    format!("\x1b[38;2;{};{};{};48;2;{};{};{}m", fg.r, fg.g, fg.b, bg.r, bg.g, bg.b)
}

/// One tab's place in the tab bar, for clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabSpan {
    /// First column.
    pub start: u16,
    /// One past the last column.
    pub end: u16,
}

/// The tab bar's content: each tab as ` n title `, the active one in the pane colours and the rest
/// dimmed, the remaining width in the bar colour. Returns the bytes and each tab's columns.
pub fn tab_bar(titles: &[String], active: usize, cols: u16, colors: &Colors) -> (Vec<u8>, Vec<TabSpan>) {
    let bar = crate::gui::look::mix(colors.background, colors.foreground, 0.12);
    let dim = crate::gui::look::mix(colors.foreground, colors.background, 0.35);
    let each = (cols as usize / titles.len().max(1)).clamp(8, 40);
    let mut out = String::new();
    let mut spans = Vec::new();
    let mut col = 0u16;
    for (i, t) in titles.iter().enumerate() {
        let label = truncate(&format!(" {} {} ", i + 1, if t.is_empty() { "Terminus" } else { t }), each);
        let width = label.chars().count() as u16;
        if col + width > cols {
            break;
        }
        let (fg, bg) = if i == active { (colors.foreground, colors.background) } else { (dim, bar) };
        out.push_str(&sgr_colors(fg, bg));
        out.push_str(if i == active { "\x1b[1m" } else { "\x1b[22m" });
        out.push_str(&label);
        spans.push(TabSpan { start: col, end: col + width });
        col += width;
    }
    out.push_str(&sgr_colors(dim, bar));
    out.push_str("\x1b[K");
    (out.into_bytes(), spans)
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

/// The IME preedit: the composing text, underlined, in the pane colours.
pub fn preedit(text: &str, colors: &Colors) -> Vec<u8> {
    format!("{}\x1b[4m{}", sgr_colors(colors.foreground, colors.background), text).into_bytes()
}

/// Cuts a label to `max` characters, ending in an ellipsis when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(2)).collect();
    t.push_str("\u{2026} ");
    t
}
