//! Command line arguments for `terminus +<action>`, typed by the catalogue.
//!
//! `--name value` and `--name=value` set a parameter; a boolean may be given as a bare `--name`.
//! One bare word fills the action's first required parameter, so `terminus +send-text "ls\n"`
//! works. `\n`, `\r`, `\t`, `\e` and `\\` in a text parameter are unescaped, so a shell that cannot
//! type a newline can still send one.

use serde_json::{Map, Value, json};
use terminus_protocol::{ActionDef, CATALOGUE, ParamKind, action};

/// Parses the arguments of an action into the JSON object the IPC request carries.
pub fn parse(name: &str, rest: &[String]) -> Result<Map<String, Value>, String> {
    let def = action(name).ok_or_else(|| format!("unknown command '+{name}' (see terminus +help)"))?;
    if !def.cli {
        return Err(format!("'{name}' is not a command line action; use +run-action --name {name}"));
    }
    let mut out = Map::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if let Some(flag) = a.strip_prefix("--") {
            let (key, inline) = match flag.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (flag, None),
            };
            let p = def.params.iter().find(|p| p.name == key).ok_or_else(|| format!("+{name} has no --{key}"))?;
            let value = match (inline, p.kind) {
                (Some(v), _) => v,
                (None, ParamKind::Bool) if rest.get(i + 1).is_none_or(|n| n.starts_with("--")) => "true".into(),
                (None, _) => {
                    i += 1;
                    rest.get(i).cloned().ok_or_else(|| format!("--{key} needs a value"))?
                }
            };
            out.insert(key.to_string(), typed(p.kind, &value).map_err(|e| format!("--{key}: {e}"))?);
        } else {
            let p = def
                .params
                .iter()
                .find(|p| p.required && !out.contains_key(p.name))
                .ok_or_else(|| format!("unexpected argument '{a}'"))?;
            out.insert(p.name.to_string(), typed(p.kind, a)?);
        }
        i += 1;
    }
    check_required(def, &out)?;
    Ok(out)
}

/// Fails when a required parameter is missing.
fn check_required(def: &ActionDef, args: &Map<String, Value>) -> Result<(), String> {
    match def.params.iter().find(|p| p.required && !args.contains_key(p.name)) {
        Some(p) => Err(format!("+{} needs --{}", def.name, p.name)),
        None => Ok(()),
    }
}

/// Converts a word to the parameter's JSON type.
fn typed(kind: ParamKind, v: &str) -> Result<Value, String> {
    Ok(match kind {
        ParamKind::Int | ParamKind::Pane => {
            json!(v.trim().parse::<i64>().map_err(|_| format!("'{v}' is not an integer"))?)
        }
        ParamKind::Bool => match v {
            "true" | "1" | "yes" => json!(true),
            "false" | "0" | "no" => json!(false),
            _ => return Err(format!("'{v}' is not true or false")),
        },
        ParamKind::Choice(c) if !c.contains(&v) => return Err(format!("'{v}' is not one of {}", c.join(", "))),
        ParamKind::Text => json!(unescape(v)),
        _ => json!(v),
    })
}

/// Replaces `\n`, `\r`, `\t`, `\e` and `\\` with the characters they name.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The `+help` text: every command line action with its parameters.
pub fn usage() -> String {
    let mut s = String::from(
        "terminus                      start the Terminus window\nterminus +<command> [args]    send a command to the running Terminus\nterminus +mcp                 serve the commands as MCP tools on stdin and stdout\n\nCommands:\n",
    );
    for a in CATALOGUE.iter().filter(|a| a.cli) {
        let params: Vec<String> = a
            .params
            .iter()
            .map(|p| if p.required { format!("--{}", p.name) } else { format!("[--{}]", p.name) })
            .collect();
        s.push_str(&format!("  +{:<26} {}\n  {:<27} {}\n", a.name, params.join(" "), "", a.summary));
    }
    s.push_str("\nFull reference: docs/commands.md\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns string literals into an argument vector.
    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_positionals() {
        let a = parse("send-text", &v(&["ls\\n", "--pane", "3"])).unwrap();
        assert_eq!(a["text"], "ls\n");
        assert_eq!(a["pane"], 3);
        let a = parse("screen", &v(&["--json"])).unwrap();
        assert_eq!(a["json"], true);
        let a = parse("split", &v(&["--direction=down"])).unwrap();
        assert_eq!(a["direction"], "down");
        assert!(parse("split", &v(&["--direction", "sideways"])).is_err());
        assert!(parse("resize", &v(&["--cols", "80"])).unwrap_err().contains("--rows"));
        assert!(parse("copy", &[]).unwrap_err().contains("run-action"));
        assert!(parse("nope", &[]).is_err());
    }
}
