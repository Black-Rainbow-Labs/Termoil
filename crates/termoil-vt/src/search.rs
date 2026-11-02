//! Search over the screen and scrollback with a regular expression.
//!
//! Each logical line (rows joined by the wrap flag) is searched as one string, so a match can
//! cross a wrap. Compressed pages are decompressed into temporaries while they are read. Results
//! are row indexes and columns; the host turns the ones it keeps into pins so they survive new
//! output. The host runs this on its search thread and takes the terminal lock in slices of rows.

use regex::Regex;

use crate::pagelist::PageList;
use crate::text::{LogicalLine, for_lines, line_start, logical_line};

/// One match: inclusive first and last cells as (row index, column).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SearchMatch {
    /// First cell.
    pub start: (usize, u16),
    /// Last cell.
    pub end: (usize, u16),
}

/// Compiles a search pattern. `case_sensitive` false folds case.
pub fn compile(pattern: &str, case_sensitive: bool) -> Result<Regex, regex::Error> {
    regex::RegexBuilder::new(pattern).case_insensitive(!case_sensitive).size_limit(1 << 22).build()
}

/// The matches in one logical line.
fn line_matches(line: &LogicalLine, re: &Regex, out: &mut Vec<SearchMatch>) {
    for m in re.find_iter(&line.text) {
        if m.start() == m.end() {
            continue;
        }
        out.push(SearchMatch { start: line.position_at(m.start()), end: line.position_at(m.end() - 1) });
    }
}

/// Every match in the lines that start in `range` (row indexes), in order.
pub fn find_all(pages: &PageList, re: &Regex, range: core::ops::Range<usize>) -> Vec<SearchMatch> {
    let mut out = Vec::new();
    for_lines(pages, range, |line| {
        line_matches(line, re, &mut out);
        true
    });
    out
}

/// Counts the matches in the whole list.
pub fn count(pages: &PageList, re: &Regex) -> usize {
    find_all(pages, re, 0..pages.total_rows()).len()
}

/// The next match after (forward) or before (backward) `from`, wrapping around the list when
/// `wrap` is set.
pub fn find_next(pages: &PageList, re: &Regex, from: (usize, u16), forward: bool, wrap: bool) -> Option<SearchMatch> {
    if forward { find_forward(pages, re, from, wrap) } else { find_backward(pages, re, from, wrap) }
}

/// Searches forward line by line from the line holding `from`.
fn find_forward(pages: &PageList, re: &Regex, from: (usize, u16), wrap: bool) -> Option<SearchMatch> {
    let mut found = None;
    for_lines(pages, from.0..pages.total_rows(), |line| {
        let mut ms = Vec::new();
        line_matches(line, re, &mut ms);
        found = ms.into_iter().find(|m| m.start > from);
        found.is_none()
    });
    if found.is_none() && wrap {
        for_lines(pages, 0..from.0 + 1, |line| {
            let mut ms = Vec::new();
            line_matches(line, re, &mut ms);
            found = ms.into_iter().next();
            found.is_none()
        });
    }
    found
}

/// Searches backward line by line from the line holding `from`.
fn find_backward(pages: &PageList, re: &Regex, from: (usize, u16), wrap: bool) -> Option<SearchMatch> {
    let mut y = line_start(pages, from.0) as isize;
    while y >= 0 {
        let line = logical_line(pages, y as usize);
        let mut ms = Vec::new();
        line_matches(&line, re, &mut ms);
        if let Some(m) = ms.into_iter().rev().find(|m| m.start < from) {
            return Some(m);
        }
        y = if line.first_row == 0 { -1 } else { line_start(pages, line.first_row - 1) as isize };
    }
    if !wrap {
        return None;
    }
    let last = pages.total_rows() - 1;
    let all = find_all(pages, re, line_start(pages, last)..last + 1);
    all.into_iter().last().or_else(|| find_all(pages, re, 0..pages.total_rows()).into_iter().last())
}
