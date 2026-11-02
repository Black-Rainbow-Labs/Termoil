//! Termoil IPC messages and the action catalogue (`docs/TDD.md` section 4.9).
//!
//! Every action a person or a program can take is a row in [`CATALOGUE`]. Keybindings, the command
//! palette, `termoil +command` and the IPC socket all name actions from this table, and
//! `App::run_action` is the one place an action becomes a change. `docs/commands.md` and the MCP
//! tool list (`docs/mcp-tools.json`) are generated from the table and checked by tests, never
//! written by hand. The crate has no window, GPU or font dependency and builds for wasm.

mod catalogue;
#[cfg(any(windows, unix))]
mod client;
mod docs;
mod ipc;

pub use catalogue::{ActionDef, CATALOGUE, Context, ParamDef, ParamKind, action};
#[cfg(any(windows, unix))]
pub use client::Client;
pub use docs::{commands_markdown, mcp_tools_json};
pub use ipc::{
    ActionCall, ActionError, Request, Response, decode_request, decode_response, encode_request, encode_response,
    separate_socket_name, socket_name,
};

/// The Termoil version, which `termoil +version` and XTVERSION report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
