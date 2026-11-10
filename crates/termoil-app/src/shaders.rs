//! The shaders Termoil ships (task-2590): WGSL files in `crates/termoil-app/shaders/`, built into the
//! program. `window.shader` takes one of these names as well as a file path, so a color theme and a
//! shader combine freely. Choosing one works like choosing a theme: `preview` applies it to the
//! running app, `set` also writes it to the configuration file.

use crate::app::App;

/// One shader that ships with Termoil.
pub struct Builtin {
    /// The name `window.shader` and `set-shader` take.
    pub name: &'static str,
    /// One line saying what it looks like.
    pub summary: &'static str,
    /// The WGSL source, following the contract in `docs/shaders.md`.
    pub source: &'static str,
}

/// Declares a built in shader from its file in `shaders/`.
macro_rules! builtin {
    ($name:literal, $summary:literal) => {
        Builtin { name: $name, summary: $summary, source: include_str!(concat!("../shaders/", $name, ".wgsl")) }
    };
}

/// Every built in shader, in the order the shader list shows them.
pub const BUILTINS: &[Builtin] = &[
    builtin!("crt", "Curved glass tube, scanlines, aperture grille, color fringes, dark corners"),
    builtin!("retro", "Old terminal monitor: soft curve, glow, scanlines, noise and flicker"),
    builtin!("neon", "Text glows like neon tubing, colors pushed to full saturation"),
    builtin!("amber", "Monochrome amber phosphor, like an IBM 3270"),
    builtin!("green-phosphor", "Monochrome green phosphor with a slow phosphor trail, like a VT100"),
    builtin!("scanlines", "Only the dark lines between rows, colors unchanged"),
    builtin!("bloom", "A soft halo of light around bright text"),
    builtin!("vhs", "Worn video tape: wobble, color drift and a rolling noise band"),
    builtin!("glitch", "A digital signal that breaks into shifted, split bands now and then"),
    builtin!("chromatic", "Red and blue slide apart toward the edges, as through a cheap lens"),
    builtin!("vignette", "Corners and edges fall gently into shadow"),
    builtin!("film-grain", "Fine moving grain, stronger in the dark parts"),
    builtin!("tft", "An old LCD seen up close: red, green and blue stripes"),
    builtin!("game-boy", "The four greens of the original handheld, dithered"),
    builtin!("sepia", "Warm brown tones of an old photograph"),
    builtin!("noir", "High contrast black and white"),
    builtin!("hologram", "Projected cyan display with sweeping lines and flicker"),
    builtin!("synthwave", "80s sunset grade: purple shadows, pink highlights, a magenta wash"),
    builtin!("underwater", "Rippling picture under a blue green tint with drifting light"),
    builtin!("cursor-glow", "A soft pulsing light around the cursor"),
    builtin!("blueprint", "White lines on drafting blue with a faint grid"),
];

/// The word that means no shader in `set-shader` and the shader list.
pub const NONE: &str = "none";

/// The built in shader with this name, ignoring case.
pub fn builtin(name: &str) -> Option<&'static Builtin> {
    let name = name.trim();
    BUILTINS.iter().find(|b| b.name.eq_ignore_ascii_case(name))
}

/// The value `window.shader` gets for a name chosen from the list: empty for `none`, else the
/// built in shader's own spelling. An unknown name is an error that lists the names.
fn setting_value(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case(NONE) {
        return Ok(String::new());
    }
    builtin(name).map(|b| b.name.to_string()).ok_or_else(|| {
        let names: Vec<&str> = BUILTINS.iter().map(|b| b.name).collect();
        format!("no shader named '{name}'; the shaders are {NONE}, {}", names.join(", "))
    })
}

/// Applies a built in shader (or `none`) to the running app without saving it (the list's preview).
pub fn preview(app: &mut App, name: &str) -> Result<String, String> {
    let value = setting_value(name)?;
    app.config.window.shader = value.clone();
    app.effects.push(crate::app::Effect::ConfigChanged);
    Ok(value)
}

/// Puts back a `window.shader` value exactly as it was, file path or name, without saving it
/// (Escape in the list).
pub fn restore(app: &mut App, value: &str) {
    app.config.window.shader = value.to_string();
    app.effects.push(crate::app::Effect::ConfigChanged);
}

/// Applies a built in shader (or `none`) and writes it to `window.shader` in the configuration file,
/// keeping the rest of the file as it is.
pub fn set(app: &mut App, name: &str) -> Result<String, String> {
    let value = preview(app, name)?;
    let path = app.config_path.clone();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    crate::themes::write_file(&path, &with_shader(&text, &value)?)?;
    Ok(value)
}

/// The configuration text with `window.shader` set to `value`.
pub fn with_shader(text: &str, value: &str) -> Result<String, String> {
    let setting = crate::settings::all()
        .into_iter()
        .find(|s| s.path() == "window.shader")
        .expect("window.shader is in the schema");
    crate::settings::with_value(text, &setting, value)
}

/// The name the shader list highlights for a `window.shader` value: the built in name, `none` when
/// empty, or None for a file path.
pub fn list_name(value: &str) -> Option<&'static str> {
    if value.trim().is_empty() {
        return Some(NONE);
    }
    builtin(value).map(|b| b.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_or_more_shaders_with_unique_names_and_the_entry_function() {
        assert!(BUILTINS.len() >= 20, "{} shaders", BUILTINS.len());
        for (i, b) in BUILTINS.iter().enumerate() {
            assert!(b.source.contains("fn termoil_shade("), "{} defines termoil_shade", b.name);
            assert!(!b.source.contains("@group"), "{} declares no bindings", b.name);
            assert!(BUILTINS[i + 1..].iter().all(|o| o.name != b.name), "{} is listed once", b.name);
            assert_ne!(b.name, NONE);
        }
    }

    #[test]
    fn names_resolve_ignoring_case_and_none_clears() {
        assert_eq!(setting_value("CRT").unwrap(), "crt");
        assert_eq!(setting_value("none").unwrap(), "");
        assert_eq!(setting_value("").unwrap(), "");
        let err = setting_value("nope").unwrap_err();
        assert!(err.contains("no shader named 'nope'") && err.contains("neon"), "{err}");
        assert_eq!(list_name(""), Some(NONE));
        assert_eq!(list_name("Neon"), Some("neon"));
        assert_eq!(list_name("fx/mine.wgsl"), None);
    }

    #[test]
    fn writes_the_shader_into_the_window_table_and_keeps_the_rest() {
        assert_eq!(with_shader("", "crt").unwrap(), "[window]\nshader = \"crt\"\n");
        let text = "theme = \"Nord\"\n\n[window]\nopacity = 0.9 # see through\nshader = \"x.wgsl\"\n";
        assert_eq!(
            with_shader(text, "neon").unwrap(),
            "theme = \"Nord\"\n\n[window]\nopacity = 0.9 # see through\nshader = \"neon\"\n"
        );
    }
}
