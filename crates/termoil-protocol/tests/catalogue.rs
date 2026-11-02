//! A1 and A2: the generated command reference and MCP tool list match the checked in files, and the
//! catalogue is well formed. `TERMOIL_BLESS=1` rewrites the files.

use std::path::PathBuf;

use termoil_protocol::{
    CATALOGUE, ParamKind, Request, Response, commands_markdown, decode_request, decode_response, encode_request,
    encode_response, mcp_tools_json,
};

/// The repository's docs folder.
fn docs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

/// Compares generated text with a checked in file, rewriting it when blessing.
fn check_generated(name: &str, generated: &str) {
    let path = docs_dir().join(name);
    if std::env::var("TERMOIL_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(&path, generated).expect("write generated file");
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(on_disk, generated, "{name} is out of date with the catalogue; run with TERMOIL_BLESS=1");
}

/// A1: docs/commands.md is generated from the catalogue.
#[test]
fn commands_md_matches_catalogue() {
    check_generated("commands.md", &commands_markdown());
}

/// A2: docs/mcp-tools.json is generated from the catalogue.
#[test]
fn mcp_tools_match_catalogue() {
    check_generated("mcp-tools.json", &mcp_tools_json());
}

/// Every action name is unique, kebab case, and every parameter name is unique within its action.
#[test]
fn catalogue_is_well_formed() {
    let mut names = std::collections::HashSet::new();
    for a in CATALOGUE {
        assert!(names.insert(a.name), "duplicate action {}", a.name);
        assert!(a.name.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "action name {}", a.name);
        assert!(a.summary.ends_with('.'), "summary of {} is a sentence", a.name);
        let mut params = std::collections::HashSet::new();
        for p in a.params {
            assert!(params.insert(p.name), "duplicate parameter {} in {}", p.name, a.name);
            if let ParamKind::Choice(c) = p.kind {
                assert!(!c.is_empty());
            }
        }
    }
}

/// Every command named by PRD section 6 for 1.0 is in the catalogue as a command line action.
#[test]
fn every_1_0_command_is_offered() {
    let wanted = [
        "new-window",
        "new-tab",
        "split",
        "close",
        "focus",
        "send-text",
        "send-key",
        "screen",
        "scrollback",
        "wait-prompt",
        "list",
        "resize",
        "set-title",
        "run-action",
        "show-config",
        "reload-config",
        "install-shell-integration",
        "version",
    ];
    for w in wanted {
        let a = termoil_protocol::action(w).unwrap_or_else(|| panic!("missing {w}"));
        assert!(a.cli, "{w} is a command line action");
    }
}

/// Requests and responses survive a round trip through their line encoding.
#[test]
fn ipc_round_trip() {
    let line = r#"{"id":7,"action":"new-tab","args":{"cwd":"C:/src"}}"#;
    let req: Request = decode_request(line).expect("decode");
    assert_eq!(req.id, 7);
    assert_eq!(req.call.action, "new-tab");
    assert_eq!(req.call.args["cwd"], "C:/src");
    assert_eq!(decode_request(&encode_request(&req)).expect("again"), req);
    let ok = Response::from_result(7, Ok(serde_json::json!({ "pane": 3 })));
    assert_eq!(encode_response(&ok), "{\"id\":7,\"ok\":true,\"result\":{\"pane\":3}}\n");
    let err = Response::from_result(8, Err(termoil_protocol::ActionError::Unknown("nope".into())));
    let back = decode_response(&encode_response(&err)).expect("decode");
    assert_eq!(back.error.as_deref(), Some("unknown action 'nope'"));
    assert!(decode_request("{\"id\":1}").is_err());
}
