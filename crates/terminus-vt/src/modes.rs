//! Terminal modes: every ANSI and DEC private mode Terminus knows, with its default and its
//! DECRQM answer.
//!
//! Each mode is a row in [`MODES`]. A mode the terminal recognises but does not let a program
//! change is "permanently set" or "permanently reset", and DECRQM reports it as 3 or 4. Modes
//! that are not in the table get 0 (not recognised), which is also the answer for the non goals
//! in the PRD (VT52, DRCS, Tektronix).

/// Whether a mode number is an ANSI mode (`CSI Pm h`) or a DEC private mode (`CSI ? Pm h`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ModeKind {
    /// ANSI (ECMA-48) mode.
    Ansi,
    /// DEC private mode.
    Dec,
}

/// Names for the modes the terminal reads, as indexes into [`MODES`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum Mode {
    KeyboardAction,
    Insert,
    SendReceive,
    LinefeedNewline,
    CursorKeys,
    Ansi,
    Column132,
    SmoothScroll,
    ReverseVideo,
    Origin,
    Autowrap,
    AutoRepeat,
    MouseX10,
    CursorBlink,
    CursorVisible,
    Allow132,
    ReverseWrap,
    AltScreenLegacy,
    KeypadApplication,
    BackarrowSendsBs,
    LeftRightMargins,
    SixelDisplay,
    NoClearOnColumnChange,
    MouseNormal,
    MouseHighlight,
    MouseButtonEvent,
    MouseAnyEvent,
    FocusEvents,
    MouseUtf8,
    MouseSgr,
    AlternateScroll,
    MouseUrxvt,
    MouseSgrPixels,
    MetaSendsEscape,
    AltSendsEscape,
    ReverseWrapExtended,
    AltScreen,
    SaveCursor,
    AltScreenSaveCursor,
    SixelPrivateColors,
    BracketedPaste,
    SynchronizedOutput,
    GraphemeCluster,
    ColorSchemeReport,
    InBandResize,
    Win32Input,
    PrintFormFeed,
    PrintExtent,
    HebrewEncoding,
    NationalCharset,
    HorizontalCursorCoupling,
    MoreFix,
}

/// One row of the mode table.
#[derive(Clone, Copy, Debug)]
pub struct ModeDef {
    /// The mode's name.
    pub mode: Mode,
    /// ANSI or DEC.
    pub kind: ModeKind,
    /// The number programs use.
    pub number: u16,
    /// The value after a reset.
    pub default: bool,
    /// `Some(v)` when the mode is fixed at `v` and cannot be changed.
    pub fixed: Option<bool>,
}

/// Shorthand for a row of [`MODES`].
const fn def(mode: Mode, kind: ModeKind, number: u16, default: bool, fixed: Option<bool>) -> ModeDef {
    ModeDef { mode, kind, number, default, fixed }
}

use ModeKind::{Ansi as A, Dec as D};

/// Every mode, in the order of [`Mode`].
pub const MODES: &[ModeDef] = &[
    def(Mode::KeyboardAction, A, 2, false, None),
    def(Mode::Insert, A, 4, false, None),
    def(Mode::SendReceive, A, 12, true, None),
    def(Mode::LinefeedNewline, A, 20, false, None),
    def(Mode::CursorKeys, D, 1, false, None),
    def(Mode::Ansi, D, 2, true, Some(true)),
    def(Mode::Column132, D, 3, false, None),
    def(Mode::SmoothScroll, D, 4, false, None),
    def(Mode::ReverseVideo, D, 5, false, None),
    def(Mode::Origin, D, 6, false, None),
    def(Mode::Autowrap, D, 7, true, None),
    def(Mode::AutoRepeat, D, 8, true, Some(true)),
    def(Mode::MouseX10, D, 9, false, None),
    def(Mode::CursorBlink, D, 12, false, None),
    def(Mode::CursorVisible, D, 25, true, None),
    def(Mode::Allow132, D, 40, false, None),
    def(Mode::ReverseWrap, D, 45, false, None),
    def(Mode::AltScreenLegacy, D, 47, false, None),
    def(Mode::KeypadApplication, D, 66, false, None),
    def(Mode::BackarrowSendsBs, D, 67, false, None),
    def(Mode::LeftRightMargins, D, 69, false, None),
    def(Mode::SixelDisplay, D, 80, false, None),
    def(Mode::NoClearOnColumnChange, D, 95, false, None),
    def(Mode::MouseNormal, D, 1000, false, None),
    def(Mode::MouseHighlight, D, 1001, false, Some(false)),
    def(Mode::MouseButtonEvent, D, 1002, false, None),
    def(Mode::MouseAnyEvent, D, 1003, false, None),
    def(Mode::FocusEvents, D, 1004, false, None),
    def(Mode::MouseUtf8, D, 1005, false, None),
    def(Mode::MouseSgr, D, 1006, false, None),
    def(Mode::AlternateScroll, D, 1007, false, None),
    def(Mode::MouseUrxvt, D, 1015, false, None),
    def(Mode::MouseSgrPixels, D, 1016, false, None),
    def(Mode::MetaSendsEscape, D, 1036, true, None),
    def(Mode::AltSendsEscape, D, 1039, true, None),
    def(Mode::ReverseWrapExtended, D, 1045, false, None),
    def(Mode::AltScreen, D, 1047, false, None),
    def(Mode::SaveCursor, D, 1048, false, None),
    def(Mode::AltScreenSaveCursor, D, 1049, false, None),
    def(Mode::SixelPrivateColors, D, 1070, true, None),
    def(Mode::BracketedPaste, D, 2004, false, None),
    def(Mode::SynchronizedOutput, D, 2026, false, None),
    def(Mode::GraphemeCluster, D, 2027, true, None),
    def(Mode::ColorSchemeReport, D, 2031, false, None),
    def(Mode::InBandResize, D, 2048, false, None),
    def(Mode::Win32Input, D, 9001, false, Some(false)),
    def(Mode::PrintFormFeed, D, 18, false, None),
    def(Mode::PrintExtent, D, 19, true, None),
    def(Mode::HebrewEncoding, D, 35, false, None),
    def(Mode::NationalCharset, D, 42, false, None),
    def(Mode::HorizontalCursorCoupling, D, 60, false, Some(false)),
    def(Mode::MoreFix, D, 41, false, None),
];

/// ANSI modes the terminal knows by name and never turns on: DECRQM reports them as permanently
/// reset (4). They have no entry in [`MODES`] because nothing reads their value. The numbers are
/// GATM, SRTM, VEM, HEM, PUM, FEAM, FETM, MATM, TTM, SATM, TSM and EBM.
pub const PERMANENTLY_RESET_ANSI: &[u16] = &[1, 5, 7, 10, 11, 13, 14, 15, 16, 17, 18, 19];

/// Finds the table row for a mode number.
pub fn lookup(kind: ModeKind, number: u16) -> Option<&'static ModeDef> {
    MODES.iter().find(|d| d.kind == kind && d.number == number)
}

/// The current value of every mode, plus the values XTSAVE stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeSet {
    bits: u64,
    saved: u64,
    saved_mask: u64,
}

impl Default for ModeSet {
    /// Every mode at its default.
    fn default() -> Self {
        let mut bits = 0;
        for (i, d) in MODES.iter().enumerate() {
            if d.default {
                bits |= 1 << i;
            }
        }
        ModeSet { bits, saved: 0, saved_mask: 0 }
    }
}

impl ModeSet {
    /// The value of a mode.
    #[inline]
    pub fn get(&self, mode: Mode) -> bool {
        self.bits & (1 << mode as u8) != 0
    }

    /// Sets a mode, ignoring fixed modes. Returns false when the mode is fixed.
    pub fn set(&mut self, mode: Mode, on: bool) -> bool {
        if MODES[mode as usize].fixed.is_some() {
            return false;
        }
        if on {
            self.bits |= 1 << mode as u8;
        } else {
            self.bits &= !(1 << mode as u8);
        }
        true
    }

    /// The DECRQM answer for a mode number: 0 not recognised, 1 set, 2 reset, 3 permanently set,
    /// 4 permanently reset.
    pub fn report(&self, kind: ModeKind, number: u16) -> u8 {
        match lookup(kind, number) {
            None if kind == ModeKind::Ansi && PERMANENTLY_RESET_ANSI.contains(&number) => 4,
            None => 0,
            Some(d) => match d.fixed {
                Some(true) => 3,
                Some(false) => 4,
                None if self.get(d.mode) => 1,
                None => 2,
            },
        }
    }

    /// XTSAVE: remembers the current value of a mode.
    pub fn save(&mut self, mode: Mode) {
        let bit = 1u64 << mode as u8;
        self.saved_mask |= bit;
        self.saved = (self.saved & !bit) | (self.bits & bit);
    }

    /// XTRESTORE: returns the remembered value of a mode, if one was saved.
    pub fn saved(&self, mode: Mode) -> Option<bool> {
        let bit = 1u64 << mode as u8;
        (self.saved_mask & bit != 0).then_some(self.saved & bit != 0)
    }

    /// Every mode back to its default (RIS). Saved values are forgotten.
    pub fn reset(&mut self) {
        *self = ModeSet::default();
    }
}
