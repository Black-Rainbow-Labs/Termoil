//! IPC messages: JSON lines over a named pipe (Windows) or a Unix socket.
//!
//! A request is one line `{"id":1,"action":"new-tab","args":{"cwd":"C:/src"}}`; the answer is one
//! line `{"id":1,"ok":true,"result":...}` or `{"id":1,"ok":false,"error":"..."}`. A connection may
//! send several requests; they are answered one at a time in the order sent, because a Windows pipe
//! opened for synchronous I/O cannot read and write at once. A client that wants a long
//! `wait-prompt` beside other requests opens a second connection.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// An action with its arguments, as a keybinding, the palette, the CLI or IPC names it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionCall {
    /// The action's catalogue name.
    pub action: String,
    /// Arguments by parameter name.
    #[serde(default)]
    pub args: Map<String, Value>,
}

/// One request line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Chosen by the client; echoed in the response.
    pub id: u64,
    /// The action.
    #[serde(flatten)]
    pub call: ActionCall,
}

/// One response line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The request's id.
    pub id: u64,
    /// Whether the action succeeded.
    pub ok: bool,
    /// The result on success.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub result: Value,
    /// The reason on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Why an action failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    /// No such action in the catalogue.
    Unknown(String),
    /// A required argument is missing or an argument has the wrong type.
    BadArgs(String),
    /// The action needs a pane or window that does not exist.
    NotFound(String),
    /// The action ran and failed.
    Failed(String),
    /// `wait-prompt` and the like ran out of time.
    Timeout,
}

impl std::fmt::Display for ActionError {
    /// A one line description, which is the IPC error text.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::Unknown(a) => write!(f, "unknown action '{a}'"),
            ActionError::BadArgs(s) => write!(f, "bad arguments: {s}"),
            ActionError::NotFound(s) => write!(f, "not found: {s}"),
            ActionError::Failed(s) => write!(f, "{s}"),
            ActionError::Timeout => write!(f, "timed out"),
        }
    }
}

impl std::error::Error for ActionError {}

impl Response {
    /// A response for an action's outcome.
    pub fn from_result(id: u64, result: Result<Value, ActionError>) -> Response {
        match result {
            Ok(v) => Response { id, ok: true, result: v, error: None },
            Err(e) => Response { id, ok: false, result: Value::Null, error: Some(e.to_string()) },
        }
    }
}

/// Encodes a request as one line, newline included.
pub fn encode_request(r: &Request) -> String {
    let mut s = serde_json::to_string(r).unwrap_or_default();
    s.push('\n');
    s
}

/// Decodes one request line. Lines longer than 16 MiB are refused.
pub fn decode_request(line: &str) -> Result<Request, String> {
    if line.len() > 16 << 20 {
        return Err("request too large".into());
    }
    serde_json::from_str(line.trim_end()).map_err(|e| e.to_string())
}

/// Encodes a response as one line, newline included.
pub fn encode_response(r: &Response) -> String {
    let mut s = serde_json::to_string(r).unwrap_or_default();
    s.push('\n');
    s
}

/// Decodes one response line.
pub fn decode_response(line: &str) -> Result<Response, String> {
    serde_json::from_str(line.trim_end()).map_err(|e| e.to_string())
}

/// The name of the running instance's socket: `\\.\pipe\terminus-<user>` on Windows,
/// `$XDG_RUNTIME_DIR/terminus.sock` (or the temp folder) elsewhere. `TERMINUS_SOCKET` overrides it,
/// which the tests use to run instances side by side.
pub fn socket_name() -> String {
    if let Ok(s) = std::env::var("TERMINUS_SOCKET") {
        return s;
    }
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into());
    if cfg!(windows) {
        format!(r"\\.\pipe\terminus-{user}")
    } else {
        let dir =
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
        format!("{dir}/terminus-{user}.sock")
    }
}
