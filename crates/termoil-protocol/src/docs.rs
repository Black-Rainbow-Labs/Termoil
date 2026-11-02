//! The generated documentation: `docs/commands.md` and the MCP tool list `docs/mcp-tools.json`.
//! Both are produced from [`crate::CATALOGUE`] and compared with the checked in files by tests (A1,
//! A2); `TERMOIL_BLESS=1` rewrites them.

use serde_json::{Value, json};

use crate::catalogue::{ActionDef, CATALOGUE, Context, ParamKind};

/// The text of `docs/commands.md`.
pub fn commands_markdown() -> String {
    let mut out = String::from(
        "# Termoil commands\n\nGenerated from the action catalogue in `crates/termoil-protocol`; do not edit by hand.\nEvery action runs through `App::run_action`, whether a keybinding, the command palette, `termoil +<command>`\nor the IPC socket asked for it.\n\n",
    );
    out.push_str("| Action | Command line | Palette | Needs | Summary |\n|---|---|---|---|---|\n");
    for a in CATALOGUE {
        let needs = match a.context {
            Context::Global => "nothing",
            Context::Window => "a window",
            Context::Pane => "a pane",
        };
        out.push_str(&format!(
            "| [`{}`](#{}) | {} | {} | {} | {} |\n",
            a.name,
            a.name,
            if a.cli { "yes" } else { "no" },
            if a.palette { "yes" } else { "no" },
            needs,
            a.summary
        ));
    }
    for a in CATALOGUE {
        out.push_str(&section(a));
    }
    out
}

/// The section for one action.
fn section(a: &ActionDef) -> String {
    let mut s = format!("\n## {}\n\n{}\n\n", a.name, a.summary);
    if a.cli {
        let usage: Vec<String> = a
            .params
            .iter()
            .map(|p| {
                if p.required {
                    format!("--{} <{}>", p.name, kind_name(p.kind))
                } else {
                    format!("[--{} <{}>]", p.name, kind_name(p.kind))
                }
            })
            .collect();
        s.push_str(&format!("```\ntermoil +{} {}\n```\n\n", a.name, usage.join(" ")).replace(" \n", "\n"));
        s.push_str(&format!(
            "Over IPC: `{{\"id\":1,\"action\":\"{}\",\"args\":{{...}}}}`. MCP tool: `termoil_{}`.\n\n",
            a.name,
            a.name.replace('-', "_")
        ));
    }
    if a.params.is_empty() {
        s.push_str("No parameters.\n");
    } else {
        s.push_str("| Parameter | Type | Required | Meaning |\n|---|---|---|---|\n");
        for p in a.params {
            s.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                p.name,
                kind_name(p.kind),
                if p.required { "yes" } else { "no" },
                p.help
            ));
        }
    }
    s
}

/// A parameter type as the documentation writes it.
fn kind_name(k: ParamKind) -> String {
    match k {
        ParamKind::Text => "text".into(),
        ParamKind::Int => "integer".into(),
        ParamKind::Bool => "true|false".into(),
        ParamKind::Pane => "pane id".into(),
        ParamKind::Path => "path".into(),
        ParamKind::Key => "key".into(),
        ParamKind::Action => "action".into(),
        ParamKind::Choice(c) => c.join("|"),
    }
}

/// The MCP tool list: one tool per command line action, with a JSON schema of its parameters.
pub fn mcp_tools_json() -> String {
    let tools: Vec<Value> = CATALOGUE.iter().filter(|a| a.cli).map(tool).collect();
    let mut s = serde_json::to_string_pretty(&json!({ "tools": tools })).unwrap_or_default();
    s.push('\n');
    s
}

/// One MCP tool definition.
fn tool(a: &ActionDef) -> Value {
    let mut props = serde_json::Map::new();
    for p in a.params {
        let mut schema = json!({ "type": p.kind.json_type(), "description": p.help });
        if let ParamKind::Choice(c) = p.kind {
            schema["enum"] = json!(c);
        }
        props.insert(p.name.to_string(), schema);
    }
    let required: Vec<&str> = a.params.iter().filter(|p| p.required).map(|p| p.name).collect();
    json!({
        "name": format!("termoil_{}", a.name.replace('-', "_")),
        "description": a.summary,
        "inputSchema": { "type": "object", "properties": props, "required": required },
    })
}
