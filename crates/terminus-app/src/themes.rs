//! Choosing a theme: applying one to the running app, and writing it to the configuration file so it
//! stays. Writing keeps the rest of the file as it is: an existing top level `theme = ...` line or the
//! `name =` line of a `[theme]` table is replaced, otherwise `theme = "..."` is added at the top.

use std::path::Path;

/// Applies a bundled theme to the running app without saving it (the switcher's preview).
pub fn preview(app: &mut crate::app::App, name: &str) -> Result<(), String> {
    crate::config::theme(name).ok_or_else(|| format!("no theme named '{name}'"))?;
    app.config.theme.name = name.to_string();
    app.config.theme.dark = None;
    app.config.theme.light = None;
    app.effects.push(crate::app::Effect::ConfigChanged);
    Ok(())
}

/// Applies a theme and writes it to the configuration file.
pub fn set(app: &mut crate::app::App, name: &str) -> Result<(), String> {
    preview(app, name)?;
    let path = app.config_path.clone();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    write_file(&path, &with_theme(&text, name))
}

/// Writes the file, creating its folder.
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// The configuration text with its theme set to `name`.
pub fn with_theme(text: &str, name: &str) -> String {
    let quoted = format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""));
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut section: Option<String> = None;
    let mut first_section = None;
    for (i, line) in lines.iter_mut().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            section = Some(t.trim_matches(['[', ']']).trim().to_string());
            first_section.get_or_insert(i);
            continue;
        }
        let key = t.split('=').next().unwrap_or("").trim();
        let replace = match section.as_deref() {
            None => key == "theme",
            Some("theme") => key == "name",
            _ => false,
        };
        if replace && t.contains('=') {
            *line = format!("{key} = {quoted}");
            return lines.join("\n") + "\n";
        }
    }
    let has_table = lines.iter().any(|l| l.trim() == "[theme]");
    if has_table {
        let at = lines.iter().position(|l| l.trim() == "[theme]").expect("checked above") + 1;
        lines.insert(at, format!("name = {quoted}"));
    } else {
        lines.insert(first_section.unwrap_or(lines.len()).min(lines.len()), format!("theme = {quoted}"));
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::with_theme;

    #[test]
    fn writes_the_theme_and_keeps_the_rest() {
        assert_eq!(with_theme("", "Dracula"), "theme = \"Dracula\"\n");
        assert_eq!(
            with_theme("# mine\ntheme = \"Old\"\n[font]\nsize = 12\n", "Dracula"),
            "# mine\ntheme = \"Dracula\"\n[font]\nsize = 12\n"
        );
        assert_eq!(with_theme("[font]\nsize = 12\n", "Nord"), "theme = \"Nord\"\n[font]\nsize = 12\n");
        assert_eq!(
            with_theme("[theme]\nname = \"A\"\ndark = \"B\"\n", "Nord"),
            "[theme]\nname = \"Nord\"\ndark = \"B\"\n"
        );
        assert_eq!(
            with_theme("[font]\nname = \"x\"\n[theme]\ndark = \"B\"\n", "Nord"),
            "[font]\nname = \"x\"\n[theme]\nname = \"Nord\"\ndark = \"B\"\n"
        );
    }
}
