//! A3: every keybinding in the default configuration names a catalogue action. A4: the command
//! palette lists every action (docs/TEST-SUITE.md section 14).

use termoil_app::config::{Config, Platform, default_keybindings_for};
use termoil_app::palette;
use termoil_protocol::{CATALOGUE, action};

#[test]
fn a3_default_keybindings_name_catalogue_actions() {
    let mut missing = Vec::new();
    for platform in [Platform::Windows, Platform::MacOs, Platform::Linux] {
        for b in default_keybindings_for(platform) {
            if action(&b.action).is_none() {
                missing.push(format!("{platform:?}: {} -> {}", b.keys, b.action));
            }
        }
    }
    assert!(missing.is_empty(), "bindings naming no catalogue action:\n{}", missing.join("\n"));
}

#[test]
fn a3_effective_bindings_parse() {
    let config = Config::default();
    for platform in [Platform::Windows, Platform::MacOs, Platform::Linux] {
        for b in config.resolved_keybindings(platform) {
            assert!(b.chords().is_ok(), "{platform:?}: '{}' does not parse", b.keys);
        }
    }
}

#[test]
fn a4_palette_lists_every_action() {
    let entries = palette::entries(&Config::default().resolved_keybindings(Platform::current()));
    for a in CATALOGUE {
        assert!(entries.iter().any(|e| e.action == a.name), "the palette does not list {}", a.name);
    }
    assert_eq!(entries.iter().filter(|e| e.preset.is_none()).count(), CATALOGUE.len());
}
