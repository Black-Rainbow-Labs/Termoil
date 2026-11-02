//! `termoil +mcp`: a Model Context Protocol server over stdio (JSON-RPC 2.0, one message per
//! line). Its tools are the generated tool list; a call is forwarded to the running instance as the
//! catalogue action of the same name.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::{Map, Value, json};

/// The protocol version answered to `initialize`.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Reads requests from stdin until it closes.
pub fn serve() -> ExitCode {
    let tools: Value = serde_json::from_str(&termoil_protocol::mcp_tools_json()).unwrap_or(json!({ "tools": [] }));
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Some(reply) = handle(&line, &tools) else { continue };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

/// Answers one message, or None for a notification.
fn handle(line: &str, tools: &Value) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } }),
            );
        }
    };
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let result = match method {
        "initialize" => Ok(
            json!({ "protocolVersion": PROTOCOL_VERSION, "capabilities": { "tools": {} }, "serverInfo": { "name": "termoil", "version": termoil_protocol::VERSION } }),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools.clone()),
        "tools/call" => Ok(call_tool(msg.get("params").unwrap_or(&Value::Null))),
        _ => Err(json!({ "code": -32601, "message": format!("unknown method {method}") })),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

/// Runs a tool: `termoil_new_tab` is the action `new-tab`.
fn call_tool(params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let action = name.strip_prefix("termoil_").unwrap_or(name).replace('_', "-");
    let args: Map<String, Value> = params.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
    match crate::call(&action, args) {
        Ok(r) if r.ok => {
            let text = match &r.result {
                Value::String(s) => s.clone(),
                Value::Null => "ok".to_string(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        Ok(r) => json!({ "content": [{ "type": "text", "text": r.error.unwrap_or_default() }], "isError": true }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}
