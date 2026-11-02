//! OSC: titles, colours, the working directory, hyperlinks, notifications, progress, the
//! clipboard, semantic prompts and the iTerm2 extensions.

use super::colorspec::parse_color;
use super::{Event, Hyperlink, PromptEvent, Terminal};
use crate::cell::{Semantic, row_flags};
use crate::parser::Terminator;
use crate::style::Rgb;

impl Terminal {
    /// Dispatches an OSC payload.
    pub(super) fn osc_dispatch(&mut self, data: &[u8], term: Terminator) {
        let (code, rest) = split_code(data);
        match code {
            Some(0) => {
                self.set_title(rest);
                self.set_icon_name(rest);
            }
            Some(1) => self.set_icon_name(rest),
            Some(2) => self.set_title(rest),
            Some(4) => self.osc_palette(rest, term),
            Some(5) => self.osc_special_colors(rest, term),
            Some(7) => self.osc_cwd(rest),
            Some(8) => self.osc_hyperlink(rest),
            Some(9) => self.osc_9(rest),
            Some(n @ 10..=19) => self.osc_dynamic_colors(n, rest, term),
            Some(52) => self.osc_clipboard(rest),
            Some(99) => self.osc_kitty_notification(rest, term),
            Some(104) => self.osc_reset_palette(rest),
            Some(105) => self.osc_reset_special_colors(rest),
            Some(n @ 110..=119) => self.reset_dynamic_color(n - 100),
            Some(133) => self.osc_semantic(rest),
            Some(777) => self.osc_777(rest),
            Some(1337) => self.osc_iterm2(rest),
            _ => {}
        }
    }

    /// OSC 2: the window title, bounded in length.
    fn set_title(&mut self, raw: &[u8]) {
        let Some(raw) = self.decode_title(raw) else { return };
        let raw = &raw[..];
        let title = bounded_string(raw, self.config.max_title);
        self.title = title.clone();
        self.event(Event::Title(title));
    }

    /// OSC 1: the icon name.
    fn set_icon_name(&mut self, raw: &[u8]) {
        let Some(raw) = self.decode_title(raw) else { return };
        let raw = &raw[..];
        let name = bounded_string(raw, self.config.max_title);
        self.icon_name = name.clone();
        self.event(Event::IconName(name));
    }

    /// OSC 4: sets or queries palette entries, given as pairs of index and colour. Indexes 256 to
    /// 260 are the special colours (bold, underline, blink, reverse, italic).
    fn osc_palette(&mut self, rest: &[u8], term: Terminator) {
        let text = String::from_utf8_lossy(rest);
        let parts: Vec<&str> = text.split(';').collect();
        for pair in parts.chunks(2) {
            let (Some(idx), Some(spec)) = (pair.first().and_then(|s| s.parse::<u16>().ok()), pair.get(1)) else {
                continue;
            };
            match idx {
                0..=255 => self.palette_entry(idx, spec, term),
                256..=260 => self.special_color(idx - 256, idx, spec, term, 4),
                _ => {}
            }
        }
    }

    /// Sets or queries one palette entry for OSC 4.
    fn palette_entry(&mut self, idx: u16, spec: &str, term: Terminator) {
        if spec == "?" {
            let c = self.palette[idx as usize];
            self.reply(format!("\x1b]4;{idx};{}{}", rgb_spec(c), terminator(term)).as_bytes());
        } else if let Some(c) = parse_color(spec) {
            self.palette[idx as usize] = c;
            self.event(Event::ColorsChanged);
            self.mark_all_dirty();
        }
    }

    /// OSC 5: sets or queries the special colours, given as pairs of number (0 to 4) and colour.
    fn osc_special_colors(&mut self, rest: &[u8], term: Terminator) {
        let text = String::from_utf8_lossy(rest);
        let parts: Vec<&str> = text.split(';').collect();
        for pair in parts.chunks(2) {
            let (Some(n), Some(spec)) = (pair.first().and_then(|s| s.parse::<u16>().ok()), pair.get(1)) else {
                continue;
            };
            if n < 5 {
                self.special_color(n, n, spec, term, 5);
            }
        }
    }

    /// Sets or queries special colour `n`. A reply names the colour the way the request did:
    /// `osc` is the OSC number used and `shown` the colour number in the request.
    fn special_color(&mut self, n: u16, shown: u16, spec: &str, term: Terminator, osc: u16) {
        if spec == "?" {
            let c = self.special_colors[n as usize].or_else(|| self.dynamic_color(10)).unwrap_or_default();
            self.reply(format!("\x1b]{osc};{shown};{}{}", rgb_spec(c), terminator(term)).as_bytes());
        } else if let Some(c) = parse_color(spec) {
            self.special_colors[n as usize] = Some(c);
            self.event(Event::ColorsChanged);
        }
    }

    /// OSC 104: resets the listed palette entries (256 to 260 are special colours), or all of them,
    /// to the palette the host gave.
    fn osc_reset_palette(&mut self, rest: &[u8]) {
        let defaults = self.base_palette;
        let text = String::from_utf8_lossy(rest);
        if text.is_empty() {
            self.palette = defaults;
            self.special_colors = [None; 5];
        } else {
            for idx in text.split(';').filter_map(|s| s.parse::<u16>().ok()) {
                match idx {
                    0..=255 => self.palette[idx as usize] = defaults[idx as usize],
                    256..=260 => self.special_colors[idx as usize - 256] = None,
                    _ => {}
                }
            }
        }
        self.event(Event::ColorsChanged);
        self.mark_all_dirty();
    }

    /// OSC 105: resets the listed special colours (0 to 4), or all of them.
    fn osc_reset_special_colors(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest);
        if text.is_empty() {
            self.special_colors = [None; 5];
        } else {
            for n in text.split(';').filter_map(|s| s.parse::<usize>().ok()).filter(|n| *n < 5) {
                self.special_colors[n] = None;
            }
        }
        self.event(Event::ColorsChanged);
    }

    /// OSC 10 to 19: sets or queries the dynamic colours; several may follow in one sequence.
    fn osc_dynamic_colors(&mut self, first: u16, rest: &[u8], term: Terminator) {
        let text = String::from_utf8_lossy(rest).into_owned();
        for (k, spec) in text.split(';').enumerate() {
            let n = first + k as u16;
            if n > 19 {
                break;
            }
            if spec == "?" {
                let c = self.dynamic_color(n).unwrap_or_default();
                self.reply(format!("\x1b]{n};{}{}", rgb_spec(c), terminator(term)).as_bytes());
            } else if let Some(c) = parse_color(spec) {
                self.set_dynamic_color(n, Some(c));
            }
        }
    }

    /// The value of dynamic colour `n`, falling back to the host's default.
    fn dynamic_color(&self, n: u16) -> Option<Rgb> {
        let (cur, def) = (&self.colors, &self.default_colors);
        match n {
            10 => cur.foreground.or(def.foreground),
            11 => cur.background.or(def.background),
            12 => cur.cursor.or(def.cursor).or(cur.foreground).or(def.foreground),
            17 => cur.selection_background.or(def.selection_background),
            19 => cur.selection_foreground.or(def.selection_foreground),
            13 | 15 | 18 => {
                self.extra_color_slot(n).and_then(|i| self.extra_colors[i]).or_else(|| self.dynamic_color(10))
            }
            14 | 16 => self.extra_color_slot(n).and_then(|i| self.extra_colors[i]).or_else(|| self.dynamic_color(11)),
            _ => None,
        }
    }

    /// The slot in `extra_colors` for the dynamic colours Termoil stores only to answer queries
    /// (13 mouse foreground, 14 mouse background, 15 Tektronix foreground, 16 Tektronix
    /// background, 18 Tektronix cursor).
    fn extra_color_slot(&self, n: u16) -> Option<usize> {
        match n {
            13 => Some(0),
            14 => Some(1),
            15 => Some(2),
            16 => Some(3),
            18 => Some(4),
            _ => None,
        }
    }

    /// Sets dynamic colour `n` (None resets it).
    fn set_dynamic_color(&mut self, n: u16, c: Option<Rgb>) {
        match n {
            10 => self.colors.foreground = c,
            11 => self.colors.background = c,
            12 => self.colors.cursor = c,
            17 => self.colors.selection_background = c,
            19 => self.colors.selection_foreground = c,
            _ => {
                let Some(slot) = self.extra_color_slot(n) else { return };
                self.extra_colors[slot] = c;
                self.event(Event::ColorsChanged);
                return;
            }
        }
        self.event(Event::ColorsChanged);
        self.mark_all_dirty();
    }

    /// OSC 110 to 119: resets one dynamic colour.
    fn reset_dynamic_color(&mut self, n: u16) {
        self.set_dynamic_color(n, None);
    }

    /// OSC 7: the working directory as a `file://` URL. Anything else is ignored.
    fn osc_cwd(&mut self, rest: &[u8]) {
        let Ok(url) = core::str::from_utf8(rest) else { return };
        if url.len() > 8192 || !url.starts_with("file://") {
            return;
        }
        self.working_directory = Some(url.to_string());
        self.event(Event::WorkingDirectory(url.to_string()));
    }

    /// OSC 8: starts (with a URI) or ends (with an empty URI) a hyperlink.
    fn osc_hyperlink(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest);
        let Some((params, uri)) = text.split_once(';') else { return };
        if uri.is_empty() || uri.len() > self.config.max_hyperlink_uri || !has_scheme(uri) {
            self.screen_mut().cursor.hyperlink = None;
            return;
        }
        let id = params.split(':').find_map(|kv| kv.strip_prefix("id=")).map(str::to_string);
        let link = Hyperlink { id, uri: uri.to_string() };
        let index = Some(self.intern_hyperlink(link));
        self.screen_mut().cursor.hyperlink = index;
    }

    /// OSC 9: a notification, or ConEmu's 9;4 progress and 9;9 working directory.
    fn osc_9(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest).into_owned();
        if let Some(progress) = text.strip_prefix("4;") {
            let mut it = progress.split(';');
            let state = it.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(0).min(4);
            let value = it.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(0).min(100);
            self.event(Event::Progress { state, value });
        } else if let Some(path) = text.strip_prefix("9;") {
            let path = path.trim_matches('"').to_string();
            self.working_directory = Some(path.clone());
            self.event(Event::WorkingDirectory(path));
        } else {
            self.event(Event::Notification { title: String::new(), body: text });
        }
    }

    /// OSC 777: `notify;title;body` (urxvt).
    fn osc_777(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest).into_owned();
        let mut it = text.splitn(3, ';');
        if it.next() == Some("notify") {
            let title = it.next().unwrap_or("").to_string();
            let body = it.next().unwrap_or("").to_string();
            self.event(Event::Notification { title, body });
        }
    }

    /// OSC 52: writes the clipboard, or asks the host to read it when allowed.
    fn osc_clipboard(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest).into_owned();
        let Some((sel, data)) = text.split_once(';') else { return };
        let selection = if sel.is_empty() {
            "c".to_string()
        } else {
            sel.chars().filter(|c| "cpqs01234567".contains(*c)).collect()
        };
        if data == "?" {
            if self.config.allow_osc52_read {
                self.event(Event::ClipboardRead { selection });
            }
            return;
        }
        // Characters outside the base64 alphabet are skipped, as xterm does, so a stray quote or
        // newline does not lose the whole write.
        let cleaned: Vec<u8> = data.bytes().filter(|b| b.is_ascii_alphanumeric() || b"+/=".contains(b)).collect();
        if let Some(bytes) = decode_base64(&cleaned) {
            self.event(Event::ClipboardWrite { selection, data: bytes });
        }
    }

    /// OSC 133: semantic prompt marks.
    fn osc_semantic(&mut self, rest: &[u8]) {
        let text = String::from_utf8_lossy(rest).into_owned();
        let mut parts = text.split(';');
        let kind = parts.next().unwrap_or("");
        let options: Vec<String> = parts.map(str::to_string).collect();
        match kind {
            "A" | "P" | "N" => self.prompt_start(&options),
            "B" => {
                self.screen_mut().cursor.semantic = Semantic::Input;
                self.event(Event::Prompt(PromptEvent::InputStart));
            }
            "C" => {
                self.screen_mut().cursor.semantic = Semantic::Output;
                self.event(Event::Prompt(PromptEvent::OutputStart));
            }
            "D" => {
                let code = options.first().and_then(|s| s.parse::<i32>().ok());
                self.last_exit_code = code;
                self.screen_mut().cursor.semantic = Semantic::Output;
                self.add_mark(super::MarkKind::CommandEnd, code);
                self.event(Event::Prompt(PromptEvent::CommandFinished(code)));
            }
            _ => {}
        }
    }

    /// OSC 133 A: a prompt starts on the cursor row.
    fn prompt_start(&mut self, options: &[String]) {
        let continuation = options.iter().any(|o| o == "k=s" || o == "k=c");
        self.click_events = options.iter().find_map(|o| o.strip_prefix("cl=")).map(str::to_string);
        let s = self.screen_mut();
        s.cursor.semantic = Semantic::Prompt;
        let row = s.cursor.row;
        let (page, i) = s.pages.active_mut(row);
        let r = page.row_mut(i);
        r.set(row_flags::SEMANTIC_PROMPT, true);
        r.set(row_flags::PROMPT_CONTINUATION, continuation);
        if !continuation {
            self.add_mark(super::MarkKind::PromptStart, None);
            self.event(Event::Prompt(PromptEvent::PromptStart));
        }
    }

    /// OSC 1337: the iTerm2 subset (CurrentDir and File).
    fn osc_iterm2(&mut self, rest: &[u8]) {
        if let Some(path) = rest.strip_prefix(b"CurrentDir=") {
            let path = String::from_utf8_lossy(path).into_owned();
            self.working_directory = Some(path.clone());
            self.event(Event::WorkingDirectory(path));
        } else if let Some(file) = rest.strip_prefix(b"File=") {
            let ctx = self.graphics_context();
            let result = self.graphics.iterm2_file(file, &ctx);
            self.apply_graphics_result(result);
        }
    }

    /// Marks every row of the shown screen for repaint.
    pub(super) fn mark_all_dirty(&mut self) {
        self.screen_mut().pages.mark_all_dirty();
    }
}

/// Splits `code;rest` and parses the code.
fn split_code(data: &[u8]) -> (Option<u16>, &[u8]) {
    let end = data.iter().position(|&b| b == b';').unwrap_or(data.len());
    let code = core::str::from_utf8(&data[..end]).ok().and_then(|s| s.parse::<u16>().ok());
    let rest = if end < data.len() { &data[end + 1..] } else { &[] };
    (code, rest)
}

/// UTF-8 text cut to at most `max` bytes at a character boundary.
fn bounded_string(raw: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(raw);
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].chars().filter(|c| !c.is_control()).collect()
}

/// True when a URI starts with a scheme (`letters:`).
fn has_scheme(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else { return false };
    !scheme.is_empty()
        && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}

/// The terminator to answer a query with: the one the query used.
pub(super) fn terminator(term: Terminator) -> &'static str {
    match term {
        Terminator::Bel => "\x07",
        Terminator::St => "\x1b\\",
    }
}

/// An `rgb:rrrr/gggg/bbbb` colour spec, the form xterm answers queries with.
pub(super) fn rgb_spec(c: Rgb) -> String {
    format!("rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}", c.r, c.r, c.g, c.g, c.b, c.b)
}

/// Decodes standard base64, tolerating missing padding. None for invalid input.
pub(super) fn decode_base64(data: &[u8]) -> Option<Vec<u8>> {
    use base64::Engine;
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    engine.decode(data).ok()
}
