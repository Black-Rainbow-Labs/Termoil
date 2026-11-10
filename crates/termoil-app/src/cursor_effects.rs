//! The cursor effects Termoil ships (task-2593): WGSL files in `crates/termoil-app/cursor-effects/`,
//! built into the program. `window.cursor_effect` names one, and it runs after the shader in the same
//! pass, so any cursor effect combines with any shader (or with no shader). A chosen cursor effect
//! replaces the cursor light of a built in shader: the shader reads `TERMOIL_SHADER_CURSOR`, which is
//! 0 while a cursor effect is chosen and 1 otherwise. Choosing one works like choosing a shader:
//! `preview` applies it to the running app, `set` also writes it to the configuration file.

use crate::app::App;

/// One cursor effect that ships with Termoil.
pub struct Effect {
    /// The name `window.cursor_effect` and `set-cursor-effect` take.
    pub name: &'static str,
    /// One line saying what it looks like.
    pub summary: &'static str,
    /// The WGSL source, defining `fn termoil_cursor_effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>`.
    pub source: &'static str,
}

/// Declares a built in cursor effect from its file in `cursor-effects/`.
macro_rules! effect {
    ($name:literal, $summary:literal) => {
        Effect { name: $name, summary: $summary, source: include_str!(concat!("../cursor-effects/", $name, ".wgsl")) }
    };
}

/// Every built in cursor effect, in the order the cursor effect list shows them.
pub const EFFECTS: &[Effect] = &[
    effect!("glow", "A steady soft light around the cursor"),
    effect!("pulse", "A light around the cursor that swells and fades slowly"),
    effect!("neon-halo", "The pulsing magenta halo of neon-glow, with any shader"),
    effect!("sonar", "Thin rings travel outward from the cursor, like a radar ping"),
    effect!("spotlight", "The screen dims away from the cursor"),
    effect!("crosshair", "Faint lines through the cursor's row and column"),
    effect!("row", "A soft band of light across the cursor's row"),
    effect!("column", "A soft band of light down the cursor's column"),
    effect!("rainbow", "A halo that cycles through every color"),
    effect!("heartbeat", "A red glow that beats twice, rests, and beats again"),
    effect!("sparkle", "Small points of light twinkle around the cursor"),
    effect!("orbit", "Three small lights circle the cursor"),
    effect!("flame", "A warm flickering glow rising above the cursor"),
    effect!("frost", "An icy halo with a fine crystal texture"),
    effect!("magnify", "A small lens over the cursor shows the text under it larger"),
    effect!("ripple", "The picture around the cursor ripples like water"),
    effect!("beacon", "A beam turns around the cursor like a lighthouse"),
    effect!("starburst", "Thin rays of light shine out from the cursor and slowly turn"),
    effect!("invert", "A small disc around the cursor shows the colors inverted"),
    effect!("aurora", "A ribbon of green and violet light waves along the cursor's row"),
];

/// The helpers every cursor effect may use (`tce_shown`, `tce_offset`, `tce_read` and the rest).
const COMMON: &str = include_str!("../cursor-effects/common.wgsl");

/// The shader used when a cursor effect is chosen with no shader: the frame unchanged.
const PLAIN: &str = "fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {\n    return color;\n}\n";

/// The word that means no cursor effect in `set-cursor-effect` and the list.
pub const NONE: &str = "none";

/// The built in cursor effect with this name, ignoring case.
pub fn effect(name: &str) -> Option<&'static Effect> {
    let name = name.trim();
    EFFECTS.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// The WGSL the renderer compiles for a shader and a cursor effect. With no effect it is the shader
/// with `TERMOIL_SHADER_CURSOR` set to 1. With an effect, the shader's `termoil_shade` is renamed,
/// and a new `termoil_shade` runs it and then the effect, with `TERMOIL_SHADER_CURSOR` set to 0 so the
/// shader's own cursor light is off. The shader's text comes first, so its error lines keep their
/// numbers.
pub fn compose(shader: Option<&str>, cursor: Option<&Effect>) -> String {
    let Some(cursor) = cursor else {
        return format!("{}\nconst TERMOIL_SHADER_CURSOR: f32 = 1.0;\n", shader.unwrap_or(PLAIN));
    };
    let base = shader.unwrap_or(PLAIN).replace("fn termoil_shade(", "fn termoil_base_shade(");
    format!(
        "{base}\nconst TERMOIL_SHADER_CURSOR: f32 = 0.0;\n{COMMON}\n{}\n\
         fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {{\n    \
         return termoil_cursor_effect(uv, termoil_base_shade(uv, color));\n}}\n",
        cursor.source
    )
}

/// The value `window.cursor_effect` gets for a name chosen from the list: empty for `none`, else the
/// effect's own spelling. An unknown name is an error that lists the names.
fn setting_value(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case(NONE) {
        return Ok(String::new());
    }
    effect(name).map(|e| e.name.to_string()).ok_or_else(|| {
        let names: Vec<&str> = EFFECTS.iter().map(|e| e.name).collect();
        format!("no cursor effect named '{name}'; the cursor effects are {NONE}, {}", names.join(", "))
    })
}

/// Applies a cursor effect (or `none`) to the running app without saving it (the list's preview).
pub fn preview(app: &mut App, name: &str) -> Result<String, String> {
    let value = setting_value(name)?;
    app.config.window.cursor_effect = value.clone();
    app.effects.push(crate::app::Effect::ConfigChanged);
    Ok(value)
}

/// Puts back a `window.cursor_effect` value exactly as it was, without saving it (Escape in the list).
pub fn restore(app: &mut App, value: &str) {
    app.config.window.cursor_effect = value.to_string();
    app.effects.push(crate::app::Effect::ConfigChanged);
}

/// Applies a cursor effect (or `none`) and writes it to `window.cursor_effect` in the configuration
/// file, keeping the rest of the file as it is.
pub fn set(app: &mut App, name: &str) -> Result<String, String> {
    let value = preview(app, name)?;
    let path = app.config_path.clone();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    crate::themes::write_file(&path, &with_cursor_effect(&text, &value)?)?;
    Ok(value)
}

/// The configuration text with `window.cursor_effect` set to `value`.
pub fn with_cursor_effect(text: &str, value: &str) -> Result<String, String> {
    let setting = crate::settings::all()
        .into_iter()
        .find(|s| s.path() == "window.cursor_effect")
        .expect("window.cursor_effect is in the schema");
    crate::settings::with_value(text, &setting, value)
}

/// The name the cursor effect list highlights for a `window.cursor_effect` value: the effect's name,
/// or `none` when empty or unknown.
pub fn list_name(value: &str) -> &'static str {
    effect(value).map_or(NONE, |e| e.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_cursor_effects_with_unique_names_and_the_entry_function() {
        assert_eq!(EFFECTS.len(), 20);
        for (i, e) in EFFECTS.iter().enumerate() {
            assert!(e.source.contains("fn termoil_cursor_effect("), "{} defines termoil_cursor_effect", e.name);
            assert!(!e.source.contains("@group"), "{} declares no bindings", e.name);
            assert!(EFFECTS[i + 1..].iter().all(|o| o.name != e.name), "{} is listed once", e.name);
            assert_ne!(e.name, NONE);
        }
    }

    #[test]
    fn names_resolve_ignoring_case_and_none_clears() {
        assert_eq!(setting_value("Sonar").unwrap(), "sonar");
        assert_eq!(setting_value("none").unwrap(), "");
        let err = setting_value("nope").unwrap_err();
        assert!(err.contains("no cursor effect named 'nope'") && err.contains("glow"), "{err}");
        assert_eq!(list_name(""), NONE);
        assert_eq!(list_name("ORBIT"), "orbit");
    }

    #[test]
    fn composing_renames_the_shader_and_turns_its_cursor_light_off() {
        let alone =
            compose(Some("fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color; }"), None);
        assert!(alone.contains("fn termoil_shade(") && alone.contains("TERMOIL_SHADER_CURSOR: f32 = 1.0"));
        let both = compose(
            Some("fn termoil_shade(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> { return color; }"),
            effect("glow"),
        );
        assert!(both.starts_with("fn termoil_base_shade("), "{both}");
        assert!(
            both.contains("TERMOIL_SHADER_CURSOR: f32 = 0.0")
                && both.contains("termoil_cursor_effect(uv, termoil_base_shade")
        );
        assert_eq!(both.matches("fn termoil_shade(").count(), 1);
        assert!(compose(None, effect("orbit")).contains("fn termoil_base_shade("));
    }

    #[test]
    fn writes_the_cursor_effect_into_the_window_table_and_keeps_the_rest() {
        let text = "[window]\nshader = \"crt\"\n";
        assert_eq!(
            with_cursor_effect(text, "sonar").unwrap(),
            "[window]\nshader = \"crt\"\ncursor_effect = \"sonar\"\n"
        );
    }
}
