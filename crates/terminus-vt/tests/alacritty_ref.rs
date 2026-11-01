//! C1: Alacritty's ref tests replayed through terminus-vt.
//!
//! Each case in `corpus/alacritty-ref/<name>/` is Alacritty's recording, its size and its config,
//! plus `expected.txt`: Alacritty's recorded grid translated once into the comparison format below
//! (`TERMINUS_ALACRITTY_GRIDS=<dir holding <name>/grid.json>` regenerates it). Where Terminus
//! differs from Alacritty on purpose, `terminus.txt` is the expected file and `NOTE.md` says why.
//!
//! Comparison format, one line per row: `S<r>` for screen rows top to bottom, then `H<k>` for
//! history rows newest first (trailing blank history rows dropped). A row is an optional `[wrap]`
//! and its cells up to the last non blank one, each `'text'` plus width, colours, underline and
//! attributes. Colours compare by value: SGR 31 and 38;5;1 are both `p1`. Rules applied to both
//! sides: a tab Alacritty stores in a cell reads as a space (Terminus does not store tabs);
//! blink and overline are not compared (Alacritty does not keep them); hyperlinks compare by URI.

// Test tables are written inline as tuples; naming each one would not help a reader.
#![allow(clippy::type_complexity)]

use std::path::{Path, PathBuf};

use serde_json::Value;
use terminus_vt::cell::{Wide, row_flags};
use terminus_vt::page::Page;
use terminus_vt::pagelist::ScrollbackLimit;
use terminus_vt::style::{Color, Underline, attr};
use terminus_vt::{Terminal, TerminalConfig};

/// The corpus directory.
fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/alacritty-ref")
}

/// The test cases, sorted.
fn cases() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(corpus())
        .expect("corpus")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("alacritty.recording").exists())
        .collect();
    v.sort();
    v
}

/// One cell in comparison form.
struct CellRepr {
    text: String,
    wide: &'static str,
    fg: String,
    bg: String,
    ul: &'static str,
    ulc: String,
    attrs: Vec<&'static str>,
    link: String,
}

impl CellRepr {
    /// The cell as one string.
    fn render(&self) -> String {
        let mut s = format!("'{}'", self.text);
        for (k, v) in [
            ("", self.wide),
            ("fg=", self.fg.as_str()),
            ("bg=", self.bg.as_str()),
            ("ul=", self.ul),
            ("ulc=", self.ulc.as_str()),
            ("link=", self.link.as_str()),
        ] {
            if !v.is_empty() {
                s.push(' ');
                s.push_str(k);
                s.push_str(v);
            }
        }
        for a in &self.attrs {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

/// Joins cells into a row string, dropping trailing blank cells.
fn render_row(wrap: bool, cells: &[String]) -> String {
    let last = cells.iter().rposition(|c| c != "' '").map(|p| p + 1).unwrap_or(0);
    let mut out = if wrap { "[wrap]".to_string() } else { String::new() };
    for c in &cells[..last] {
        if !out.is_empty() {
            out.push_str(" | ");
        }
        out.push_str(c);
    }
    out
}

/// A Terminus colour in comparison form (empty for the default).
fn color(c: Color) -> String {
    match c {
        Color::Default => String::new(),
        Color::Palette(i) => format!("p{i}"),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// One Terminus cell in comparison form.
fn terminus_cell(term: &Terminal, page: &Page, row: usize, col: usize) -> String {
    let cell = page.cells(row)[col];
    let style = page.style(cell.style_id());
    let index = page.cell_index(row, col);
    let mut text: String = cell.char().map(|c| c.to_string()).unwrap_or_else(|| " ".into());
    text.extend(page.grapheme(index));
    let wide = match cell.wide() {
        Wide::Narrow => "",
        Wide::Wide => "w2",
        Wide::SpacerTail => "tail",
        Wide::SpacerHead => "head",
    };
    let ul = match style.underline {
        Underline::None => "",
        Underline::Single => "single",
        Underline::Double => "double",
        Underline::Curly => "curly",
        Underline::Dotted => "dotted",
        Underline::Dashed => "dashed",
    };
    let names = [
        (attr::BOLD, "bold"),
        (attr::FAINT, "faint"),
        (attr::ITALIC, "italic"),
        (attr::INVERSE, "inverse"),
        (attr::INVISIBLE, "hidden"),
        (attr::STRIKE, "strike"),
    ];
    let attrs = names.iter().filter(|(b, _)| style.has(*b)).map(|(_, n)| *n).collect();
    let link = cell
        .has_hyperlink()
        .then(|| page.hyperlink(index))
        .flatten()
        .and_then(|id| term.hyperlink(id))
        .map(|h| h.uri.clone())
        .unwrap_or_default();
    let ulc = if style.underline == Underline::None { String::new() } else { color(style.underline_color) };
    CellRepr { text, wide, fg: color(style.fg), bg: color(style.bg), ul, ulc, attrs, link }.render()
}

/// Every row of a Terminus terminal in comparison form.
fn terminus_rows(term: &Terminal) -> Vec<String> {
    let pages = &term.screen().pages;
    let mut out = Vec::new();
    let row_string = |y: usize| {
        pages.with_row(y, |page, i| {
            let cells: Vec<String> = (0..page.cols() as usize).map(|c| terminus_cell(term, page, i, c)).collect();
            render_row(page.row(i).has(row_flags::WRAP), &cells)
        })
    };
    for r in 0..term.rows() {
        out.push(format!("S{r}: {}", row_string(pages.active_y(r))));
    }
    if !term.is_alt_screen() {
        let hist = pages.history_rows();
        for k in 0..hist {
            out.push(format!("H{k}: {}", row_string(hist - 1 - k)));
        }
    }
    trim_blank_history(out)
}

/// Drops blank history rows at the old end.
fn trim_blank_history(mut rows: Vec<String>) -> Vec<String> {
    while rows.last().is_some_and(|r| r.starts_with('H') && r.ends_with(": ")) {
        rows.pop();
    }
    rows
}

/// An Alacritty colour value in comparison form for the foreground or background position.
fn alacritty_color(v: &Value, background: bool) -> String {
    if let Some(name) = v.get("Named").and_then(Value::as_str) {
        const NAMES: [&str; 16] = [
            "Black",
            "Red",
            "Green",
            "Yellow",
            "Blue",
            "Magenta",
            "Cyan",
            "White",
            "BrightBlack",
            "BrightRed",
            "BrightGreen",
            "BrightYellow",
            "BrightBlue",
            "BrightMagenta",
            "BrightCyan",
            "BrightWhite",
        ];
        return match name {
            "Foreground" if !background => String::new(),
            "Background" if background => String::new(),
            n => NAMES.iter().position(|x| *x == n).map(|i| format!("p{i}")).unwrap_or_else(|| format!("named:{n}")),
        };
    }
    if let Some(i) = v.get("Indexed").and_then(Value::as_u64) {
        return format!("p{i}");
    }
    if let Some(s) = v.get("Spec") {
        let c = |k: &str| s.get(k).and_then(Value::as_u64).unwrap_or(0);
        return format!("#{:02x}{:02x}{:02x}", c("r"), c("g"), c("b"));
    }
    String::new()
}

/// One Alacritty cell in comparison form; also returns whether it carries WRAPLINE.
fn alacritty_cell(v: &Value) -> (String, bool) {
    let flags = v.get("flags").and_then(Value::as_str).unwrap_or("");
    let has = |f: &str| flags.split(" | ").any(|x| x.trim() == f);
    let mut text = v.get("c").and_then(Value::as_str).unwrap_or(" ").to_string();
    if text == "\t" {
        text = " ".into();
    }
    let extra = v.get("extra").filter(|e| !e.is_null());
    if let Some(zw) = extra.and_then(|e| e.get("zerowidth")).and_then(Value::as_array) {
        text.extend(zw.iter().filter_map(Value::as_str));
    }
    let wide = if has("WIDE_CHAR") {
        "w2"
    } else if has("WIDE_CHAR_SPACER") {
        "tail"
    } else if has("LEADING_WIDE_CHAR_SPACER") {
        "head"
    } else {
        ""
    };
    let ul = if has("UNDERLINE") {
        "single"
    } else if has("DOUBLE_UNDERLINE") {
        "double"
    } else if has("UNDERCURL") {
        "curly"
    } else if has("DOTTED_UNDERLINE") {
        "dotted"
    } else if has("DASHED_UNDERLINE") {
        "dashed"
    } else {
        ""
    };
    let names = [
        ("BOLD", "bold"),
        ("DIM", "faint"),
        ("ITALIC", "italic"),
        ("INVERSE", "inverse"),
        ("HIDDEN", "hidden"),
        ("STRIKEOUT", "strike"),
    ];
    let attrs = names.iter().filter(|(f, _)| has(f)).map(|(_, n)| *n).collect();
    let link = extra
        .and_then(|e| e.get("hyperlink"))
        .and_then(|h| h.get("inner"))
        .and_then(|i| i.get("uri"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let ulc = match (ul, extra.and_then(|e| e.get("underline_color")).filter(|c| !c.is_null())) {
        ("", _) | (_, None) => String::new(),
        (_, Some(c)) => alacritty_color(c, false),
    };
    let repr = CellRepr {
        text,
        wide,
        fg: alacritty_color(&v["fg"], false),
        bg: alacritty_color(&v["bg"], true),
        ul,
        ulc,
        attrs,
        link,
    };
    (repr.render(), has("WRAPLINE"))
}

/// Translates an Alacritty grid.json into comparison rows.
fn translate_grid(grid: &Value) -> Vec<String> {
    let lines = grid["lines"].as_u64().expect("lines") as usize;
    let inner = grid["raw"]["inner"].as_array().expect("rows");
    let row_string = |i: usize| {
        let cells = inner[i]["inner"].as_array().expect("cells");
        let mut wrap = false;
        let strings: Vec<String> = cells
            .iter()
            .map(|c| {
                let (s, w) = alacritty_cell(c);
                wrap |= w;
                s
            })
            .collect();
        render_row(wrap, &strings)
    };
    let mut out = Vec::new();
    for r in 0..lines {
        out.push(format!("S{r}: {}", row_string(lines - 1 - r)));
    }
    for (k, i) in (lines..inner.len()).enumerate() {
        out.push(format!("H{k}: {}", row_string(i)));
    }
    trim_blank_history(out)
}

/// Runs a recording through a fresh terminal with `chunk` deciding read sizes.
fn replay(dir: &Path, chunk: &dyn Fn(usize) -> usize) -> Terminal {
    let size: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("size.json")).expect("size")).expect("size json");
    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).expect("config")).expect("config json");
    let history = cfg["history_size"].as_u64().unwrap_or(0) as usize;
    let config = TerminalConfig {
        scrollback: ScrollbackLimit { bytes: usize::MAX, lines: Some(history) },
        ..TerminalConfig::default()
    };
    let mut term = Terminal::new(
        size["columns"].as_u64().expect("columns") as u16,
        size["screen_lines"].as_u64().expect("lines") as u16,
        config,
    );
    let bytes = std::fs::read(dir.join("alacritty.recording")).expect("recording");
    let mut i = 0;
    let mut k = 0;
    while i < bytes.len() {
        let n = chunk(k).max(1);
        let end = (i + n).min(bytes.len());
        term.advance(&bytes[i..end]);
        i = end;
        k += 1;
    }
    term
}

/// The expected rows for a case: `terminus.txt` when Terminus differs on purpose.
fn expected(dir: &Path) -> Vec<String> {
    let path = if dir.join("terminus.txt").exists() { dir.join("terminus.txt") } else { dir.join("expected.txt") };
    std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// Regenerates `expected.txt` files from Alacritty's grid.json files when asked.
fn translate_if_asked() {
    let Ok(grids) = std::env::var("TERMINUS_ALACRITTY_GRIDS") else { return };
    for dir in cases() {
        let name = dir.file_name().expect("name");
        let grid_path = Path::new(&grids).join(name).join("grid.json");
        let grid: Value =
            serde_json::from_str(&std::fs::read_to_string(&grid_path).expect("grid.json")).expect("grid json");
        let rows = translate_grid(&grid);
        std::fs::write(dir.join("expected.txt"), rows.join("\n") + "\n").expect("write expected");
    }
}

#[test]
fn alacritty_ref_tests() {
    translate_if_asked();
    let mut failures = Vec::new();
    let all = cases();
    let bless = std::env::var("TERMINUS_BLESS").is_ok_and(|v| v == "1");
    for dir in &all {
        if bless && dir.join("NOTE.md").exists() {
            let got = terminus_rows(&replay(dir, &|_| usize::MAX));
            std::fs::write(dir.join("terminus.txt"), got.join("\n") + "\n").expect("write terminus.txt");
        }
        let want = expected(dir);
        let chunkers: [(&str, Box<dyn Fn(usize) -> usize>); 3] = [
            ("whole", Box::new(|_| usize::MAX)),
            ("bytes", Box::new(|_| 1)),
            (
                "random",
                Box::new(|k| {
                    let h = (k as u64 ^ 0x2545_f491).wrapping_mul(0x9e37_79b9_7f4a_7c15);
                    1 + ((h >> 29) % 97) as usize
                }),
            ),
        ];
        for (name, chunk) in chunkers.iter() {
            let got = terminus_rows(&replay(dir, chunk.as_ref()));
            if let Some(d) = terminus_vt::dump::diff(&want, &got) {
                failures.push(format!("{} ({name}): {d}", dir.file_name().unwrap().to_string_lossy()));
                break;
            }
        }
    }
    eprintln!("C1: {} of {} Alacritty ref tests pass", all.len() - failures.len(), all.len());
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
