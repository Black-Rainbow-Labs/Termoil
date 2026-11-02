//! Links under the pointer: OSC 8 hyperlinks, and URLs, Unix paths, Windows paths and
//! `file:line:col` found in the text (`docs/TDD.md` section 4.8). Opening only ever happens on a
//! click; output never opens anything. Only http, https, file and mailto URLs are opened.

use std::sync::OnceLock;

use regex::Regex;
use termoil_vt::Terminal;
use termoil_vt::text::{line_start, logical_line};

/// The cells a link covers, as (absolute row, first column, last column) per row.
pub type LinkCells = Vec<(usize, u16, u16)>;

/// A hovered link: the pane it is in and its cells.
pub type Hover = (crate::model::PaneId, LinkCells);

/// What a link opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// A URL.
    Url(String),
    /// A file, with an optional line and column.
    Path { path: String, line: Option<u32>, col: Option<u32> },
}

/// The patterns for links found in text, tried in order.
fn patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            r#"(?:https?|file)://[^\s<>"'`)\]]+|mailto:[^\s<>"'`]+"#,
            r#"[A-Za-z]:\\[^\s<>"'`|:*?]+(?::\d+(?::\d+)?)?"#,
            r#"(?:~|\.{1,2}|[\w.-]+)?/[^\s<>"'`:]+(?::\d+(?::\d+)?)?"#,
        ]
        .iter()
        .map(|p| Regex::new(p).expect("link pattern compiles"))
        .collect()
    })
}

/// The link at absolute row `y`, column `col` of the shown screen: the cell's OSC 8 hyperlink, else
/// a URL or path in the logical line that covers the cell.
pub fn link_at(term: &Terminal, y: usize, col: u16) -> Option<Link> {
    if let Some(uri) = hyperlink_at(term, y, col) {
        return Some(Link::Url(uri));
    }
    let pages = &term.screen().pages;
    let line = logical_line(pages, line_start(pages, y));
    let at = line.offset_of(y, col);
    for re in patterns() {
        if let Some(m) = re.find_iter(&line.text).find(|m| m.start() <= at && at < m.end()) {
            return Some(classify(m.as_str().trim_end_matches(['.', ',', ';', ':'])));
        }
    }
    None
}

/// The cells a link at (y, col) covers, as (absolute row, first column, last column) per row, and
/// whether it is an OSC 8 hyperlink. `detected` also looks for URLs and paths in the text.
pub fn span_at(term: &Terminal, y: usize, col: u16, detected: bool) -> Option<(LinkCells, bool)> {
    if let Some(id) = hyperlink_id(term, y, col) {
        let pages = &term.screen().pages;
        let same = |c: u16| hyperlink_id(term, y, c) == Some(id);
        let (mut a, mut b) = (col, col);
        while a > 0 && same(a - 1) {
            a -= 1;
        }
        while b + 1 < pages.cols() && same(b + 1) {
            b += 1;
        }
        return Some((vec![(y, a, b)], true));
    }
    if !detected {
        return None;
    }
    let pages = &term.screen().pages;
    let line = logical_line(pages, line_start(pages, y));
    let at = line.offset_of(y, col);
    let cols = pages.cols();
    for re in patterns() {
        if let Some(m) = re.find_iter(&line.text).find(|m| m.start() <= at && at < m.end()) {
            let text = m.as_str().trim_end_matches(['.', ',', ';', ':']);
            let (start, end) = (line.position_at(m.start()), line.position_at(m.start() + text.len().max(1) - 1));
            let rows = (start.0..=end.0)
                .map(|r| (r, if r == start.0 { start.1 } else { 0 }, if r == end.0 { end.1 } else { cols - 1 }))
                .collect();
            return Some((rows, false));
        }
    }
    None
}

/// The OSC 8 hyperlink id of a cell.
fn hyperlink_id(term: &Terminal, y: usize, col: u16) -> Option<u32> {
    term.screen().pages.with_row(y, |page, i| {
        let cell = *page.cells(i).get(col as usize)?;
        if !cell.has_hyperlink() {
            return None;
        }
        page.hyperlink(page.cell_index(i, col as usize))
    })
}

/// Every link that starts on rows `top..top + rows`: OSC 8 hyperlinks (one per run of cells with the
/// same link) and URLs and paths in the text, as (link, absolute row, column), top to bottom.
pub fn visible_links(term: &Terminal, top: usize, rows: usize) -> Vec<(Link, usize, u16)> {
    let pages = &term.screen().pages;
    let end = (top + rows).min(pages.total_rows());
    let mut out = Vec::new();
    for y in top..end {
        let mut col = 0;
        while col < pages.cols() {
            match hyperlink_id(term, y, col) {
                Some(id) => {
                    if let Some(h) = term.hyperlink(id) {
                        out.push((Link::Url(h.uri.clone()), y, col));
                    }
                    while col < pages.cols() && hyperlink_id(term, y, col) == Some(id) {
                        col += 1;
                    }
                }
                None => col += 1,
            }
        }
    }
    let mut y = line_start(pages, top);
    while y < end {
        let line = logical_line(pages, y);
        for re in patterns() {
            for m in re.find_iter(&line.text) {
                let (row, c) = line.position_at(m.start());
                if row >= top && row < end && !out.iter().any(|(_, r, cc)| *r == row && *cc == c) {
                    out.push((classify(m.as_str().trim_end_matches(['.', ',', ';', ':'])), row, c));
                }
            }
        }
        y = line.last_row + 1;
    }
    out.sort_by_key(|(_, r, c)| (*r, *c));
    out
}

/// Labels for `n` hints: single letters from the home row outwards, then pairs.
pub fn hint_labels(n: usize) -> Vec<String> {
    const KEYS: &str = "asdfghjklqwertyuiopzxcvbnm";
    let singles: Vec<String> = KEYS.chars().map(String::from).collect();
    if n <= singles.len() {
        return singles.into_iter().take(n).collect();
    }
    KEYS.chars().flat_map(|a| KEYS.chars().map(move |b| format!("{a}{b}"))).take(n).collect()
}

/// The OSC 8 URI of a cell, when it has one.
fn hyperlink_at(term: &Terminal, y: usize, col: u16) -> Option<String> {
    let pages = &term.screen().pages;
    let id = pages.with_row(y, |page, i| {
        let cell = *page.cells(i).get(col as usize)?;
        if !cell.has_hyperlink() {
            return None;
        }
        page.hyperlink(page.cell_index(i, col as usize))
    })?;
    term.hyperlink(id).map(|h| h.uri.clone())
}

/// Turns matched text into a URL or a path with its line and column.
fn classify(text: &str) -> Link {
    if text.contains("://") || text.starts_with("mailto:") {
        return Link::Url(text.to_string());
    }
    let mut parts = text.rsplitn(3, ':').collect::<Vec<_>>();
    parts.reverse();
    let num = |s: &&str| s.parse::<u32>().ok();
    match parts.as_slice() {
        [p, l, c] if num(l).is_some() && num(c).is_some() => {
            Link::Path { path: p.to_string(), line: num(l), col: num(c) }
        }
        [p, l] if num(l).is_some() => Link::Path { path: p.to_string(), line: num(l), col: None },
        _ => Link::Path { path: text.to_string(), line: None, col: None },
    }
}

/// True for the URL schemes Termoil opens.
pub fn allowed_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "file://", "mailto:"].iter().any(|s| lower.starts_with(s))
}

/// Opens a link: URLs through the platform opener; paths through the configured opener template
/// (`{path}`, `{line}`, `{col}`) or the platform opener when the template is empty. A relative
/// path is taken from `cwd`. Returns what was run, for the log.
pub fn open(link: &Link, opener: &str, cwd: Option<&std::path::Path>) -> Option<String> {
    match link {
        Link::Url(u) if allowed_url(u) => {
            crate::platform::open(u);
            Some(u.clone())
        }
        Link::Url(_) => None,
        Link::Path { path, line, col } => {
            let full = expand(path, cwd);
            if opener.trim().is_empty() {
                crate::platform::open(&full);
                return Some(full);
            }
            let cmd = opener
                .replace("{path}", &full)
                .replace("{line}", &line.unwrap_or(1).to_string())
                .replace("{col}", &col.unwrap_or(1).to_string());
            let words = crate::app::split_command(&cmd);
            let (prog, args) = words.split_first()?;
            std::process::Command::new(prog).args(args).spawn().ok()?;
            Some(cmd)
        }
    }
}

/// A path made absolute against `cwd`, with `~` as the home folder.
fn expand(path: &str, cwd: Option<&std::path::Path>) -> String {
    if let Some(rest) = path.strip_prefix('~')
        && let Some(home) = crate::app::home_dir()
    {
        return format!("{}{rest}", home.to_string_lossy());
    }
    let p = std::path::Path::new(path);
    match cwd {
        Some(dir) if p.is_relative() => dir.join(p).to_string_lossy().into_owned(),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termoil_vt::TerminalConfig;

    /// The link at a column of the first row after writing `bytes`.
    fn at(bytes: &[u8], col: u16) -> Option<Link> {
        let mut t = Terminal::new(60, 3, TerminalConfig::default());
        t.advance(bytes);
        let y = t.screen().pages.active_y(0);
        link_at(&t, y, col)
    }

    #[test]
    fn finds_urls_paths_and_osc8() {
        assert_eq!(at(b"see https://example.com/a?b=1. ok", 10), Some(Link::Url("https://example.com/a?b=1".into())));
        assert_eq!(
            at(b"error at src/main.rs:12:5 here", 12),
            Some(Link::Path { path: "src/main.rs".into(), line: Some(12), col: Some(5) })
        );
        assert_eq!(
            at(br"open C:\work\notes.txt now", 9),
            Some(Link::Path { path: r"C:\work\notes.txt".into(), line: None, col: None })
        );
        assert_eq!(
            at(b"\x1b]8;;https://termoil.dev/\x1b\\link\x1b]8;;\x1b\\ plain", 1),
            Some(Link::Url("https://termoil.dev/".into()))
        );
        assert_eq!(at(b"nothing here", 3), None);
        assert!(!allowed_url("javascript:alert(1)"));
        assert!(allowed_url("HTTPS://x"));
    }
}

#[cfg(test)]
mod span_tests {
    use super::*;
    use termoil_vt::TerminalConfig;

    #[test]
    fn spans_cover_the_link_cells() {
        let mut t = Terminal::new(40, 3, TerminalConfig::default());
        t.advance(b"ab \x1b]8;;https://x.dev/\x1b\\link\x1b]8;;\x1b\\ see https://e.com/p. end");
        let y = t.screen().pages.active_y(0);
        assert_eq!(span_at(&t, y, 4, false), Some((vec![(y, 3, 6)], true)));
        assert_eq!(span_at(&t, y, 15, false), None);
        assert_eq!(span_at(&t, y, 15, true), Some((vec![(y, 12, 26)], false)));
    }
}
