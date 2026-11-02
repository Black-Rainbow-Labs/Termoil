//! Paste wrapping and focus reports.

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// Removes every bracketed paste marker from the text, repeating until none is left so that
/// removing one marker cannot assemble another.
fn strip_paste_markers(text: &str) -> String {
    let mut current = text.to_string();
    loop {
        let next = current.replace(PASTE_END, "").replace(PASTE_START, "");
        if next == current {
            return next;
        }
        current = next;
    }
}

/// Converts line endings to a carriage return, which is what Enter sends.
fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\r").replace('\n', "\r")
}

/// Encodes pasted text. Bracketed mode wraps the text in `ESC [ 200 ~` and `ESC [ 201 ~` after removing
/// any bracket markers inside it, so a paste cannot end the bracket early. Unbracketed mode converts line
/// endings to a carriage return.
pub fn encode_paste(text: &str, bracketed: bool, out: &mut Vec<u8>) {
    if bracketed {
        out.extend_from_slice(PASTE_START.as_bytes());
        out.extend_from_slice(strip_paste_markers(text).as_bytes());
        out.extend_from_slice(PASTE_END.as_bytes());
    } else {
        out.extend_from_slice(normalize_line_endings(text).as_bytes());
    }
}

/// Encodes a focus change report: `ESC [ I` for focus in and `ESC [ O` for focus out.
pub fn encode_focus(focused: bool, out: &mut Vec<u8>) {
    out.extend_from_slice(if focused { b"\x1b[I" } else { b"\x1b[O" });
}
