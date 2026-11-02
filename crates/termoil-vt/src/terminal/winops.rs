//! XTWINOPS (`CSI Ps t`): window state reports, resize requests, the title stack and the title
//! modes.
//!
//! The terminal keeps a virtual window (position, iconified, maximized, full screen) so reports
//! and the tests that move or iconify a window have something to read back. The host that owns a
//! real window listens for `Event::ResizeRequest` and reads `Terminal::window` when it cares.

use super::{Event, Terminal};
use crate::parser::Params;

/// What the terminal remembers about its window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    /// Left edge in pixels, set by XTWINOPS 3.
    pub x: i32,
    /// Top edge in pixels, set by XTWINOPS 3.
    pub y: i32,
    /// True after XTWINOPS 2 and until XTWINOPS 1.
    pub iconified: bool,
    /// The size in cells before a maximize or full screen request, restored by the undo form.
    pub restore: Option<(u16, u16)>,
}

/// One entry of the title stack. Each part is None when the push did not save it.
#[derive(Clone, Debug, Default)]
pub(super) struct TitleEntry {
    /// The window title.
    pub title: Option<String>,
    /// The icon name.
    pub icon: Option<String>,
}

/// Title mode bit: labels are set in hexadecimal (`CSI > 0 t`).
const SET_HEX: u8 = 1;
/// Title mode bit: labels are reported in hexadecimal (`CSI > 1 t`).
const QUERY_HEX: u8 = 2;

impl Terminal {
    /// The size of the display in cells, which is what a maximize or a zero size asks for.
    fn display_cells(&self) -> (u16, u16) {
        let (w, h) = self.cell_pixel;
        let (sw, sh) = self.screen_pixels;
        ((sw / w as u32).clamp(1, u16::MAX as u32) as u16, (sh / h as u32).clamp(1, u16::MAX as u32) as u16)
    }

    /// Asks the host for a new size in cells. When the host asked for requests to be applied
    /// (the headless front end does), the terminal resizes itself at once, so a report that
    /// follows in the same chunk of output already sees the new size.
    pub(super) fn request_resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.clamp(1, 1000), rows.clamp(1, 1000));
        self.event(Event::ResizeRequest { cols, rows });
        if self.apply_resize && (cols, rows) != (self.cols(), self.rows()) {
            self.resize(cols, rows);
        }
    }

    /// XTWINOPS: dispatches on the first parameter.
    pub(super) fn window_op(&mut self, p: &Params) {
        match p.get(0) {
            1 => self.window.iconified = false,
            2 => self.window.iconified = true,
            3 => self.move_window(p),
            4 => self.resize_pixels(p),
            8 => self.resize_cells(p),
            9 => self.maximize(p.get(1)),
            10 => self.full_screen(p.get(1)),
            11 => self.reply(if self.window.iconified { b"\x1b[2t" } else { b"\x1b[1t" }),
            13 => self.reply(format!("\x1b[3;{};{}t", self.window.x, self.window.y).as_bytes()),
            14..=19 => self.report_sizes(p.get(0)),
            20 => self.report_label('L', self.icon_name.clone()),
            21 => self.report_label('l', self.title.clone()),
            22 => self.push_title(p.get(1)),
            23 => self.pop_title(p.get(1)),
            n if n >= 24 => self.request_resize(self.cols(), n),
            _ => {}
        }
    }

    /// XTWINOPS 3: moves the virtual window; an omitted coordinate is 0.
    fn move_window(&mut self, p: &Params) {
        self.window.x = p.get(1) as i32;
        self.window.y = p.get(2) as i32;
    }

    /// XTWINOPS 4: resizes the text area to a size in pixels. A parameter that is missing or empty keeps
    /// that direction as it is and a 0 uses the whole display.
    fn resize_pixels(&mut self, p: &Params) {
        let (cw, ch) = self.cell_pixel;
        let (dw, dh) = self.display_cells();
        let rows = p.get_opt(1).map_or(self.rows(), |v| zero_is(v / ch, dh));
        let cols = p.get_opt(2).map_or(self.cols(), |v| zero_is(v / cw, dw));
        self.request_resize(cols, rows);
    }

    /// XTWINOPS 8: resizes the text area to a size in cells, with the same rules as pixels.
    fn resize_cells(&mut self, p: &Params) {
        let (dw, dh) = self.display_cells();
        let rows = p.get_opt(1).map_or(self.rows(), |v| zero_is(v, dh));
        let cols = p.get_opt(2).map_or(self.cols(), |v| zero_is(v, dw));
        self.request_resize(cols, rows);
    }

    /// XTWINOPS 9: 0 undoes a maximize, 1 maximizes both ways, 2 vertically, 3 horizontally.
    fn maximize(&mut self, how: u16) {
        let (dw, dh) = self.display_cells();
        let (cols, rows) = (self.cols(), self.rows());
        let target = match how {
            0 => self.window.restore.take(),
            1 => Some((dw, dh)),
            2 => Some((cols, dh)),
            3 => Some((dw, rows)),
            _ => return,
        };
        if how != 0 && self.window.restore.is_none() {
            self.window.restore = Some((cols, rows));
        }
        if let Some((c, r)) = target {
            self.request_resize(c, r);
        }
    }

    /// XTWINOPS 10: 0 leaves full screen, 1 enters it, 2 toggles.
    fn full_screen(&mut self, how: u16) {
        let enter = match how {
            0 => false,
            1 => true,
            2 => self.window.restore.is_none(),
            _ => return,
        };
        self.maximize(if enter { 1 } else { 0 });
    }

    /// The size reports, XTWINOPS 14 to 19. The shell window has no frame, so asking for the
    /// window size (14 with 2) gives the text area size.
    fn report_sizes(&mut self, op: u16) {
        let (cw, ch) = self.cell_pixel;
        let (cols, rows) = (self.cols() as u32, self.rows() as u32);
        let (dw, dh) = self.display_cells();
        let reply = match op {
            14 => format!("\x1b[4;{};{}t", rows * ch as u32, cols * cw as u32),
            15 => format!("\x1b[5;{};{}t", self.screen_pixels.1, self.screen_pixels.0),
            16 => format!("\x1b[6;{ch};{cw}t"),
            18 => format!("\x1b[8;{rows};{cols}t"),
            19 => format!("\x1b[9;{dh};{dw}t"),
            _ => return,
        };
        self.reply(reply.as_bytes());
    }

    /// XTWINOPS 20 and 21: reports a label as `OSC L` or `OSC l`, in hexadecimal when the query
    /// hex title mode is on.
    fn report_label(&mut self, kind: char, text: String) {
        let body =
            if self.title_modes & QUERY_HEX != 0 { text.bytes().map(|b| format!("{b:02x}")).collect() } else { text };
        self.reply(format!("\x1b]{kind}{body}\x1b\\").as_bytes());
    }

    /// XTWINOPS 22: pushes the icon name (1), the window title (2) or both (0). A push of only
    /// one part fills the part still missing in the top entry, so an icon push followed by a
    /// window push makes one entry.
    fn push_title(&mut self, which: u16) {
        if which > 2 {
            return;
        }
        let (title, icon) = (self.title.clone(), self.icon_name.clone());
        let fits = |top: &TitleEntry| match which {
            1 => top.icon.is_none(),
            2 => top.title.is_none(),
            _ => false,
        };
        if self.title_stack.last().is_some_and(fits) {
            let top = self.title_stack.last_mut().expect("checked above");
            if which == 1 {
                top.icon = Some(icon);
            } else {
                top.title = Some(title);
            }
            return;
        }
        if self.title_stack.len() == self.config.title_stack_depth {
            self.title_stack.remove(0);
        }
        let entry = match which {
            1 => TitleEntry { title: None, icon: Some(icon) },
            2 => TitleEntry { title: Some(title), icon: None },
            _ => TitleEntry { title: Some(title), icon: Some(icon) },
        };
        self.title_stack.push(entry);
    }

    /// XTWINOPS 23: pops the top entry and restores the window title (2), the icon name (1) or
    /// both (0). The entry is removed whichever part was asked for.
    fn pop_title(&mut self, which: u16) {
        if which > 2 {
            return;
        }
        let Some(entry) = self.title_stack.pop() else { return };
        if which != 2
            && let Some(icon) = entry.icon
            && icon != self.icon_name
        {
            self.icon_name = icon.clone();
            self.event(Event::IconName(icon));
        }
        if which != 1
            && let Some(title) = entry.title
            && title != self.title
        {
            self.title = title.clone();
            self.event(Event::Title(title));
        }
    }

    /// `CSI > Ps t` sets and `CSI > Ps T` resets title modes: 0 set labels in hex, 1 report
    /// labels in hex, 2 and 3 are the UTF-8 forms, which a terminal that is always UTF-8 only
    /// records.
    pub(super) fn set_title_modes(&mut self, p: &Params, on: bool) {
        for group in p.iter() {
            let bit = 1u8 << (group.first().copied().unwrap_or(0).min(7));
            if on {
                self.title_modes |= bit;
            } else {
                self.title_modes &= !bit;
            }
        }
    }

    /// Decodes a title sent in hexadecimal when the set hex title mode is on. None when the text
    /// is not valid hex, in which case the title is left alone.
    pub(super) fn decode_title<'a>(&self, raw: &'a [u8]) -> Option<std::borrow::Cow<'a, [u8]>> {
        if self.title_modes & SET_HEX == 0 {
            return Some(std::borrow::Cow::Borrowed(raw));
        }
        if !raw.len().is_multiple_of(2) {
            return None;
        }
        let bytes: Option<Vec<u8>> =
            raw.chunks(2).map(|pair| u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok()).collect();
        bytes.map(std::borrow::Cow::Owned)
    }
}

/// `value`, or `max` when the value is 0.
fn zero_is(value: u16, max: u16) -> u16 {
    if value == 0 { max } else { value }
}
