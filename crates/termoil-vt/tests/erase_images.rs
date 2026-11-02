//! Erasing the screen removes the images on it (task-2472). A program that clears the screen with ED 2 and draws a
//! new page must not find a kitty, sixel or iTerm2 image from the last page still drawn over it. Kitty and Ghostty
//! both drop the placements on the screen for ED 2; erasing part of the screen (ED 0, ED 1) leaves them.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use termoil_vt::{Terminal, TerminalConfig};

/// A PNG of `w` by `h` opaque red pixels.
fn red_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(&[255, 0, 0].repeat((w * h) as usize)).unwrap();
    writer.finish().unwrap();
    out
}

/// A terminal of 80 by 24 cells of 10 by 20 pixels showing one image of each kind: kitty at row 2, sixel at row
/// 8 and iTerm2 at row 14, each two cells high.
fn terminal_with_three_images() -> Terminal {
    let mut t = Terminal::new(80, 24, TerminalConfig::default());
    t.set_cell_pixels(10, 20);
    let png = STANDARD.encode(red_png(20, 40));
    t.advance(format!("\x1b[2;1H\x1b_Ga=T,f=100,q=2,C=1;{png}\x1b\\").as_bytes());
    t.advance(b"\x1b[8;1H\x1bPq#0;2;100;0;0#0!20~-!20~-!20~-!20~-!20~-!20~-!20~\x1b\\");
    t.advance(format!("\x1b[14;1H\x1b]1337;File=inline=1:{png}\x07").as_bytes());
    t
}

/// How many placements the terminal draws.
fn drawn(t: &Terminal) -> usize {
    t.graphics().placements().filter(|p| !p.virtual_placement).count()
}

#[test]
fn ed2_removes_every_image_on_the_screen() {
    let mut t = terminal_with_three_images();
    assert_eq!(drawn(&t), 3, "kitty, sixel and iTerm2 each make a placement");
    t.advance(b"\x1b[2J");
    assert_eq!(drawn(&t), 0, "ED 2 leaves no image on the screen");
    assert_eq!(t.graphics().images().count(), 0, "images sent only for display are freed");
}

#[test]
fn partial_erases_keep_the_images() {
    let mut t = terminal_with_three_images();
    t.advance(b"\x1b[20;1H\x1b[0J\x1b[1;1H\x1b[1J");
    assert_eq!(drawn(&t), 3, "ED 0 and ED 1 leave the images");
}

#[test]
fn an_image_drawn_after_ed2_stays() {
    let mut t = terminal_with_three_images();
    t.advance(b"\x1b[2J");
    let png = STANDARD.encode(red_png(20, 40));
    t.advance(format!("\x1b[3;3H\x1b]1337;File=inline=1:{png}\x07").as_bytes());
    assert_eq!(drawn(&t), 1);
}
