//! A page: a block of rows that share one style table, one grapheme store and one hyperlink map.
//!
//! The design is Ghostty's, in Rust (see `docs/research/architectures.md` section 1.2). Every
//! reference inside a page is an index, never a pointer, so a page can be cloned, compressed and
//! restored as plain data. Rows are reached through headers that hold the offset of their cells,
//! so scrolling inside a page swaps headers instead of copying cells.
//!
//! A page with more styles or grapheme storage than the standard capacity is marked oversized.
//! The page list never puts an oversized page back in its pool, which is the fix for the leak
//! Ghostty had when it reset an oversized page to standard metadata.

use crate::cell::{Cell, Row, Wide, row_flags};
use crate::hash::FxHashMap;
use crate::style::{Color, Style, Underline};

/// Cells in a standard page: 215 columns by 215 rows.
pub const STANDARD_CELLS: usize = 215 * 215;
/// Styles a standard page holds before it counts as oversized.
pub const STANDARD_STYLES: usize = 128;
/// Grapheme tail code points a standard page holds before it counts as oversized (8 KiB).
pub const STANDARD_GRAPHEME_CHARS: usize = 2048;

/// Source of page identities, so a cached style id can tell a reused page from the one it was
/// computed for.
static NEXT_UID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

/// A fresh page identity.
fn next_uid() -> u64 {
    NEXT_UID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// The number of rows a standard page holds at `cols` columns.
pub fn standard_rows(cols: u16) -> u16 {
    (STANDARD_CELLS / cols.max(1) as usize).clamp(1, u16::MAX as usize) as u16
}

/// Reference count value that marks a freed style table entry.
const FREE: u32 = u32::MAX;

#[derive(Clone, Debug, Default)]
struct StyleTable {
    entries: Vec<(Style, u32)>,
    map: FxHashMap<Style, u16>,
    free: Vec<u16>,
}

impl StyleTable {
    /// Number of styles in use, not counting the default.
    fn live(&self) -> usize {
        self.map.len()
    }
}

/// A block of rows with their cells, styles, grapheme tails and hyperlinks.
#[derive(Clone, Debug)]
pub struct Page {
    uid: u64,
    cols: u16,
    capacity: u16,
    len: u16,
    rows: Vec<Row>,
    cells: Vec<Cell>,
    styles: StyleTable,
    grapheme_chars: Vec<char>,
    graphemes: FxHashMap<u32, (u32, u16)>,
    grapheme_garbage: usize,
    hyperlinks: FxHashMap<u32, u32>,
    oversized: bool,
}

impl Page {
    /// A page of `capacity` empty rows at `cols` columns, with no rows in use.
    pub fn new(cols: u16, capacity: u16) -> Page {
        let cols = cols.max(1);
        let capacity = capacity.max(1);
        let rows = (0..capacity).map(|i| Row { offset: i as u32 * cols as u32, flags: 0, written: 0 }).collect();
        Page {
            uid: next_uid(),
            cols,
            capacity,
            len: 0,
            rows,
            cells: vec![Cell::BLANK; cols as usize * capacity as usize],
            styles: StyleTable { entries: vec![(Style::default(), 0)], ..StyleTable::default() },
            grapheme_chars: Vec::new(),
            graphemes: FxHashMap::default(),
            grapheme_garbage: 0,
            hyperlinks: FxHashMap::default(),
            oversized: false,
        }
    }

    /// This page's identity. It changes when the page is reset for reuse, so a style id cached
    /// against it is never applied to a different page's table.
    #[inline]
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// Columns per row.
    #[inline]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Rows in use.
    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// True when no rows are in use.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Rows the page can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// True when every row is in use.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == self.capacity
    }

    /// True when the page holds more styles or grapheme storage than a standard page.
    #[inline]
    pub fn is_oversized(&self) -> bool {
        self.oversized
    }

    /// Takes one more row into use and returns its index. The row is blank. None when full.
    pub fn push_row(&mut self) -> Option<usize> {
        if self.is_full() {
            return None;
        }
        let i = self.len as usize;
        self.len += 1;
        // Rows past `len` hold nothing that needs releasing (truncate released them, or reset
        // threw the tables away), so a raw fill up to the written mark is enough.
        let row = self.rows[i];
        let start = row.offset as usize;
        self.cells[start..start + row.written as usize].fill(Cell::BLANK);
        self.rows[i] = Row { offset: row.offset, flags: 0, written: 0 };
        if let Some(next) = self.rows.get(i + 1) {
            prefetch_cells(&self.cells, next.offset as usize, self.cols as usize);
        }
        Some(i)
    }

    /// Drops rows from the end so `len` rows remain, clearing what they held.
    pub fn truncate(&mut self, len: usize) {
        while self.len as usize > len {
            let i = self.len as usize - 1;
            self.clear_row(i);
            self.rows[i].flags = 0;
            self.len -= 1;
        }
    }

    /// Removes the first `k` rows; the rows after them move up to index 0.
    pub fn remove_front(&mut self, k: usize) {
        let k = k.min(self.len as usize);
        for i in 0..k {
            self.clear_row(i);
            self.rows[i].flags = 0;
        }
        self.rows[..self.len as usize].rotate_left(k);
        self.len -= k as u16;
    }

    /// Clears any wide character of row `i` that columns `from..to` would cut in half: one whose
    /// head is left of `from` with its tail inside, or whose head is inside with its tail at `to`.
    pub fn clear_split_wide(&mut self, i: usize, from: usize, to: usize) {
        let cols = self.cols as usize;
        if from > 0 && from < cols && self.cells(i)[from].wide() == Wide::SpacerTail {
            self.clear_cells(i, from - 1, from + 1, Cell::BLANK, false);
        }
        if to > 0 && to < cols && self.cells(i)[to].wide() == Wide::SpacerTail {
            self.clear_cells(i, to - 1, to + 1, Cell::BLANK, false);
        }
    }

    /// A one row page holding a copy of row `i`, used to move a row when source and destination
    /// may be the same page.
    pub fn extract_row(&self, i: usize) -> Page {
        let mut p = Page::new(self.cols, 1);
        p.push_row();
        p.copy_row_from(0, self, i);
        p
    }

    /// Clears every row and marks the page empty, ready for reuse from the pool.
    pub fn reset(&mut self) {
        // Cells are not cleared here; each row is cleared up to its written mark when it is
        // taken back into use.
        self.len = 0;
        self.grapheme_chars.clear();
        self.graphemes.clear();
        self.grapheme_garbage = 0;
        self.hyperlinks.clear();
        self.styles = StyleTable { entries: vec![(Style::default(), 0)], ..StyleTable::default() };
        self.uid = next_uid();
    }

    /// The header of row `i`.
    #[inline]
    pub fn row(&self, i: usize) -> &Row {
        &self.rows[i]
    }

    /// The header of row `i`, mutable.
    #[inline]
    pub fn row_mut(&mut self, i: usize) -> &mut Row {
        &mut self.rows[i]
    }

    /// The cells of row `i`.
    #[inline]
    pub fn cells(&self, i: usize) -> &[Cell] {
        let start = self.rows[i].offset as usize;
        &self.cells[start..start + self.cols as usize]
    }

    /// The cells of row `i`, mutable. Writers must keep the style counts, grapheme store and
    /// hyperlink map in step; [`Page::write_cell`] and [`Page::clear_cells`] do.
    #[inline]
    pub fn cells_mut(&mut self, i: usize) -> &mut [Cell] {
        let start = self.rows[i].offset as usize;
        &mut self.cells[start..start + self.cols as usize]
    }

    /// The row header and cells of row `i` together, both mutable.
    #[inline]
    pub fn row_and_cells_mut(&mut self, i: usize) -> (&mut Row, &mut [Cell]) {
        let start = self.rows[i].offset as usize;
        (&mut self.rows[i], &mut self.cells[start..start + self.cols as usize])
    }

    /// The index in the cell array of row `i`, column `col`; the key for the side maps.
    #[inline]
    pub fn cell_index(&self, i: usize, col: usize) -> u32 {
        self.rows[i].offset + col as u32
    }

    /// Interns `style` and returns its id without taking a reference. The default style is 0.
    pub fn intern_style(&mut self, style: &Style) -> u16 {
        if style.is_default() {
            return 0;
        }
        if let Some(&id) = self.styles.map.get(style) {
            return id;
        }
        let id = match self.styles.free.pop() {
            Some(id) => {
                self.styles.entries[id as usize] = (*style, 0);
                id
            }
            None => {
                if self.styles.entries.len() > u16::MAX as usize {
                    return 0;
                }
                self.styles.entries.push((*style, 0));
                (self.styles.entries.len() - 1) as u16
            }
        };
        self.styles.map.insert(*style, id);
        if self.styles.live() > STANDARD_STYLES {
            self.oversized = true;
        }
        id
    }

    /// The style for an id. Unknown ids read as the default style.
    #[inline]
    pub fn style(&self, id: u16) -> Style {
        if id == 0 { Style::default() } else { self.styles.entries.get(id as usize).map(|e| e.0).unwrap_or_default() }
    }

    /// True when `id` is a live entry holding `style`, so a cached id can be used again.
    #[inline]
    pub fn style_id_valid(&self, id: u16, style: &Style) -> bool {
        if id == 0 {
            return style.is_default();
        }
        self.styles.entries.get(id as usize).is_some_and(|e| e.1 != FREE && e.0 == *style)
    }

    /// Adds `n` references to style `id`.
    #[inline]
    pub fn add_style_refs(&mut self, id: u16, n: u32) {
        if id == 0 || n == 0 {
            return;
        }
        if self.styles.entries[id as usize].1 == FREE {
            // The id was looked up, then a clear released its last cell before the write. Nothing
            // was interned in between, so the entry still holds the same style: bring it back.
            self.styles.free.retain(|&f| f != id);
            self.styles.entries[id as usize].1 = 0;
            let style = self.styles.entries[id as usize].0;
            self.styles.map.entry(style).or_insert(id);
        }
        self.styles.entries[id as usize].1 += n;
    }

    /// Removes one reference from style `id`, freeing it at zero.
    #[inline]
    pub fn release_style(&mut self, id: u16) {
        if id == 0 {
            return;
        }
        let entry = &mut self.styles.entries[id as usize];
        if entry.1 == FREE {
            return;
        }
        entry.1 = entry.1.saturating_sub(1);
        if entry.1 == 0 {
            entry.1 = FREE;
            let style = entry.0;
            self.styles.map.remove(&style);
            self.styles.free.push(id);
        }
    }

    /// A style id that was interned but never referenced is freed again.
    pub fn drop_unused_style(&mut self, id: u16) {
        if id != 0 && self.styles.entries.get(id as usize).is_some_and(|e| e.1 == 0) && self.styles.map.get(&self.styles.entries[id as usize].0) == Some(&id) {
            let style = self.styles.entries[id as usize].0;
            self.styles.entries[id as usize].1 = FREE;
            self.styles.map.remove(&style);
            self.styles.free.push(id);
        }
    }

    /// The reference count of each live style, for the property tests.
    pub fn style_ref_counts(&self) -> Vec<(u16, u32)> {
        let mut v: Vec<(u16, u32)> = self.styles.map.values().map(|&id| (id, self.styles.entries[id as usize].1)).collect();
        v.sort_unstable();
        v
    }

    /// Releases what a cell holds besides its code point: its style reference, grapheme tail and
    /// hyperlink.
    #[inline]
    fn release_cell(&mut self, index: u32, cell: Cell) {
        self.release_style(cell.style_id());
        if cell.is_cluster() {
            self.remove_grapheme(index);
        }
        if cell.has_hyperlink() {
            self.hyperlinks.remove(&index);
        }
    }

    /// True when row `i` may hold cells with a style, grapheme tail or hyperlink that must be
    /// released before they are overwritten.
    #[inline]
    pub fn row_has_side_data(&self, i: usize) -> bool {
        self.rows[i].flags & (row_flags::STYLED | row_flags::GRAPHEME | row_flags::HYPERLINK) != 0
    }

    /// Releases what the cell at row `i`, column `col` holds and leaves it unchanged; the caller
    /// overwrites it next with [`Page::put_raw`].
    #[inline]
    pub fn release_at(&mut self, i: usize, col: usize) {
        let index = self.cell_index(i, col);
        let old = self.cells[index as usize];
        self.release_cell(index, old);
    }

    /// Stores `cell` at row `i`, column `col` without touching style counts or side data. The
    /// caller adds the style references (in bulk) and has released the old cell.
    #[inline]
    pub fn put_raw(&mut self, i: usize, col: usize, cell: Cell) {
        let index = self.rows[i].offset as usize + col;
        self.cells[index] = cell;
    }

    /// Stores one cell per byte of `ascii` from column `col` of row `i`, each `base` with the byte
    /// as its code point. Raw like [`Page::put_raw`]: the caller releases and counts.
    #[inline]
    pub fn put_ascii(&mut self, i: usize, col: usize, ascii: &[u8], base: Cell) {
        let start = self.rows[i].offset as usize + col;
        let dst = &mut self.cells[start..start + ascii.len()];
        for (d, &b) in dst.iter_mut().zip(ascii) {
            *d = Cell(base.0 | b as u64);
        }
    }

    /// Records that columns before `upto` of row `i` may hold something (after raw writes).
    #[inline]
    pub fn mark_written(&mut self, i: usize, upto: usize) {
        let r = &mut self.rows[i];
        r.written = r.written.max(upto.min(self.cols as usize) as u16);
    }

    /// Writes `cell` at row `i`, column `col`, releasing what was there and referencing the new
    /// cell's style. The new cell must not carry the cluster or hyperlink flags.
    pub fn write_cell(&mut self, i: usize, col: usize, cell: Cell) {
        let index = self.cell_index(i, col);
        let old = self.cells[index as usize];
        self.release_cell(index, old);
        self.add_style_refs(cell.style_id(), 1);
        self.cells[index as usize] = cell;
        let row = &mut self.rows[i];
        row.written = row.written.max(col as u16 + 1);
        if cell.style_id() != 0 {
            row.flags |= row_flags::STYLED;
        }
        row.flags |= row_flags::DIRTY;
    }

    /// Sets columns `from..to` of row `i` to `blank`, releasing what they held. With
    /// `keep_protected`, cells marked protected are left alone (selective erase).
    pub fn clear_cells(&mut self, i: usize, from: usize, to: usize, blank: Cell, keep_protected: bool) {
        let to = to.min(self.cols as usize);
        if from >= to {
            return;
        }
        let row = self.rows[i];
        let needs_release = row.has(row_flags::STYLED) || row.has(row_flags::GRAPHEME) || row.has(row_flags::HYPERLINK);
        let start = row.offset as usize;
        if !needs_release && !keep_protected {
            self.cells[start + from..start + to].fill(blank);
            self.add_style_refs(blank.style_id(), (to - from) as u32);
            let row = &mut self.rows[i];
            if blank != Cell::BLANK {
                row.written = row.written.max(to as u16);
            }
            if blank.style_id() != 0 {
                row.flags |= row_flags::STYLED;
            }
            row.flags |= row_flags::DIRTY;
            return;
        }
        for col in from..to {
            let index = (start + col) as u32;
            let old = self.cells[index as usize];
            if keep_protected && old.protected() {
                continue;
            }
            if needs_release {
                self.release_cell(index, old);
            }
            self.cells[index as usize] = blank;
        }
        self.add_style_refs(blank.style_id(), self.count_written(i, from, to, blank));
        let row = &mut self.rows[i];
        if blank != Cell::BLANK {
            row.written = row.written.max(to as u16);
        }
        if blank.style_id() != 0 {
            row.flags |= row_flags::STYLED;
        }
        row.flags |= row_flags::DIRTY;
    }

    /// How many cells in `from..to` now equal `blank` (used to count style references after a
    /// clear that may have skipped protected cells).
    fn count_written(&self, i: usize, from: usize, to: usize, blank: Cell) -> u32 {
        if blank.style_id() == 0 {
            return 0;
        }
        self.cells(i)[from..to].iter().filter(|c| **c == blank).count() as u32
    }

    /// Clears the whole row to blank default cells and drops its content flags.
    pub fn clear_row(&mut self, i: usize) {
        let cols = self.cols as usize;
        self.clear_cells(i, 0, cols, Cell::BLANK, false);
        let row = &mut self.rows[i];
        row.flags &= !(row_flags::CONTENT | row_flags::WRAP | row_flags::WRAP_CONTINUATION | row_flags::SEMANTIC_PROMPT | row_flags::PROMPT_CONTINUATION | row_flags::LINE_SIZE);
        row.flags |= row_flags::DIRTY;
    }

    /// The grapheme tail of the cell at `index` (the code points after the first).
    pub fn grapheme(&self, index: u32) -> &[char] {
        match self.graphemes.get(&index) {
            Some(&(start, len)) => &self.grapheme_chars[start as usize..start as usize + len as usize],
            None => &[],
        }
    }

    /// Appends `c` to the grapheme tail of row `i`, column `col`, and marks the cell a cluster.
    /// Returns false when the cluster is already at the 64 code point limit and `c` was dropped.
    pub fn append_grapheme(&mut self, i: usize, col: usize, c: char, limit: usize) -> bool {
        let index = self.cell_index(i, col);
        let tail_len = self.grapheme(index).len();
        if tail_len + 2 > limit {
            return false;
        }
        let mut tail: Vec<char> = Vec::with_capacity(tail_len + 1);
        tail.extend_from_slice(self.grapheme(index));
        tail.push(c);
        self.set_grapheme(index, &tail);
        self.cells[index as usize].set_cluster(true);
        self.rows[i].flags |= row_flags::GRAPHEME | row_flags::DIRTY;
        true
    }

    /// Stores `tail` as the grapheme tail of the cell at `index`, replacing any previous tail.
    pub fn set_grapheme(&mut self, index: u32, tail: &[char]) {
        if let Some(&(start, len)) = self.graphemes.get(&index) {
            if tail.len() <= len as usize {
                let s = start as usize;
                self.grapheme_chars[s..s + tail.len()].copy_from_slice(tail);
                self.grapheme_garbage += len as usize - tail.len();
                self.graphemes.insert(index, (start, tail.len() as u16));
                return;
            }
            self.grapheme_garbage += len as usize;
        }
        self.maybe_compact_graphemes();
        let start = self.grapheme_chars.len() as u32;
        self.grapheme_chars.extend_from_slice(tail);
        self.graphemes.insert(index, (start, tail.len() as u16));
        if self.grapheme_chars.len() - self.grapheme_garbage > STANDARD_GRAPHEME_CHARS {
            self.oversized = true;
        }
    }

    /// Forgets the grapheme tail of the cell at `index`.
    fn remove_grapheme(&mut self, index: u32) {
        if let Some((_, len)) = self.graphemes.remove(&index) {
            self.grapheme_garbage += len as usize;
        }
    }

    /// Rewrites the grapheme store without the space freed tails left behind, once that space is
    /// more than half of it.
    fn maybe_compact_graphemes(&mut self) {
        if self.grapheme_garbage < 64 || self.grapheme_garbage * 2 < self.grapheme_chars.len() {
            return;
        }
        let mut chars = Vec::with_capacity(self.grapheme_chars.len() - self.grapheme_garbage);
        for (start, len) in self.graphemes.values_mut() {
            let s = *start as usize;
            let new_start = chars.len() as u32;
            chars.extend_from_slice(&self.grapheme_chars[s..s + *len as usize]);
            *start = new_start;
        }
        self.grapheme_chars = chars;
        self.grapheme_garbage = 0;
    }

    /// The hyperlink id of the cell at `index`.
    pub fn hyperlink(&self, index: u32) -> Option<u32> {
        self.hyperlinks.get(&index).copied()
    }

    /// Attaches hyperlink `id` to row `i`, column `col`.
    pub fn set_hyperlink(&mut self, i: usize, col: usize, id: u32) {
        let index = self.cell_index(i, col);
        self.hyperlinks.insert(index, id);
        self.cells[index as usize].set_hyperlink(true);
        self.rows[i].flags |= row_flags::HYPERLINK;
    }

    /// Moves rows `top..=bottom` up by `n` (content leaves at the top and blank rows appear at the
    /// bottom), by rotating row headers. Only for whole width scrolls inside one page.
    pub fn rotate_up(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        for i in top..top + n {
            self.clear_row(i);
        }
        self.rows[top..=bottom].rotate_left(n);
        for row in &mut self.rows[top..=bottom] {
            row.flags |= row_flags::DIRTY;
        }
    }

    /// Moves rows `top..=bottom` down by `n` (blank rows appear at the top), by rotating headers.
    pub fn rotate_down(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        for i in bottom + 1 - n..=bottom {
            self.clear_row(i);
        }
        self.rows[top..=bottom].rotate_right(n);
        for row in &mut self.rows[top..=bottom] {
            row.flags |= row_flags::DIRTY;
        }
    }

    /// Copies row `src_row` of `src` (columns `0..cols` of the shorter row) into row `dst_row` of
    /// this page, re-interning styles and copying grapheme tails and hyperlinks.
    pub fn copy_row_from(&mut self, dst_row: usize, src: &Page, src_row: usize) {
        self.clear_row(dst_row);
        let n = self.cols.min(src.cols) as usize;
        let src_header = *src.row(src_row);
        for col in 0..n {
            let cell = src.cells(src_row)[col];
            self.copy_cell_from(dst_row, col, src, src_header.offset + col as u32, cell);
        }
        let keep = row_flags::WRAP | row_flags::WRAP_CONTINUATION | row_flags::SEMANTIC_PROMPT | row_flags::PROMPT_CONTINUATION | row_flags::LINE_SIZE | row_flags::KITTY_PLACEHOLDER;
        self.rows[dst_row].flags |= src_header.flags & keep;
    }

    /// Copies one cell from another page (or this one, through a snapshot) into row `i`, column
    /// `col`, translating its style id and copying its side data.
    pub fn copy_cell_from(&mut self, i: usize, col: usize, src: &Page, src_index: u32, cell: Cell) {
        let style = src.style(cell.style_id());
        let id = self.intern_style(&style);
        let mut out = cell;
        out.set_style_id(id);
        out.set_cluster(false);
        out.set_hyperlink(false);
        self.write_cell(i, col, out);
        if cell.is_cluster() {
            let tail: Vec<char> = src.grapheme(src_index).to_vec();
            let index = self.cell_index(i, col);
            self.set_grapheme(index, &tail);
            self.cells[index as usize].set_cluster(true);
            self.rows[i].flags |= row_flags::GRAPHEME;
        }
        if let Some(link) = cell.has_hyperlink().then(|| src.hyperlink(src_index)).flatten() {
            self.set_hyperlink(i, col, link);
        }
    }

    /// The bytes a page counts against the scrollback budget: its cell and row allocation, which
    /// does not change while the page lives. Side data (styles, grapheme tails, links) is small
    /// next to it and is left out so the count never drifts.
    pub fn budget_bytes(&self) -> usize {
        self.cells.len() * 8 + self.rows.len() * 8 + 256
    }

    /// Bytes this page holds in memory now, including side data.
    pub fn memory_bytes(&self) -> usize {
        self.cells.len() * 8 + self.rows.len() * 8 + self.styles.entries.len() * 24 + self.grapheme_chars.len() * 4 + self.graphemes.len() * 16 + self.hyperlinks.len() * 16 + 128
    }

    /// The text of row `i`: each cell's code points, a space for blank cells, nothing for spacer
    /// cells.
    pub fn row_text(&self, i: usize, out: &mut String) {
        let row = *self.row(i);
        for (col, cell) in self.cells(i).iter().enumerate() {
            if cell.is_spacer() {
                continue;
            }
            match cell.char() {
                None => out.push(' '),
                Some(c) => {
                    out.push(c);
                    if cell.is_cluster() {
                        out.extend(self.grapheme(row.offset + col as u32));
                    }
                }
            }
        }
    }

    /// True when `cell` at the end of a row is a spacer head left by a wide character that wrapped.
    pub fn ends_with_spacer_head(&self, i: usize) -> bool {
        self.cells(i).last().is_some_and(|c| c.wide() == Wide::SpacerHead)
    }

    /// Serialises the page into a compact byte form and compresses it with LZ4.
    pub fn compress(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len as usize * self.cols as usize * 8 / 2);
        put_u16(&mut out, self.cols);
        put_u16(&mut out, self.capacity);
        put_u16(&mut out, self.len);
        out.push(self.oversized as u8);
        for i in 0..self.len as usize {
            put_u16(&mut out, self.rows[i].flags);
            for cell in self.cells(i) {
                out.extend_from_slice(&cell.0.to_le_bytes());
            }
        }
        self.serialize_side_data(&mut out);
        lz4_flex::compress_prepend_size(&out)
    }

    /// Writes the style table, grapheme tails and hyperlinks, with cell indexes rewritten to the
    /// row order the compressed form uses.
    fn serialize_side_data(&self, out: &mut Vec<u8>) {
        put_u32(out, self.styles.entries.len() as u32);
        for (style, refs) in &self.styles.entries {
            put_style(out, style);
            put_u32(out, *refs);
        }
        let mut graphemes: Vec<(u32, &[char])> = Vec::new();
        let mut links: Vec<(u32, u32)> = Vec::new();
        for i in 0..self.len as usize {
            let base = i as u32 * self.cols as u32;
            for col in 0..self.cols as usize {
                let index = self.cell_index(i, col);
                let cell = self.cells[index as usize];
                if cell.is_cluster() {
                    graphemes.push((base + col as u32, self.grapheme(index)));
                }
                if let Some(id) = cell.has_hyperlink().then(|| self.hyperlink(index)).flatten() {
                    links.push((base + col as u32, id));
                }
            }
        }
        put_u32(out, graphemes.len() as u32);
        for (index, tail) in graphemes {
            put_u32(out, index);
            put_u16(out, tail.len() as u16);
            for &c in tail {
                put_u32(out, c as u32);
            }
        }
        put_u32(out, links.len() as u32);
        for (index, id) in links {
            put_u32(out, index);
            put_u32(out, id);
        }
    }

    /// Restores a page from [`Page::compress`] output. Returns None for corrupt input.
    pub fn decompress(bytes: &[u8]) -> Option<Page> {
        let raw = lz4_flex::decompress_size_prepended(bytes).ok()?;
        let mut r = Reader { buf: &raw, pos: 0 };
        let cols = r.u16()?;
        let capacity = r.u16()?;
        let len = r.u16()?;
        let oversized = r.u8()? != 0;
        let mut page = Page::new(cols, capacity);
        page.len = len;
        page.oversized = oversized;
        for i in 0..len as usize {
            page.rows[i].flags = r.u16()?;
            page.rows[i].written = cols;
            for col in 0..cols as usize {
                page.cells[i * cols as usize + col] = Cell(r.u64()?);
            }
        }
        page.deserialize_side_data(&mut r)?;
        Some(page)
    }

    /// Reads back what [`Page::serialize_side_data`] wrote.
    fn deserialize_side_data(&mut self, r: &mut Reader<'_>) -> Option<()> {
        let n = r.u32()? as usize;
        self.styles.entries.clear();
        for id in 0..n {
            let style = r.style()?;
            let refs = r.u32()?;
            self.styles.entries.push((style, refs));
            if id > 0 && refs > 0 && refs != FREE {
                self.styles.map.insert(style, id as u16);
            } else if id > 0 {
                self.styles.free.push(id as u16);
            }
        }
        for _ in 0..r.u32()? {
            let index = r.u32()?;
            let len = r.u16()? as usize;
            let mut tail = Vec::with_capacity(len);
            for _ in 0..len {
                tail.push(char::from_u32(r.u32()?)?);
            }
            self.set_grapheme(index, &tail);
        }
        for _ in 0..r.u32()? {
            let index = r.u32()?;
            let id = r.u32()?;
            self.hyperlinks.insert(index, id);
        }
        Some(())
    }
}

/// Asks the CPU to start loading the first cells of a row the terminal is about to write, so
/// the next line feed does not wait for memory. Scrollback pages are recycled and cold.
#[inline]
fn prefetch_cells(cells: &[Cell], start: usize, cols: usize) {
    #[cfg(all(target_arch = "x86_64", not(miri)))]
    {
        let end = (start + cols.min(32)).min(cells.len());
        let mut k = start;
        while k < end {
            // SAFETY: `k < cells.len()`, so the pointer is inside the slice; a prefetch never
            // faults and does not change memory.
            unsafe { core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(cells.as_ptr().add(k) as *const i8) };
            k += 8;
        }
    }
    #[cfg(not(all(target_arch = "x86_64", not(miri))))]
    {
        let _ = (cells, start, cols);
    }
}

/// Appends a little endian u16.
fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Appends a little endian u32.
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Appends a colour as a tag byte and three component bytes.
fn put_color(out: &mut Vec<u8>, c: Color) {
    match c {
        Color::Default => out.extend_from_slice(&[0, 0, 0, 0]),
        Color::Palette(i) => out.extend_from_slice(&[1, i, 0, 0]),
        Color::Rgb(r, g, b) => out.extend_from_slice(&[2, r, g, b]),
    }
}

/// Appends a style.
fn put_style(out: &mut Vec<u8>, s: &Style) {
    put_color(out, s.fg);
    put_color(out, s.bg);
    put_color(out, s.underline_color);
    put_u16(out, s.attrs);
    out.push(s.underline as u8);
}

/// A cursor over a byte buffer for decompression.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    /// Takes `n` bytes.
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.buf.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(s)
    }

    /// Reads a byte.
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    /// Reads a little endian u16.
    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Reads a little endian u32.
    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Reads a little endian u64.
    fn u64(&mut self) -> Option<u64> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Some(u64::from_le_bytes(a))
    }

    /// Reads a colour.
    fn color(&mut self) -> Option<Color> {
        let b = self.take(4)?;
        Some(match b[0] {
            0 => Color::Default,
            1 => Color::Palette(b[1]),
            _ => Color::Rgb(b[1], b[2], b[3]),
        })
    }

    /// Reads a style.
    fn style(&mut self) -> Option<Style> {
        let fg = self.color()?;
        let bg = self.color()?;
        let underline_color = self.color()?;
        let attrs = self.u16()?;
        let underline = Underline::from_sub(self.u8()? as u16)?;
        Some(Style { fg, bg, underline_color, attrs, underline })
    }
}
