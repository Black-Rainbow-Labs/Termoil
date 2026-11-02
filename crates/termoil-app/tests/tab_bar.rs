//! The tab bar (task-2468, redesigned in task-2487): pixel goldens of the bar in each state and in each theme taken from
//! Unluminous, rendered with the CPU renderer on the vendored JetBrains Mono. The accepted images are under
//! `corpus/render/<platform>/tab_bar_*.png`; `TERMOIL_BLESS=1` accepts new ones.

use std::path::PathBuf;

use termoil_app::config::{Config, TabBar, chrome_for, theme};
use termoil_app::gui::chrome::Strip;
use termoil_app::gui::look;
use termoil_app::gui::tab_bar::{self, Look, TabHit};
use termoil_font::{FontConfig, FontGrid};
use termoil_render::golden::{self, Tolerance};
use termoil_render::{BuildOptions, Renderer, Rgba, VelloCpuRenderer};

/// Width of the rendered bar in pixels.
const WIDTH: u32 = 900;

/// The corpus folder.
fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// A grid on the vendored JetBrains Mono at 16 pixels.
fn grid() -> FontGrid {
    let dir = corpus().join("fonts");
    let files = ["Regular", "Bold", "Italic", "BoldItalic"]
        .iter()
        .map(|s| dir.join(format!("JetBrainsMono-{s}.ttf")))
        .collect();
    FontGrid::new(FontConfig {
        families: vec!["JetBrains Mono".into()],
        font_files: files,
        system_fonts: false,
        size_px: 16.0,
        ..FontConfig::default()
    })
    .expect("font")
}

/// One picture of the bar: the theme, which tab is showing, what is under the pointer, and whether
/// the window has the keyboard.
struct Case {
    name: &'static str,
    theme: &'static str,
    active: usize,
    hover: Option<TabHit>,
    focused: bool,
}

/// Renders the bar for `case` with three tabs, the way the window builds it.
fn render(case: &Case, g: &mut FontGrid) -> termoil_render::Image {
    let mut config = Config::default();
    config.theme.name = case.theme.into();
    let colors = look::colors(&config, true);
    let chrome = chrome_for(&config.theme(true));
    let sizes = TabBar::default();
    let (cw, ch) = (g.metrics().width, g.metrics().height);
    let height = ch + 2 * sizes.padding;
    let cols = (WIDTH / cw) as u16;
    let titles = ["pwsh".to_string(), "cargo build --release".into(), "a title far too long to fit in its tab".into()];
    let layout = tab_bar::layout(titles.len(), cols, &sizes);
    let background = [colors.background.r, colors.background.g, colors.background.b];
    let bar = tab_bar::bar_color(background, &sizes);
    let bytes = tab_bar::text(&titles, case.active, &layout, &chrome, bar);
    let opts = BuildOptions {
        padding: (0, sizes.padding),
        all_rows: true,
        minimum_contrast: 1.0,
        ..look::build_options(&config, 1.0)
    };
    let mut strip = Strip::new(1, colors.clone());
    strip.frame(cols, 1, &bytes, &colors, g, &opts);
    let look = Look {
        chrome: &chrome,
        background,
        cell: (cw, ch),
        width: WIDTH as f32,
        height: height as f32,
        scale: 1.0,
        sizes: &sizes,
        focused: case.focused,
    };
    let shapes = tab_bar::shapes(&layout, case.active, case.hover, &look);
    strip.decorate(WIDTH, height, Rgba::rgb(bar[0], bar[1], bar[2]), shapes);
    let frame = strip.frame_ref().expect("built");
    let mut r = VelloCpuRenderer::new(frame.width, frame.height, 0, false);
    r.render(frame, g).expect("render");
    r.read_pixels().expect("pixels")
}

/// Every case: the states on Unluminous Dark, then each theme taken from Unluminous, then two themes whose
/// chrome is worked out from their own colours.
fn cases() -> Vec<Case> {
    let c = |name, theme, active, hover, focused| Case { name, theme, active, hover, focused };
    vec![
        c("tab_bar_unluminous_dark", "Unluminous Dark", 1, None, true),
        c("tab_bar_hover_tab", "Unluminous Dark", 1, Some(TabHit::Tab(2)), true),
        c("tab_bar_hover_close", "Unluminous Dark", 1, Some(TabHit::Close(0)), true),
        c("tab_bar_hover_new_tab", "Unluminous Dark", 1, Some(TabHit::NewTab), true),
        c("tab_bar_unfocused", "Unluminous Dark", 1, None, false),
        c("tab_bar_unluminous_light", "Unluminous Light", 0, Some(TabHit::Tab(2)), true),
        c("tab_bar_dracula", "Islands Dracula Colorful", 2, None, true),
        c("tab_bar_palenight", "Material Palenight", 1, None, true),
        c("tab_bar_deep_ocean", "Material Deep Ocean", 1, None, true),
        c("tab_bar_monokai_pro", "Monokai Pro", 1, None, true),
        c("tab_bar_one_dark", "One Dark", 1, None, true),
        c("tab_bar_derived_termoil_dark", "Termoil Dark", 1, None, true),
        c("tab_bar_derived_termoil_light", "Termoil Light", 1, Some(TabHit::Tab(0)), true),
    ]
}

#[test]
fn tab_bar_pixel_goldens() {
    let dir = golden::platform_dir(&corpus());
    let mut g = grid();
    let cell = (g.metrics().width, g.metrics().height);
    let mut failures = Vec::new();
    for case in cases() {
        assert!(theme(case.theme).is_some(), "{} is not bundled", case.theme);
        let img = render(&case, &mut g);
        if let Err(e) = golden::check(&dir, case.name, &img, Tolerance::CPU, cell, (0, TabBar::default().padding)) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_theme_from_unluminous_has_its_own_chrome() {
    for name in [
        "Unluminous Dark",
        "Unluminous Light",
        "Islands Dracula Colorful",
        "Material Palenight",
        "Material Deep Ocean",
        "Monokai Pro",
        "One Dark",
    ] {
        let t = theme(name).unwrap_or_else(|| panic!("{name} is not bundled"));
        assert_ne!(chrome_for(t), termoil_app::config::derived_chrome(t), "{name} uses worked out chrome");
    }
    let dark = chrome_for(theme("Unluminous Dark").unwrap());
    assert_eq!(dark.accent, [0x48, 0x9f, 0xf8], "Unluminous Dark's accent is Unluminous's blue");
}

#[test]
fn the_default_theme_is_unluminous_dark() {
    assert_eq!(Config::default().theme(true).name, "Unluminous Dark");
}

#[test]
fn the_tab_bar_is_hidden_with_one_tab_and_shown_with_two() {
    let config = Config::default();
    assert_eq!(look::tab_bar_height(&config, 20, 1.0, 1), 0, "one tab: no bar");
    assert!(look::tab_bar_height(&config, 20, 1.0, 2) > 20, "two tabs: the bar is taller than a cell");
    let mut always = Config::default();
    always.tab_bar.show_with_one_tab = true;
    assert!(look::tab_bar_height(&always, 20, 1.0, 1) > 0, "show_with_one_tab brings it back");
}

#[test]
fn the_new_tab_button_is_drawn_against_the_right_edge() {
    let config = Config::default();
    let chrome = chrome_for(&config.theme(true));
    let sizes = TabBar::default();
    let layout = tab_bar::layout(2, 100, &sizes);
    let look = Look {
        chrome: &chrome,
        background: [0x1b, 0x1f, 0x26],
        cell: (9, 20),
        width: 905.0,
        height: 36.0,
        scale: 1.0,
        sizes: &sizes,
        focused: true,
    };
    let shapes = tab_bar::shapes(&layout, 0, None, &look);
    let rightmost = shapes
        .iter()
        .map(|s| match s {
            termoil_render::Shape::Line { from, to, .. } => from.0.max(to.0),
            termoil_render::Shape::RoundRect { area, .. } => area.x1,
        })
        .fold(0.0f32, f32::max);
    assert!(rightmost > 880.0 && rightmost <= 905.0, "the plus ends near the right edge, at {rightmost}");
}
