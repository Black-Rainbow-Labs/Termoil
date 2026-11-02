//! The list of every section and key with a one line description. It is the single source for the
//! documented default file, for unknown key detection, and for the tests that every field is documented.
use super::Config;
use super::keys::{Platform, default_keybindings};

/// One key of a section: its name, its description and, for a key that has no default value, an example
/// value written commented out in the default file.
pub struct Entry {
    pub key: &'static str,
    pub doc: &'static str,
    pub example: Option<&'static str>,
}

/// One `[section]` and its keys. A section named `keybindings.windows` is the table `windows` inside `keybindings`.
pub struct Section {
    pub path: &'static str,
    pub doc: &'static str,
    pub keys: &'static [Entry],
}

/// Shorthand for an entry with a default value.
const fn e(key: &'static str, doc: &'static str) -> Entry {
    Entry { key, doc, example: None }
}

/// Shorthand for an optional entry that has no default value and so needs an example line.
const fn opt(key: &'static str, doc: &'static str, example: &'static str) -> Entry {
    Entry { key, doc, example: Some(example) }
}

/// Every section of the configuration file, in the order the default file writes them.
pub static SECTIONS: &[Section] = &[
    Section {
        path: "font",
        doc: "Font face chain and size.",
        keys: &[
            e("family", "Faces to try in order; the first installed one is used."),
            e("size", "Size in points."),
            e("ligatures", "Draw programming ligatures."),
            e("ligatures_off", "Families drawn without ligatures even when ligatures is on, such as [\"Fira Code\"]."),
            e("hinting", "Hint glyphs to the pixel grid. Off draws them at their outline shapes."),
            e("line_height", "Line height as a multiple of the font's natural height."),
            e("fallback", "Extra faces tried for glyphs the main face lacks."),
        ],
    },
    Section {
        path: "theme",
        doc: "Colour scheme. `theme = \"Name\"` is the same as `name`. dark and light follow the OS when both are set.",
        keys: &[
            e("name", "Theme name; see `terminus +list-themes`."),
            opt("dark", "Theme used when the OS is in dark mode.", "\"Unluminous Dark\""),
            opt("light", "Theme used when the OS is in light mode.", "\"Unluminous Light\""),
        ],
    },
    Section {
        path: "colors",
        doc: "Colour overrides applied on top of the theme. Every key is optional.",
        keys: &[
            opt("foreground", "Default text colour.", "\"#dcdcdc\""),
            opt("background", "Default background colour.", "\"#1e1e1e\""),
            opt("cursor", "Cursor colour.", "\"#f5f5f5\""),
            opt("selection_background", "Selection background.", "\"#3a4a63\""),
            opt("selection_foreground", "Selection text colour.", "\"#ffffff\""),
            e("palette", "Up to 16 colours replacing palette entries 0 to 15, as \"#rrggbb\"."),
        ],
    },
    Section {
        path: "cursor",
        doc: "Cursor shape and blinking.",
        keys: &[
            e("style", "block, bar or underline."),
            e("blink", "Blink the cursor."),
            e("blink_interval_ms", "Milliseconds between blinks."),
        ],
    },
    Section {
        path: "window",
        doc: "Window size, chrome and rendering.",
        keys: &[
            e("padding", "Horizontal and vertical padding around the grid, in pixels."),
            e("opacity", "Window opacity from 0.0 to 1.0."),
            e("blur", "Blur what is behind a translucent window where the OS supports it."),
            e("decorations", "Draw the OS title bar and borders."),
            e("columns", "Columns in a new window."),
            e("rows", "Rows in a new window."),
            e("minimum_contrast", "Lift text colours that fall below this contrast ratio against their background."),
            e("renderer", "auto, gpu or cpu."),
            e("title_follows_cwd", "Show the working directory in the title when the program does not set one."),
            e("show_tab_bar", "Show the tab bar."),
            e("palette_hint", "Show the one line hint about the command palette until it is first used."),
            e("quick_terminal_edge", "Screen edge the quick terminal slides from: top, bottom, left or right."),
            e("quick_terminal_hotkey", "Global hotkey that toggles the quick terminal."),
            e(
                "shader",
                "Path to a WGSL file that runs as a post process pass over every frame (relative paths are read from the folder of this file; empty means none). The file defines fn terminus_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>, see docs/shaders.md. Only the GPU renderer runs it; the CPU renderer ignores this setting. A shader with an error is reported in the window's banner and the window draws without it.",
            ),
            e(
                "shader_poll_ms",
                "Milliseconds between checks of the shader file's modified time, so an edit applies without a restart.",
            ),
            e(
                "repaint_retry_ms",
                "Milliseconds before a frame the window could not present (covered, minimised, or a surface being resized) is drawn again. Each further miss doubles the wait.",
            ),
            e(
                "repaint_retry_max_ms",
                "The longest wait between attempts to draw a frame the window could not present.",
            ),
        ],
    },
    Section {
        path: "tab_bar",
        doc: "The tab bar's sizes. Its colours come from the theme.",
        keys: &[
            e("padding", "Pixels above and below the tab titles."),
            e("corner_radius", "Corner radius of a tab, in pixels."),
            e("inset", "Pixels between a tab and its neighbours and the bar's edges."),
            e("underline", "Thickness of the accent line under the tab that is showing, in pixels."),
            e("min_columns", "The narrowest a tab is drawn, in columns, before tabs start being left off the end."),
            e("max_columns", "The widest a tab is drawn, in columns."),
            e("show_with_one_tab", "Show the tab bar, with its new tab button, when the window has one tab."),
        ],
    },
    Section {
        path: "scrollback",
        doc: "How much history each pane keeps.",
        keys: &[
            e("limit_bytes", "Memory limit for scrollback in bytes."),
            opt("limit_lines", "Also limit scrollback to this many lines.", "100000"),
        ],
    },
    Section {
        path: "session",
        doc: "Saving the open windows, tabs and splits on exit and reopening them on start.",
        keys: &[
            e(
                "restore",
                "Save windows, tabs, splits, working directories and recent output on quit, and reopen them on start.",
            ),
            e("scrollback_lines", "Lines of each pane's history saved for restore."),
        ],
    },
    Section {
        path: "shell",
        doc: "The program a new pane runs.",
        keys: &[
            opt("program", "Program to run; the registered shell when unset.", "\"pwsh\""),
            e("args", "Arguments passed to the program."),
            opt("cwd", "Directory a new pane starts in; the current one when unset.", "\"~\""),
            e("integration", "Inject the shell integration scripts."),
            e("env", "Extra environment variables for the program, as a table."),
        ],
    },
    Section {
        path: "clipboard",
        doc: "Copy, paste and OSC 52.",
        keys: &[
            e("copy_on_select", "Copy the selection as soon as it is made."),
            e("allow_osc52_read", "Let programs read the clipboard through OSC 52."),
            e("paste_confirm_newline", "Ask before pasting text that contains a newline."),
            e("word_separators", "Characters that end a word for double click selection."),
        ],
    },
    Section {
        path: "notify",
        doc: "Desktop notifications and the bell.",
        keys: &[
            e("after_seconds", "Notify when a command longer than this finishes in an unfocused window."),
            e("on_bell", "visual, audible or none."),
            e("command_finished", "Raise the notification for finished commands."),
        ],
    },
    Section {
        path: "accessibility",
        doc: "Screen reader access: the focused pane's screen is exposed as text.",
        keys: &[
            e("enabled", "Expose the screen to UI Automation, NSAccessibility and AT-SPI."),
            e("update_interval_ms", "Shortest time between accessibility tree updates while the screen changes."),
        ],
    },
    Section {
        path: "mouse",
        doc: "Pointer behaviour.",
        keys: &[
            e("focus_follows_mouse", "Focus the pane under the pointer."),
            e("hide_while_typing", "Hide the pointer while typing."),
            e("scroll_multiplier", "Lines scrolled per wheel notch."),
        ],
    },
    Section {
        path: "openers",
        doc: "Commands run when a path or URL is clicked. Empty means the platform opener.",
        keys: &[
            e("path", "Template for paths; {path}, {line} and {col} are replaced."),
            e("url", "Template for URLs; {url} is replaced."),
        ],
    },
    Section {
        path: "keybindings",
        doc: "Bindings added to the defaults. Each is { keys = \"ctrl+shift+t\", action = \"new-tab\" }. A sequence is \
              written \"ctrl+a>c\". action = \"none\" removes a default. `table` puts a binding in a key table, \
              used only while the action activate-key-table (args { name, once }) has made the table active; \
              keys = \"catch_all\" in a table takes every key the table binds no other way.",
        keys: &[e("bindings", "Your bindings; see the comment below for the defaults.")],
    },
    Section {
        path: "keybindings.windows",
        doc: "Bindings that apply only on Windows, after the common ones.",
        keys: &[e("bindings", "Windows only bindings.")],
    },
    Section {
        path: "keybindings.macos",
        doc: "Bindings that apply only on macOS, after the common ones.",
        keys: &[e("bindings", "macOS only bindings.")],
    },
    Section {
        path: "keybindings.linux",
        doc: "Bindings that apply only on Linux, after the common ones.",
        keys: &[e("bindings", "Linux only bindings.")],
    },
    Section { path: "updates", doc: "Update check.", keys: &[e("check", "Check for a newer release at startup.")] },
    Section { path: "log", doc: "Logging.", keys: &[e("level", "error, warn, info, debug or trace.")] },
];

/// The section at a dotted path such as `window` or `keybindings.macos`.
pub fn section(path: &str) -> Option<&'static Section> {
    SECTIONS.iter().find(|s| s.path == path)
}

/// Looks up the value at a dotted path in a serialized configuration table.
fn lookup<'a>(table: &'a toml::Table, section: &str, key: &str) -> Option<&'a toml::Value> {
    let mut current = table;
    for part in section.split('.') {
        current = current.get(part)?.as_table()?;
    }
    current.get(key)
}

/// Formats a value for the default file. Tables are written inline.
fn format_value(value: &toml::Value) -> String {
    match value {
        toml::Value::Table(t) if t.is_empty() => "{}".to_string(),
        toml::Value::Table(t) => {
            let inner: Vec<String> = t.iter().map(|(k, v)| format!("{k} = {}", format_value(v))).collect();
            format!("{{ {} }}", inner.join(", "))
        }
        other => other.to_string(),
    }
}

/// One binding as a commented line of the default file.
fn binding_comment(keys: &str, action: &str) -> String {
    format!("#   {{ keys = \"{keys}\", action = \"{action}\" }},\n")
}

/// The commented list of default bindings written inside the `[keybindings]` section.
fn keybinding_defaults_comment() -> String {
    let mut text = String::from(
        "# Default bindings on Windows and Linux (macOS uses Cmd, see the list in the docs):\n# bindings = [\n",
    );
    for b in default_keybindings() {
        text.push_str(&binding_comment(&b.keys, &b.action));
    }
    text.push_str("# ]\n");
    text
}

/// Writes one section of the default file: a comment, the header and one commented line per key.
fn write_section(out: &mut String, defaults: &toml::Table, section: &Section) {
    out.push_str(&format!("# {}\n[{}]\n", section.doc, section.path));
    if section.path == "keybindings" {
        out.push_str(&keybinding_defaults_comment());
    }
    for entry in section.keys {
        out.push_str(&format!("# {}\n", entry.doc));
        match (lookup(defaults, section.path, entry.key), entry.example) {
            (Some(value), _) => out.push_str(&format!("{} = {}\n", entry.key, format_value(value))),
            (None, Some(example)) => out.push_str(&format!("# {} = {}\n", entry.key, example)),
            (None, None) => out.push_str(&format!("# {} has no default value\n", entry.key)),
        }
    }
    out.push('\n');
}

impl Config {
    /// The documented default file printed by `terminus +show-config --default`: every section and key with its
    /// default value and a one line comment. Parsing it gives `Config::default()`.
    pub fn default_file_text() -> String {
        let defaults = toml::Table::try_from(Config::default()).unwrap_or_default();
        let mut out = String::from(
            "# Terminus configuration. Every key is optional; a missing key uses the value shown here.\n# The file is reloaded when it changes.\n\n",
        );
        for section in SECTIONS {
            write_section(&mut out, &defaults, section);
        }
        out
    }

    /// The effective configuration as TOML, printed by `terminus +show-config`. Keys that have no value are
    /// omitted. The bindings in effect on `platform` follow as comments.
    pub fn effective_text_for(&self, platform: Platform) -> String {
        let mut out = toml::to_string(self).unwrap_or_default();
        out.push_str("\n# Keybindings in effect (defaults, then the file, then the platform section):\n");
        for b in self.resolved_keybindings(platform) {
            out.push_str(&binding_comment(&b.keys, &b.action));
        }
        out
    }

    /// The effective configuration for the platform this binary runs on.
    pub fn effective_text(&self) -> String {
        self.effective_text_for(Platform::current())
    }
}
