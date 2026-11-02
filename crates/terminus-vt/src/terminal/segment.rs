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
use crate::unicode::{GraphemeBreaker, char_width, cluster_width_parts, packed_props, props};

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
    raw_end: usize,
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
        raw_end: 0,
    };
    // With autowrap off the cursor stays on the tail of a wide character written in the last
    // column. When the text joins that character (a zero width joiner, say) it must not be
    // cleared first, so this runs only when there is no cell to join; the general write clears a
    // tail it overwrites.
    if seg.last.is_none() && seg.x > 0 && !seg.pending && page.wide_at(i, seg.x as usize) == Wide::SpacerTail {
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
            let id = page.intern_style(&style);
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
        match write_char(page, i, c, base, link, side, cols, m, seg) {
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
        // Cells at or past the written mark are blank and hold nothing to release.
        let upto = (x + n).min(page.row(i).written as usize);
        for col in x..upto {
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
    seg.raw_end = seg.raw_end.max(x + n);
    page.mark_written(i, x + n);
    // A wide character overwritten up to its head leaves its tail; clearing it now keeps a later
    // write at that column from taking out the cell just written.
    if x + n < page.cols() as usize && page.wide_at(i, x + n) == Wide::SpacerTail {
        page.clear_cells(i, x + n, x + n + 1, Cell::BLANK, false);
    }
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

/// Writes or joins one character through the general rules. A plain character (see
/// [`packed_props`]) skips the grapheme state machine: it never joins what came before unless
/// that ended in a Prepend character, and it leaves the breaker in its plain state.
fn write_char(
    page: &mut Page, i: usize, c: char, base: Cell, link: Option<u32>, side: bool, cols: u16, m: PrintModes,
    seg: &mut Seg,
) -> Step {
    if ('\u{80}'..='\u{9f}').contains(&c) {
        return Step::Done;
    }
    let packed = packed_props(c);
    let after_prepend = seg.last.is_some_and(|l| l.breaker.is_some_and(|b| b.joins_ascii()));
    let general = packed & !3 != 0 || packed & 3 == 0 || after_prepend || c == KITTY_PLACEHOLDER;
    let (width, breaker) = if general {
        let p = props(c);
        if let Some(l) = seg.last {
            let mut br = l.breaker();
            let joins = if m.cluster { !br.should_break_props(&p) } else { char_width(c) == 0 };
            if joins {
                join(page, i, l, c, br, cols, m, seg);
                return Step::Done;
            }
        }
        (
            if m.cluster { p.width.max(1) } else { char_width(c).max(1) }.clamp(1, 2) as u16,
            GraphemeBreaker::from_props(p),
        )
    } else {
        (packed & 3, GraphemeBreaker::PLAIN)
    };
    if seg.pending {
        return Step::Stop;
    }
    if width == 2 && seg.x >= m.right {
        return no_room_for_wide(page, i, cols, m, seg);
    }
    let simple = link.is_none() && c != KITTY_PLACEHOLDER;
    match (simple, width) {
        (true, 2) => put_wide(page, i, seg.x as usize, c, base, side, seg),
        (true, _) => put_narrow(page, i, seg.x as usize, c, base, side, seg),
        _ => put_cell(page, i, seg.x as usize, c, width, base, link),
    }
    seg.last = Some(Last { col: seg.x, breaker: Some(breaker), ascii: 0 });
    advance(seg, width, m);
    Step::Done
}

/// Handles a wide character at the last column: it is dropped when the row cannot wrap, and
/// otherwise a spacer head is left in the last column and the segment stops so the caller wraps.
fn no_room_for_wide(page: &mut Page, i: usize, cols: u16, m: PrintModes, seg: &mut Seg) -> Step {
    if !m.autowrap || m.right == 0 {
        return Step::Done;
    }
    if seg.x + 1 == cols {
        let mut head = Cell::BLANK;
        head.set_wide(Wide::SpacerHead);
        page.write_cell(i, seg.x as usize, head);
    }
    seg.pending = true;
    Step::Stop
}

/// Releases what the cell at `col` holds before a character is written over it. A cell at or
/// past both the row's written mark and the segment's raw end is blank and holds nothing. A cell
/// this segment already wrote raw has its style reference still pending, so the pending
/// references are added first, which lets the release take one back.
fn release_old(page: &mut Page, i: usize, col: usize, base: Cell, side: bool, seg: &mut Seg) {
    if col < seg.raw_end {
        page.add_style_refs(base.style_id(), seg.ascii_refs);
        seg.ascii_refs = 0;
        page.release_at(i, col);
    } else if side && col < page.row(i).written as usize {
        page.release_at(i, col);
    }
}

/// Writes one narrow character with no hyperlink: the style reference is counted in bulk at the
/// end of the row like an ASCII run.
fn put_narrow(page: &mut Page, i: usize, col: usize, c: char, base: Cell, side: bool, seg: &mut Seg) {
    page.clear_split_wide(i, col, col);
    page.clear_tail_of_overwritten_head(i, col);
    release_old(page, i, col, base, side, seg);
    let mut cell = base;
    cell.set_char(c);
    page.put_raw(i, col, cell);
    seg.ascii_refs += 1;
    seg.raw_end = seg.raw_end.max(col + 1);
    page.mark_written(i, col + 1);
}

/// Writes one wide character with no hyperlink: a head and a spacer tail raw, the two style
/// references added at once (a later join can narrow the character and release the tail, so the
/// references are not left for the end of the row).
fn put_wide(page: &mut Page, i: usize, col: usize, c: char, base: Cell, side: bool, seg: &mut Seg) {
    page.clear_split_wide(i, col, col);
    if page.wide_at(i, col + 1) == Wide::Wide {
        page.clear_split_wide(i, col + 2, col + 2);
    }
    release_old(page, i, col, base, side, seg);
    release_old(page, i, col + 1, base, side, seg);
    let mut head = base;
    head.set_char(c);
    head.set_wide(Wide::Wide);
    let mut tail = base;
    tail.set_wide(Wide::SpacerTail);
    page.put_raw(i, col, head);
    page.put_raw(i, col + 1, tail);
    page.add_style_refs(base.style_id(), 2);
    seg.raw_end = seg.raw_end.max(col + 2);
    page.mark_written(i, col + 2);
}

/// Writes one cell (and its tail when wide) through the releasing write.
fn put_cell(page: &mut Page, i: usize, col: usize, c: char, width: u16, base: Cell, link: Option<u32>) {
    // Writing over the tail of a wide character (with autowrap off the last column is written
    // again and again) clears that character first.
    page.clear_split_wide(i, col, col);
    if width == 1 {
        page.clear_tail_of_overwritten_head(i, col);
    }
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
    let width = cluster_width_parts(cell.char().unwrap_or(' '), page.grapheme(page.cell_index(i, col)));
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
    // Looked at before the written mark moves, so a cell past the old mark (blank by the mark's
    // meaning) is not read from memory that may be cold.
    let end = if seg.pending { seg.x as usize + 1 } else { seg.x as usize };
    let orphan = end > start_x as usize
        && end < cols as usize
        && page.wide_at(i, end) == Wide::SpacerTail
        && page.cells(i)[end - 1].wide() != Wide::Wide;
    page.mark_written(i, seg.x as usize + 1);
    let flags = &mut page.row_mut(i).flags;
    *flags |= row_flags::DIRTY;
    if base.style_id() != 0 {
        *flags |= row_flags::STYLED;
    }
    if orphan {
        page.clear_cells(i, end, end + 1, Cell::BLANK, false);
    }
}
