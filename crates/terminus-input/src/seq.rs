//! Small byte writers shared by the encoders, plus the keyboard layout helpers they need.

pub(crate) const ESC: u8 = 0x1b;

/// Appends a decimal number to the output.
pub(crate) fn push_num(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(n.to_string().as_bytes());
}

/// Appends a character as UTF-8.
pub(crate) fn push_char(out: &mut Vec<u8>, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// Appends ESC [ (the control sequence introducer).
pub(crate) fn push_csi(out: &mut Vec<u8>) {
    out.extend_from_slice(&[ESC, b'[']);
}

/// Appends `CSI code ; mods u`, leaving the modifier field out when no modifier is set.
pub(crate) fn push_csi_u(out: &mut Vec<u8>, code: u32, wire_mods: u32) {
    push_csi(out);
    push_num(out, code);
    if wire_mods > 1 {
        out.push(b';');
        push_num(out, wire_mods);
    }
    out.push(b'u');
}

/// Returns the lowercase form of a character when lowercasing yields exactly one character.
pub(crate) fn unshifted(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Returns the character a key produces with shift held on a US layout.
pub(crate) fn us_shifted(c: char) -> char {
    match c {
        'a'..='z' => c.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '`' => '~',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        _ => c,
    }
}

/// True when the text contains a C0 control, DEL or a C1 control.
pub(crate) fn has_control(text: &str) -> bool {
    text.chars().any(|c| c.is_control())
}
