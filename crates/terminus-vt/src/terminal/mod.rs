//! The terminal: two screens, the modes, the palette, the title stack, the graphics store, and
//! the queue of events and replies the host drains.
//!
//! `Terminal::advance` feeds bytes through the parser and applies every action. The terminal is
//! a plain value with no thread or lock; the host decides where it lives and when it is read.

mod colorspec;
mod control;
mod csi;
mod dcs;
mod edit;
mod osc;
mod print;
mod report;
mod segment;
mod sgr;
mod winops;

use crate::graphics::GraphicsStore;
use crate::modes::{Mode, ModeSet};
use crate::pagelist::{ReflowCursor, ScrollbackLimit};
use crate::parser::{Action, Handler, Parser, ParserLimits};
use crate::screen::Screen;
use crate::style::{Rgb, Style, default_palette};

pub use winops::WindowState;

/// Settings a host chooses when it creates a terminal.
#[derive(Clone, Debug)]
pub struct TerminalConfig {
    /// Scrollback budget for the primary screen.
    pub scrollback: ScrollbackLimit,
    /// Parser payload limits.
    pub parser_limits: ParserLimits,
    /// Allow OSC 52 to read the clipboard. Off by default.
    pub allow_osc52_read: bool,
    /// Grapheme clustering (mode 2027) after a reset.
    pub grapheme_clustering: bool,
    /// Reflow wrapped lines when the width changes.
    pub reflow: bool,
    /// Name and version reported by XTVERSION.
    pub version_string: String,
    /// Kitty graphics store quota in bytes.
    pub graphics_quota: usize,
    /// Code points kept in one cell's cluster (the 65th is dropped).
    pub max_cluster_len: usize,
    /// Depth of the title stack (XTWINOPS 22 and 23).
    pub title_stack_depth: usize,
    /// Depth of the XTPUSHSGR stack.
    pub sgr_stack_depth: usize,
    /// Longest OSC 8 URI kept.
    pub max_hyperlink_uri: usize,
    /// Longest title kept, in bytes.
    pub max_title: usize,
    /// Prompt marks kept before the oldest are dropped.
    pub max_marks: usize,
}

impl Default for TerminalConfig {
    /// The defaults from the TDD.
    fn default() -> Self {
        TerminalConfig {
            scrollback: ScrollbackLimit::default(),
            parser_limits: ParserLimits::default(),
            allow_osc52_read: false,
            grapheme_clustering: true,
            reflow: true,
            version_string: format!("Terminus({})", env!("CARGO_PKG_VERSION")),
            graphics_quota: 320 << 20,
            max_cluster_len: 64,
            title_stack_depth: 10,
            sgr_stack_depth: 10,
            max_hyperlink_uri: 4096,
            max_title: 4096,
            max_marks: 100_000,
        }
    }
}

/// OSC 133 prompt marks and command boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptEvent {
    /// `A`: a prompt starts.
    PromptStart,
    /// `B`: the prompt ended and input starts.
    InputStart,
    /// `C`: the command was submitted and output starts.
    OutputStart,
    /// `D`: the command finished, with its exit code if the shell gave one.
    CommandFinished(Option<i32>),
}

/// Something the host should act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// BEL.
    Bell,
    /// OSC 0 or 2.
    Title(String),
    /// OSC 0 or 1.
    IconName(String),
    /// OSC 7 or OSC 1337 CurrentDir: the shell's working directory, as a URL or path.
    WorkingDirectory(String),
    /// OSC 52 write: the selection letters and the decoded data.
    ClipboardWrite { selection: String, data: Vec<u8> },
    /// OSC 52 read request (only when allowed); the host answers with [`Terminal::reply_clipboard`].
    ClipboardRead { selection: String },
    /// OSC 9 or OSC 99 desktop notification.
    Notification { title: String, body: String },
    /// OSC 9;4 progress: state 0 remove, 1 normal, 2 error, 3 indeterminate, 4 paused.
    Progress { state: u8, value: u8 },
    /// OSC 133.
    Prompt(PromptEvent),
    /// A colour in the palette or a dynamic colour changed.
    ColorsChanged,
    /// DECSCUSR changed the cursor shape.
    CursorShapeChanged,
    /// A mode the host cares about changed (mouse, keyboard, paste, focus, 2026).
    ModeChanged(Mode),
    /// The terminal wants the window resized (XTWINOPS 8), in cells.
    ResizeRequest { cols: u16, rows: u16 },
    /// The program asked for the colour scheme (CSI ? 996 n); the host answers.
    ColorSchemeQuery,
    /// An OSC payload, DCS or APC was dropped for being over its limit.
    Dropped,
}

/// DECSCUSR cursor shapes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorShape {
    /// The host's configured default.
    #[default]
    Default,
    /// Block, blinking or steady.
    Block { blink: bool },
    /// Underline.
    Underline { blink: bool },
    /// Bar.
    Bar { blink: bool },
}

/// Dynamic colours set by OSC 10 to 19; None means the host's default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DynamicColors {
    /// OSC 10.
    pub foreground: Option<Rgb>,
    /// OSC 11.
    pub background: Option<Rgb>,
    /// OSC 12.
    pub cursor: Option<Rgb>,
    /// OSC 17.
    pub selection_background: Option<Rgb>,
    /// OSC 19.
    pub selection_foreground: Option<Rgb>,
}

/// One OSC 8 hyperlink.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Hyperlink {
    /// The `id=` parameter, if given.
    pub id: Option<String>,
    /// The URI.
    pub uri: String,
}

/// What an OSC 133 mark records.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkKind {
    /// A prompt started here.
    PromptStart,
    /// A command finished here.
    CommandEnd,
}

/// An OSC 133 mark: a pin on the primary screen and what happened there.
#[derive(Clone, Copy, Debug)]
pub struct Mark {
    /// Where it is.
    pub pin: crate::pagelist::PinId,
    /// What it marks.
    pub kind: MarkKind,
    /// The exit code, for a command end.
    pub exit_code: Option<i32>,
}

/// The terminal.
pub struct Terminal {
    parser: Parser,
    primary: Screen,
    alt: Option<Screen>,
    alt_active: bool,
    modes: ModeSet,
    config: TerminalConfig,
    palette: [Rgb; 256],
    default_colors: DynamicColors,
    colors: DynamicColors,
    title: String,
    icon_name: String,
    title_stack: Vec<winops::TitleEntry>,
    window: WindowState,
    screen_pixels: (u32, u32),
    apply_resize: bool,
    title_modes: u8,
    vt_level: u8,
    c1_8bit: bool,
    sace: u16,
    sasd: u16,
    status_line_type: u16,
    sgr_stack: Vec<Style>,
    hyperlinks: Vec<Hyperlink>,
    hyperlink_index: std::collections::HashMap<Hyperlink, u32>,
    events: Vec<Event>,
    replies: Vec<u8>,
    graphics: GraphicsStore,
    cursor_shape: CursorShape,
    working_directory: Option<String>,
    sync_pending: bool,
    sync_generation: u64,
    cell_pixel: (u16, u16),
    last_exit_code: Option<i32>,
    tmux_passthrough: Vec<Vec<u8>>,
    modify_other_keys: u8,
    notification_pending: (String, String),
    click_events: Option<String>,
    marks: Vec<Mark>,
    protect_iso: bool,
    special_colors: [Option<Rgb>; 5],
}

impl Terminal {
    /// A terminal of `cols` by `rows` cells with the given settings.
    pub fn new(cols: u16, rows: u16, config: TerminalConfig) -> Terminal {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut modes = ModeSet::default();
        modes.set(Mode::GraphemeCluster, config.grapheme_clustering);
        Terminal {
            parser: Parser::new(config.parser_limits),
            primary: Screen::new(cols, rows, config.scrollback, true),
            alt: None,
            alt_active: false,
            modes,
            palette: default_palette(),
            default_colors: DynamicColors::default(),
            colors: DynamicColors::default(),
            title: String::new(),
            icon_name: String::new(),
            title_stack: Vec::new(),
            window: WindowState::default(),
            screen_pixels: (1920, 1080),
            apply_resize: false,
            title_modes: 0,
            vt_level: 5,
            c1_8bit: false,
            sace: 0,
            sasd: 0,
            status_line_type: 0,
            sgr_stack: Vec::new(),
            hyperlinks: Vec::new(),
            hyperlink_index: std::collections::HashMap::new(),
            events: Vec::new(),
            replies: Vec::new(),
            graphics: GraphicsStore::new(config.graphics_quota),
            cursor_shape: CursorShape::Default,
            working_directory: None,
            sync_pending: false,
            sync_generation: 0,
            cell_pixel: (10, 20),
            last_exit_code: None,
            tmux_passthrough: Vec::new(),
            modify_other_keys: 0,
            notification_pending: (String::new(), String::new()),
            click_events: None,
            marks: Vec::new(),
            protect_iso: false,
            special_colors: [None; 5],
            config,
        }
    }

    /// Feeds bytes from the program into the terminal.
    pub fn advance(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::take(&mut self.parser);
        parser.advance(bytes, self);
        self.parser = parser;
    }

    /// Uses the SIMD parser front end (default) or the scalar one.
    pub fn set_simd(&mut self, on: bool) {
        self.parser.set_simd(on);
    }

    /// Columns.
    pub fn cols(&self) -> u16 {
        self.primary.cols()
    }

    /// Rows.
    pub fn rows(&self) -> u16 {
        self.primary.rows()
    }

    /// The screen being shown.
    pub fn screen(&self) -> &Screen {
        if self.alt_active { self.alt.as_ref().unwrap_or(&self.primary) } else { &self.primary }
    }

    /// The screen being shown, mutable.
    pub fn screen_mut(&mut self) -> &mut Screen {
        if self.alt_active { self.alt.as_mut().unwrap_or(&mut self.primary) } else { &mut self.primary }
    }

    /// The primary screen, whatever is being shown.
    pub fn primary(&self) -> &Screen {
        &self.primary
    }

    /// The primary screen, mutable.
    pub fn primary_mut(&mut self) -> &mut Screen {
        &mut self.primary
    }

    /// True while the alternate screen is shown.
    pub fn is_alt_screen(&self) -> bool {
        self.alt_active
    }

    /// The mode set.
    pub fn modes(&self) -> &ModeSet {
        &self.modes
    }

    /// The value of one mode.
    pub fn mode(&self, mode: Mode) -> bool {
        self.modes.get(mode)
    }

    /// The settings.
    pub fn config(&self) -> &TerminalConfig {
        &self.config
    }

    /// The window title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The icon name.
    pub fn icon_name(&self) -> &str {
        &self.icon_name
    }

    /// The last working directory a shell reported.
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    /// The palette.
    pub fn palette(&self) -> &[Rgb; 256] {
        &self.palette
    }

    /// The dynamic colours in effect (overrides only; None means the host default).
    pub fn dynamic_colors(&self) -> DynamicColors {
        self.colors
    }

    /// Tells the terminal the host's default colours, so OSC 10 and 11 queries answer them.
    pub fn set_default_colors(&mut self, colors: DynamicColors) {
        self.default_colors = colors;
    }

    /// Replaces the base palette (a theme); colours set by OSC 4 since are reset.
    pub fn set_palette(&mut self, palette: [Rgb; 256]) {
        self.palette = palette;
    }

    /// The cursor shape from DECSCUSR.
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// The hyperlink with id `id`.
    pub fn hyperlink(&self, id: u32) -> Option<&Hyperlink> {
        self.hyperlinks.get(id as usize)
    }

    /// The kitty graphics store.
    pub fn graphics(&self) -> &GraphicsStore {
        &self.graphics
    }

    /// The kitty graphics store, mutable.
    pub fn graphics_mut(&mut self) -> &mut GraphicsStore {
        &mut self.graphics
    }

    /// The modifyOtherKeys level a program asked for (0, 1 or 2).
    pub fn modify_other_keys(&self) -> u8 {
        self.modify_other_keys
    }

    /// The click mode the shell asked for in OSC 133 A (`cl=line`, `m`, `v`, `w`), if any.
    pub fn prompt_click_mode(&self) -> Option<&str> {
        self.click_events.as_deref()
    }

    /// The OSC 133 marks on the primary screen, oldest first.
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// The exit code the last OSC 133 D reported.
    pub fn last_exit_code(&self) -> Option<i32> {
        self.last_exit_code
    }

    /// Takes the queued events.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Takes the bytes the terminal wants written back to the program (replies to reports).
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// True when replies are waiting.
    pub fn has_replies(&self) -> bool {
        !self.replies.is_empty()
    }

    /// Sequences tmux asked to pass through to the outer terminal (DCS tmux; ...), as raw bytes.
    pub fn take_passthrough(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.tmux_passthrough)
    }

    /// True while a synchronized update (mode 2026) is open; the host holds frames until it
    /// closes or a second passes.
    pub fn sync_pending(&self) -> bool {
        self.sync_pending
    }

    /// Counts synchronized updates opened, so the host can time out the one it is holding.
    pub fn sync_generation(&self) -> u64 {
        self.sync_generation
    }

    /// The host gave up waiting for a synchronized update: the mode is reset.
    pub fn sync_timeout(&mut self) {
        self.sync_pending = false;
        self.modes.set(Mode::SynchronizedOutput, false);
    }

    /// Tells the terminal the size of one cell in pixels, for pixel reports and images.
    pub fn set_cell_pixels(&mut self, width: u16, height: u16) {
        self.cell_pixel = (width.max(1), height.max(1));
    }

    /// The size of one cell in pixels.
    pub fn cell_pixels(&self) -> (u16, u16) {
        self.cell_pixel
    }

    /// The virtual window state XTWINOPS changes (position, iconified).
    pub fn window(&self) -> WindowState {
        self.window
    }

    /// Tells the terminal the size of the display in pixels, for XTWINOPS 15 and 19 and for the
    /// resize requests that ask for the whole display.
    pub fn set_screen_pixels(&mut self, width: u32, height: u32) {
        self.screen_pixels = (width.max(1), height.max(1));
    }

    /// When on, a resize request from XTWINOPS or DECCOLM also resizes the terminal at once, as
    /// well as queueing `Event::ResizeRequest`. A front end with no window turns this on.
    pub fn set_apply_resize_requests(&mut self, on: bool) {
        self.apply_resize = on;
    }

    /// Parser counters.
    pub fn parser_stats(&self) -> crate::parser::ParserStats {
        self.parser.stats()
    }

    /// Answers an OSC 52 read the host was asked for.
    pub fn reply_clipboard(&mut self, selection: &str, data: &[u8]) {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        self.reply(format!("\x1b]52;{selection};{encoded}\x1b\\").as_bytes());
    }

    /// Answers a colour scheme query: true for dark.
    pub fn reply_color_scheme(&mut self, dark: bool) {
        self.reply(if dark { b"\x1b[?997;1n" } else { b"\x1b[?997;2n" });
    }

    /// Changes the size, reflowing the primary screen.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let reflow = self.config.reflow;
        resize_screen(&mut self.primary, cols, rows, reflow);
        if let Some(alt) = self.alt.as_mut() {
            resize_screen(alt, cols, rows, false);
        }
        if self.modes.get(Mode::InBandResize) {
            self.report_size_in_band();
        }
    }

    /// Queues bytes to send back to the program. After S8C1T the 7 bit control introducers and
    /// terminators in the bytes (ESC [, ESC P, ESC ], ESC _, ESC \) go out as single C1 bytes.
    fn reply(&mut self, bytes: &[u8]) {
        if !self.c1_8bit {
            self.replies.extend_from_slice(bytes);
            return;
        }
        let mut i = 0;
        while i < bytes.len() {
            let c1 = match (bytes[i], bytes.get(i + 1)) {
                (0x1b, Some(b'[')) => Some(0x9b),
                (0x1b, Some(b'P')) => Some(0x90),
                (0x1b, Some(b']')) => Some(0x9d),
                (0x1b, Some(b'_')) => Some(0x9f),
                (0x1b, Some(b'\\')) => Some(0x9c),
                _ => None,
            };
            match c1 {
                Some(byte) => {
                    self.replies.push(byte);
                    i += 2;
                }
                None => {
                    self.replies.push(bytes[i]);
                    i += 1;
                }
            }
        }
    }

    /// Queues an event.
    fn event(&mut self, e: Event) {
        self.events.push(e);
    }
}

/// Resizes one screen and keeps its cursor, margins and tab stops consistent.
fn resize_screen(screen: &mut Screen, cols: u16, rows: u16, reflow: bool) {
    let cur = ReflowCursor { row: screen.cursor.row, col: screen.cursor.col };
    let old_cols = screen.cols();
    let moved = screen.pages.resize(cols, rows, cur, reflow);
    screen.cursor.row = moved.row.min(rows - 1);
    screen.cursor.col = moved.col.min(cols - 1);
    screen.cursor.pending_wrap = false;
    screen.cursor.style_cache = None;
    screen.last_print = None;
    if cols != old_cols {
        let mut tabs = crate::screen::default_tabs(cols);
        for (i, t) in screen.tabs.iter().enumerate().take(cols as usize) {
            tabs[i] = *t;
        }
        screen.tabs = tabs;
    }
    screen.reset_margins();
    if let Some(saved) = screen.saved.as_mut() {
        saved.row = saved.row.min(rows - 1);
        saved.col = saved.col.min(cols - 1);
    }
    screen.pages.mark_all_dirty();
}

impl Handler for Terminal {
    /// Applies one parsed action.
    fn handle(&mut self, action: Action<'_>) {
        match action {
            Action::Print(text) => self.print(text, false),
            Action::PrintAscii(text) => self.print(text, true),
            Action::Execute(byte) => self.execute(byte),
            Action::Esc { intermediates, final_byte } => self.esc_dispatch(intermediates, final_byte),
            Action::Csi(seq) => self.csi_dispatch(seq),
            Action::Osc(data, term) => self.osc_dispatch(data, term),
            Action::Dcs(seq, data) => self.dcs_dispatch(seq, data),
            Action::Apc(data) => self.apc_dispatch(data),
        }
    }
}
