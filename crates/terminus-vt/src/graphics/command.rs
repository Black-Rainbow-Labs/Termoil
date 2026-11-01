//! Pure parser for the kitty graphics control data: `key=value,key=value;payload`.
//!
//! The parser knows nothing about images. It turns the bytes between `ESC _ G` and `ESC \` into a list of
//! integer valued keys plus the (still base64 encoded) payload, so the fuzz target can call it alone.

/// Keys whose value is a single ASCII character (action, transmission medium, compression, delete selector).
const CHAR_KEYS: &[u8] = b"atod";

/// Keys whose value is an integer. Any other letter is ignored so the protocol can grow.
const INT_KEYS: &[u8] = b"qfsvSOiIpmNxywhXYcrCUzPQHV";

/// Smallest value any integer key can hold (INT32_MIN, used by `z`, `H` and `V`).
const MIN_VALUE: i64 = i32::MIN as i64;

/// Largest value any integer key can hold (UINT32_MAX, used by ids and colours).
const MAX_VALUE: i64 = u32::MAX as i64;

/// Longest decimal number accepted for a value, including a leading minus sign.
const MAX_VALUE_DIGITS: usize = 11;

/// A parsed kitty graphics command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KittyCommand {
    /// The keys that were present, as (key letter, value). Character keys hold the character's byte value.
    pub keys: Vec<(u8, i64)>,
    /// The payload after the first `;`, exactly as received (base64 for direct transmission).
    pub payload: Vec<u8>,
}

/// Why the control data of a kitty graphics command could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KittyError {
    /// The control data contains a byte outside printable ASCII.
    NonAscii,
    /// A key is empty, is not a single letter, or has no `=` after it.
    BadKey,
    /// The value of the named key is not valid for that key.
    BadValue(u8),
    /// The value of the named key is outside the 32 bit range the protocol allows.
    ValueOutOfRange(u8),
}

impl KittyError {
    /// The error code string a reply carries for this error.
    pub fn code(&self) -> &'static str {
        "EINVAL"
    }

    /// A short message for the reply and for logs.
    pub fn message(&self) -> String {
        match self {
            KittyError::NonAscii => "Control data is not ASCII".to_string(),
            KittyError::BadKey => "Malformed key in control data".to_string(),
            KittyError::BadValue(k) => format!("Invalid value for key {}", *k as char),
            KittyError::ValueOutOfRange(k) => format!("Value out of range for key {}", *k as char),
        }
    }
}

impl KittyCommand {
    /// The value of a key if it was present.
    pub fn get(&self, key: u8) -> Option<i64> {
        self.keys.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// Whether the key was present.
    pub fn has(&self, key: u8) -> bool {
        self.get(key).is_some()
    }

    /// The value of a character key, or `default` when the key is absent.
    pub fn char_or(&self, key: u8, default: u8) -> u8 {
        self.get(key).map(|v| v as u8).unwrap_or(default)
    }

    /// The value of an unsigned key, 0 when absent. Negative values read as 0 (see `has_negative_unsigned`).
    pub fn uint(&self, key: u8) -> u32 {
        self.get(key).unwrap_or(0).clamp(0, MAX_VALUE) as u32
    }

    /// The value of a signed key (`z`, `H`, `V`), 0 when absent.
    pub fn int(&self, key: u8) -> i32 {
        self.get(key).unwrap_or(0).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    /// Whether an unsigned key holds a negative value, or a signed key a value outside the i32 range.
    pub fn has_bad_sign(&self) -> bool {
        self.keys.iter().any(|(k, v)| match k {
            b'z' | b'H' | b'V' => *v < i32::MIN as i64 || *v > i32::MAX as i64,
            _ if CHAR_KEYS.contains(k) => false,
            _ => *v < 0,
        })
    }
}

/// Parses the control data and payload of a kitty graphics command (the bytes after the leading `G`).
/// Empty segments between commas are ignored and unknown keys are skipped. The first `;` ends the control data.
pub fn parse_kitty_command(payload: &[u8]) -> Result<KittyCommand, KittyError> {
    let split = payload.iter().position(|&b| b == b';');
    let (control, data) = match split {
        Some(i) => (&payload[..i], &payload[i + 1..]),
        None => (payload, &payload[payload.len()..]),
    };
    if control.iter().any(|&b| !(0x20..0x7f).contains(&b)) {
        return Err(KittyError::NonAscii);
    }
    let mut keys: Vec<(u8, i64)> = Vec::new();
    for segment in control.split(|&b| b == b',').filter(|s| !s.is_empty()) {
        if let Some((key, value)) = parse_segment(segment)? {
            match keys.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => keys.push((key, value)),
            }
        }
    }
    Ok(KittyCommand { keys, payload: data.to_vec() })
}

/// Parses one `key=value` segment into a known key and its value, or `None` for an unknown letter.
fn parse_segment(segment: &[u8]) -> Result<Option<(u8, i64)>, KittyError> {
    let eq = segment.iter().position(|&b| b == b'=').ok_or(KittyError::BadKey)?;
    if eq != 1 || !segment[0].is_ascii_alphabetic() {
        return Err(KittyError::BadKey);
    }
    let (key, value) = (segment[0], &segment[eq + 1..]);
    if CHAR_KEYS.contains(&key) {
        if value.len() != 1 {
            return Err(KittyError::BadValue(key));
        }
        return Ok(Some((key, value[0] as i64)));
    }
    if !INT_KEYS.contains(&key) {
        return Ok(None);
    }
    parse_number(key, value).map(|v| Some((key, v)))
}

/// Parses a decimal integer value for `key`, checking digits and the 32 bit range.
fn parse_number(key: u8, value: &[u8]) -> Result<i64, KittyError> {
    let (negative, digits) = match value.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, value),
    };
    if digits.is_empty() || value.len() > MAX_VALUE_DIGITS || !digits.iter().all(u8::is_ascii_digit) {
        return Err(KittyError::BadValue(key));
    }
    let magnitude = digits.iter().fold(0i64, |acc, d| acc * 10 + (*d - b'0') as i64);
    let number = if negative { -magnitude } else { magnitude };
    if !(MIN_VALUE..=MAX_VALUE).contains(&number) {
        return Err(KittyError::ValueOutOfRange(key));
    }
    Ok(number)
}
