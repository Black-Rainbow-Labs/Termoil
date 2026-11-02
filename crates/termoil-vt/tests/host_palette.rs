//! A host's palette survives the resets a program can ask for (task-2489).
//!
//! OSC 104 and RIS used to put back xterm's palette, so a program running `tput reset` in a themed
//! terminal lost the theme's colours. They put back the palette the host gave with `set_palette`.

use termoil_vt::style::{Rgb, default_palette};
use termoil_vt::{Terminal, TerminalConfig};

/// A palette that differs from xterm's in every entry, so a reset to the wrong one shows.
fn host_palette() -> [Rgb; 256] {
    let mut palette = default_palette();
    for (index, entry) in palette.iter_mut().enumerate() {
        *entry = Rgb::new(index as u8, 1, 2);
    }
    palette
}

/// OSC 104 with no index puts every entry back to the host's palette.
#[test]
fn resetting_every_colour_puts_back_the_host_palette() {
    let mut term = Terminal::new(10, 3, TerminalConfig::default());
    term.set_palette(host_palette());
    term.advance(b"\x1b]4;1;rgb:ff/ff/ff\x1b\\");
    assert_eq!(term.palette()[1], Rgb::new(255, 255, 255));
    term.advance(b"\x1b]104\x1b\\");
    assert_eq!(*term.palette(), host_palette());
}

/// OSC 104 with an index puts back only that entry, from the host's palette.
#[test]
fn resetting_one_colour_puts_back_that_host_colour() {
    let mut term = Terminal::new(10, 3, TerminalConfig::default());
    term.set_palette(host_palette());
    term.advance(b"\x1b]4;1;rgb:ff/ff/ff;2;rgb:ff/ff/ff\x1b\\");
    term.advance(b"\x1b]104;1\x1b\\");
    assert_eq!(term.palette()[1], Rgb::new(1, 1, 2));
    assert_eq!(term.palette()[2], Rgb::new(255, 255, 255));
}

/// RIS puts back the host's palette as well.
#[test]
fn a_full_reset_keeps_the_host_palette() {
    let mut term = Terminal::new(10, 3, TerminalConfig::default());
    term.set_palette(host_palette());
    term.advance(b"\x1b]4;3;rgb:ff/ff/ff\x1b\\");
    term.advance(b"\x1bc");
    assert_eq!(*term.palette(), host_palette());
}

/// With no host palette given, a reset is xterm's palette, as before.
#[test]
fn without_a_host_palette_a_reset_is_the_xterm_palette() {
    let mut term = Terminal::new(10, 3, TerminalConfig::default());
    term.advance(b"\x1b]4;1;rgb:ff/ff/ff\x1b\\");
    term.advance(b"\x1b]104\x1b\\");
    assert_eq!(*term.palette(), default_palette());
}
