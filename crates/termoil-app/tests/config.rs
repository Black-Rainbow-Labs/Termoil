//! Tests for the configuration module: the default file, partial files, errors, renames, key parsing,
//! platform overrides, themes and live reload (A3, F12 and the config part of E5).
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use termoil_app::config::*;

/// Parses text that must be valid, returning the config and its warnings.
fn parse_ok(text: &str) -> (Config, Vec<Warning>) {
    Config::parse(text).unwrap_or_else(|e| panic!("expected a valid config, got {e}"))
}

/// A fresh empty folder under the system temp dir for one test.
fn temp_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termoil-config-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp folder");
    dir
}

#[test]
fn default_file_text_parses_back_to_the_default_config() {
    let text = Config::default_file_text();
    let (config, warnings) = parse_ok(&text);
    assert_eq!(config, Config::default());
    assert!(warnings.is_empty(), "the default file produced warnings: {warnings:?}");
}

#[test]
fn every_field_of_every_section_appears_in_the_default_file() {
    let text = Config::default_file_text();
    let value = toml::Table::try_from(Config::default()).unwrap();
    for section in SECTIONS {
        assert!(text.contains(&format!("[{}]", section.path)), "section {} missing", section.path);
        for entry in section.keys {
            assert!(text.contains(&format!("{} = ", entry.key)), "{}.{} missing", section.path, entry.key);
        }
    }
    // Every serialized field of the default config is documented by the schema.
    for (name, section) in &value {
        let section = section.as_table().expect("every top level value is a section");
        let documented =
            SECTIONS.iter().find(|s| s.path == name).unwrap_or_else(|| panic!("section {name} undocumented"));
        for key in section.keys().filter(|k| !["windows", "macos", "linux"].contains(&k.as_str())) {
            assert!(documented.keys.iter().any(|e| e.key == key.as_str()), "{name}.{key} is not in the schema");
        }
    }
}

#[test]
fn schema_names_every_struct_field_including_optional_ones() {
    // Setting every schema key to a value of the right kind must raise no unknown key warning.
    let text = "[theme]\nname='x'\ndark='a'\nlight='b'\n[colors]\nforeground='#fff'\nbackground='#000'\ncursor='#111'\n\
                selection_background='#222'\nselection_foreground='#333'\n[scrollback]\nlimit_lines=5\n\
                [shell]\nprogram='sh'\ncwd='/'\n";
    let (config, warnings) = parse_ok(text);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.colors.foreground, Some(Rgb([255, 255, 255])));
    assert_eq!(config.scrollback.limit_lines, Some(5));
    assert_eq!(config.shell.program.as_deref(), Some("sh"));
}

#[test]
fn a_partial_file_overrides_only_what_it_sets() {
    let (config, warnings) = parse_ok("[font]\nsize = 16.5\n\n[window]\ncolumns = 80\nrenderer = \"cpu\"\n");
    let mut expected = Config::default();
    expected.font.size = 16.5;
    expected.window.columns = 80;
    expected.window.renderer = Renderer::Cpu;
    assert_eq!(config, expected);
    assert!(warnings.is_empty());
}

#[test]
fn an_empty_file_is_the_default() {
    assert_eq!(parse_ok("").0, Config::default());
    assert_eq!(parse_ok("# only a comment\n").0, Config::default());
}

#[test]
fn theme_can_be_a_string_or_a_table() {
    assert_eq!(parse_ok("theme = \"Dracula\"").0.theme.name, "Dracula");
    let (config, _) = parse_ok("[theme]\nname = \"A\"\ndark = \"B\"\nlight = \"C\"");
    assert_eq!(
        (config.theme.name.as_str(), config.theme.dark.as_deref(), config.theme.light.as_deref()),
        ("A", Some("B"), Some("C"))
    );
    let (config, _) = parse_ok("theme.dark = \"B\"\ntheme.light = \"C\"");
    assert_eq!(config.theme.dark.as_deref(), Some("B"));
}

#[test]
fn a_syntax_error_reports_line_and_column() {
    let error = Config::parse("[font]\nsize = 13\n\n[window\ncolumns = 80\n").unwrap_err();
    assert_eq!(error.line, 4, "{error}");
    assert!(error.column >= 1);
    assert!(!error.message.is_empty());
    let error = Config::parse("[font]\nsize = = 3\n").unwrap_err();
    assert_eq!(error.line, 2, "{error}");
    assert_eq!(error.column, 8, "{error}");
}

#[test]
fn a_value_of_the_wrong_type_is_an_error_on_its_line() {
    let error = Config::parse("[font]\nsize = 13\n[window]\ncolumns = \"wide\"\n").unwrap_err();
    assert_eq!(error.line, 4, "{error}");
    let error = Config::parse("[cursor]\nstyle = \"wavy\"\n").unwrap_err();
    assert_eq!(error.line, 2, "{error}");
    let error = Config::parse("[colors]\nforeground = \"not a colour\"\n").unwrap_err();
    assert_eq!(error.line, 2, "{error}");
}

#[test]
fn an_unknown_key_is_a_warning_with_its_line() {
    let (config, warnings) = parse_ok("stray = true\n[font]\nsize = 14\nsise = 3\n\n[nothing]\nx = 1\n");
    assert_eq!(config.font.size, 14.0);
    let lines: Vec<usize> = warnings.iter().map(|w| w.line).collect();
    assert_eq!(lines, vec![1, 4, 6], "{warnings:?}");
    assert!(warnings[0].message.contains("stray"));
    assert!(warnings[1].message.contains("font.sise"));
    assert!(warnings[2].message.contains("nothing"));
}

#[test]
fn every_rename_table_entry_is_applied_with_a_warning() {
    type Case = (&'static str, fn(&Config) -> bool);
    let cases: &[Case] = &[
        ("font-size = 15.0", |c| c.font.size == 15.0),
        ("font-family = [\"Hack\"]", |c| c.font.family == ["Hack"]),
        ("font-ligatures = false", |c| !c.font.ligatures),
        ("line-height = 1.25", |c| c.font.line_height == 1.25),
        ("[font]\nligature = false", |c| !c.font.ligatures),
        ("[scrollback]\nlines = 1234", |c| c.scrollback.limit_lines == Some(1234)),
        ("[scrollback]\nlimit = 99", |c| c.scrollback.limit_bytes == 99),
        ("scrollback-limit = 98", |c| c.scrollback.limit_bytes == 98),
        ("background-opacity = 0.5", |c| c.window.opacity == 0.5),
        ("[window]\nbackground_opacity = 0.25", |c| c.window.opacity == 0.25),
        ("window-padding = [1, 2]", |c| c.window.padding == [1, 2]),
        ("[window]\ncontrast = 7.0", |c| c.window.minimum_contrast == 7.0),
        ("cursor-style = \"bar\"", |c| c.cursor.style == CursorStyle::Bar),
        ("cursor-blink = true", |c| c.cursor.blink),
        ("copy-on-select = true", |c| c.clipboard.copy_on_select),
        ("[shell]\ncommand = \"fish\"", |c| c.shell.program.as_deref() == Some("fish")),
    ];
    assert_eq!(cases.len(), RENAMES.len(), "a rename has no case");
    for (text, check) in cases {
        let (config, warnings) = parse_ok(text);
        assert!(check(&config), "rename not applied: {text}");
        assert_eq!(warnings.len(), 1, "{text}: {warnings:?}");
        assert!(warnings[0].message.contains("renamed"), "{text}: {warnings:?}");
    }
}

#[test]
fn a_rename_keeps_line_numbers_correct_for_a_later_error() {
    let error = Config::parse("font-size = 12\n\n[window]\ncolumns = \"x\"\n").unwrap_err();
    assert_eq!(error.line, 4, "{error}");
    let (_, warnings) = parse_ok("\n\nfont-size = 12\n");
    assert_eq!(warnings[0].line, 3);
}

#[test]
fn when_old_and_new_names_are_both_set_the_new_one_wins() {
    let (config, warnings) = parse_ok("font-size = 20.0\n[font]\nsize = 11.0\n");
    assert_eq!(config.font.size, 11.0);
    assert!(warnings.iter().any(|w| w.message.contains("ignored")));
}

#[test]
fn out_of_range_values_are_corrected_with_a_warning() {
    let (config, warnings) = parse_ok("[window]\nopacity = 3.0\ncolumns = 0\n[font]\nsize = -4.0\n");
    assert_eq!(config.window.opacity, 1.0);
    assert_eq!(config.window.columns, 1);
    assert_eq!(config.font.size, 13.0);
    let lines: Vec<usize> = warnings.iter().map(|w| w.line).collect();
    assert!(lines.contains(&2) && lines.contains(&3) && lines.contains(&5), "{warnings:?}");
}

#[test]
fn key_parsing_table() {
    let chord = |ctrl, shift, alt, sup, key: &str| KeyChord {
        mods: Mods { ctrl, shift, alt, super_key: sup },
        key: key.to_string(),
    };
    let table: Vec<(&str, Vec<KeyChord>)> = vec![
        ("ctrl+shift+t", vec![chord(true, true, false, false, "t")]),
        ("Ctrl+Shift+T", vec![chord(true, true, false, false, "t")]),
        ("CTRL+SHIFT+t", vec![chord(true, true, false, false, "t")]),
        ("cmd+t", vec![chord(false, false, false, true, "t")]),
        ("command+t", vec![chord(false, false, false, true, "t")]),
        ("super+t", vec![chord(false, false, false, true, "t")]),
        ("alt+left", vec![chord(false, false, true, false, "left")]),
        ("option+Left", vec![chord(false, false, true, false, "left")]),
        ("f11", vec![chord(false, false, false, false, "f11")]),
        ("F11", vec![chord(false, false, false, false, "f11")]),
        ("ctrl+plus", vec![chord(true, false, false, false, "plus")]),
        ("ctrl++", vec![chord(true, false, false, false, "plus")]),
        ("ctrl+minus", vec![chord(true, false, false, false, "-")]),
        ("ctrl+-", vec![chord(true, false, false, false, "-")]),
        ("ctrl+0", vec![chord(true, false, false, false, "0")]),
        ("`", vec![chord(false, false, false, false, "`")]),
        ("ctrl+`", vec![chord(true, false, false, false, "`")]),
        ("ctrl+shift+pgup", vec![chord(true, true, false, false, "pageup")]),
        ("shift+PageDown", vec![chord(false, true, false, false, "pagedown")]),
        ("ctrl+shift+space", vec![chord(true, true, false, false, "space")]),
        ("ctrl+tab", vec![chord(true, false, false, false, "tab")]),
        ("ctrl+alt+shift+super+enter", vec![chord(true, true, true, true, "enter")]),
        ("ctrl+a>c", vec![chord(true, false, false, false, "a"), chord(false, false, false, false, "c")]),
        (
            "ctrl+a > shift+Left > x",
            vec![
                chord(true, false, false, false, "a"),
                chord(false, true, false, false, "left"),
                chord(false, false, false, false, "x"),
            ],
        ),
    ];
    for (text, expected) in table {
        assert_eq!(parse_keys(text), Ok(expected), "parsing {text:?}");
    }
    assert_eq!(normalize_keys("Ctrl+Shift+T").unwrap(), "ctrl+shift+t");
    assert_eq!(normalize_keys("ctrl+a>C").unwrap(), "ctrl+a>c");
}

#[test]
fn invalid_key_strings_are_errors() {
    for text in [
        "",
        " ",
        "ctrl+",
        "ctrl+shift+",
        "hyper+t",
        "ctrl+nosuchkey",
        "ctrl+a>",
        ">c",
        "ctrl+f25",
        "ctrl+f0",
        "ctrl+a>>c",
    ] {
        assert!(parse_keys(text).is_err(), "{text:?} should be rejected");
    }
}

#[test]
fn a_binding_with_bad_keys_warns_on_its_line_and_is_skipped() {
    let text = "[keybindings]\nbindings = [\n  { keys = \"ctrl+q\", action = \"copy\" },\n  { keys = \"hyper+q\", action = \"paste\" },\n]\n";
    let (config, warnings) = parse_ok(text);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].line, 4);
    let resolved = config.resolved_keybindings(Platform::Linux);
    assert!(resolved.iter().any(|b| b.keys == "ctrl+q"));
    assert!(!resolved.iter().any(|b| b.keys == "hyper+q"));
}

#[test]
fn default_keybindings_are_the_prd_list() {
    let defaults = default_keybindings();
    let expected = [
        ("ctrl+shift+t", "new-tab"),
        ("ctrl+shift+d", "split-right"),
        ("ctrl+shift+e", "split-down"),
        ("ctrl+shift+w", "close"),
        ("ctrl+tab", "next-tab"),
        ("ctrl+shift+tab", "previous-tab"),
        ("alt+left", "focus-left"),
        ("alt+right", "focus-right"),
        ("alt+up", "focus-up"),
        ("alt+down", "focus-down"),
        ("ctrl+shift+f", "search"),
        ("ctrl+shift+p", "command-palette"),
        ("ctrl+shift+up", "previous-prompt"),
        ("ctrl+shift+down", "next-prompt"),
        ("ctrl+shift+space", "select-last-output"),
        ("ctrl+shift+c", "copy"),
        ("ctrl+shift+v", "paste"),
        ("shift+pageup", "scroll-page-up"),
        ("shift+pagedown", "scroll-page-down"),
        ("ctrl+shift+home", "scroll-to-top"),
        ("ctrl+shift+end", "scroll-to-bottom"),
        ("ctrl+plus", "increase-font-size"),
        ("ctrl+minus", "decrease-font-size"),
        ("ctrl+0", "reset-font-size"),
        ("f11", "toggle-fullscreen"),
        ("ctrl+shift+n", "new-window"),
        ("ctrl+shift+k", "clear-scrollback"),
        ("ctrl+shift+h", "show-hints"),
        ("ctrl+,", "open-settings"),
        ("ctrl+shift+alt+left", "resize-split"),
        ("ctrl+shift+alt+right", "resize-split"),
        ("ctrl+shift+alt+up", "resize-split"),
        ("ctrl+shift+alt+down", "resize-split"),
    ];
    let got: Vec<(&str, &str)> = defaults.iter().map(|b| (b.keys.as_str(), b.action.as_str())).collect();
    assert_eq!(got, expected);
    for platform in [Platform::Windows, Platform::MacOs, Platform::Linux] {
        for b in Config::default().resolved_keybindings(platform) {
            assert!(parse_keys(&b.keys).is_ok(), "default {} does not parse", b.keys);
            assert!(!b.action.contains('_') && b.action == b.action.to_lowercase(), "{} is not kebab case", b.action);
        }
    }
    let mac = Config::default().resolved_keybindings(Platform::MacOs);
    assert!(mac.iter().any(|b| b.keys == "cmd+t" && b.action == "new-tab"));
    assert!(!mac.iter().any(|b| b.keys == "ctrl+shift+t"));
}

#[test]
fn platform_sections_override_the_common_bindings_and_none_removes_a_default() {
    let text = r#"
[keybindings]
bindings = [
  { keys = "ctrl+shift+t", action = "new-window" },
  { keys = "ctrl+shift+k", action = "none" },
  { keys = "ctrl+a>c", action = "new-tab" },
  { keys = "ctrl+b", action = "send-text", args = { text = "hi" } },
]
[keybindings.windows]
bindings = [ { keys = "Ctrl+Shift+T", action = "split-right" }, { keys = "alt+left", action = "none" } ]
[keybindings.macos]
bindings = [ { keys = "cmd+t", action = "new-window" } ]
"#;
    let (config, warnings) = parse_ok(text);
    assert!(warnings.is_empty(), "{warnings:?}");
    let action_of = |list: &[Binding], keys: &str| list.iter().find(|b| b.keys == keys).map(|b| b.action.clone());

    let linux = config.resolved_keybindings(Platform::Linux);
    assert_eq!(action_of(&linux, "ctrl+shift+t").as_deref(), Some("new-window"));
    assert_eq!(action_of(&linux, "ctrl+shift+k"), None);
    assert_eq!(action_of(&linux, "ctrl+a>c").as_deref(), Some("new-tab"));
    assert_eq!(action_of(&linux, "alt+left").as_deref(), Some("focus-left"));
    let ctrl_b = linux.iter().find(|b| b.keys == "ctrl+b").unwrap();
    assert_eq!(ctrl_b.args.get("text").and_then(|v| v.as_str()), Some("hi"));
    assert_eq!(linux.iter().filter(|b| b.keys == "ctrl+shift+t").count(), 1);

    let windows = config.resolved_keybindings(Platform::Windows);
    assert_eq!(action_of(&windows, "ctrl+shift+t"), None, "the override replaced the binding under the same keys");
    assert_eq!(windows.iter().filter(|b| b.keys == "Ctrl+Shift+T").count(), 1);
    assert_eq!(action_of(&windows, "alt+left"), None);
    assert_eq!(action_of(&windows, "alt+right").as_deref(), Some("focus-right"));

    let mac = config.resolved_keybindings(Platform::MacOs);
    assert_eq!(action_of(&mac, "cmd+t").as_deref(), Some("new-window"));
    assert_eq!(action_of(&mac, "ctrl+shift+k"), None);
}

#[test]
fn a_key_table_binding_is_kept_apart_from_the_root_table() {
    let text = "[keybindings]\nbindings = [ { keys = \"h\", action = \"focus-left\", table = \"pane\" }, { keys = \"h\", action = \"copy\" } ]\n";
    let (config, _) = parse_ok(text);
    let list = config.resolved_keybindings(Platform::Linux);
    assert_eq!(list.iter().filter(|b| b.keys == "h").count(), 2);
}

#[test]
fn effective_text_parses_back_to_the_same_config() {
    let mut config = Config::default();
    config.font.size = 17.0;
    config.theme.dark = Some("Dracula".into());
    config.colors.foreground = Some(Rgb([1, 2, 3]));
    config.colors.palette = vec![Rgb([4, 5, 6])];
    config.scrollback.limit_lines = Some(10);
    config.shell.env.insert("A".into(), "b".into());
    config.keybindings.bindings.push(Binding::new("ctrl+b", "send-text"));
    let text = config.effective_text();
    let (parsed, warnings) = parse_ok(&text);
    assert_eq!(parsed, config);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(parse_ok(&Config::default().effective_text()).0, Config::default());
}

#[test]
fn load_of_a_missing_file_is_the_default_and_a_file_is_read() {
    let dir = temp_folder("load");
    let (config, warnings) = Config::load(&dir.join("absent.toml")).unwrap();
    assert_eq!((config, warnings), (Config::default(), vec![]));
    std::fs::write(dir.join("c.toml"), "[mouse]\nscroll_multiplier = 7\n").unwrap();
    assert_eq!(Config::load(&dir.join("c.toml")).unwrap().0.mouse.scroll_multiplier, 7);
}

#[test]
fn path_ends_with_termoil_config_toml() {
    let path = Config::path();
    if std::env::var_os("TERMOIL_CONFIG").is_none() {
        assert!(path.ends_with(PathBuf::from("termoil").join("config.toml")), "{}", path.display());
    }
}

#[test]
fn opener_falls_back_to_the_platform_opener_without_unluminous() {
    let openers = Config::default().openers;
    assert_eq!(openers.path_command(true), "unluminous {path}:{line}:{col}");
    assert_eq!(openers.path_command(false), "");
    let custom = Openers { path: "code -g {path}:{line}".into(), url: String::new() };
    assert_eq!(custom.path_command(false), "code -g {path}:{line}");
}

#[test]
fn more_than_three_hundred_themes_load_with_sixteen_colours_each() {
    let all = themes();
    assert!(all.len() > 300, "only {} themes", all.len());
    for t in all {
        assert_eq!(t.palette.len(), 16, "{}", t.name);
        assert!(!t.name.is_empty());
    }
    let mut names: Vec<String> = all.iter().map(|t| t.name.to_lowercase()).collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total, "duplicate theme names");
    assert!(themes().iter().any(|t| t.name == "Dracula"));
}

#[test]
fn termoil_dark_exists_and_meets_the_contrast_rule() {
    let dark = theme("Termoil Dark").expect("Termoil Dark is bundled");
    assert_eq!(dark.background, [0x1e, 0x1e, 0x1e]);
    assert_eq!(dark.foreground, [0xdc, 0xdc, 0xdc]);
    for (i, colour) in dark.palette.iter().enumerate().skip(1) {
        let ratio = contrast_ratio(*colour, dark.background);
        assert!(ratio >= 4.5, "palette {i} has contrast {ratio:.2}");
    }
    assert!(contrast_ratio(dark.foreground, dark.background) >= 4.5);
    assert!(theme("Termoil Light").is_some());
}

#[test]
fn theme_lookup_is_case_insensitive() {
    assert_eq!(theme("termoil dark").map(|t| t.name.as_str()), Some("Termoil Dark"));
    assert_eq!(theme("TERMOIL LIGHT").map(|t| t.name.as_str()), Some("Termoil Light"));
    assert_eq!(theme("DRACULA").map(|t| t.name.as_str()), Some("Dracula"));
    assert!(theme("no such theme").is_none());
}

#[test]
fn config_theme_applies_the_selection_then_the_colour_overrides() {
    let (config, _) =
        parse_ok("theme = \"dracula\"\n[colors]\nbackground = \"#101010\"\npalette = [\"#ff0000\", \"#00ff00\"]\n");
    let t = config.theme(true);
    let dracula = theme("Dracula").unwrap();
    assert_eq!(t.name, "Dracula");
    assert_eq!(t.background, [0x10, 0x10, 0x10]);
    assert_eq!(t.foreground, dracula.foreground);
    assert_eq!(t.palette[0], [255, 0, 0]);
    assert_eq!(t.palette[1], [0, 255, 0]);
    assert_eq!(t.palette[2], dracula.palette[2]);
    assert_eq!(Config::default().theme(true).name, "Unluminous Dark");
    let (unknown, _) = parse_ok("theme = \"nonexistent\"");
    assert_eq!(unknown.theme(true).name, "Unluminous Dark");
}

#[test]
fn theme_dark_and_light_follow_the_os_when_both_are_set() {
    let (config, _) = parse_ok("[theme]\nname = \"Dracula\"\ndark = \"Termoil Dark\"\nlight = \"Termoil Light\"\n");
    assert_eq!(config.theme(true).name, "Termoil Dark");
    assert_eq!(config.theme(false).name, "Termoil Light");
    let (only_dark, _) = parse_ok("[theme]\nname = \"Dracula\"\ndark = \"Termoil Dark\"\n");
    assert_eq!(only_dark.theme(false).name, "Dracula");
}

/// Waits for the next callback value, failing after a long deadline instead of sleeping a fixed time.
fn next_result(rx: &mpsc::Receiver<LoadResult>, accept: impl Fn(&LoadResult) -> bool) -> LoadResult {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(result) = rx.recv_timeout(Duration::from_millis(200))
            && accept(&result)
        {
            return result;
        }
    }
    panic!("no matching callback within the deadline");
}

#[test]
fn the_watcher_reports_a_new_config_and_then_a_parse_error() {
    let dir = temp_folder("watch");
    let path = dir.join("config.toml");
    std::fs::write(&path, "[font]\nsize = 12.0\n").unwrap();
    let (tx, rx) = mpsc::channel();
    let _watcher = Watcher::start(path.clone(), Duration::from_millis(50), move |r| {
        let _ = tx.send(r);
    })
    .expect("watcher starts");

    std::fs::write(&path, "[font]\nsize = 21.0\n").unwrap();
    let got = next_result(&rx, |r| matches!(r, Ok((c, _)) if c.font.size == 21.0));
    assert_eq!(got.unwrap().0.font.size, 21.0);

    std::fs::write(&path, "[font\nsize = 5\n").unwrap();
    let got = next_result(&rx, |r| r.is_err());
    assert_eq!(got.unwrap_err().line, 1);

    // Fixing the file clears the error, and an editor that replaces the file by rename is also seen.
    let temp = dir.join("config.toml.tmp");
    std::fs::write(&temp, "[font]\nsize = 9.0\n").unwrap();
    std::fs::rename(&temp, &path).unwrap();
    let got = next_result(&rx, |r| matches!(r, Ok((c, _)) if c.font.size == 9.0));
    assert!(got.is_ok());
}

#[test]
fn the_watcher_sees_a_file_created_after_it_started() {
    let dir = temp_folder("watch-create");
    let path = dir.join("termoil").join("config.toml");
    let (tx, rx) = mpsc::channel();
    let _watcher = Watcher::start(path.clone(), Duration::from_millis(50), move |r| {
        let _ = tx.send(r);
    })
    .expect("watcher starts and creates the folder");
    std::fs::write(&path, "[log]\nlevel = \"debug\"\n").unwrap();
    let got = next_result(&rx, |r| matches!(r, Ok((c, _)) if c.log.level == "debug"));
    assert!(got.is_ok());
}

#[test]
fn the_parser_never_panics_on_odd_input() {
    let samples = [
        "\u{0}",
        "[[[[",
        "a.b.c.d = 1",
        "[font]\nfamily = 5",
        "[keybindings]\nbindings = 3",
        "[keybindings]\nbindings = [ 1, 2 ]",
        "[keybindings]\nbindings = [ { keys = 1 } ]",
        "theme = 5",
        "[window]\npadding = [1]",
        "font-size = \"x\"",
        "\"quoted key\" = 1\nfont-size = 5\n[font]\nsize = 6",
        "é = 1\n[window]\ncolumns = \"é\"",
    ];
    for s in samples {
        let _ = Config::parse(s);
    }
}
