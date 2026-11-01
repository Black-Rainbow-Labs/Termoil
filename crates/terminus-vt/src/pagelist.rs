//! The page list: every row of one screen, scrollback and active area, as a list of pages.
//!
//! Rows are addressed two ways. A row index `y` counts from the oldest row still kept (0) to the
//! newest (`total_rows() - 1`); the active area is the last `rows` of them. A serial is a number
//! given to a row when it is created that never changes while the row exists, so pins (selection
//! ends, prompt marks, search results, the viewport) hold serials and survive pruning. Reflow and
//! clearing the scrollback give rows new serials and move every pin to match.
//!
//! The list has a byte budget. When it is over, whole pages of history are pruned from the
//! front, oldest first. Standard pages go back into a pool; oversized pages are freed. Pages that
//! are entirely history can be compressed with LZ4 by [`PageList::compress_cold`], which the app
//! calls from an idle timer.

use std::collections::VecDeque;

use crate::cell::{Cell, Row, Wide, row_flags};
use crate::page::{Page, standard_rows};

/// A handle to a pin. Pins are created and removed by the owner of the list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PinId(u32);

/// Where a pin points: a row serial and a column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pin {
    /// The row's serial.
    pub serial: u64,
    /// The column.
    pub col: u16,
}

/// Where a cursor ends up after a reflow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReflowCursor {
    /// Active row.
    pub row: u16,
    /// Column.
    pub col: u16,
}

#[derive(Debug)]
enum NodeData {
    Live(Box<Page>),
    Compressed { bytes: Vec<u8> },
}

#[derive(Debug)]
struct Node {
    first_serial: u64,
    len: usize,
    data: NodeData,
}

impl Node {
    /// Bytes this node counts against the budget. The value is fixed for a live page (its
    /// allocation) so the running total stays exact as the page's contents change.
    fn bytes(&self) -> usize {
        match &self.data {
            NodeData::Live(p) => p.budget_bytes(),
            NodeData::Compressed { bytes, .. } => bytes.len() + 64,
        }
    }
}

/// Scrollback limits for a page list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollbackLimit {
    /// Largest number of bytes the list may hold; pages are pruned beyond it.
    pub bytes: usize,
    /// Largest number of history lines, if set.
    pub lines: Option<usize>,
}

impl Default for ScrollbackLimit {
    /// 50 MB per terminal, the Ghostty 1.4 default.
    fn default() -> Self {
        ScrollbackLimit { bytes: 50_000_000, lines: None }
    }
}

/// All rows of one screen.
#[derive(Debug)]
pub struct PageList {
    nodes: VecDeque<Node>,
    cols: u16,
    rows: u16,
    total: usize,
    next_serial: u64,
    limit: ScrollbackLimit,
    history: bool,
    bytes: usize,
    // Boxed so a page moves between the pool and the list without copying its header.
    #[allow(clippy::vec_box)]
    pool: Vec<Box<Page>>,
    pins: Vec<Option<Pin>>,
    free_pins: Vec<u32>,
    pruned_pages: u64,
}

impl PageList {
    /// A list with `rows` blank active rows at `cols` columns. Without `history` it never keeps
    /// rows above the active area (the alternate screen).
    pub fn new(cols: u16, rows: u16, limit: ScrollbackLimit, history: bool) -> PageList {
        let mut list = PageList {
            nodes: VecDeque::new(),
            cols: cols.max(1),
            rows: rows.max(1),
            total: 0,
            next_serial: 0,
            limit,
            history,
            bytes: 0,
            pool: Vec::new(),
            pins: Vec::new(),
            free_pins: Vec::new(),
            pruned_pages: 0,
        };
        for _ in 0..list.rows {
            list.push_blank_row();
        }
        list
    }

    /// Columns.
    #[inline]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Active rows.
    #[inline]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Rows kept, history plus active.
    #[inline]
    pub fn total_rows(&self) -> usize {
        self.total
    }

    /// Rows of history above the active area.
    #[inline]
    pub fn history_rows(&self) -> usize {
        self.total - self.rows as usize
    }

    /// The serial of the oldest kept row.
    #[inline]
    pub fn first_serial(&self) -> u64 {
        self.nodes.front().map(|n| n.first_serial).unwrap_or(self.next_serial)
    }

    /// Bytes held, live pages at their size and compressed pages at their compressed size.
    #[inline]
    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    /// Number of pages, live and compressed.
    pub fn page_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of compressed pages.
    pub fn compressed_count(&self) -> usize {
        self.nodes.iter().filter(|n| matches!(n.data, NodeData::Compressed { .. })).count()
    }

    /// Pages pruned since the list was made.
    pub fn pruned_pages(&self) -> u64 {
        self.pruned_pages
    }

    /// The scrollback limit.
    pub fn limit(&self) -> ScrollbackLimit {
        self.limit
    }

    /// Changes the scrollback limit and prunes to it.
    pub fn set_limit(&mut self, limit: ScrollbackLimit) {
        self.limit = limit;
        self.prune();
    }

    /// The row index of active row `r`.
    #[inline]
    pub fn active_y(&self, r: u16) -> usize {
        self.total - self.rows as usize + r as usize
    }

    /// The serial of the row at index `y`.
    #[inline]
    pub fn serial_of(&self, y: usize) -> u64 {
        self.first_serial() + y as u64
    }

    /// The row index of a serial, or None when that row was pruned or never existed.
    pub fn y_of(&self, serial: u64) -> Option<usize> {
        let first = self.first_serial();
        (serial >= first && serial < first + self.total as u64).then(|| (serial - first) as usize)
    }

    /// Finds the node and the row inside it for row index `y`. Searches from the end, since
    /// nearly every access is to the active area.
    #[inline]
    fn locate(&self, y: usize) -> (usize, usize) {
        let serial = self.first_serial() + y as u64;
        let n = self.nodes.len();
        let last = &self.nodes[n - 1];
        if serial >= last.first_serial {
            return (n - 1, (serial - last.first_serial) as usize);
        }
        let idx = self.nodes.partition_point(|node| node.first_serial <= serial) - 1;
        (idx, (serial - self.nodes[idx].first_serial) as usize)
    }

    /// Makes node `idx` live, decompressing it if needed, and returns its page.
    fn live(&mut self, idx: usize) -> &mut Page {
        if let NodeData::Compressed { bytes, .. } = &self.nodes[idx].data {
            let page = Page::decompress(bytes).unwrap_or_else(|| Page::new(self.cols, standard_rows(self.cols)));
            let before = self.nodes[idx].bytes();
            self.nodes[idx].data = NodeData::Live(Box::new(page));
            self.bytes = self.bytes + self.nodes[idx].bytes() - before;
        }
        match &mut self.nodes[idx].data {
            NodeData::Live(p) => p,
            NodeData::Compressed { .. } => unreachable!("decompressed above"),
        }
    }

    /// The page and row index holding row `y`, mutable (decompresses if needed).
    #[inline]
    pub fn row_mut(&mut self, y: usize) -> (&mut Page, usize) {
        let (n, r) = self.locate(y);
        (self.live(n), r)
    }

    /// The page and row index of active row `r`, mutable.
    #[inline]
    pub fn active_mut(&mut self, r: u16) -> (&mut Page, usize) {
        let y = self.active_y(r);
        self.row_mut(y)
    }

    /// Reads row `y` without decompressing the list's copy: a compressed page is decompressed
    /// into a temporary. The callback gets the page and the row index.
    pub fn with_row<T>(&self, y: usize, f: impl FnOnce(&Page, usize) -> T) -> T {
        let (n, r) = self.locate(y);
        match &self.nodes[n].data {
            NodeData::Live(p) => f(p, r),
            NodeData::Compressed { bytes, .. } => {
                let page = Page::decompress(bytes).unwrap_or_else(|| Page::new(self.cols, 1));
                f(&page, r)
            }
        }
    }

    /// Calls `f` for every row in `range` in order with the page and row index. Each compressed
    /// page is decompressed once into a temporary.
    pub fn for_rows(&self, range: core::ops::Range<usize>, mut f: impl FnMut(usize, &Page, usize)) {
        if range.is_empty() {
            return;
        }
        let (mut n, mut r) = self.locate(range.start);
        let mut y = range.start;
        while y < range.end && n < self.nodes.len() {
            let node = &self.nodes[n];
            let temp;
            let page: &Page = match &node.data {
                NodeData::Live(p) => p,
                NodeData::Compressed { bytes, .. } => {
                    temp = Page::decompress(bytes).unwrap_or_else(|| Page::new(self.cols, 1));
                    &temp
                }
            };
            while r < node.len && y < range.end {
                f(y, page, r);
                r += 1;
                y += 1;
            }
            n += 1;
            r = 0;
        }
    }

    /// The header of row `y`.
    pub fn row_header(&self, y: usize) -> Row {
        self.with_row(y, |p, r| *p.row(r))
    }

    /// The text of row `y` with trailing blanks kept.
    pub fn row_text(&self, y: usize) -> String {
        let mut s = String::new();
        self.with_row(y, |p, r| p.row_text(r, &mut s));
        s
    }

    /// Takes a page from the pool or allocates a standard one.
    fn new_page(&mut self) -> Box<Page> {
        let cap = if self.history { standard_rows(self.cols) } else { self.rows.max(1) };
        match self.pool.pop() {
            Some(p) if p.cols() == self.cols && p.capacity() == cap as usize => p,
            _ => Box::new(Page::new(self.cols, cap)),
        }
    }

    /// Appends a blank row at the end.
    fn push_blank_row(&mut self) {
        let need_page = match self.nodes.back() {
            Some(Node { data: NodeData::Live(p), .. }) => p.is_full(),
            _ => true,
        };
        if need_page {
            let page = self.new_page();
            let node = Node { first_serial: self.next_serial, len: 0, data: NodeData::Live(page) };
            self.bytes += node.bytes();
            self.nodes.push_back(node);
        }
        let node = self.nodes.back_mut().expect("a page was just ensured");
        if let NodeData::Live(p) = &mut node.data {
            p.push_row();
        }
        node.len += 1;
        self.total += 1;
        self.next_serial += 1;
    }

    /// Scrolls the whole active area up by one: the top active row becomes history and a blank
    /// row appears at the bottom. Without history, the top row is discarded instead.
    pub fn grow(&mut self) {
        if !self.history {
            let rows = self.rows;
            self.scroll_up(0, rows - 1, 1, None);
            return;
        }
        self.push_blank_row();
        self.prune();
    }

    /// Prunes whole pages of history from the front until the list is within its limits.
    pub fn prune(&mut self) {
        while self.nodes.len() > 1 {
            let front_len = self.nodes[0].len;
            if self.total - front_len < self.rows as usize {
                break;
            }
            let over_bytes = self.bytes > self.limit.bytes;
            let over_lines = self.limit.lines.is_some_and(|l| self.history_rows() - front_len >= l);
            let no_history = !self.history;
            if !(over_bytes || over_lines || no_history) {
                break;
            }
            self.pop_front_page();
        }
        self.trim_to_line_limit();
    }

    /// Removes rows from the front of the oldest page so history is at most the line limit.
    fn trim_to_line_limit(&mut self) {
        let Some(limit) = self.limit.lines else { return };
        let excess = self.history_rows().saturating_sub(limit);
        if excess == 0 || self.nodes.is_empty() {
            return;
        }
        let k = excess.min(self.nodes[0].len - usize::from(self.nodes.len() == 1));
        if k == 0 {
            return;
        }
        let before = self.nodes[0].bytes();
        self.live(0).remove_front(k);
        let node = &mut self.nodes[0];
        node.first_serial += k as u64;
        node.len -= k;
        self.total -= k;
        self.bytes = self.bytes - before + self.nodes[0].bytes();
        if self.nodes[0].len == 0 && self.nodes.len() > 1 {
            self.pop_front_page();
        }
    }

    /// Removes the oldest page, returning a standard live page to the pool.
    fn pop_front_page(&mut self) {
        let node = self.nodes.pop_front().expect("caller checked there are at least two pages");
        self.bytes -= node.bytes();
        self.total -= node.len;
        self.pruned_pages += 1;
        if let NodeData::Live(mut page) = node.data
            && !page.is_oversized()
            && self.pool.len() < 4
        {
            page.reset();
            self.pool.push(page);
        }
    }

    /// Compresses every live page that is entirely history, except the `keep_live` pages closest
    /// to the active area. Returns how many pages were compressed.
    pub fn compress_cold(&mut self, keep_live: usize) -> usize {
        let active_start = self.first_serial() + self.history_rows() as u64;
        let mut count = 0;
        let n = self.nodes.len();
        for i in 0..n.saturating_sub(keep_live + 1) {
            let node = &self.nodes[i];
            let fully_history = node.first_serial + node.len as u64 <= active_start;
            if let (true, NodeData::Live(page)) = (fully_history, &node.data) {
                let bytes = page.compress();
                let before = node.bytes();
                self.nodes[i].data = NodeData::Compressed { bytes };
                self.bytes = self.bytes - before + self.nodes[i].bytes();
                count += 1;
            }
        }
        count
    }

    /// Scrolls active rows `top..=bottom` up by `n`. Rows leaving the top are discarded (or, when
    /// `into_history` is given for a region starting at the top of a primary screen, kept as
    /// history). `margins` limits the scroll to columns `left..right`.
    pub fn scroll_up(&mut self, top: u16, bottom: u16, n: u16, margins: Option<(u16, u16)>) {
        let n = n.min(bottom + 1 - top);
        if n == 0 {
            return;
        }
        if let Some((left, right)) = margins {
            return self.scroll_cols_up(top, bottom, n, left, right);
        }
        let (top_y, bottom_y) = (self.active_y(top), self.active_y(bottom));
        let (tn, tr) = self.locate(top_y);
        let (bn, br) = self.locate(bottom_y);
        if tn == bn {
            self.live(tn).rotate_up(tr, br, n as usize);
            return;
        }
        for y in top_y..bottom_y + 1 - n as usize {
            self.copy_row(y + n as usize, y);
        }
        for y in bottom_y + 1 - n as usize..=bottom_y {
            let (p, r) = self.row_mut(y);
            p.clear_row(r);
        }
    }

    /// Scrolls active rows `top..=bottom` down by `n`; blank rows appear at the top.
    pub fn scroll_down(&mut self, top: u16, bottom: u16, n: u16, margins: Option<(u16, u16)>) {
        let n = n.min(bottom + 1 - top);
        if n == 0 {
            return;
        }
        if let Some((left, right)) = margins {
            return self.scroll_cols_down(top, bottom, n, left, right);
        }
        let (top_y, bottom_y) = (self.active_y(top), self.active_y(bottom));
        let (tn, tr) = self.locate(top_y);
        let (bn, br) = self.locate(bottom_y);
        if tn == bn {
            self.live(tn).rotate_down(tr, br, n as usize);
            return;
        }
        for y in (top_y + n as usize..=bottom_y).rev() {
            self.copy_row(y - n as usize, y);
        }
        for y in top_y..top_y + n as usize {
            let (p, r) = self.row_mut(y);
            p.clear_row(r);
        }
    }

    /// Copies row `src` over row `dst` (row indexes), across pages if needed.
    pub fn copy_row(&mut self, src: usize, dst: usize) {
        let tmp = {
            let (p, r) = self.row_mut(src);
            p.extract_row(r)
        };
        let (p, r) = self.row_mut(dst);
        p.copy_row_from(r, &tmp, 0);
    }

    /// Scrolls the cells in columns `left..right` of rows `top..=bottom` up by `n` (DECSLRM).
    fn scroll_cols_up(&mut self, top: u16, bottom: u16, n: u16, left: u16, right: u16) {
        self.clear_split_at_margins(top, bottom, left, right);
        for r in top..=bottom {
            let src = r + n;
            if src <= bottom {
                self.copy_cols(src, r, left, right);
            } else {
                let (p, i) = self.active_mut(r);
                p.clear_cells(i, left as usize, right as usize, Cell::BLANK, false);
            }
        }
    }

    /// Scrolls the cells in columns `left..right` of rows `top..=bottom` down by `n`.
    fn scroll_cols_down(&mut self, top: u16, bottom: u16, n: u16, left: u16, right: u16) {
        self.clear_split_at_margins(top, bottom, left, right);
        for r in (top..=bottom).rev() {
            if r >= top + n {
                self.copy_cols(r - n, r, left, right);
            } else {
                let (p, i) = self.active_mut(r);
                p.clear_cells(i, left as usize, right as usize, Cell::BLANK, false);
            }
        }
    }

    /// Clears wide characters that straddle column `left` or `right` in rows `top..=bottom`, so
    /// moving the columns between the margins never separates a head from its tail.
    fn clear_split_at_margins(&mut self, top: u16, bottom: u16, left: u16, right: u16) {
        for r in top..=bottom {
            let (p, i) = self.active_mut(r);
            p.clear_split_wide(i, left as usize, right as usize);
        }
    }

    /// Copies columns `left..right` from active row `src` to active row `dst`.
    fn copy_cols(&mut self, src: u16, dst: u16, left: u16, right: u16) {
        let tmp = {
            let (p, i) = self.active_mut(src);
            p.extract_row(i)
        };
        let (p, i) = self.active_mut(dst);
        for col in left as usize..right as usize {
            let cell = tmp.cells(0)[col];
            p.copy_cell_from(i, col, &tmp, tmp.cell_index(0, col), cell);
        }
    }

    /// Removes every row of history. Pins in history become invalid.
    pub fn clear_history(&mut self) {
        let keep_from = self.active_y(0);
        let (n, r) = self.locate(keep_from);
        for _ in 0..n {
            let node = self.nodes.pop_front().expect("node index came from locate");
            self.bytes -= node.bytes();
            self.total -= node.len;
        }
        if r > 0 {
            let before = self.nodes[0].bytes();
            self.live(0).remove_front(r);
            let node = &mut self.nodes[0];
            node.first_serial += r as u64;
            node.len -= r;
            self.total -= r;
            self.bytes = self.bytes - before + self.nodes[0].bytes();
        }
    }

    /// Creates a pin at row `y`, column `col`.
    pub fn pin_create(&mut self, y: usize, col: u16) -> PinId {
        let pin = Pin { serial: self.serial_of(y), col };
        match self.free_pins.pop() {
            Some(i) => {
                self.pins[i as usize] = Some(pin);
                PinId(i)
            }
            None => {
                self.pins.push(Some(pin));
                PinId(self.pins.len() as u32 - 1)
            }
        }
    }

    /// Where a pin points, as a row index and column. None when its row was pruned.
    pub fn pin_get(&self, id: PinId) -> Option<(usize, u16)> {
        let pin = self.pins.get(id.0 as usize).copied().flatten()?;
        Some((self.y_of(pin.serial)?, pin.col))
    }

    /// The raw pin value.
    pub fn pin_raw(&self, id: PinId) -> Option<Pin> {
        self.pins.get(id.0 as usize).copied().flatten()
    }

    /// Moves a pin.
    pub fn pin_set(&mut self, id: PinId, y: usize, col: u16) {
        let serial = self.serial_of(y);
        if let Some(slot) = self.pins.get_mut(id.0 as usize) {
            *slot = Some(Pin { serial, col });
        }
    }

    /// Removes a pin.
    pub fn pin_remove(&mut self, id: PinId) {
        if let Some(slot @ Some(_)) = self.pins.get_mut(id.0 as usize) {
            *slot = None;
            self.free_pins.push(id.0);
        }
    }

    /// Every live pin, for the property tests.
    pub fn pins(&self) -> impl Iterator<Item = (PinId, Pin)> + '_ {
        self.pins.iter().enumerate().filter_map(|(i, p)| p.map(|p| (PinId(i as u32), p)))
    }

    /// Changes the size. Columns changes reflow wrapped lines when `reflow` is set; row changes
    /// move rows between history and the active area. Returns where the cursor goes.
    pub fn resize(&mut self, cols: u16, rows: u16, cursor: ReflowCursor, reflow: bool) -> ReflowCursor {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut cursor = cursor;
        if cols != self.cols {
            cursor = if reflow && self.history {
                crate::reflow::reflow(self, cols, cursor)
            } else {
                self.resize_cols_truncate(cols, cursor)
            };
        }
        if rows != self.rows {
            cursor = self.resize_rows(rows, cursor);
        }
        // A narrower width makes more rows; the scrollback limits apply to them too.
        self.prune();
        cursor
    }

    /// Changes the column count without reflow: rows are cut or padded.
    fn resize_cols_truncate(&mut self, cols: u16, cursor: ReflowCursor) -> ReflowCursor {
        let old = std::mem::take(&mut self.nodes);
        let old_cols = self.cols;
        let total = self.total;
        let first = old.front().map(|n| n.first_serial).unwrap_or(self.next_serial);
        let src = PageList {
            nodes: old,
            cols: old_cols,
            rows: self.rows,
            total,
            next_serial: self.next_serial,
            limit: self.limit,
            history: self.history,
            bytes: 0,
            pool: Vec::new(),
            pins: Vec::new(),
            free_pins: Vec::new(),
            pruned_pages: 0,
        };
        self.cols = cols;
        self.total = 0;
        self.bytes = 0;
        self.pool.clear();
        self.next_serial = first;
        for y in 0..total {
            self.push_blank_row();
            let tmp = src.with_row(y, |p, r| p.extract_row(r));
            let (p, r) = self.row_mut(y);
            p.copy_row_from(r, &tmp, 0);
            if cols > old_cols && p.cells(r)[old_cols as usize - 1].wide() == Wide::SpacerHead {
                p.clear_cells(r, old_cols as usize - 1, old_cols as usize, Cell::BLANK, false);
            }
            if cols < old_cols {
                p.row_mut(r).set(row_flags::WRAP, false);
                // A wide character cut at the new last column loses its tail; clear it.
                if p.cells(r)[cols as usize - 1].wide() == Wide::Wide {
                    p.clear_cells(r, cols as usize - 1, cols as usize, Cell::BLANK, false);
                }
            }
        }
        for pin in self.pins.iter_mut().flatten() {
            pin.col = pin.col.min(cols - 1);
        }
        ReflowCursor { row: cursor.row, col: cursor.col.min(cols - 1) }
    }

    /// Changes the number of active rows.
    fn resize_rows(&mut self, rows: u16, cursor: ReflowCursor) -> ReflowCursor {
        let old = self.rows;
        let mut cursor = cursor;
        if rows < old {
            let shrink = old - rows;
            // Blank rows below the cursor are removed first so the cursor's text stays in view.
            let blank_below = self.blank_rows_below(cursor.row);
            let trim = shrink.min(blank_below);
            for _ in 0..trim {
                self.pop_back_row();
            }
            let pushed = shrink - trim;
            self.rows = rows;
            cursor.row = cursor.row.saturating_sub(pushed).min(rows - 1);
            if !self.history {
                self.drop_history_rows();
            }
        } else {
            let grow = rows - old;
            let pull = if self.history { (grow as usize).min(self.history_rows()) as u16 } else { 0 };
            self.rows = rows;
            for _ in 0..(grow - pull) {
                self.push_blank_row();
            }
            cursor.row += pull;
        }
        self.prune();
        cursor
    }

    /// Number of blank rows at the bottom of the active area below the cursor row.
    fn blank_rows_below(&self, cursor_row: u16) -> u16 {
        let mut n = 0;
        for r in (cursor_row + 1..self.rows).rev() {
            let y = self.active_y(r);
            let blank = self.with_row(y, |p, i| p.cells(i).iter().all(|c| *c == Cell::BLANK));
            if !blank {
                break;
            }
            n += 1;
        }
        n
    }

    /// Removes the last row.
    fn pop_back_row(&mut self) {
        let node = self.nodes.back_mut().expect("the list always has rows");
        if let NodeData::Live(p) = &mut node.data {
            p.truncate(node.len - 1);
        }
        node.len -= 1;
        self.total -= 1;
        self.next_serial -= 1;
        if node.len == 0 && self.nodes.len() > 1 {
            let node = self.nodes.pop_back().expect("checked above");
            self.bytes -= node.bytes();
        }
    }

    /// Drops rows above the active area (for a list without history after it shrinks).
    fn drop_history_rows(&mut self) {
        let extra = self.history_rows();
        if extra == 0 {
            return;
        }
        let tmp_rows: Vec<Page> =
            (0..self.rows).map(|r| self.with_row(self.active_y(r), |p, i| p.extract_row(i))).collect();
        let first = self.first_serial() + extra as u64;
        self.nodes.clear();
        self.total = 0;
        self.bytes = 0;
        self.next_serial = first;
        self.pool.clear();
        for (r, tmp) in tmp_rows.iter().enumerate() {
            self.push_blank_row();
            let (p, i) = self.row_mut(r);
            p.copy_row_from(i, tmp, 0);
        }
    }

    /// Replaces the contents with rows built by the reflow module.
    pub(crate) fn replace_from_reflow(&mut self, cols: u16, built: Vec<Page>, first_serial: u64, total: usize) {
        self.nodes.clear();
        self.pool.clear();
        self.cols = cols;
        self.bytes = 0;
        let mut serial = first_serial;
        for page in built {
            let len = page.len();
            let node = Node { first_serial: serial, len, data: NodeData::Live(Box::new(page)) };
            serial += len as u64;
            self.bytes += node.bytes();
            self.nodes.push_back(node);
        }
        self.total = total;
        self.next_serial = serial;
        while self.total < self.rows as usize {
            self.push_blank_row();
        }
    }

    /// Rewrites every pin with `f`, used by reflow.
    pub(crate) fn map_pins(&mut self, mut f: impl FnMut(Pin) -> Option<Pin>) {
        for slot in self.pins.iter_mut() {
            if let Some(p) = slot {
                *slot = f(*p).or(Some(Pin { serial: 0, col: 0 }));
            }
        }
    }

    /// The list's pages in order, decompressing into temporaries, for reflow.
    pub(crate) fn snapshot_rows(&self) -> Vec<(u64, Page)> {
        let mut out = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let page = match &node.data {
                NodeData::Live(p) => (**p).clone(),
                NodeData::Compressed { bytes, .. } => {
                    Page::decompress(bytes).unwrap_or_else(|| Page::new(self.cols, 1))
                }
            };
            out.push((node.first_serial, page));
        }
        out
    }

    /// Marks every active row dirty, so the renderer repaints the whole screen.
    pub fn mark_all_dirty(&mut self) {
        for r in 0..self.rows {
            let (p, i) = self.active_mut(r);
            p.row_mut(i).flags |= row_flags::DIRTY;
        }
    }

    /// True when the cell at active row `r`, column `col` is the spacer tail of a wide character.
    pub fn is_spacer_tail(&mut self, r: u16, col: u16) -> bool {
        let (p, i) = self.active_mut(r);
        p.cells(i).get(col as usize).is_some_and(|c| c.wide() == Wide::SpacerTail)
    }
}
