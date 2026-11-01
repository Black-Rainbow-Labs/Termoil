//! Replies to kitty graphics commands: `ESC _ G i=<id>[,I=<n>][,p=<p>] ; OK ESC \` or `;CODE:message`.

/// An error answer: a code from the protocol (EINVAL, ENOENT, EBADF, ...) and a human readable message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KErr {
    /// The error code, for example `ENOENT`.
    pub code: &'static str,
    /// Printable ASCII text after the code.
    pub msg: String,
}

impl KErr {
    /// Builds an error from a code and a message.
    pub fn new(code: &'static str, msg: impl Into<String>) -> KErr {
        KErr { code, msg: msg.into() }
    }
}

/// The ids a reply echoes back to the client. Zero means the client did not use that key.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ReplyIds {
    /// Image id (`i`): the client's own, or the one the terminal assigned for an image number.
    pub id: u32,
    /// Image number (`I`).
    pub number: u32,
    /// Placement id (`p`).
    pub placement: u32,
}

/// Builds the reply bytes for a command, or `None` when no reply is due.
///
/// A reply is due only when the client used `i` or `I`, except for queries (`always`). `quiet` is the `q`
/// key: 1 suppresses OK replies and 2 suppresses failure replies.
pub(crate) fn build_reply(ids: &ReplyIds, outcome: Result<(), &KErr>, quiet: u8, always: bool) -> Option<Vec<u8>> {
    if ids.id == 0 && ids.number == 0 && !always {
        return None;
    }
    if (quiet == 1 && outcome.is_ok()) || (quiet == 2 && outcome.is_err()) {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    if ids.id != 0 {
        keys.push(format!("i={}", ids.id));
    }
    if ids.number != 0 {
        keys.push(format!("I={}", ids.number));
    }
    if ids.placement != 0 {
        keys.push(format!("p={}", ids.placement));
    }
    let text = match outcome {
        Ok(()) => "OK".to_string(),
        Err(e) => format!("{}:{}", e.code, printable(&e.msg)),
    };
    Some(format!("\x1b_G{};{}\x1b\\", keys.join(","), text).into_bytes())
}

/// Replaces every byte that is not printable ASCII with `?` so the reply text is safe to send.
fn printable(text: &str) -> String {
    text.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect()
}
