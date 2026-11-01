//! The byte level parser: the vt100.net DEC ANSI state machine with a bulk text front end.
//!
//! The parser turns bytes into typed [`Action`] values and hands each one to a [`Handler`]. In
//! ground state it does not walk bytes one at a time: it finds the end of the printable run with the
//! SIMD scan, validates UTF-8 for the run in bulk, and emits one `Print` for the whole run. The
//! states for escape, CSI, DCS, OSC and the SOS, PM and APC strings follow
//! <https://vt100.net/emu/dec_ansi_parser>, with three additions the vte crate does not have:
//! APC payloads are kept (the kitty graphics protocol travels in them), 8 bit C1 controls are
//! recognised when they are not part of a UTF-8 sequence, and every buffer has a hard limit.
//!
//! Limits (see `docs/TDD.md` section 4.3 and `docs/decisions/parser.md`): 16 parameters per
//! sequence with 5 sub parameters each, a parameter value is clamped at 65535, 2 intermediates,
//! 8 MiB per OSC payload, 65 MiB per DCS or APC payload. A payload over its limit is dropped and
//! counted in [`ParserStats`], never grown.

use crate::scan;

/// Maximum number of parameters in one CSI or DCS sequence.
pub const MAX_PARAMS: usize = 16;
/// Maximum sub parameters (colon separated) after each parameter. Five rather than four, because
/// `38:2:<colour space>:r:g:b` needs five.
pub const MAX_SUBPARAMS: usize = 5;
/// Maximum intermediate bytes in an escape, CSI or DCS sequence.
pub const MAX_INTERMEDIATES: usize = 2;

/// Payload limits for the string sequences. Defaults are Ghostty's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParserLimits {
    /// Largest OSC payload kept, in bytes.
    pub osc_bytes: usize,
    /// Largest DCS payload kept, in bytes (sixel images arrive in DCS).
    pub dcs_bytes: usize,
    /// Largest APC payload kept, in bytes (kitty graphics arrive in APC).
    pub apc_bytes: usize,
}

impl Default for ParserLimits {
    /// The defaults from the TDD: 8 MiB for OSC, 65 MiB for DCS and APC.
    fn default() -> Self {
        ParserLimits { osc_bytes: 8 << 20, dcs_bytes: 65 << 20, apc_bytes: 65 << 20 }
    }
}

/// Counters for input the parser dropped because it broke a limit or the grammar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParserStats {
    /// OSC payloads dropped for being over the limit.
    pub osc_dropped: u64,
    /// DCS payloads dropped for being over the limit.
    pub dcs_dropped: u64,
    /// APC payloads dropped for being over the limit.
    pub apc_dropped: u64,
    /// CSI sequences ignored because they had too many intermediates or a malformed parameter.
    pub csi_ignored: u64,
    /// Parameters beyond [`MAX_PARAMS`] or sub parameters beyond [`MAX_SUBPARAMS`] that were dropped.
    pub params_clamped: u64,
}

/// The parameters of a CSI or DCS sequence: up to 16 groups, each a value and its sub parameters.
#[derive(Clone, Copy)]
pub struct Params {
    values: [[u16; MAX_SUBPARAMS + 1]; MAX_PARAMS],
    lens: [u8; MAX_PARAMS],
    count: u8,
    /// Bit `i` is set when group `i` was written with no digits (`CSI 8 ; ; 80 t`), as opposed
    /// to an explicit 0.
    empty: u16,
}

impl PartialEq for Params {
    /// Two lists are equal when they have the same groups. Slots past the count and the record
    /// of empty groups are not part of the value.
    fn eq(&self, other: &Self) -> bool {
        self.count == other.count && self.iter().eq(other.iter())
    }
}

impl Eq for Params {}

impl Default for Params {
    /// An empty parameter list.
    fn default() -> Self {
        Params { values: [[0; MAX_SUBPARAMS + 1]; MAX_PARAMS], lens: [0; MAX_PARAMS], count: 0, empty: 0 }
    }
}

impl core::fmt::Debug for Params {
    /// Prints the parameters the way they were written, with `:` between sub parameters.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let groups: Vec<&[u16]> = self.iter().collect();
        f.debug_list().entries(groups).finish()
    }
}

impl Params {
    /// Number of parameter groups.
    #[inline]
    pub fn len(&self) -> usize {
        self.count as usize
    }

    /// True when the sequence had no parameters at all.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The group at `index`: the parameter followed by its sub parameters. Empty when out of range.
    #[inline]
    pub fn group(&self, index: usize) -> &[u16] {
        if index < self.count as usize { &self.values[index][..self.lens[index] as usize] } else { &[] }
    }

    /// The first value of the group at `index`, or 0 when the group is missing.
    #[inline]
    pub fn get(&self, index: usize) -> u16 {
        if index < self.count as usize { self.values[index][0] } else { 0 }
    }

    /// The first value of the group at `index`; None when the group is missing or was left empty
    /// (`CSI 8 ; ; 80 t` has no value for its second parameter, which is not the same as 0).
    #[inline]
    pub fn get_opt(&self, index: usize) -> Option<u16> {
        (index < self.count as usize && self.empty & (1 << index) == 0).then(|| self.values[index][0])
    }

    /// The parameter at `index`, with 0 and a missing parameter both read as `default`.
    #[inline]
    pub fn get_or(&self, index: usize, default: u16) -> u16 {
        match self.get(index) {
            0 => default,
            v => v,
        }
    }

    /// Iterates over the groups in order.
    pub fn iter(&self) -> impl Iterator<Item = &[u16]> + '_ {
        (0..self.count as usize).map(move |i| self.group(i))
    }

    /// Builds a parameter list from groups, for tests and for synthesised sequences.
    pub fn from_groups(groups: &[&[u16]]) -> Params {
        let mut p = Params::default();
        for g in groups.iter().take(MAX_PARAMS) {
            let i = p.count as usize;
            let n = g.len().min(MAX_SUBPARAMS + 1);
            p.values[i][..n].copy_from_slice(&g[..n]);
            p.lens[i] = n as u8;
            p.count += 1;
        }
        p
    }
}

/// A complete CSI sequence, or the header of a DCS sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CsiSeq {
    /// The parameters.
    pub params: Params,
    /// The private marker byte (`<`, `=`, `>` or `?`) if the sequence started with one, else 0.
    pub private: u8,
    /// Intermediate bytes (0x20 to 0x2F), at most two.
    pub intermediates: [u8; MAX_INTERMEDIATES],
    /// How many of `intermediates` are used.
    pub intermediate_len: u8,
    /// The final byte.
    pub final_byte: u8,
}

impl CsiSeq {
    /// The intermediate bytes that were present.
    #[inline]
    pub fn intermediates(&self) -> &[u8] {
        &self.intermediates[..self.intermediate_len as usize]
    }
}

/// How a string sequence (OSC, DCS, APC) was terminated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Terminator {
    /// BEL (0x07), accepted for OSC only.
    Bel,
    /// ST as ESC backslash, or the 8 bit 0x9C.
    St,
}

/// One thing the parser recognised. Borrowed data lives until the handler returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action<'a> {
    /// A run of printable text with no control characters in it.
    Print(&'a str),
    /// A C0 control, or a C1 control that is not one of the sequence introducers.
    Execute(u8),
    /// An escape sequence: intermediates and the final byte.
    Esc { intermediates: &'a [u8], final_byte: u8 },
    /// A CSI sequence.
    Csi(&'a CsiSeq),
    /// An OSC payload, everything between `ESC ]` and the terminator.
    Osc(&'a [u8], Terminator),
    /// A DCS sequence: the header and the payload.
    Dcs(&'a CsiSeq, &'a [u8]),
    /// An APC payload, everything between `ESC _` and ST.
    Apc(&'a [u8]),
}

/// Receives the actions the parser produces.
pub trait Handler {
    /// Applies one action.
    fn handle(&mut self, action: Action<'_>);
}

impl<F: FnMut(Action<'_>)> Handler for F {
    /// A closure is a handler.
    fn handle(&mut self, action: Action<'_>) {
        self(action)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsPassthrough,
    DcsIgnore,
    OscString,
    SosPmString,
    ApcString,
    /// An ESC arrived inside a string; the next byte decides whether it was ST.
    StringEscape,
}

/// Which string sequence a buffered payload belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StringKind {
    Osc,
    Dcs,
    Apc,
    Ignore,
}

/// The parser. Holds the state machine, the sequence being collected, and a partial UTF-8 code
/// point carried across calls.
pub struct Parser {
    state: State,
    seq: CsiSeq,
    param: u32,
    sub_index: usize,
    seq_valid: bool,
    has_params: bool,
    param_digits: bool,
    string: Vec<u8>,
    string_kind: StringKind,
    string_overflow: bool,
    string_utf8_pending: u8,
    string_origin: State,
    utf8: [u8; 4],
    utf8_len: u8,
    limits: ParserLimits,
    stats: ParserStats,
    simd: bool,
}

impl Default for Parser {
    /// A parser in ground state with the default limits.
    fn default() -> Self {
        Parser::new(ParserLimits::default())
    }
}

impl Parser {
    /// Creates a parser in ground state with the given payload limits.
    pub fn new(limits: ParserLimits) -> Parser {
        Parser {
            state: State::Ground,
            seq: CsiSeq::default(),
            param: 0,
            sub_index: 0,
            seq_valid: true,
            has_params: false,
            param_digits: false,
            string: Vec::new(),
            string_kind: StringKind::Ignore,
            string_overflow: false,
            string_utf8_pending: 0,
            string_origin: State::Ground,
            utf8: [0; 4],
            utf8_len: 0,
            limits,
            stats: ParserStats::default(),
            simd: true,
        }
    }

    /// Chooses between the SIMD front end (the default) and the plain byte loop. Both must give the
    /// same actions; the tests run every case through both.
    pub fn set_simd(&mut self, on: bool) {
        self.simd = on;
    }

    /// The counters of dropped and clamped input.
    pub fn stats(&self) -> ParserStats {
        self.stats
    }

    /// True when the parser is in ground state with no partial code point pending.
    pub fn is_ground(&self) -> bool {
        self.state == State::Ground && self.utf8_len == 0
    }

    /// Feeds bytes through the state machine, calling `handler` for every action recognised.
    pub fn advance<H: Handler>(&mut self, bytes: &[u8], handler: &mut H) {
        let mut i = 0;
        while i < bytes.len() {
            if self.utf8_len > 0 {
                i += self.finish_partial_utf8(&bytes[i..], handler);
                continue;
            }
            match self.state {
                State::Ground => i += self.advance_ground(&bytes[i..], handler),
                State::OscString | State::ApcString | State::DcsPassthrough => {
                    i += self.advance_string_bulk(&bytes[i..], handler)
                }
                State::CsiEntry | State::CsiParam => match self.advance_csi_bulk(&bytes[i..], handler) {
                    0 => {
                        self.step(bytes[i], handler);
                        i += 1;
                    }
                    n => i += n,
                },
                _ => {
                    self.step(bytes[i], handler);
                    i += 1;
                }
            }
        }
    }

    /// Processes ground state text. Returns how many bytes were consumed.
    fn advance_ground<H: Handler>(&mut self, bytes: &[u8], handler: &mut H) -> usize {
        let first = bytes[0];
        if (0x20..0x7f).contains(&first) {
            let n = if self.simd { scan::first_non_printable(bytes) } else { scan::first_non_printable_scalar(bytes) };
            let text = scan::ascii_str(&bytes[..n]);
            handler.handle(Action::Print(text));
            return n;
        }
        match first {
            0x1b if bytes.get(1) == Some(&b'[') => {
                // ESC [ goes straight to CSI, the most common sequence by far.
                self.clear_seq();
                self.state = State::CsiEntry;
                return 2;
            }
            0x1b => self.enter_escape(),
            0x18 | 0x1a | 0x7f => self.step(first, handler),
            0x00..=0x1f => handler.handle(Action::Execute(first)),
            _ => return self.advance_utf8(bytes, handler),
        }
        1
    }

    /// Reads CSI parameter bytes, a private marker and the final byte in one loop, the common
    /// shape of every CSI sequence. Stops (returning bytes consumed) at anything else, which the
    /// general state machine then handles: intermediates, controls inside the sequence, ESC.
    fn advance_csi_bulk<H: Handler>(&mut self, bytes: &[u8], handler: &mut H) -> usize {
        for (k, &b) in bytes.iter().enumerate() {
            match b {
                b'0'..=b'9' => {
                    self.has_params = true;
                    self.param_digits = true;
                    self.param = (self.param * 10 + (b - b'0') as u32).min(u16::MAX as u32);
                    self.state = State::CsiParam;
                }
                b';' | b':' => {
                    self.param_byte(b);
                    self.state = State::CsiParam;
                }
                0x3c..=0x3f if self.state == State::CsiEntry => {
                    self.seq.private = b;
                    self.state = State::CsiParam;
                }
                0x40..=0x7e => {
                    self.dispatch_csi(b, handler);
                    return k + 1;
                }
                _ => return k,
            }
        }
        bytes.len()
    }

    /// Decodes a run of UTF-8 text that starts with a non ASCII byte, up to the next C0 control.
    /// Invalid bytes become U+FFFD, except a lone 8 bit C1 control which is executed.
    fn advance_utf8<H: Handler>(&mut self, bytes: &[u8], handler: &mut H) -> usize {
        let end = if self.simd { scan::first_control(bytes) } else { scan::first_control_scalar(bytes) };
        let run = &bytes[..end];
        let result = if self.simd {
            simdutf8::compat::from_utf8(run).map_err(|e| (e.valid_up_to(), e.error_len()))
        } else {
            core::str::from_utf8(run).map_err(|e| (e.valid_up_to(), e.error_len()))
        };
        match result {
            Ok(text) => {
                handler.handle(Action::Print(text));
                end
            }
            Err((valid, error_len)) => {
                if valid > 0 {
                    // The prefix was validated by the call above.
                    let text = core::str::from_utf8(&run[..valid]).unwrap_or_default();
                    handler.handle(Action::Print(text));
                    return valid;
                }
                self.handle_invalid_utf8(run, error_len, end < bytes.len(), handler)
            }
        }
    }

    /// Handles invalid or incomplete UTF-8 at the start of `run`. Returns bytes consumed.
    fn handle_invalid_utf8<H: Handler>(
        &mut self, run: &[u8], error_len: Option<usize>, more_follows: bool, handler: &mut H,
    ) -> usize {
        match error_len {
            Some(1) if (0x80..0xa0).contains(&run[0]) => {
                self.c1_control(run[0], handler);
                1
            }
            Some(n) => {
                handler.handle(Action::Print("\u{FFFD}"));
                n
            }
            None if more_follows => {
                // A sequence cut short by a control character.
                handler.handle(Action::Print("\u{FFFD}"));
                run.len()
            }
            None => {
                // The chunk ended inside a code point; keep the bytes for the next call.
                self.utf8[..run.len()].copy_from_slice(run);
                self.utf8_len = run.len() as u8;
                run.len()
            }
        }
    }

    /// Completes a code point split across two calls. Returns bytes consumed from `bytes`.
    fn finish_partial_utf8<H: Handler>(&mut self, bytes: &[u8], handler: &mut H) -> usize {
        let have = self.utf8_len as usize;
        let need = utf8_len_of(self.utf8[0]).max(have + 1);
        let mut buf = [0u8; 4];
        buf[..have].copy_from_slice(&self.utf8[..have]);
        let take = (need - have).min(bytes.len());
        buf[have..have + take].copy_from_slice(&bytes[..take]);
        let total = have + take;
        match core::str::from_utf8(&buf[..total]) {
            Ok(text) => {
                handler.handle(Action::Print(text));
                self.utf8_len = 0;
                take
            }
            Err(e) if e.error_len().is_none() && total < need => {
                self.utf8[..total].copy_from_slice(&buf[..total]);
                self.utf8_len = total as u8;
                take
            }
            Err(e) => {
                // The kept bytes plus the new ones do not form a code point. The kept bytes
                // are one replacement character; the new bytes are parsed again normally.
                self.utf8_len = 0;
                handler.handle(Action::Print("\u{FFFD}"));
                let used_new = e.error_len().map(|n| n.saturating_sub(have)).unwrap_or(0);
                used_new.min(take)
            }
        }
    }

    /// Handles an 8 bit C1 control byte found outside a UTF-8 sequence, as its 7 bit ESC form.
    fn c1_control<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        self.enter_escape();
        self.step(byte - 0x40, handler);
    }

    /// Clears the collected sequence and enters escape state.
    fn enter_escape(&mut self) {
        self.state = State::Escape;
        self.clear_seq();
    }

    /// Resets the sequence being collected. Only the counts are cleared: parameter slots past
    /// the counts are never read, so the 200 bytes of values are left as they are.
    fn clear_seq(&mut self) {
        self.seq.params.count = 0;
        self.seq.params.empty = 0;
        self.seq.private = 0;
        self.seq.intermediate_len = 0;
        self.seq.final_byte = 0;
        self.param = 0;
        self.sub_index = 0;
        self.seq_valid = true;
        self.has_params = false;
        self.param_digits = false;
    }

    /// Advances the state machine by one byte (every state but the bulk text paths).
    fn step<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        if self.state == State::StringEscape {
            self.string_escape_byte(byte, handler);
            return;
        }
        if self.anywhere(byte, handler) {
            return;
        }
        match self.state {
            State::Ground => self.ground_byte(byte, handler),
            State::Escape => self.escape_byte(byte, handler),
            State::EscapeIntermediate => self.escape_intermediate_byte(byte, handler),
            State::CsiEntry | State::CsiParam | State::CsiIntermediate | State::CsiIgnore => {
                self.csi_byte(byte, handler)
            }
            State::DcsEntry | State::DcsParam | State::DcsIntermediate | State::DcsIgnore => self.dcs_header_byte(byte),
            State::DcsPassthrough | State::OscString | State::ApcString | State::SosPmString => {
                self.string_byte(byte, handler)
            }
            State::StringEscape => {}
        }
    }

    /// The transitions that apply in every state: CAN and SUB abort, ESC restarts, and the 8 bit
    /// C1 introducers outside string states. Returns true when the byte was consumed.
    fn anywhere<H: Handler>(&mut self, byte: u8, handler: &mut H) -> bool {
        let in_string = matches!(
            self.state,
            State::OscString | State::DcsPassthrough | State::ApcString | State::SosPmString | State::DcsIgnore
        );
        match byte {
            0x18 | 0x1a => {
                if in_string {
                    self.abort_string();
                }
                handler.handle(Action::Execute(byte));
                self.state = State::Ground;
                true
            }
            0x1b => {
                if in_string {
                    self.string_origin = self.state;
                    self.state = State::StringEscape;
                } else {
                    self.enter_escape();
                }
                true
            }
            0x80..=0x9f if !in_string => {
                self.c1_control(byte, handler);
                true
            }
            _ => false,
        }
    }

    /// A single byte in ground state that is not part of a text run: C0 controls and DEL.
    fn ground_byte<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        match byte {
            0x00..=0x1f => handler.handle(Action::Execute(byte)),
            0x7f => {}
            _ => {
                let mut buf = [0u8; 4];
                let s = (byte as char).encode_utf8(&mut buf);
                handler.handle(Action::Print(s));
            }
        }
    }

    /// A byte after ESC.
    fn escape_byte<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        match byte {
            0x00..=0x1f => handler.handle(Action::Execute(byte)),
            0x20..=0x2f => {
                self.collect(byte);
                self.state = State::EscapeIntermediate;
            }
            b'[' => {
                self.clear_seq();
                self.state = State::CsiEntry;
            }
            b']' => self.start_string(StringKind::Osc, State::OscString),
            b'P' => {
                self.clear_seq();
                self.start_string(StringKind::Ignore, State::DcsEntry);
            }
            b'_' => self.start_string(StringKind::Apc, State::ApcString),
            b'X' | b'^' => self.start_string(StringKind::Ignore, State::SosPmString),
            0x30..=0x7e => self.dispatch_esc(byte, handler),
            _ => {}
        }
    }

    /// A byte after ESC and at least one intermediate.
    fn escape_intermediate_byte<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        match byte {
            0x00..=0x1f => handler.handle(Action::Execute(byte)),
            0x20..=0x2f => self.collect(byte),
            0x30..=0x7e => self.dispatch_esc(byte, handler),
            _ => {}
        }
    }

    /// Emits an escape sequence and returns to ground.
    fn dispatch_esc<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        self.state = State::Ground;
        if self.seq_valid {
            let n = self.seq.intermediate_len as usize;
            let inter = self.seq.intermediates;
            handler.handle(Action::Esc { intermediates: &inter[..n], final_byte: byte });
        }
    }

    /// Records an intermediate byte; a third one makes the sequence invalid.
    fn collect(&mut self, byte: u8) {
        let n = self.seq.intermediate_len as usize;
        if n < MAX_INTERMEDIATES {
            self.seq.intermediates[n] = byte;
            self.seq.intermediate_len += 1;
        } else {
            self.seq_valid = false;
        }
    }

    /// One byte of a CSI sequence after `ESC [`.
    fn csi_byte<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        match (self.state, byte) {
            (_, 0x00..=0x1f) => handler.handle(Action::Execute(byte)),
            (_, 0x7f) => {}
            (State::CsiIgnore, 0x40..=0x7e) => self.state = State::Ground,
            (State::CsiIgnore, _) => {}
            (State::CsiEntry, 0x3c..=0x3f) => {
                self.seq.private = byte;
                self.state = State::CsiParam;
            }
            (State::CsiEntry | State::CsiParam, b'0'..=b'9' | b';' | b':') => {
                self.param_byte(byte);
                self.state = State::CsiParam;
            }
            (State::CsiParam, 0x3c..=0x3f) => self.state = State::CsiIgnore,
            (State::CsiIntermediate, 0x30..=0x3f) => self.state = State::CsiIgnore,
            (_, 0x20..=0x2f) => {
                self.collect(byte);
                self.state = State::CsiIntermediate;
            }
            (_, 0x40..=0x7e) => self.dispatch_csi(byte, handler),
            _ => {}
        }
    }

    /// One byte of a DCS header, before the payload starts.
    fn dcs_header_byte(&mut self, byte: u8) {
        match (self.state, byte) {
            (_, 0x00..=0x1f | 0x7f) => {}
            (State::DcsIgnore, _) => {}
            (State::DcsEntry, 0x3c..=0x3f) => {
                self.seq.private = byte;
                self.state = State::DcsParam;
            }
            (State::DcsEntry | State::DcsParam, b'0'..=b'9' | b';' | b':') => {
                self.param_byte(byte);
                self.state = State::DcsParam;
            }
            (State::DcsParam, 0x3c..=0x3f) | (State::DcsIntermediate, 0x30..=0x3f) => self.state = State::DcsIgnore,
            (_, 0x20..=0x2f) => {
                self.collect(byte);
                self.state = State::DcsIntermediate;
            }
            (_, 0x40..=0x7e) => {
                self.finish_params();
                self.seq.final_byte = byte;
                let kind = if self.seq_valid { StringKind::Dcs } else { StringKind::Ignore };
                self.start_string(kind, State::DcsPassthrough);
            }
            _ => {}
        }
    }

    /// Accumulates a digit, `;` or `:` into the parameter list.
    fn param_byte(&mut self, byte: u8) {
        self.has_params = true;
        match byte {
            b'0'..=b'9' => {
                self.param_digits = true;
                self.param = (self.param * 10 + (byte - b'0') as u32).min(u16::MAX as u32);
            }
            b':' => {
                self.store_param();
                if self.sub_index < MAX_SUBPARAMS {
                    self.sub_index += 1;
                } else {
                    self.stats.params_clamped += 1;
                    self.sub_index = MAX_SUBPARAMS + 1;
                }
            }
            _ => {
                self.store_param();
                self.close_group();
            }
        }
    }

    /// Writes the parameter being read into the current group slot.
    fn store_param(&mut self) {
        let group = self.seq.params.count as usize;
        if group >= MAX_PARAMS {
            self.param = 0;
            return;
        }
        if self.sub_index <= MAX_SUBPARAMS {
            self.seq.params.values[group][self.sub_index] = self.param as u16;
            self.seq.params.lens[group] = self.sub_index as u8 + 1;
        }
        if self.sub_index == 0 && !self.param_digits {
            self.seq.params.empty |= 1 << group;
        }
        self.param_digits = false;
        self.param = 0;
    }

    /// Ends the current group after a `;`.
    fn close_group(&mut self) {
        if (self.seq.params.count as usize) < MAX_PARAMS {
            self.seq.params.count += 1;
        } else {
            self.stats.params_clamped += 1;
        }
        self.sub_index = 0;
    }

    /// Stores the last parameter when the final byte arrives.
    fn finish_params(&mut self) {
        if self.has_params {
            self.store_param();
            self.close_group();
        }
    }

    /// Emits a CSI sequence and returns to ground.
    fn dispatch_csi<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        self.state = State::Ground;
        if !self.seq_valid {
            self.stats.csi_ignored += 1;
            return;
        }
        self.finish_params();
        self.seq.final_byte = byte;
        handler.handle(Action::Csi(&self.seq));
    }

    /// Begins collecting a string payload of the given kind.
    fn start_string(&mut self, kind: StringKind, state: State) {
        self.string.clear();
        self.string_kind = kind;
        self.string_overflow = false;
        self.string_utf8_pending = 0;
        self.state = state;
    }

    /// The payload limit for the current string kind.
    fn string_limit(&self) -> usize {
        match self.string_kind {
            StringKind::Osc => self.limits.osc_bytes,
            StringKind::Dcs => self.limits.dcs_bytes,
            StringKind::Apc => self.limits.apc_bytes,
            StringKind::Ignore => 0,
        }
    }

    /// Appends payload bytes, dropping the whole payload once it passes the limit.
    fn push_string(&mut self, bytes: &[u8]) {
        if self.string_overflow || self.string_kind == StringKind::Ignore {
            return;
        }
        if self.string.len() + bytes.len() > self.string_limit() {
            self.string_overflow = true;
            self.string = Vec::new();
            return;
        }
        self.string.extend_from_slice(bytes);
    }

    /// Consumes a run of payload bytes in OSC, DCS or APC state, stopping at a byte that needs
    /// the state machine. Returns bytes consumed.
    fn advance_string_bulk<H: Handler>(&mut self, bytes: &[u8], handler: &mut H) -> usize {
        let n = if self.simd { scan::first_non_printable(bytes) } else { scan::first_non_printable_scalar(bytes) };
        if n == 0 {
            self.step(bytes[0], handler);
            return 1;
        }
        self.string_utf8_pending = 0;
        self.push_string(&bytes[..n]);
        n
    }

    /// One byte of a string payload that is not plain ASCII text.
    fn string_byte<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        if byte == 0x07 && self.state == State::OscString {
            self.finish_string(Terminator::Bel, handler);
            return;
        }
        if byte == 0x9c && self.string_utf8_pending == 0 {
            self.finish_string(Terminator::St, handler);
            return;
        }
        let keep_c0 = self.state == State::DcsPassthrough;
        if (byte < 0x20 && !keep_c0) || self.state == State::SosPmString {
            return;
        }
        self.track_string_utf8(byte);
        self.push_string(&[byte]);
    }

    /// Tracks whether the payload is inside a UTF-8 sequence, so that 0x9C as a continuation
    /// byte is text and only a free standing 0x9C ends the string.
    fn track_string_utf8(&mut self, byte: u8) {
        self.string_utf8_pending = match byte {
            0xc2..=0xdf => 1,
            0xe0..=0xef => 2,
            0xf0..=0xf4 => 3,
            0x80..=0xbf => self.string_utf8_pending.saturating_sub(1),
            _ => 0,
        };
    }

    /// The byte after an ESC inside a string. A backslash completes ST. In DCS a second ESC is a
    /// literal ESC in the payload, which is how tmux wraps sequences it passes through. Anything
    /// else ends the string and starts a new escape sequence with that byte.
    fn string_escape_byte<H: Handler>(&mut self, byte: u8, handler: &mut H) {
        let origin = self.string_origin;
        if byte == b'\\' {
            self.state = origin;
            self.finish_string(Terminator::St, handler);
            return;
        }
        if byte == 0x1b && origin == State::DcsPassthrough {
            self.state = origin;
            self.push_string(&[0x1b]);
            return;
        }
        self.state = origin;
        self.finish_string(Terminator::St, handler);
        self.enter_escape();
        self.step(byte, handler);
    }

    /// Dispatches the collected payload and returns to ground.
    fn finish_string<H: Handler>(&mut self, term: Terminator, handler: &mut H) {
        self.state = State::Ground;
        if self.string_overflow {
            self.count_overflow();
            self.string_overflow = false;
            return;
        }
        match self.string_kind {
            StringKind::Osc => handler.handle(Action::Osc(&self.string, term)),
            StringKind::Apc => handler.handle(Action::Apc(&self.string)),
            StringKind::Dcs => {
                let seq = self.seq;
                handler.handle(Action::Dcs(&seq, &self.string));
            }
            StringKind::Ignore => {}
        }
        self.shrink_string();
    }

    /// Drops a payload after CAN or SUB.
    fn abort_string(&mut self) {
        self.string.clear();
        self.string_overflow = false;
        self.shrink_string();
    }

    /// Releases a large payload buffer so one big image does not pin memory forever.
    fn shrink_string(&mut self) {
        self.string.clear();
        if self.string.capacity() > 64 * 1024 {
            self.string = Vec::new();
        }
    }

    /// Counts a payload dropped for size.
    fn count_overflow(&mut self) {
        match self.string_kind {
            StringKind::Osc => self.stats.osc_dropped += 1,
            StringKind::Dcs => self.stats.dcs_dropped += 1,
            StringKind::Apc => self.stats.apc_dropped += 1,
            StringKind::Ignore => {}
        }
    }
}

/// The length of the UTF-8 sequence a lead byte starts (1 for anything that is not a lead byte).
fn utf8_len_of(lead: u8) -> usize {
    match lead {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}
