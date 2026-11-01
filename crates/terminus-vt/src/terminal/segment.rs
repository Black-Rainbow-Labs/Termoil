//! The print hot path: writes as much of a text run as fits on the cursor's row in one pass.
//!
//! The page and row are looked up once per segment, the pen's style id once, and ASCII cells are
//! stored directly with their style references added in bulk at the end. Non ASCII characters
//! get their width from the Unicode tables, join the previous cell when the grapheme rules say
//! so (widening it for an emoji presentation selector), and wide characters that do not fit end
//! the segment so the caller can wrap. Insert mode and translated character sets do not come
//! here; they use the per character path in `print.rs`.

use crate::cell::{Cell, Semantic, Wide, row_flags};
use crate::page::Page;
use crate::screen::{LastPrint, Screen};
use crate::unicode::{GraphemeBreaker, char_width, cluster_width};

/// The kitty graphics Unicode placeholder.
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// The mode values printing depends on, read once per run.
#[derive(Clone, Copy)]
pub(super) struct PrintModes {
    pub autowrap: bool,
    pub cluster: bool,
    pub right: u16,
    pub max_cluster: usize,
}

/// The cell text was last written into, while the segment runs.
#[derive(Clone, Copy)]
struct Last {
    col: u16,
    /// The breaker after the cell's last code point; None for a plain ASCII cell, whose breaker
    /// is built only if a non ASCII character follows.
    breaker: Option<GraphemeBreaker>,
    ascii: u8,
}

impl Last {
    /// The grapheme break state after this cell.
    fn breaker(&self) -> GraphemeBreaker {
        self.breaker.unwrap_or_else(|| GraphemeBreaker::new(self.ascii as char))
    }
}

/// Mutable state of one segment.
struct Seg {
    x: u16,
    pending: bool,
    last: Option<Last>,
    ascii_refs: u32,
}

/// Writes text from the start of `text` onto the cursor row of `s`. Returns the bytes consumed.
/// It stops when the row is full and autowrap is on (the cursor is left in the pending wrap
/// state for the caller to wrap), or when a wide character needs the next row.
pub(super) fn write_segment(s: &mut Screen, text: &str, m: PrintModes) -> usize {
    let Screen { pages, cursor, last_print, .. } = s;
    let row = cursor.row;
    let cols = pages.cols();
    let (page, i) = pages.active_mut(row);
    let base = pen_on_page(page, cursor);
    let link = cursor.hyperlink;
    let joinable = last_print.filter(|lp| lp.after == (cursor.row, cursor.col, cursor.pending_wrap));
    let mut seg = Seg {
        x: cursor.col,
        pending: cursor.pending_wrap && m.autowrap,
        last: joinable.map(|lp| Last { col: lp.col, breaker: lp.state, ascii: lp.ascii }),
        ascii_refs: 0,
    };
    if seg.x > 0 && !seg.pending && page.cells(i)[seg.x as usize].wide() == Wide::SpacerTail {
        page.clear_cells(i, seg.x as usize - 1, seg.x as usize + 1, Cell::BLANK, false);
    }
    let start_x = seg.x;
    let side = page.row_has_side_data(i);
    let consumed = write_loop(page, i, text, base, link, side, cols, m, &mut seg);
    finish_row(page, i, base, start_x, &seg, cols);
    cursor.col = seg.x;
    cursor.pending_wrap = seg.pending;
    if let Some(l) = seg.last {
        *last_print =
            Some(LastPrint { row, col: l.col, state: l.breaker, ascii: l.ascii, after: (row, seg.x, seg.pending) });
    }
    consumed
}

/// The pen as a blank cell with its style id in `page`: the cursor's cached id when it still
/// holds, then one of the recently used ids, then a lookup in the page's style table.
fn pen_on_page(page: &mut Page, cursor: &mut crate::screen::Cursor) -> Cell {
    let style = cursor.style;
    let uid = page.uid();
    let id = match cursor.style_cache {
        Some((u, id)) if u == uid && page.style_id_valid(id, &style) => id,
        _ => {
            let recent = cursor
                .recent_styles
                .iter()
                .flatten()
                .find(|(u, s, id)| *u == uid && *s == style && page.style_id_valid(*id, &style))
                .map(|e| e.2);
            let id = recent.unwrap_or_else(|| page.intern_style(&style));
            if recent.is_none() && id != 0 {
                let slot = cursor.recent_next as usize % cursor.recent_styles.len();
                cursor.recent_styles[slot] = Some((uid, style, id));
                cursor.recent_next = cursor.recent_next.wrapping_add(1);
            }
            cursor.style_cache = Some((uid, id));
            id
        }
    };
    let mut cell = Cell::blank(id);
    cell.set_protected(cursor.protected);
    if cursor.semantic != Semantic::Output {
        cell.set_semantic(cursor.semantic);
    }
    cell
}

/// The character loop of a segment. Returns bytes consumed.
fn write_loop(
    page: &mut Page, i: usize, text: &str, base: Cell, link: Option<u32>, side: bool, cols: u16, m: PrintModes,
    seg: &mut Seg,
) -> usize {
    let bytes = text.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let b = bytes[pos];
        let ascii_fast = b < 0x80
            && !seg.pending
            && seg.last.is_none_or(|l| l.breaker.is_none_or(|br| !br.joins_ascii() || !m.cluster));
        if ascii_fast && (m.autowrap || seg.x < m.right) {
            let n = ascii_run(page, i, &bytes[pos..], base, link, side, m, seg);
            pos += n;
            continue;
        }
        let c = text[pos..].chars().next().unwrap_or(' ');
        match write_char(page, i, c, base, link, cols, m, seg) {
            Step::Done => pos += c.len_utf8(),
            Step::Stop => break,
        }
    }
    pos
}

/// Writes the run of ASCII at the start of `bytes` that fits on the row, as one bulk copy.
/// Returns how many bytes were written (at least one).
fn ascii_run(
    page: &mut Page, i: usize, bytes: &[u8], base: Cell, link: Option<u32>, side: bool, m: PrintModes, seg: &mut Seg,
) -> usize {
    let room = (m.right - seg.x + 1) as usize;
    // Without autowrap the last column is written through the general path, which overwrites it.
    let room = if m.autowrap { room } else { room - 1 };
    // Only as far as the row has room is scanned; a long run is written one row at a time.
    let n = crate::scan::first_non_printable(&bytes[..bytes.len().min(room)]).max(1);
    let x = seg.x as usize;
    if side {
        for col in x..x + n {
            page.release_at(i, col);
        }
    }
    page.put_ascii(i, x, &bytes[..n], base);
    if let Some(id) = link {
        for col in x..x + n {
            page.set_hyperlink(i, col, id);
        }
    }
    seg.ascii_refs += n as u32;
    seg.last = Some(Last { col: (x + n - 1) as u16, breaker: None, ascii: bytes[n - 1] });
    advance(seg, n as u16, m);
    n
}

/// What happened to one character.
enum Step {
    /// The character was written, joined or dropped.
    Done,
    /// The row is full; the caller wraps and the character is written on the next row.
    Stop,
}

/// Writes or joins one character through the general rules.
fn write_char(
    page: &mut Page, i: usize, c: char, base: Cell, link: Option<u32>, cols: u16, m: PrintModes, seg: &mut Seg,
) -> Step {
    if ('\u{80}'..='\u{9f}').contains(&c) {
        return Step::Done;
    }
    if let Some(l) = seg.last {
        let mut br = l.breaker();
        let joins = if m.cluster { !br.should_break(c) } else { char_width(c) == 0 };
        if joins {
            join(page, i, l, c, br, cols, m, seg);
            return Step::Done;
        }
    }
    if seg.pending {
        return Step::Stop;
    }
    let width = if m.cluster { cluster_width(&[c]) } else { char_width(c).max(1) }.clamp(1, 2) as u16;
    if width == 2 && seg.x >= m.right {
        if !m.autowrap || m.right == 0 {
            return Step::Done;
        }
        if seg.x + 1 == cols {
            let mut head = Cell::BLANK;
            head.set_wide(Wide::SpacerHead);
            page.write_cell(i, seg.x as usize, head);
        }
        seg.pending = true;
        return Step::Stop;
    }
    put_cell(page, i, seg.x as usize, c, width, base, link);
    seg.last = Some(Last { col: seg.x, breaker: Some(GraphemeBreaker::new(c)), ascii: 0 });
    advance(seg, width, m);
    Step::Done
}

/// Writes one cell (and its tail when wide) through the releasing write.
fn put_cell(page: &mut Page, i: usize, col: usize, c: char, width: u16, base: Cell, link: Option<u32>) {
    // Writing over the tail of a wide character (with autowrap off the last column is written
    // again and again) clears that character first.
    page.clear_split_wide(i, col, col);
    let mut cell = base;
    cell.set_char(c);
    if width == 2 {
        cell.set_wide(Wide::Wide);
        if page.cells(i)[col + 1].wide() == Wide::Wide {
            page.clear_split_wide(i, col + 2, col + 2);
        }
    }
    page.write_cell(i, col, cell);
    if width == 2 {
        let mut tail = base;
        tail.set_wide(Wide::SpacerTail);
        page.write_cell(i, col + 1, tail);
    }
    if c == KITTY_PLACEHOLDER {
        page.row_mut(i).flags |= row_flags::KITTY_PLACEHOLDER;
    }
    if let Some(id) = link {
        for k in col..col + width as usize {
            page.set_hyperlink(i, k, id);
        }
    }
}

/// Moves the segment's column past a cell of `width`.
fn advance(seg: &mut Seg, width: u16, m: PrintModes) {
    let end = seg.x + width;
    if end > m.right {
        seg.x = m.right;
        seg.pending = m.autowrap;
    } else {
        seg.x = end;
    }
}

/// Adds `c` to the cluster of the last cell, then makes the cell wide or narrow when the
/// cluster's width changed.
fn join(page: &mut Page, i: usize, l: Last, c: char, br: GraphemeBreaker, cols: u16, m: PrintModes, seg: &mut Seg) {
    let col = l.col as usize;
    if !page.append_grapheme(i, col, c, m.max_cluster) {
        return;
    }
    seg.last = Some(Last { col: l.col, breaker: Some(br), ascii: 0 });
    if !m.cluster {
        return;
    }
    let cell = page.cells(i)[col];
    let mut chars = vec![cell.char().unwrap_or(' ')];
    chars.extend_from_slice(page.grapheme(page.cell_index(i, col)));
    let width = cluster_width(&chars);
    if width == 2 && cell.wide() == Wide::Narrow && !seg.pending && l.col < m.right && l.col + 1 < cols {
        widen(page, i, col, cell);
        seg.x = l.col + 1;
        advance(seg, 1, m);
    } else if width == 1 && cell.wide() == Wide::Wide {
        let mut head = cell;
        head.set_wide(Wide::Narrow);
        page.put_raw(i, col, head);
        page.clear_cells(i, col + 1, col + 2, Cell::BLANK, false);
        seg.x = l.col + 1;
        seg.pending = false;
    }
}

/// Makes the narrow cell at `col` wide, putting a tail in the next column.
fn widen(page: &mut Page, i: usize, col: usize, cell: Cell) {
    page.clear_split_wide(i, col + 2, col + 2);
    let mut head = cell;
    head.set_wide(Wide::Wide);
    page.put_raw(i, col, head);
    let mut tail = Cell::blank(head.style_id());
    tail.set_protected(head.protected());
    tail.set_semantic(head.semantic());
    tail.set_wide(Wide::SpacerTail);
    page.write_cell(i, col + 1, tail);
}

/// Adds the bulk style references and row flags, and clears a tail orphaned at the end.
fn finish_row(page: &mut Page, i: usize, base: Cell, start_x: u16, seg: &Seg, cols: u16) {
    page.add_style_refs(base.style_id(), seg.ascii_refs);
    page.mark_written(i, seg.x as usize + 1);
    let flags = &mut page.row_mut(i).flags;
    *flags |= row_flags::DIRTY;
    if base.style_id() != 0 {
        *flags |= row_flags::STYLED;
    }
    let end = if seg.pending { seg.x as usize + 1 } else { seg.x as usize };
    if end > start_x as usize
        && end < cols as usize
        && page.cells(i)[end].wide() == Wide::SpacerTail
        && page.cells(i)[end - 1].wide() != Wide::Wide
    {
        page.clear_cells(i, end, end + 1, Cell::BLANK, false);
    }
}
