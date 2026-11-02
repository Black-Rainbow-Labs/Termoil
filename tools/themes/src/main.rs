//! Converts a folder of iTerm2 colour schemes (`.itermcolors`, XML plist) into
//! `crates/terminus-app/themes/themes.txt`, one theme per line:
//! `name|fg|bg|cursor|selection_bg|selection_fg|c0|...|c15` with colours as `rrggbb`.
//!
//! Usage: `cargo run -p terminus-themes -- <scheme folder> [output folder]`
//! The output folder defaults to `crates/terminus-app/themes` relative to the current directory.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// An sRGB colour as three bytes.
type Rgb = [u8; 3];

/// One theme as read from a scheme file, before it is written out.
struct Scheme {
    name: String,
    foreground: Rgb,
    background: Rgb,
    cursor: Rgb,
    selection_background: Rgb,
    selection_foreground: Option<Rgb>,
    palette: [Rgb; 16],
}

/// Splits plist XML into `(tag, text)` pairs: for `<key>Foo</key>` the pair `("key", "Foo")`, for `<dict>` the
/// pair `("dict", "")` and for `</dict>` the pair `("/dict", "")`.
fn tokenize(xml: &str) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else { break };
        let tag = &rest[start + 1..start + end];
        rest = &rest[start + end + 1..];
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        let name = tag.split_whitespace().next().unwrap_or("").to_string();
        if name.starts_with('/') {
            if name == "/dict" {
                tokens.push((name, String::new()));
            }
            continue;
        }
        let text_end = rest.find('<').unwrap_or(rest.len());
        let text = rest[..text_end].trim().to_string();
        tokens.push((name, text));
    }
    tokens
}

/// Reads the components of a colour dict starting just after its `<dict>` token and returns the colour and the
/// index after the closing `</dict>`. Components are 0..1 floats used as sRGB whatever the declared colour space.
fn read_color(tokens: &[(String, String)], mut i: usize) -> (Option<Rgb>, usize) {
    let mut comps: HashMap<String, f64> = HashMap::new();
    while i < tokens.len() && tokens[i].0 != "/dict" {
        if tokens[i].0 == "key"
            && i + 1 < tokens.len()
            && tokens[i + 1].0 == "real"
            && let Ok(v) = tokens[i + 1].1.parse::<f64>()
        {
            comps.insert(tokens[i].1.clone(), v);
        }
        i += 1;
    }
    let get = |k: &str| comps.get(k).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let color = match (get("Red Component"), get("Green Component"), get("Blue Component")) {
        (Some(r), Some(g), Some(b)) => Some([r, g, b]),
        _ => None,
    };
    (color, i + 1)
}

/// Collects every top level colour entry (`Ansi 0 Color`, `Background Color`, ...) of a plist into a map.
fn read_colors(xml: &str) -> HashMap<String, Rgb> {
    let tokens = tokenize(xml);
    let mut colors = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].0 == "key" && i + 1 < tokens.len() && tokens[i + 1].0 == "dict" {
            let (color, next) = read_color(&tokens, i + 2);
            if let Some(c) = color {
                colors.insert(tokens[i].1.clone(), c);
            }
            i = next;
        } else {
            i += 1;
        }
    }
    colors
}

/// Builds a scheme from a plist's text, or `None` when it lacks the sixteen colours, foreground or background.
fn parse_scheme(name: &str, xml: &str) -> Option<Scheme> {
    let colors = read_colors(xml);
    let mut palette = [[0u8; 3]; 16];
    for (n, slot) in palette.iter_mut().enumerate() {
        *slot = *colors.get(&format!("Ansi {n} Color"))?;
    }
    let foreground = *colors.get("Foreground Color")?;
    let background = *colors.get("Background Color")?;
    let cursor = colors.get("Cursor Color").copied().unwrap_or(foreground);
    let selection_background = colors.get("Selection Color").copied().unwrap_or_else(|| blend(background, foreground));
    let selection_foreground = colors.get("Selected Text Color").copied();
    Some(Scheme {
        name: name.to_string(),
        foreground,
        background,
        cursor,
        selection_background,
        selection_foreground,
        palette,
    })
}

/// Mixes 70 percent of `a` with 30 percent of `b`, used for a selection colour a scheme does not define.
fn blend(a: Rgb, b: Rgb) -> Rgb {
    let mix = |x: u8, y: u8| ((x as f64) * 0.7 + (y as f64) * 0.3).round() as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

/// Formats a colour as six lowercase hex digits.
fn hex(c: Rgb) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Formats a scheme as one line of `themes.txt`.
fn to_line(s: &Scheme) -> String {
    let mut fields = vec![
        s.name.clone(),
        hex(s.foreground),
        hex(s.background),
        hex(s.cursor),
        hex(s.selection_background),
        s.selection_foreground.map(hex).unwrap_or_default(),
    ];
    fields.extend(s.palette.iter().map(|c| hex(*c)));
    fields.join("|")
}

/// Converts `#rrggbb` into bytes.
fn rgb(s: &str) -> Rgb {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).expect("valid hex colour");
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Converts sixteen hex strings into a palette.
fn pal(list: [&str; 16]) -> [Rgb; 16] {
    list.map(rgb)
}

/// The two themes Terminus ships itself. Every Terminus Dark colour except black meets 4.5:1 against its background.
fn builtin_schemes() -> Vec<Scheme> {
    let dark = Scheme {
        name: "Terminus Dark".into(),
        foreground: rgb("#dcdcdc"),
        background: rgb("#1e1e1e"),
        cursor: rgb("#f5f5f5"),
        selection_background: rgb("#3a4a63"),
        selection_foreground: None,
        palette: pal([
            "#45475a", "#ff6b6b", "#7ee787", "#e5c07b", "#6cb6ff", "#d291e4", "#56d4dd", "#dcdcdc", "#8a8a8a",
            "#ff8b8b", "#9ff3a5", "#f5d58f", "#8cc8ff", "#e3aaf2", "#7ae6ee", "#ffffff",
        ]),
    };
    let light = Scheme {
        name: "Terminus Light".into(),
        foreground: rgb("#2b2b2b"),
        background: rgb("#fafafa"),
        cursor: rgb("#2b2b2b"),
        selection_background: rgb("#c9d8f0"),
        selection_foreground: None,
        palette: pal([
            "#2b2b2b", "#b3261e", "#1b6e2d", "#7a5c00", "#1f5fbf", "#8a3ea8", "#00727a", "#5c5c5c", "#555555",
            "#c93c34", "#2c8a41", "#8f6d00", "#3474d4", "#a052be", "#118a93", "#1a1a1a",
        ]),
    };
    vec![dark, light]
}

/// One theme from a `themes.txt` line, for the themes written here rather than converted.
fn scheme_from_line(line: &str) -> Scheme {
    let f: Vec<&str> = line.split('|').collect();
    let mut palette = [[0u8; 3]; 16];
    for (slot, text) in palette.iter_mut().zip(&f[6..]) {
        *slot = rgb(text);
    }
    Scheme {
        name: f[0].into(),
        foreground: rgb(f[1]),
        background: rgb(f[2]),
        cursor: rgb(f[3]),
        selection_background: rgb(f[4]),
        selection_foreground: (!f[5].is_empty()).then(|| rgb(f[5])),
        palette,
    }
}

/// The themes Unluminous ships (task-2468). Ground, text, cursor and selection are Unluminous's `editor`, `text`,
/// `accent` and `text_selection`; Unluminous Dark and Light use Unluminous's own sixteen terminal colours
/// (`unluminous-terminal/src/palette.rs`), the others their scheme's published terminal colours. Their tab bar
/// colours are in `crates/terminus-app/themes/chrome.txt`.
fn unluminous_schemes() -> Vec<Scheme> {
    [
        "Unluminous Dark|e8ebf1|1a1f26|489ff8|304361||242a33|e04a4a|5cc87a|febc2e|489ff8|c37cf0|4fc1c4|c8cedb|535c6b|ff6e6e|7ee79b|ffd566|7cbbff|da9eff|74e0e3|ffffff",
        "Unluminous Light|1e2530|fbfcfd|2a63f0|c6d8ff||24292f|cf222e|116329|8a6100|0969da|8250df|1b7c83|6e7781|57606a|a40e26|1a7f37|633c01|218bff|a475f9|3192aa|8c959f",
        "Islands Dracula Colorful|f8f8f2|282a36|ff79c6|44475a||21222c|ff5555|50fa7b|f1fa8c|bd93f9|ff79c6|8be9fd|f8f8f2|6272a4|ff6e6e|69ff94|ffffa5|d6acff|ff92df|a4ffff|ffffff",
        "Material Palenight|a6accd|292d3e|ab47bc|424865||3a3f58|ff5370|c3e88d|ffcb6b|82aaff|c792ea|89ddff|d0d0d0|676e95|ff5370|c3e88d|ffcb6b|82aaff|c792ea|89ddff|ffffff",
        "Material Deep Ocean|c3cee3|0f111a|84ffff|3b3f51||2c3043|ff5370|c3e88d|ffcb6b|82aaff|c792ea|89ddff|c3cee3|4b526d|ff5370|c3e88d|ffcb6b|82aaff|c792ea|89ddff|ffffff",
        "One Dark|abb2bf|282c34|61afef|3e4451||3f4451|e06c75|98c379|e5c07b|61afef|c678dd|56b6c2|abb2bf|5c6370|e06c75|98c379|e5c07b|61afef|c678dd|56b6c2|ffffff",
    ]
    .iter()
    .map(|l| scheme_from_line(l))
    .collect()
}

/// Reads every `.itermcolors` file in a folder and returns the schemes in directory order.
fn read_folder(dir: &Path) -> Vec<Scheme> {
    let mut schemes = Vec::new();
    let entries = fs::read_dir(dir).expect("scheme folder is readable");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("itermcolors") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").replace('|', "/");
        let Ok(xml) = fs::read_to_string(&path) else { continue };
        match parse_scheme(&name, &xml) {
            Some(s) => schemes.push(s),
            None => eprintln!("skipped {name}: missing colours"),
        }
    }
    schemes
}

/// Asks git for the commit the scheme folder was checked out at, for the README line.
fn source_commit(dir: &Path) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

/// Writes themes.txt, README and, when the source repository has one, LICENSE into the output folder.
fn write_output(schemes: &[Scheme], out: &Path, commit: &str, license: Option<String>) {
    fs::create_dir_all(out).expect("output folder");
    let body: Vec<String> = schemes.iter().map(to_line).collect();
    fs::write(out.join("themes.txt"), body.join("\n") + "\n").expect("write themes.txt");
    let readme = format!(
        "Colour schemes converted from https://github.com/mbadolato/iTerm2-Color-Schemes at commit {commit} (MIT licence, see LICENSE). Regenerate with `cargo run -p terminus-themes -- <scheme folder>`. Terminus Dark and Terminus Light are written by the Terminus authors; Unluminous Dark, Unluminous Light, Islands Dracula Colorful, Material Palenight, Material Deep Ocean and One Dark are the themes Unluminous ships, and chrome.txt holds their tab bar colours.\n"
    );
    fs::write(out.join("README"), readme).expect("write README");
    if let Some(text) = license {
        fs::write(out.join("LICENSE"), text).expect("write LICENSE");
    }
}

/// Entry point: converts the scheme folder given on the command line.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.first() else {
        eprintln!("usage: terminus-themes <scheme folder> [output folder]");
        std::process::exit(2);
    };
    let dir = PathBuf::from(dir);
    let out = args.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("crates/terminus-app/themes"));
    let ours: Vec<Scheme> = builtin_schemes().into_iter().chain(unluminous_schemes()).collect();
    let taken = |name: &str| ours.iter().any(|o| o.name.eq_ignore_ascii_case(name));
    let mut schemes: Vec<Scheme> = read_folder(&dir).into_iter().filter(|s| !taken(&s.name)).collect();
    schemes.extend(ours);
    schemes.sort_by_key(|s| s.name.to_lowercase());
    schemes.dedup_by_key(|s| s.name.to_lowercase());
    let root = dir.parent().unwrap_or(&dir);
    let license = fs::read_to_string(root.join("LICENSE")).ok();
    write_output(&schemes, &out, &source_commit(&dir), license);
    println!("wrote {} themes to {}", schemes.len(), out.display());
}
