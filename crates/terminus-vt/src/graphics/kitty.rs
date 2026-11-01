//! The kitty graphics command entry point: chunk assembly, transmission, query and put.

use super::media::{MediaRequest, read_media};
use super::pixels::{Decoded, MAX_PAYLOAD, PixelSpec, decode_base64, decode_pixels};
use super::place::{NewImage, PlaceParams};
use super::reply::{KErr, ReplyIds, build_reply};
use super::{GraphicsContext, GraphicsResult, GraphicsStore, KittyCommand, parse_kitty_command};

/// An upload in progress: the keys of the first chunk and the base64 text received so far.
#[derive(Debug)]
pub(super) struct Pending {
    /// The first chunk's command; its payload is replaced by the whole text when the upload completes.
    first: KittyCommand,
    /// Base64 text accumulated over all chunks so far.
    data: Vec<u8>,
    /// The upload grew past the limit: the rest is swallowed and the final chunk answers EFBIG.
    overflowed: bool,
}

/// The most base64 text one chunked upload may accumulate.
const MAX_ENCODED: usize = MAX_PAYLOAD / 3 * 4 + 8;

/// Keys a continuation chunk may carry.
const CHUNK_KEYS: &[u8] = b"mq";

impl GraphicsStore {
    /// Handles a kitty graphics command: the APC payload after the leading `G`.
    pub fn kitty_command(&mut self, payload: &[u8], ctx: &GraphicsContext) -> GraphicsResult {
        let cmd = match parse_kitty_command(payload) {
            Ok(cmd) => cmd,
            Err(_) => {
                self.pending = None;
                self.count_dropped();
                return GraphicsResult::default();
            }
        };
        match self.take_chunk(cmd) {
            Ok(Chunked::Waiting) => GraphicsResult::default(),
            Ok(Chunked::Complete(whole)) => self.run_command(&whole, ctx),
            Ok(Chunked::Overflow(first)) => self.overflow_reply(&first),
            Err(cmd) => self.run_command(&cmd, ctx),
        }
    }

    /// Feeds a command to the chunk assembler. `Err` hands the command back when it is not part of an upload.
    fn take_chunk(&mut self, cmd: KittyCommand) -> Result<Chunked, KittyCommand> {
        let is_chunk_action = matches!(cmd.char_or(b'a', b't'), b't' | b'T' | b'q' | b'f');
        if let Some(pending) = self.pending.take() {
            if !is_continuation(&cmd, &pending.first) {
                self.count_dropped();
                return self.start_chunk(cmd, is_chunk_action);
            }
            return Ok(self.append_chunk(pending, cmd));
        }
        self.start_chunk(cmd, is_chunk_action)
    }

    /// Begins an upload when the command has `m=1`; otherwise it is an ordinary command.
    fn start_chunk(&mut self, cmd: KittyCommand, is_chunk_action: bool) -> Result<Chunked, KittyCommand> {
        if !(is_chunk_action && cmd.uint(b'm') == 1) {
            return Err(cmd);
        }
        let mut first = cmd;
        let data = std::mem::take(&mut first.payload);
        let overflowed = data.len() > MAX_ENCODED;
        self.pending = Some(Pending { first, data: if overflowed { Vec::new() } else { data }, overflowed });
        Ok(Chunked::Waiting)
    }

    /// Adds a continuation chunk to an upload; the chunk with `m=0` (or no `m`) completes it.
    fn append_chunk(&mut self, mut pending: Pending, chunk: KittyCommand) -> Chunked {
        if !pending.overflowed {
            pending.data.extend_from_slice(&chunk.payload);
            if pending.data.len() > MAX_ENCODED {
                pending.overflowed = true;
                pending.data = Vec::new();
            }
        }
        if chunk.has(b'q') {
            set_key(&mut pending.first, b'q', chunk.get(b'q').unwrap_or(0));
        }
        if chunk.uint(b'm') == 1 {
            self.pending = Some(pending);
            return Chunked::Waiting;
        }
        if pending.overflowed {
            return Chunked::Overflow(pending.first);
        }
        let mut whole = pending.first;
        whole.payload = pending.data;
        Chunked::Complete(whole)
    }

    /// Answers an upload that grew past the size limit.
    fn overflow_reply(&mut self, first: &KittyCommand) -> GraphicsResult {
        self.count_dropped();
        let err = KErr::new("EFBIG", "Chunked payload is too large");
        let reply = build_reply(&reply_ids(first, 0), Err(&err), first.uint(b'q') as u8, false);
        GraphicsResult { reply, ..GraphicsResult::default() }
    }

    /// Runs a complete command (a whole upload, or any command that is not chunked).
    fn run_command(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext) -> GraphicsResult {
        let action = cmd.char_or(b'a', b't');
        if cmd.has_bad_sign() {
            let err = KErr::new("EINVAL", "Negative value for an unsigned key");
            return self.finish(cmd, 0, Err(err), None);
        }
        match action {
            b't' | b'T' => self.run_transmit(cmd, ctx, action == b'T'),
            b'q' => self.run_query(cmd, ctx),
            b'p' => self.run_put(cmd, ctx),
            b'd' => self.run_delete(cmd, ctx),
            b'f' => self.run_frame(cmd, ctx),
            b'a' => self.run_animation_control(cmd),
            b'c' => self.run_compose(cmd),
            _ => self.finish(cmd, 0, Err(KErr::new("EINVAL", "Unknown action")), None),
        }
    }

    /// Builds the final result: the reply (honouring `q`), the dirty flag and the cursor movement.
    pub(super) fn finish(
        &mut self, cmd: &KittyCommand, assigned: u32, outcome: Result<(), KErr>, mv: Option<(u16, u16)>,
    ) -> GraphicsResult {
        if outcome.is_err() && matches!(cmd.char_or(b'a', b't'), b't' | b'T' | b'q' | b'f') {
            self.count_dropped();
        }
        let ids = reply_ids(cmd, assigned);
        let always = cmd.char_or(b'a', b't') == b'q';
        let reply = build_reply(&ids, outcome.as_ref().map(|_| ()), cmd.uint(b'q') as u8, always);
        GraphicsResult { reply, move_cursor: mv, cursor_to_column_zero: false, dirty: false }
    }

    /// Decodes the pixels a command transmits, from the payload or from the medium it names.
    pub(super) fn load_pixels(
        &self, cmd: &KittyCommand, default_size: Option<(u32, u32)>, allow_files: bool,
    ) -> Result<Decoded, KErr> {
        let format = if cmd.has(b'f') { cmd.uint(b'f') } else { 32 };
        let (dw, dh) = default_size.unwrap_or((0, 0));
        let spec = PixelSpec {
            format,
            width: if cmd.has(b's') { cmd.uint(b's') } else { dw },
            height: if cmd.has(b'v') { cmd.uint(b'v') } else { dh },
            compressed: match cmd.char_or(b'o', 0) {
                0 => false,
                b'z' => true,
                _ => return Err(KErr::new("EINVAL", "Unsupported compression")),
            },
        };
        let limits = self.decode_limits();
        let bytes = match cmd.char_or(b't', b'd') {
            b'd' => decode_base64(&cmd.payload)?,
            m @ (b'f' | b't' | b's') => {
                let max_bytes = (limits.max_pixels.saturating_mul(4) as usize).min(self.quota).max(1 << 20);
                let request = MediaRequest {
                    medium: m,
                    size: cmd.uint(b'S'),
                    offset: cmd.uint(b'O'),
                    allowed: allow_files,
                    max_bytes,
                };
                read_media(&request, &cmd.payload)?
            }
            _ => return Err(KErr::new("EINVAL", "Unsupported transmission medium")),
        };
        decode_pixels(&spec, bytes, &limits)
    }

    /// Transmits an image (`a=t`) and, for `a=T`, places it at the cursor.
    fn run_transmit(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext, display: bool) -> GraphicsResult {
        if cmd.uint(b'i') != 0 && cmd.uint(b'I') != 0 {
            return self.finish(cmd, 0, Err(KErr::new("EINVAL", "Cannot specify both i and I")), None);
        }
        let stored = self.load_pixels(cmd, None, ctx.allow_file_transfer).and_then(|d| self.store_decoded(cmd, d));
        let id = match stored {
            Ok(id) => id,
            Err(e) => return self.finish(cmd, 0, Err(e), None),
        };
        if !display {
            return self.finish(cmd, id, Ok(()), None);
        }
        let params = PlaceParams::from_command(cmd);
        match self.place_image(id, &params, ctx) {
            Ok(placed) => {
                let mut result = self.finish(cmd, id, Ok(()), placed.move_cursor);
                result.dirty = placed.visible;
                result
            }
            Err(e) => self.finish(cmd, id, Err(e), None),
        }
    }

    /// Stores a decoded image under the command's id or number, returning the id it got.
    fn store_decoded(&mut self, cmd: &KittyCommand, d: Decoded) -> Result<u32, KErr> {
        self.insert_image(NewImage {
            id: cmd.uint(b'i'),
            number: cmd.uint(b'I'),
            width: d.width,
            height: d.height,
            rgba: d.rgba,
            persistent: cmd.char_or(b'a', b't') == b't',
            transient: cmd.uint(b'N') & 1 == 1,
        })
    }

    /// Validates a transmission and answers without storing anything (`a=q`).
    fn run_query(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext) -> GraphicsResult {
        let outcome = self.load_pixels(cmd, None, ctx.allow_file_transfer).and_then(|d| self.check_fits(&d));
        let assigned = cmd.uint(b'i');
        self.finish(cmd, assigned, outcome, None)
    }

    /// Checks a decoded image would fit the quota on its own.
    fn check_fits(&self, d: &Decoded) -> Result<(), KErr> {
        if d.rgba.len() > self.quota {
            return Err(KErr::new("ENOSPC", "Image is larger than the storage quota"));
        }
        Ok(())
    }

    /// Places an already transmitted image (`a=p`).
    fn run_put(&mut self, cmd: &KittyCommand, ctx: &GraphicsContext) -> GraphicsResult {
        let found = self.find_target(cmd).map(|i| self.images[i].id);
        let params = PlaceParams::from_command(cmd);
        let placed = found.and_then(|id| self.place_image(id, &params, ctx));
        match placed {
            Ok(p) => {
                let mut result = self.finish(cmd, p.image_id, Ok(()), p.move_cursor);
                result.reply = build_reply(&put_ids(cmd, &p), Ok(()), cmd.uint(b'q') as u8, false);
                result.dirty = p.visible;
                result
            }
            Err(e) => self.finish(cmd, cmd.uint(b'i'), Err(e), None),
        }
    }
}

/// What the chunk assembler decided about a command.
enum Chunked {
    /// More chunks are due; nothing to do yet.
    Waiting,
    /// The upload finished: run this command.
    Complete(KittyCommand),
    /// The upload grew too large; this is its first chunk, for the reply.
    Overflow(KittyCommand),
}

/// Whether `cmd` continues the upload begun by `first`: it carries only `m` and `q`, plus optionally the same
/// action (`a=f` chunks repeat it) and the same image id or number.
fn is_continuation(cmd: &KittyCommand, first: &KittyCommand) -> bool {
    let same_action = |c: u8| c == b'a' && cmd.char_or(b'a', 0) == first.char_or(b'a', b't');
    let same_image = |k: u8| (k == b'i' || k == b'I') && cmd.get(k) == first.get(k);
    cmd.keys.iter().all(|(k, _)| CHUNK_KEYS.contains(k) || same_action(*k) || same_image(*k))
}

/// Sets a key on a command, replacing an existing value.
fn set_key(cmd: &mut KittyCommand, key: u8, value: i64) {
    match cmd.keys.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => cmd.keys.push((key, value)),
    }
}

/// The ids a reply echoes: the client's `i` (or the id assigned for `I`), its `I`, and its `p`.
fn reply_ids(cmd: &KittyCommand, assigned: u32) -> ReplyIds {
    let number = cmd.uint(b'I');
    let id = if cmd.uint(b'i') != 0 {
        cmd.uint(b'i')
    } else if number != 0 {
        assigned
    } else {
        0
    };
    ReplyIds { id, number, placement: cmd.uint(b'p') }
}

/// Reply ids for a put: the image id that was found (so `I` resolves to its id) and the stored placement id.
fn put_ids(cmd: &KittyCommand, placed: &super::place::Placed) -> ReplyIds {
    let number = cmd.uint(b'I');
    let id = if cmd.uint(b'i') != 0 || number != 0 { placed.image_id } else { 0 };
    ReplyIds { id, number, placement: placed.placement_id }
}
