//! The render state: a copy of what the screen shows, taken under the terminal lock.
//!
//! The renderer holds the lock only long enough to copy the rows that changed since the last
//! frame (their dirty flag is cleared as they are copied) plus the cursor, selection, colours and
//! image placements. Shaping and building the frame happen afterwards, outside the lock.

use terminus_vt::Terminal;
use terminus_vt::cell::{Semantic, Wide, row_flags};
use terminus_vt::modes::Mode;
use terminus_vt::style::{Color, Rgb, Style};
use terminus_vt::terminal::CursorShape;

/// What one cell shows.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CellState {
    /// The text: empty for a blank cell, one or more code points for a cluster.
    pub text: CellText,
    /// The cell's style.
    pub style: Style,
    /// The width property.
    pub wide: CellWidth,
    /// The OSC 8 hyperlink id, if any.
    pub link: Option<u32>,
    /// The OSC 133 content type.
    pub prompt: bool,
}

/// A cell's text, stored inline for a single code point.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum CellText {
    /// Nothing.
    #[default]
    Blank,
    /// One code point.
    Char(char),
    /// A grapheme cluster of several code points.
    Cluster(Box<str>),
}

impl CellText {
    /// The first code point, if any.
    pub fn first(&self) -> Option<char> {
        match self {
            CellText::Blank => None,
            CellText::Char(c) => Some(*c),
            CellText::Cluster(s) => s.chars().next(),
        }
    }

    /// Appends the text to `out`.
    pub fn push_to(&self, out: &mut String) {
        match self {
            CellText::Blank => out.push(' '),
            CellText::Char(c) => out.push(*c),
            CellText::Cluster(s) => out.push_str(s),
        }
    }
}

/// The width property of a cell as the renderer needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CellWidth {
    /// One column.
    #[default]
    Narrow,
    /// Two columns, drawn at this cell.
    Wide,
    /// The second column of a wide character: nothing is drawn here.
    Tail,
}

/// One visible row.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RowState {
    /// One entry per column.
    pub cells: Vec<CellState>,
    /// The row changed since the last frame the renderer drew.
    pub dirty: bool,
    /// DECDWL or DECDHL.
    pub double_width: bool,
    /// The serial of the terminal row shown here, which identifies it while it scrolls.
    pub serial: u64,
    /// The row's content changed (not only its position); a cached row frame cannot be reused.
    pub content_changed: bool,
}

/// The cursor as the frame shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorState {
    /// Visible row.
    pub row: u16,
    /// Column.
    pub col: u16,
    /// DECSCUSR shape.
    pub shape: CursorShape,
    /// DECTCEM.
    pub visible: bool,
    /// The cursor sits on a wide character, so it covers two columns.
    pub wide: bool,
}

/// Colours in effect, with the host's defaults applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Colors {
    /// The 256 colour palette.
    pub palette: [Rgb; 256],
    /// Default foreground.
    pub foreground: Rgb,
    /// Default background.
    pub background: Rgb,
    /// Cursor colour.
    pub cursor: Rgb,
    /// Selection background.
    pub selection_background: Rgb,
    /// Selection foreground, or None to keep each cell's own.
    pub selection_foreground: Option<Rgb>,
}

impl Colors {
    /// The xterm palette on a dark background.
    pub fn dark_default() -> Colors {
        Colors {
            palette: terminus_vt::style::default_palette(),
            foreground: Rgb::new(0xdc, 0xdc, 0xdc),
            background: Rgb::new(0x1e, 0x1e, 0x1e),
            cursor: Rgb::new(0xff, 0xff, 0xff),
            selection_background: Rgb::new(0x26, 0x4f, 0x78),
            selection_foreground: None,
        }
    }

    /// Resolves a cell colour; `Default` becomes `default`.
    pub fn resolve(&self, c: Color, default: Rgb) -> Rgb {
        match c {
            Color::Default => default,
            Color::Palette(i) => self.palette[i as usize],
            Color::Rgb(r, g, b) => Rgb::new(r, g, b),
        }
    }
}

/// Everything the frame builder needs.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderState {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Visible rows, top to bottom.
    pub lines: Vec<RowState>,
    /// The cursor, when the viewport shows the active area.
    pub cursor: Option<CursorState>,
    /// Selected cells as (row, first column, last column) per visible row.
    pub selection: Vec<(u16, u16, u16)>,
    /// The colours in effect: the defaults with the program's OSC changes applied.
    pub colors: Colors,
    /// The configured colours, which an OSC reset (110 to 119) returns to.
    pub defaults: Colors,
    /// DECSCNM.
    pub reverse_video: bool,
    /// The window has focus (a hollow cursor otherwise).
    pub focused: bool,
    /// The next update copies every row (nothing has been copied yet, or the size changed).
    pub needs_full: bool,
    /// Serial of the row shown at the top last time; when it changes the screen scrolled and
    /// every visible row shows different content, whatever the rows' dirty flags say.
    pub top_serial: Option<u64>,
}

impl RenderState {
    /// An empty state of the given size.
    pub fn new(cols: u16, rows: u16, colors: Colors) -> RenderState {
        let row = RowState {
            cells: vec![CellState::default(); cols as usize],
            dirty: true,
            double_width: false,
            serial: u64::MAX,
            content_changed: true,
        };
        RenderState {
            cols,
            rows,
            lines: vec![row; rows as usize],
            cursor: None,
            selection: Vec::new(),
            defaults: colors.clone(),
            colors,
            reverse_video: false,
            focused: true,
            needs_full: true,
            top_serial: None,
        }
    }

    /// Copies what changed from the terminal: the dirty rows of the viewport (clearing their dirty
    /// flags), the cursor, the colours and the modes. `scroll` is how many rows the viewport is
    /// above the bottom; a scrolled viewport has no cursor and every row is copied.
    pub fn update(&mut self, term: &mut Terminal, scroll: usize) {
        let (cols, rows) = (term.cols(), term.rows());
        let resized = cols != self.cols || rows != self.rows;
        if resized {
            let defaults = self.defaults.clone();
            *self = RenderState::new(cols, rows, defaults);
        }
        let top = {
            let pages = &term.screen().pages;
            pages.serial_of(pages.active_y(0).saturating_sub(scroll))
        };
        let scrolled = self.top_serial != Some(top);
        self.top_serial = Some(top);
        let full = resized || scroll > 0 || self.needs_full || scrolled;
        self.copy_rows(term, scroll, full);
        self.needs_full = false;
        self.copy_cursor(term, scroll);
        self.copy_colors(term);
        self.reverse_video = term.mode(Mode::ReverseVideo);
    }

    /// Copies the visible rows that are dirty (or all of them when `full`).
    fn copy_rows(&mut self, term: &mut Terminal, scroll: usize, full: bool) {
        let pages = &mut term.screen_mut().pages;
        let rows = pages.rows() as usize;
        let start = pages.active_y(0).saturating_sub(scroll);
        for r in 0..rows {
            let serial = pages.serial_of(start + r);
            let (page, i) = pages.row_mut(start + r);
            let header = page.row_mut(i);
            let dirty = header.has(row_flags::DIRTY);
            header.set(row_flags::DIRTY, false);
            let line = &mut self.lines[r];
            let moved = line.serial != serial;
            line.content_changed = dirty || self.needs_full;
            line.dirty = dirty || full || moved;
            line.serial = serial;
            if !line.dirty {
                continue;
            }
            line.double_width = page.row(i).flags & row_flags::LINE_SIZE != 0;
            copy_cells(page, i, &mut line.cells);
        }
    }

    /// Copies the cursor; none while the viewport is scrolled away from the active area.
    fn copy_cursor(&mut self, term: &Terminal, scroll: usize) {
        let s = term.screen();
        let c = &s.cursor;
        let wide = {
            let pages = &s.pages;
            pages.with_row(pages.active_y(c.row), |p, i| {
                p.cells(i).get(c.col as usize).is_some_and(|x| x.wide() == Wide::Wide)
            })
        };
        let state = CursorState {
            row: c.row,
            col: c.col,
            shape: term.cursor_shape(),
            visible: term.mode(Mode::CursorVisible),
            wide,
        };
        let old = self.cursor;
        self.cursor = (scroll == 0).then_some(state);
        if old != self.cursor {
            for cur in [old, self.cursor].into_iter().flatten() {
                if let Some(line) = self.lines.get_mut(cur.row as usize) {
                    line.dirty = true;
                }
            }
        }
    }

    /// Applies the terminal's palette and dynamic colour overrides.
    fn copy_colors(&mut self, term: &Terminal) {
        let dynamic = term.dynamic_colors();
        let base = &self.defaults;
        let colors = Colors {
            palette: *term.palette(),
            foreground: dynamic.foreground.unwrap_or(base.foreground),
            background: dynamic.background.unwrap_or(base.background),
            cursor: dynamic.cursor.unwrap_or(base.cursor),
            selection_background: dynamic.selection_background.unwrap_or(base.selection_background),
            selection_foreground: dynamic.selection_foreground.or(base.selection_foreground),
        };
        if colors != self.colors {
            self.colors = colors;
            self.lines.iter_mut().for_each(|l| {
                l.dirty = true;
                l.content_changed = true;
            });
        }
    }
}

/// Copies one page row into cell states.
fn copy_cells(page: &terminus_vt::page::Page, i: usize, out: &mut Vec<CellState>) {
    let cells = page.cells(i);
    out.resize(cells.len(), CellState::default());
    for (col, (cell, dst)) in cells.iter().zip(out.iter_mut()).enumerate() {
        let text = match cell.char() {
            None => CellText::Blank,
            Some(c) if !cell.is_cluster() => CellText::Char(c),
            Some(c) => {
                let mut s = String::from(c);
                s.extend(page.grapheme(page.cell_index(i, col)));
                CellText::Cluster(s.into_boxed_str())
            }
        };
        let wide = match cell.wide() {
            Wide::Wide => CellWidth::Wide,
            Wide::SpacerTail => CellWidth::Tail,
            _ => CellWidth::Narrow,
        };
        let link = cell.has_hyperlink().then(|| page.hyperlink(page.cell_index(i, col))).flatten();
        *dst = CellState {
            text,
            style: page.style(cell.style_id()),
            wide,
            link,
            prompt: cell.semantic() == Semantic::Prompt,
        };
    }
}
