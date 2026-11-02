//! U5: the graphics store driven through its public API with hand written payloads.
//!
//! Sixel images from `corpus/vt340test` are decoded and their dimensions and a checksum of the RGBA are
//! compared to `corpus/vt340test/expected.txt`. Expected values change only with `TERMINUS_BLESS=1`, which
//! rewrites that file instead of asserting.

// Test tables are written inline as tuples; naming each one would not help a reader.
#![allow(clippy::type_complexity)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::io::Write;
use terminus_vt::graphics::*;

/// A context with the cursor at serial 100, column 2, a 80 by 24 screen starting at serial 90, cells 10 by 20.
fn ctx() -> GraphicsContext {
    GraphicsContext {
        cursor_serial: 100,
        cursor_col: 2,
        screen_top_serial: 90,
        first_serial: 0,
        cols: 80,
        rows: 24,
        cell_px: (10, 20),
        allow_file_transfer: true,
    }
}

/// A context with the cursor somewhere else.
fn ctx_at(serial: u64, col: u16) -> GraphicsContext {
    GraphicsContext { cursor_serial: serial, cursor_col: col, ..ctx() }
}

/// Base64 of bytes.
fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// `w * h` RGBA pixels of one colour.
fn solid(w: usize, h: usize, px: [u8; 4]) -> Vec<u8> {
    px.iter().copied().cycle().take(w * h * 4).collect()
}

/// A PNG of the given colour type with `w * h` pixels of the data `raw`.
fn png_of(w: u32, h: u32, color: png::ColorType, raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(color);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(raw).unwrap();
    out
}

/// A solid RGBA PNG.
fn png_rgba(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
    png_of(w, h, png::ColorType::Rgba, &solid(w as usize, h as usize, px))
}

/// Zlib compresses bytes.
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// Sends `ctrl;base64(data)` with the default context and returns the result.
fn send(store: &mut GraphicsStore, ctrl: &str, data: &[u8]) -> GraphicsResult {
    send_at(store, ctrl, data, &ctx())
}

/// Sends `ctrl;base64(data)` with a given context.
fn send_at(store: &mut GraphicsStore, ctrl: &str, data: &[u8], c: &GraphicsContext) -> GraphicsResult {
    store.kitty_command(format!("{};{}", ctrl, b64(data)).as_bytes(), c)
}

/// Sends a command with no payload.
fn cmd(store: &mut GraphicsStore, ctrl: &str) -> GraphicsResult {
    store.kitty_command(ctrl.as_bytes(), &ctx())
}

/// The reply text of a result.
fn reply(r: &GraphicsResult) -> String {
    r.reply.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
}

/// The reply text with the APC wrapper removed, for example `i=1;OK`.
fn body(r: &GraphicsResult) -> String {
    reply(r).trim_start_matches("\x1b_G").trim_end_matches("\x1b\\").to_string()
}

/// A new store with a 1 GiB quota.
fn store() -> GraphicsStore {
    GraphicsStore::new(1 << 30)
}

/// A store holding image `id` (2 by 2, RGBA, transmitted with `a=t`).
fn with_image(id: u32) -> GraphicsStore {
    let mut s = store();
    send(&mut s, &format!("a=t,i={},f=32,s=2,v=2", id), &solid(2, 2, [1, 2, 3, 4]));
    s
}

/// Transmits an image of `w` by `h` pixels with id `id`, quietly.
fn add(s: &mut GraphicsStore, id: u32, w: usize, h: usize) {
    send(s, &format!("a=t,i={},f=32,s={},v={},q=2", id, w, h), &solid(w, h, [9, 9, 9, 255]));
}

/// Places image `id` with extra keys at a cursor position.
fn put(s: &mut GraphicsStore, id: u32, extra: &str, serial: u64, col: u16) -> GraphicsResult {
    let c = ctx_at(serial, col);
    s.kitty_command(format!("a=p,i={}{}{}", id, if extra.is_empty() { "" } else { "," }, extra).as_bytes(), &c)
}

/// Id and placement id of every placement, sorted.
fn pairs(s: &GraphicsStore) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = s.placements().map(|p| (p.image_id, p.placement_id)).collect();
    v.sort();
    v
}

/// Ids of all images, sorted.
fn ids(s: &GraphicsStore) -> Vec<u32> {
    let mut v: Vec<u32> = s.images().map(|i| i.id).collect();
    v.sort();
    v
}

// ---------------------------------------------------------------- parser

#[test]
fn parse_table_ok() {
    let cases: &[(&str, &[(u8, i64)], &str)] = &[
        ("a=t,i=5;AAAA", &[(b'a', b't' as i64), (b'i', 5)], "AAAA"),
        ("", &[], ""),
        ("i=1", &[(b'i', 1)], ""),
        (",,i=1,,", &[(b'i', 1)], ""),
        ("z=-5", &[(b'z', -5)], ""),
        ("Y=4294967295", &[(b'Y', 4294967295)], ""),
        ("i=1,i=2", &[(b'i', 2)], ""),
        ("k=zzz,i=3", &[(b'i', 3)], ""),
        ("i=1;a;b", &[(b'i', 1)], "a;b"),
        (";payload", &[], "payload"),
        ("z=-2147483648", &[(b'z', -2147483648)], ""),
    ];
    for (input, keys, payload) in cases {
        let c = parse_kitty_command(input.as_bytes()).unwrap_or_else(|e| panic!("{input}: {e:?}"));
        assert_eq!(&c.keys[..], *keys, "{input}");
        assert_eq!(c.payload, payload.as_bytes(), "{input}");
    }
}

#[test]
fn parse_table_errors() {
    let cases = [
        "i",
        "i=",
        "i=x",
        "ii=1",
        "1=1",
        "a=tt",
        "a=",
        "i=4294967296",
        "z=-2147483649",
        "i=--1",
        "i=123456789012",
        "\x01=1",
    ];
    for input in cases {
        assert!(parse_kitty_command(input.as_bytes()).is_err(), "{input:?} should fail");
    }
    assert_eq!(parse_kitty_command(b"i=x").unwrap_err().code(), "EINVAL");
}

#[test]
fn parse_accessors() {
    let c = parse_kitty_command(b"a=T,i=7,z=-3").unwrap();
    assert_eq!(c.char_or(b'a', b't'), b'T');
    assert_eq!(c.char_or(b'd', b'a'), b'a');
    assert_eq!(c.uint(b'i'), 7);
    assert_eq!(c.int(b'z'), -3);
    assert!(c.has(b'i') && !c.has(b'x'));
}

// ---------------------------------------------------------------- transmission

#[test]
fn transmit_rgba_direct() {
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,f=32,s=2,v=2", &solid(2, 2, [10, 20, 30, 40]));
    assert_eq!(reply(&r), "\x1b_Gi=1;OK\x1b\\");
    let img = s.image(1).unwrap();
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(&img.rgba[..4], &[10, 20, 30, 40]);
    assert_eq!(s.used_bytes(), 16);
    assert_eq!(s.placements().count(), 0);
}

#[test]
fn transmit_rgb_gets_opaque_alpha() {
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,f=24,s=2,v=1", &[1, 2, 3, 4, 5, 6]);
    assert_eq!(body(&r), "i=1;OK");
    assert_eq!(&s.image(1).unwrap().rgba[..], &[1, 2, 3, 255, 4, 5, 6, 255]);
}

#[test]
fn transmit_default_format_is_rgba() {
    let mut s = store();
    send(&mut s, "i=1,s=1,v=1", &[1, 2, 3, 4]);
    assert_eq!(&s.image(1).unwrap().rgba[..], &[1, 2, 3, 4]);
}

#[test]
fn transmit_png_variants() {
    let cases: Vec<(&str, Vec<u8>, [u8; 4])> = vec![
        ("rgba", png_of(1, 1, png::ColorType::Rgba, &[5, 6, 7, 8]), [5, 6, 7, 8]),
        ("rgb", png_of(1, 1, png::ColorType::Rgb, &[5, 6, 7]), [5, 6, 7, 255]),
        ("gray", png_of(1, 1, png::ColorType::Grayscale, &[9]), [9, 9, 9, 255]),
        ("gray alpha", png_of(1, 1, png::ColorType::GrayscaleAlpha, &[9, 3]), [9, 9, 9, 3]),
    ];
    for (name, data, want) in cases {
        let mut s = store();
        let r = send(&mut s, "a=t,i=1,f=100", &data);
        assert_eq!(body(&r), "i=1;OK", "{name}");
        assert_eq!(&s.image(1).unwrap().rgba[..], &want, "{name}");
    }
}

#[test]
fn transmit_png_indexed() {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 2, 1);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(vec![10, 20, 30, 40, 50, 60]);
    enc.write_header().unwrap().write_image_data(&[1, 0]).unwrap();
    let mut s = store();
    send(&mut s, "a=t,i=1,f=100", &out);
    assert_eq!(&s.image(1).unwrap().rgba[..], &[40, 50, 60, 255, 10, 20, 30, 255]);
}

#[test]
fn transmit_png_16_bit() {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 1, 1);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Sixteen);
    enc.write_header().unwrap().write_image_data(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]).unwrap();
    let mut s = store();
    send(&mut s, "a=t,i=1,f=100", &out);
    assert_eq!(&s.image(1).unwrap().rgba[..], &[0x12, 0x56, 0x9a, 255]);
}

#[test]
fn transmit_zlib_variants() {
    let rgba = solid(4, 4, [1, 2, 3, 4]);
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let png = png_rgba(4, 4, [1, 2, 3, 4]);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("a=t,i=1,f=32,s=4,v=4,o=z", zlib(&rgba)),
        ("a=t,i=1,f=24,s=4,v=4,o=z", zlib(&rgb)),
        ("a=t,i=1,f=100,o=z", zlib(&png)),
    ];
    for (ctrl, data) in cases {
        let mut s = store();
        let r = send(&mut s, ctrl, &data);
        assert_eq!(body(&r), "i=1;OK", "{ctrl}");
        assert_eq!(s.image(1).unwrap().rgba[..4], [1, 2, 3, if ctrl.contains("f=24") { 255 } else { 4 }], "{ctrl}");
    }
}

#[test]
fn transmit_zlib_garbage_and_bomb() {
    let mut s = store();
    assert_eq!(body(&send(&mut s, "a=t,i=1,f=32,s=2,v=2,o=z", b"not zlib")), "i=1;EINVAL:Failed to inflate zlib data");
    let bomb = zlib(&vec![0u8; 1 << 20]);
    let r = send(&mut s, "a=t,i=2,f=32,s=2,v=2,o=z", &bomb);
    assert!(body(&r).starts_with("i=2;EFBIG"), "{}", body(&r));
    assert!(s.image(2).is_none());
}

#[test]
fn transmit_unknown_compression_and_medium_and_format() {
    let mut s = store();
    for (ctrl, code) in [
        ("a=t,i=1,f=32,s=1,v=1,o=x", "EINVAL"),
        ("a=t,i=1,f=32,s=1,v=1,t=x", "EINVAL"),
        ("a=t,i=1,f=7,s=1,v=1", "EINVAL"),
    ] {
        let r = send(&mut s, ctrl, &[0; 4]);
        assert!(body(&r).contains(code), "{ctrl}: {}", body(&r));
    }
    assert_eq!(s.images().count(), 0);
}

#[test]
fn transmit_errors_table() {
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("a=t,i=1,f=32,s=2,v=2", vec![0; 15], "ENODATA"),
        ("a=t,i=1,f=32,s=2,v=2", vec![0; 17], "EINVAL"),
        ("a=t,i=1,f=24,s=2,v=2", vec![0; 11], "ENODATA"),
        ("a=t,i=1,f=32", vec![0; 4], "EINVAL"),
        ("a=t,i=1,f=32,s=0,v=1", vec![0; 4], "EINVAL"),
        ("a=t,i=1,I=2,f=32,s=1,v=1", vec![0; 4], "EINVAL"),
        ("a=t,i=1,f=100", vec![1, 2, 3], "EINVAL"),
        ("a=t,i=1,f=100", png_rgba(2, 2, [0; 4])[..30].to_vec(), "EINVAL"),
    ];
    for (ctrl, data, code) in cases {
        let mut s = store();
        let before = s.dropped();
        let r = send(&mut s, ctrl, &data);
        assert!(body(&r).contains(&format!(";{}", code)), "{ctrl}: {}", body(&r));
        assert_eq!(s.images().count(), 0, "{ctrl}");
        assert_eq!(s.dropped(), before + 1, "{ctrl}");
    }
}

#[test]
fn transmit_bad_base64() {
    let mut s = store();
    let r = s.kitty_command(b"a=t,i=1,f=32,s=1,v=1;!!!!", &ctx());
    assert!(body(&r).contains("EINVAL"));
}

#[test]
fn transmit_base64_without_padding_and_with_newlines() {
    let mut s = store();
    let text = b64(&[1, 2, 3, 4]);
    assert!(text.ends_with("=="));
    let unpadded = text.trim_end_matches('=');
    s.kitty_command(format!("a=t,i=1,f=32,s=1,v=1;{}", unpadded).as_bytes(), &ctx());
    assert!(s.image(1).is_some());
    let spaced = format!("{}\n{}", &text[..4], &text[4..]);
    s.kitty_command(format!("a=t,i=2,f=32,s=1,v=1;{}", spaced).as_bytes(), &ctx());
    assert!(s.image(2).is_some());
}

#[test]
fn transmit_too_many_pixels_is_efbig() {
    let mut s = store();
    s.set_max_pixels(100);
    let r = send(&mut s, "a=t,i=1,f=32,s=11,v=10", &[]);
    assert!(body(&r).contains("EFBIG"), "{}", body(&r));
    let r = send(&mut s, "a=t,i=2,f=100", &png_rgba(11, 10, [0; 4]));
    assert!(body(&r).contains("EFBIG"), "{}", body(&r));
    let r = send(&mut s, "a=t,i=3,f=32,s=10,v=10", &solid(10, 10, [0; 4]));
    assert_eq!(body(&r), "i=3;OK");
    assert_eq!(s.dropped(), 2);
}

#[test]
fn transmit_default_cap_is_10000_square() {
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,f=32,s=10001,v=10001", &[]);
    assert!(body(&r).contains("EFBIG"));
}

#[test]
fn transmit_replaces_image_and_deletes_placements() {
    let mut s = with_image(1);
    put(&mut s, 1, "", 100, 0);
    assert_eq!(s.placements().count(), 1);
    send(&mut s, "a=t,i=1,f=32,s=1,v=1", &[1, 1, 1, 1]);
    assert_eq!(s.placements().count(), 0);
    assert_eq!(s.image(1).unwrap().width, 1);
    assert_eq!(s.used_bytes(), 4);
}

#[test]
fn image_number_gets_assigned_id() {
    let mut s = store();
    let r = send(&mut s, "a=t,I=13,f=32,s=1,v=1", &[0; 4]);
    let text = body(&r);
    assert!(text.starts_with("i=") && text.ends_with(",I=13;OK"), "{text}");
    let id: u32 = text[2..text.find(',').unwrap()].parse().unwrap();
    assert_eq!(s.image(id).unwrap().number, 13);
    let r2 = send(&mut s, "a=t,I=13,f=32,s=1,v=1", &[0; 4]);
    let id2: u32 = body(&r2)[2..body(&r2).find(',').unwrap()].parse().unwrap();
    assert_ne!(id, id2);
    assert_eq!(s.images().count(), 2);
}

#[test]
fn put_by_number_targets_newest() {
    let mut s = store();
    send(&mut s, "a=t,I=5,f=32,s=1,v=1,q=2", &[1; 4]);
    send(&mut s, "a=t,I=5,f=32,s=2,v=2,q=2", &solid(2, 2, [1; 4]));
    let newest = s.images().last().unwrap().id;
    let r = cmd(&mut s, "a=p,I=5");
    assert!(body(&r).contains(&format!("i={}", newest)) && body(&r).contains("I=5;OK"), "{}", body(&r));
    assert_eq!(s.placements().next().unwrap().image_id, newest);
}

#[test]
fn no_id_no_reply() {
    let mut s = store();
    let r = send(&mut s, "a=t,f=32,s=1,v=1", &[0; 4]);
    assert!(r.reply.is_none());
    assert_eq!(s.images().count(), 1);
    assert!(s.images().next().unwrap().anonymous);
    let r = send(&mut s, "a=T,f=32,s=1,v=1", &[0; 4]);
    assert!(r.reply.is_none());
    assert_eq!(s.placements().count(), 1);
    let r = send(&mut s, "a=t,f=32,s=1,v=1", &[0; 3]);
    assert!(r.reply.is_none());
}

#[test]
fn anonymous_images_are_separate_and_ignore_placement_ids() {
    let mut s = store();
    send(&mut s, "a=T,p=9,f=32,s=1,v=1", &[0; 4]);
    send(&mut s, "a=T,p=9,f=32,s=1,v=1", &[0; 4]);
    assert_eq!(s.images().count(), 2);
    assert_eq!(s.placements().count(), 2);
    assert!(s.placements().all(|p| p.placement_id == 0));
}

#[test]
fn query_does_not_store() {
    let mut s = store();
    let r = send(&mut s, "a=q,i=31,f=24,s=1,v=1", &[1, 2, 3]);
    assert_eq!(reply(&r), "\x1b_Gi=31;OK\x1b\\");
    assert_eq!(s.images().count(), 0);
    let r = send(&mut s, "a=q,i=31,f=24,s=1,v=1", &[1, 2]);
    assert!(body(&r).starts_with("i=31;ENODATA"));
}

#[test]
fn query_does_not_replace_existing_image() {
    let mut s = with_image(1);
    send(&mut s, "a=q,i=1,f=32,s=1,v=1", &[0; 4]);
    assert_eq!(s.image(1).unwrap().width, 2);
}

#[test]
fn query_without_id_still_replies() {
    let mut s = store();
    let r = send(&mut s, "a=q,f=24,s=1,v=1", &[1, 2, 3]);
    assert_eq!(reply(&r), "\x1b_G;OK\x1b\\");
}

#[test]
fn quiet_modes() {
    let mut s = store();
    assert!(send(&mut s, "a=t,i=1,f=32,s=1,v=1,q=1", &[0; 4]).reply.is_none());
    assert!(send(&mut s, "a=t,i=1,f=32,s=1,v=1,q=2", &[0; 4]).reply.is_some());
    assert!(send(&mut s, "a=t,i=1,f=32,s=1,v=1,q=1", &[0; 3]).reply.is_some());
    assert!(send(&mut s, "a=t,i=1,f=32,s=1,v=1,q=2", &[0; 3]).reply.is_none());
    assert!(cmd(&mut s, "a=p,i=77,q=2").reply.is_none());
    assert!(cmd(&mut s, "a=p,i=77").reply.is_some());
    assert!(cmd(&mut s, "a=p,i=1,q=1").reply.is_none());
}

#[test]
fn unknown_action_and_malformed() {
    let mut s = store();
    assert!(body(&cmd(&mut s, "a=z,i=1")).contains("EINVAL"));
    let before = s.dropped();
    assert!(s.kitty_command(b"i=zz", &ctx()).reply.is_none());
    assert_eq!(s.dropped(), before + 1);
    assert!(body(&cmd(&mut s, "a=p,i=1,x=-4")).contains("EINVAL"));
}

// ---------------------------------------------------------------- media

/// Writes a file under the system temp directory and returns its path.
fn temp_file(name: &str, data: &[u8]) -> String {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, data).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn medium_file() {
    let path = temp_file("terminus-test-file-medium.rgba", &solid(2, 2, [7, 8, 9, 10]));
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,t=f,f=32,s=2,v=2", path.as_bytes());
    assert_eq!(body(&r), "i=1;OK");
    assert_eq!(&s.image(1).unwrap().rgba[..4], &[7, 8, 9, 10]);
    assert!(std::path::Path::new(&path).exists(), "t=f must not delete the file");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn medium_file_size_and_offset() {
    let mut data = vec![0u8; 5];
    data.extend(solid(1, 2, [3, 4, 5, 6]));
    data.extend([9u8; 7]);
    let path = temp_file("terminus-test-file-offset.bin", &data);
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,t=f,f=32,s=1,v=2,S=8,O=5", path.as_bytes());
    assert_eq!(body(&r), "i=1;OK");
    assert_eq!(&s.image(1).unwrap().rgba[..], &solid(1, 2, [3, 4, 5, 6])[..]);
    let r = send(&mut s, "a=t,i=2,t=f,f=32,s=1,v=2,S=8,O=1000", path.as_bytes());
    assert!(body(&r).starts_with("i=2;EBADF"));
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn medium_file_png() {
    let path = temp_file("terminus-test-file.png", &png_rgba(3, 2, [1, 1, 1, 1]));
    let mut s = store();
    send(&mut s, "a=t,i=1,t=f,f=100", path.as_bytes());
    assert_eq!((s.image(1).unwrap().width, s.image(1).unwrap().height), (3, 2));
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn medium_temp_file_deleted_when_named_for_protocol() {
    let path = temp_file("tty-graphics-protocol-terminus-test.rgba", &solid(1, 1, [1, 2, 3, 4]));
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,t=t,f=32,s=1,v=1", path.as_bytes());
    assert_eq!(body(&r), "i=1;OK");
    assert!(!std::path::Path::new(&path).exists());
}

#[test]
fn medium_temp_file_kept_when_not_named_for_protocol() {
    let path = temp_file("terminus-test-plain-temp.rgba", &solid(1, 1, [1, 2, 3, 4]));
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,t=t,f=32,s=1,v=1", path.as_bytes());
    assert_eq!(body(&r), "i=1;OK");
    assert!(std::path::Path::new(&path).exists());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn medium_temp_file_outside_temp_dir_kept() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test-scratch");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tty-graphics-protocol-outside.rgba");
    std::fs::write(&path, solid(1, 1, [1, 2, 3, 4])).unwrap();
    let mut s = store();
    let r = send(&mut s, "a=t,i=1,t=t,f=32,s=1,v=1", path.to_string_lossy().as_bytes());
    assert_eq!(body(&r), "i=1;OK");
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn medium_file_errors_are_identical() {
    let dir = std::env::temp_dir();
    let short = temp_file("terminus-test-short.bin", &[1, 2]);
    let mut s = store();
    let cases =
        ["/definitely/not/here.png".to_string(), dir.to_string_lossy().into_owned(), short.clone(), String::new()];
    let mut replies = Vec::new();
    for (n, p) in cases.iter().enumerate() {
        let r = send(&mut s, &format!("a=t,i={},t=f,f=32,s=2,v=2,S=16", n + 1), p.as_bytes());
        replies.push(body(&r).split_once(';').unwrap().1.to_string());
    }
    assert!(replies.iter().all(|r| r == "EBADF:Failed to read image file"), "{replies:?}");
    std::fs::remove_file(short).unwrap();
}

#[cfg(unix)]
#[test]
fn medium_sensitive_paths_refused() {
    let mut s = store();
    for p in ["/proc/self/environ", "/sys/kernel", "/dev/zero"] {
        let r = send(&mut s, "a=t,i=1,t=f,f=32,s=1,v=1", p.as_bytes());
        assert!(body(&r).contains("EBADF"), "{p}");
    }
}

#[test]
fn medium_disallowed_is_eperm() {
    let path = temp_file("terminus-test-disallowed.rgba", &solid(1, 1, [1, 2, 3, 4]));
    let c = GraphicsContext { allow_file_transfer: false, ..ctx() };
    let mut s = store();
    for t in ["f", "t", "s"] {
        let r = send_at(&mut s, &format!("a=t,i=1,t={},f=32,s=1,v=1", t), path.as_bytes(), &c);
        assert!(body(&r).contains("EPERM"), "t={t}: {}", body(&r));
    }
    assert_eq!(s.images().count(), 0);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn medium_shared_memory() {
    let mut s = store();
    if cfg!(unix) && std::path::Path::new("/dev/shm").is_dir() {
        let name = "terminus-test-shm";
        std::fs::write(format!("/dev/shm/{}", name), solid(1, 1, [4, 3, 2, 1])).unwrap();
        let r = send(&mut s, "a=t,i=1,t=s,f=32,s=1,v=1", format!("/{}", name).as_bytes());
        assert_eq!(body(&r), "i=1;OK");
        assert!(!std::path::Path::new(&format!("/dev/shm/{}", name)).exists());
        let r = send(&mut s, "a=t,i=2,t=s,f=32,s=1,v=1", b"/a/b");
        assert!(body(&r).contains("EBADF"));
        let r = send(&mut s, "a=t,i=3,t=s,f=32,s=1,v=1", b"noslash");
        assert!(body(&r).contains("EBADF"));
    } else {
        let r = send(&mut s, "a=t,i=1,t=s,f=32,s=1,v=1", b"/name");
        assert!(body(&r).contains("EINVAL") || body(&r).contains("EBADF"), "{}", body(&r));
    }
}

// ---------------------------------------------------------------- chunking

/// Sends data as base64 chunks of `size` base64 characters. The first chunk carries `first_keys`.
fn send_chunks(s: &mut GraphicsStore, first_keys: &str, data: &[u8], size: usize) -> Vec<GraphicsResult> {
    let text = b64(data);
    let pieces: Vec<&str> = text.as_bytes().chunks(size).map(|c| std::str::from_utf8(c).unwrap()).collect();
    let mut out = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        let more = if i + 1 < pieces.len() { 1 } else { 0 };
        let keys = if i == 0 { format!("{},m={}", first_keys, more) } else { format!("m={}", more) };
        out.push(s.kitty_command(format!("{};{}", keys, piece).as_bytes(), &ctx()));
    }
    out
}

#[test]
fn chunked_upload() {
    let mut s = store();
    let rs = send_chunks(&mut s, "a=t,i=1,f=32,s=4,v=4", &solid(4, 4, [1, 2, 3, 4]), 8);
    assert!(rs.len() > 3);
    assert!(rs[..rs.len() - 1].iter().all(|r| r.reply.is_none()));
    assert_eq!(body(rs.last().unwrap()), "i=1;OK");
    assert_eq!(s.image(1).unwrap().rgba.len(), 64);
}

#[test]
fn chunked_upload_and_display_moves_cursor_once() {
    let mut s = store();
    let rs = send_chunks(&mut s, "a=T,i=1,f=32,s=10,v=20", &solid(10, 20, [1, 2, 3, 4]), 64);
    assert!(rs[..rs.len() - 1].iter().all(|r| r.move_cursor.is_none()));
    assert_eq!(rs.last().unwrap().move_cursor, Some((1, 1)));
    assert_eq!(s.placements().count(), 1);
}

#[test]
fn chunked_png() {
    let mut s = store();
    send_chunks(&mut s, "a=t,i=1,f=100", &png_rgba(8, 8, [1, 2, 3, 4]), 40);
    assert_eq!(s.image(1).unwrap().width, 8);
}

#[test]
fn chunked_interrupted_by_other_image_discards_first() {
    let mut s = store();
    let text = b64(&solid(4, 4, [1; 4]));
    s.kitty_command(format!("a=t,i=1,f=32,s=4,v=4,m=1;{}", &text[..40]).as_bytes(), &ctx());
    let r = send(&mut s, "a=t,i=2,f=32,s=1,v=1", &[5; 4]);
    assert_eq!(body(&r), "i=2;OK");
    let r = s.kitty_command(format!("m=0;{}", &text[40..]).as_bytes(), &ctx());
    assert!(s.image(1).is_none());
    assert_eq!(s.dropped(), 2);
    assert!(body(&r).is_empty() || body(&r).contains("EINVAL") || body(&r).contains("ENODATA") || r.reply.is_none());
}

#[test]
fn chunked_interrupted_by_delete_and_put() {
    for interrupt in ["a=d", "a=p,i=9", "a=q,i=3,f=32,s=1,v=1"] {
        let mut s = store();
        add(&mut s, 9, 1, 1);
        let text = b64(&solid(4, 4, [1; 4]));
        s.kitty_command(format!("a=t,i=1,f=32,s=4,v=4,m=1;{}", &text[..40]).as_bytes(), &ctx());
        cmd(&mut s, interrupt);
        s.kitty_command(format!("m=0;{}", &text[40..]).as_bytes(), &ctx());
        assert!(s.image(1).is_none(), "{interrupt}");
    }
}

#[test]
fn chunked_continuation_may_repeat_image_id_and_q() {
    let mut s = store();
    let text = b64(&solid(2, 2, [1; 4]));
    s.kitty_command(format!("a=t,i=1,f=32,s=2,v=2,m=1;{}", &text[..8]).as_bytes(), &ctx());
    let r = s.kitty_command(format!("i=1,m=0,q=1;{}", &text[8..]).as_bytes(), &ctx());
    assert!(r.reply.is_none(), "q=1 on the last chunk suppresses OK");
    assert!(s.image(1).is_some());
}

#[test]
fn chunked_oversize_is_dropped_and_counted() {
    let mut s = store();
    let chunk = "A".repeat(3 * 1024 * 1024);
    let first = format!("a=t,i=1,f=32,s=1,v=1,m=1;{}", chunk);
    assert!(s.kitty_command(first.as_bytes(), &ctx()).reply.is_none());
    let mid = format!("m=1;{}", chunk);
    for _ in 0..29 {
        assert!(s.kitty_command(mid.as_bytes(), &ctx()).reply.is_none());
    }
    let r = s.kitty_command(format!("m=0;{}", chunk).as_bytes(), &ctx());
    assert!(body(&r).contains("EFBIG"), "{}", body(&r));
    assert_eq!(s.dropped(), 1);
    assert!(s.image(1).is_none());
    assert!(send(&mut s, "a=t,i=2,f=32,s=1,v=1", &[0; 4]).reply.is_some());
}

#[test]
fn chunked_frames_continue() {
    let mut s = with_image(1);
    let rs = send_chunks(&mut s, "a=f,i=1,f=32,s=2,v=2", &solid(2, 2, [8; 4]), 8);
    assert_eq!(body(rs.last().unwrap()), "i=1;OK");
    assert_eq!(s.image(1).unwrap().frames.len(), 2);
}

#[test]
fn clear_abandons_upload() {
    let mut s = store();
    s.kitty_command(b"a=t,i=1,f=32,s=1,v=1,m=1;AAAA", &ctx());
    s.clear();
    let r = s.kitty_command(b"m=0;AAAA", &ctx());
    assert!(s.image(1).is_none());
    let _ = r;
}

// ---------------------------------------------------------------- put

#[test]
fn put_defaults_whole_image_at_cursor() {
    let mut s = store();
    add(&mut s, 1, 25, 45);
    let r = put(&mut s, 1, "", 100, 4);
    assert_eq!(body(&r), "i=1;OK");
    let p = s.placements().next().unwrap();
    assert_eq!((p.serial, p.col, p.src, p.cols, p.rows), (100, 4, (0, 0, 25, 45), 3, 3));
    assert_eq!(r.move_cursor, Some((3, 3)));
    assert!(r.dirty);
}

#[test]
fn put_source_rectangle_table() {
    let cases: &[(&str, (u32, u32, u32, u32))] = &[
        ("x=5", (5, 0, 15, 40)),
        ("y=10", (0, 10, 20, 30)),
        ("w=7,h=9", (0, 0, 7, 9)),
        ("x=15,w=100", (15, 0, 5, 40)),
        ("x=2,y=3,w=4,h=5", (2, 3, 4, 5)),
        ("h=1000", (0, 0, 20, 40)),
    ];
    for (keys, want) in cases {
        let mut s = store();
        add(&mut s, 1, 20, 40);
        put(&mut s, 1, keys, 100, 0);
        assert_eq!(s.placements().next().unwrap().src, *want, "{keys}");
    }
    let mut s = store();
    add(&mut s, 1, 20, 40);
    assert!(body(&put(&mut s, 1, "x=20", 100, 0)).contains("EINVAL"));
    assert!(body(&put(&mut s, 1, "y=40", 100, 0)).contains("EINVAL"));
}

#[test]
fn put_cell_layout_table() {
    // image 20 by 40 pixels, cells 10 by 20
    let cases: &[(&str, (u16, u16))] = &[
        ("", (2, 2)),
        ("X=3", (3, 2)),
        ("Y=3", (2, 3)),
        ("c=10", (10, 10)),
        ("r=4", (4, 4)),
        ("c=5,r=7", (5, 7)),
        ("w=10,h=20", (1, 1)),
        ("c=3", (3, 3)),
    ];
    for (keys, want) in cases {
        let mut s = store();
        add(&mut s, 1, 20, 40);
        put(&mut s, 1, keys, 100, 0);
        let p = s.placements().next().unwrap();
        assert_eq!((p.cols, p.rows), *want, "{keys}");
    }
}

#[test]
fn put_offsets_are_stored_and_clamped() {
    let mut s = store();
    add(&mut s, 1, 20, 40);
    put(&mut s, 1, "X=3,Y=4", 100, 0);
    put(&mut s, 1, "X=300,Y=400", 100, 0);
    let v: Vec<_> = s.placements().map(|p| (p.x_offset, p.y_offset)).collect();
    assert_eq!(v, vec![(3, 4), (9, 19)]);
}

#[test]
fn put_cursor_policy() {
    let mut s = store();
    add(&mut s, 1, 20, 40);
    assert_eq!(put(&mut s, 1, "", 100, 0).move_cursor, Some((2, 2)));
    assert_eq!(put(&mut s, 1, "C=1", 100, 0).move_cursor, None);
    assert_eq!(put(&mut s, 1, "C=0,c=4,r=6", 100, 0).move_cursor, Some((4, 6)));
}

#[test]
fn put_missing_image_is_enoent() {
    let mut s = store();
    assert!(body(&cmd(&mut s, "a=p,i=5")).starts_with("i=5;ENOENT"));
    assert!(body(&cmd(&mut s, "a=p,I=5")).contains("ENOENT"));
    assert!(body(&cmd(&mut s, "a=p,i=5,p=3")).starts_with("i=5,p=3;ENOENT"));
    assert!(cmd(&mut s, "a=p").reply.is_none());
}

#[test]
fn put_with_i_and_big_i_is_einval() {
    let mut s = with_image(1);
    assert!(body(&cmd(&mut s, "a=p,i=1,I=2")).contains("EINVAL"));
}

#[test]
fn put_reply_carries_placement_id() {
    let mut s = with_image(1);
    assert_eq!(body(&cmd(&mut s, "a=p,i=1,p=7")), "i=1,p=7;OK");
}

#[test]
fn put_same_placement_id_replaces() {
    let mut s = with_image(1);
    put(&mut s, 1, "p=4", 100, 0);
    put(&mut s, 1, "p=4", 105, 6);
    assert_eq!(s.placements().count(), 1);
    let p = s.placements().next().unwrap();
    assert_eq!((p.serial, p.col), (105, 6));
}

#[test]
fn put_without_placement_id_adds() {
    let mut s = with_image(1);
    put(&mut s, 1, "", 100, 0);
    put(&mut s, 1, "p=0", 100, 0);
    put(&mut s, 1, "", 100, 0);
    assert_eq!(s.placements().count(), 3);
}

#[test]
fn transmit_and_display_places_at_cursor() {
    let mut s = store();
    let r = send_at(&mut s, "a=T,i=1,f=32,s=10,v=20,c=2,r=2,z=-1", &solid(10, 20, [1; 4]), &ctx_at(7, 3));
    assert_eq!(r.move_cursor, Some((2, 2)));
    let p = s.placements().next().unwrap();
    assert_eq!((p.serial, p.col, p.z), (7, 3, -1));
}

#[test]
fn transmit_and_display_errors_after_store() {
    let mut s = store();
    let r = send(&mut s, "a=T,i=1,f=32,s=1,v=1,x=5", &[0; 4]);
    assert!(body(&r).contains("EINVAL"));
    assert!(s.image(1).is_some());
}

#[test]
fn z_index_ordering() {
    let mut s = store();
    add(&mut s, 5, 1, 1);
    add(&mut s, 2, 1, 1);
    add(&mut s, 3, 1, 1);
    put(&mut s, 5, "z=0", 100, 0);
    put(&mut s, 2, "z=0", 100, 0);
    put(&mut s, 3, "z=-1", 100, 0);
    put(&mut s, 3, "z=10,p=1", 100, 0);
    put(&mut s, 3, "z=-1073741825,p=2", 100, 0);
    let order: Vec<(i32, u32)> = s.placements_by_z().iter().map(|p| (p.z, p.image_id)).collect();
    assert_eq!(order, vec![(-1073741825, 3), (-1, 3), (0, 2), (0, 5), (10, 3)]);
}

#[test]
fn virtual_placement() {
    let mut s = store();
    add(&mut s, 1, 20, 40);
    let r = put(&mut s, 1, "U=1,c=4,r=3,p=2", 100, 0);
    assert_eq!(body(&r), "i=1,p=2;OK");
    assert_eq!(r.move_cursor, None);
    assert!(!r.dirty);
    let p = s.virtual_placement(1, 2).unwrap();
    assert!(p.virtual_placement);
    assert_eq!((p.cols, p.rows), (4, 3));
    assert!(s.virtual_placement(1, 0).is_some());
    assert!(s.virtual_placement(1, 3).is_none());
    assert_eq!(s.placements_in_rows(0, 1000).len(), 0);
    assert_eq!(s.placements_by_z().len(), 0);
    assert_eq!(s.placements().count(), 1);
}

#[test]
fn transmit_and_display_virtual() {
    let mut s = store();
    let r = send(&mut s, "a=T,U=1,i=3,c=2,r=2,f=32,s=1,v=1", &[0; 4]);
    assert_eq!(r.move_cursor, None);
    assert!(s.virtual_placement(3, 0).is_some());
}

// ---------------------------------------------------------------- relative placements

/// Image 1 placed at (100, 5) with placement id 1, and images 2..=11 transmitted.
fn relative_setup() -> GraphicsStore {
    let mut s = store();
    for id in 1..=12 {
        add(&mut s, id, 10, 20);
    }
    put(&mut s, 1, "p=1", 100, 5);
    s
}

#[test]
fn relative_position_and_cursor() {
    let mut s = relative_setup();
    let r = put(&mut s, 2, "p=1,P=1,Q=1,H=3,V=2", 50, 50);
    assert_eq!(body(&r), "i=2,p=1;OK");
    assert_eq!(r.move_cursor, None);
    let p = s.placements().find(|p| p.image_id == 2).unwrap();
    assert_eq!((p.serial, p.col, p.parent, p.offset_from_parent), (102, 8, Some((1, 1)), (3, 2)));
}

#[test]
fn relative_negative_offsets_and_clamping() {
    let mut s = relative_setup();
    put(&mut s, 2, "p=1,P=1,Q=1,H=-2,V=-1", 0, 0);
    let p = s.placements().find(|p| p.image_id == 2).unwrap();
    assert_eq!((p.serial, p.col), (99, 3));
    put(&mut s, 3, "p=1,P=1,Q=1,H=-50", 0, 0);
    assert_eq!(s.placements().find(|p| p.image_id == 3).unwrap().col, 0);
}

#[test]
fn relative_follows_parent_when_replaced() {
    let mut s = relative_setup();
    put(&mut s, 2, "p=1,P=1,Q=1,H=1,V=1", 0, 0);
    put(&mut s, 1, "p=1", 200, 9);
    let p = s.placements().find(|p| p.image_id == 2).unwrap();
    assert_eq!((p.serial, p.col), (201, 10));
}

#[test]
fn relative_chain_follows() {
    let mut s = relative_setup();
    put(&mut s, 2, "p=1,P=1,Q=1,H=1", 0, 0);
    put(&mut s, 3, "p=1,P=2,Q=1,H=1", 0, 0);
    let p = s.placements().find(|p| p.image_id == 3).unwrap();
    assert_eq!((p.serial, p.col), (100, 7));
}

#[test]
fn relative_errors() {
    let mut s = relative_setup();
    assert!(body(&put(&mut s, 2, "P=1,Q=99", 0, 0)).contains("ENOPARENT"));
    assert!(body(&put(&mut s, 2, "P=77,Q=1", 0, 0)).contains("ENOPARENT"));
    assert!(body(&put(&mut s, 2, "U=1,P=1,Q=1,c=1,r=1", 0, 0)).contains("EINVAL"));
    assert_eq!(s.placements().count(), 1);
}

#[test]
fn relative_cycle_is_rejected() {
    let mut s = relative_setup();
    put(&mut s, 2, "p=1,P=1,Q=1", 0, 0);
    put(&mut s, 3, "p=1,P=2,Q=1", 0, 0);
    let r = put(&mut s, 1, "p=1,P=3,Q=1", 0, 0);
    assert!(body(&r).contains("ECYCLE"), "{}", body(&r));
    assert!(s.placements().find(|p| p.image_id == 1).unwrap().parent.is_none());
    let r = put(&mut s, 3, "p=1,P=3,Q=1", 0, 0);
    assert!(body(&r).contains("ECYCLE"), "{}", body(&r));
    assert_eq!(s.placements().count(), 3);
}

#[test]
fn relative_chain_depth_limit() {
    let mut s = relative_setup();
    for id in 2..=9 {
        let r = put(&mut s, id, &format!("p=1,P={},Q=1", id - 1), 0, 0);
        assert_eq!(body(&r), format!("i={},p=1;OK", id));
    }
    let r = put(&mut s, 10, "p=1,P=9,Q=1", 0, 0);
    assert!(body(&r).contains("ETOODEEP"), "{}", body(&r));
}

#[test]
fn relative_deleted_with_parent_and_image_freed() {
    let mut s = relative_setup();
    put(&mut s, 2, "p=1,P=1,Q=1", 0, 0);
    put(&mut s, 3, "p=1,P=2,Q=1", 0, 0);
    cmd(&mut s, "a=d,d=i,i=1,p=1");
    assert_eq!(s.placements().count(), 0);
    assert!(s.image(1).is_some(), "the parent's own image data stays after a lowercase delete");
    assert!(s.image(2).is_none() && s.image(3).is_none(), "images left with no placements are freed");
}

#[test]
fn relative_parent_may_be_virtual() {
    let mut s = relative_setup();
    put(&mut s, 2, "U=1,p=3,c=1,r=1", 100, 0);
    let r = put(&mut s, 3, "p=1,P=2,Q=3,H=1", 0, 0);
    assert_eq!(body(&r), "i=3,p=1;OK");
}

// ---------------------------------------------------------------- delete

/// Image 1 at (100, 2) 2 cols by 2 rows, image 2 at (100, 10) z=5, image 3 at (130, 0) off screen,
/// image 1 again at (101, 20) with placement id 8, plus a virtual placement of image 4.
fn delete_setup() -> GraphicsStore {
    let mut s = store();
    for id in 1..=4 {
        add(&mut s, id, 20, 40);
    }
    put(&mut s, 1, "p=7", 100, 2);
    put(&mut s, 2, "z=5", 100, 10);
    put(&mut s, 3, "", 130, 0);
    put(&mut s, 1, "p=8", 101, 20);
    put(&mut s, 4, "U=1,c=2,r=2,p=1", 100, 0);
    s
}

/// Runs a delete command and returns the placements left as (image, placement) pairs.
fn after(ctrl: &str) -> (Vec<(u32, u32)>, Vec<u32>) {
    let mut s = delete_setup();
    cmd(&mut s, ctrl);
    (pairs(&s), ids(&s))
}

#[test]
fn delete_visible_lower_and_upper() {
    assert_eq!(after("a=d").0, vec![(3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=a").0, vec![(3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=a").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=A").0, vec![(3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=A").1, vec![3, 4]);
}

#[test]
fn delete_by_id() {
    assert_eq!(after("a=d,d=i,i=1").0, vec![(2, 0), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=i,i=1").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=I,i=1").1, vec![2, 3, 4]);
    assert_eq!(after("a=d,d=i,i=1,p=8").0, vec![(1, 7), (2, 0), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=I,i=1,p=8").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=I,i=1,p=7").0.len(), 4);
    assert_eq!(after("a=d,d=i,i=4").0, vec![(1, 7), (1, 8), (2, 0), (3, 0)]);
    assert_eq!(after("a=d,d=i,i=99").0.len(), 5);
    assert_eq!(after("a=d,d=I,i=3").1, vec![1, 2, 4]);
}

#[test]
fn delete_by_number() {
    let mut s = store();
    send(&mut s, "a=T,I=4,f=32,s=1,v=1,q=2", &[0; 4]);
    send(&mut s, "a=T,I=4,f=32,s=1,v=1,q=2", &[0; 4]);
    let newest = s.images().last().unwrap().id;
    cmd(&mut s, "a=d,d=n,I=4");
    assert_eq!(s.placements().count(), 1);
    assert!(s.placements().all(|p| p.image_id != newest));
    assert_eq!(s.images().count(), 2);
    cmd(&mut s, "a=d,d=N,I=4");
    assert_eq!(s.images().count(), 1);
}

#[test]
fn delete_at_cursor() {
    let mut s = delete_setup();
    s.kitty_command(b"a=d,d=c", &ctx_at(101, 3));
    assert_eq!(pairs(&s), vec![(1, 8), (2, 0), (3, 0), (4, 1)]);
    let mut s = delete_setup();
    s.kitty_command(b"a=d,d=C", &ctx_at(100, 10));
    assert_eq!(pairs(&s), vec![(1, 7), (1, 8), (3, 0), (4, 1)]);
    assert_eq!(ids(&s), vec![1, 3, 4]);
    let mut s = delete_setup();
    s.kitty_command(b"a=d,d=c", &ctx_at(100, 4));
    assert_eq!(s.placements().count(), 5, "column 4 is past a placement 2 columns wide at column 2");
}

#[test]
fn delete_at_cell() {
    // screen top is serial 90, so y=11 is serial 100 and x=3 is column 2
    assert_eq!(after("a=d,d=p,x=3,y=11").0, vec![(1, 8), (2, 0), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=P,x=3,y=11").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=p,x=1,y=1").0.len(), 5);
    assert_eq!(after("a=d,d=p,x=11,y=12").0, vec![(1, 7), (1, 8), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=P,x=11,y=12").1, vec![1, 3, 4]);
}

#[test]
fn delete_at_cell_with_z() {
    assert_eq!(after("a=d,d=q,x=11,y=11,z=5").0, vec![(1, 7), (1, 8), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=q,x=11,y=11,z=4").0.len(), 5);
    assert_eq!(after("a=d,d=Q,x=11,y=11,z=5").1, vec![1, 3, 4]);
}

#[test]
fn delete_by_column_and_row() {
    assert_eq!(after("a=d,d=x,x=21").0, vec![(1, 7), (2, 0), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=X,x=3").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=X,x=11").1, vec![1, 3, 4]);
    assert_eq!(after("a=d,d=y,y=11").0, vec![(1, 8), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=y,y=12").0, vec![(3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=Y,y=11").1, vec![1, 3, 4]);
}

#[test]
fn delete_by_z() {
    assert_eq!(after("a=d,d=z,z=5").0, vec![(1, 7), (1, 8), (3, 0), (4, 1)]);
    assert_eq!(after("a=d,d=Z,z=5").1, vec![1, 3, 4]);
    assert_eq!(after("a=d,d=z,z=0").0, vec![(2, 0), (4, 1)]);
    assert_eq!(after("a=d,d=Z,z=0").1, vec![2, 4]);
}

#[test]
fn delete_by_id_range() {
    assert_eq!(after("a=d,d=r,x=2,y=3").0, vec![(1, 7), (1, 8), (4, 1)]);
    assert_eq!(after("a=d,d=r,x=2,y=3").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=R,x=2,y=3").1, vec![1, 4]);
    assert_eq!(after("a=d,d=R,x=4,y=4").0, vec![(1, 7), (1, 8), (2, 0), (3, 0)]);
    assert_eq!(after("a=d,d=R,x=3,y=2").1, vec![1, 2, 3, 4]);
    assert_eq!(after("a=d,d=R,x=1,y=100").1, Vec::<u32>::new());
}

#[test]
fn delete_positional_selectors_leave_virtual_placements() {
    for sel in ["a", "A", "z", "Z"] {
        let (p, _) = after(&format!("a=d,d={},z=0", sel));
        assert!(p.contains(&(4, 1)), "{sel}");
    }
}

#[test]
fn delete_lowercase_keeps_data_uppercase_frees_only_unreferenced() {
    let mut s = delete_setup();
    cmd(&mut s, "a=d,d=Z,z=5");
    assert!(s.image(2).is_none());
    assert!(s.image(1).is_some());
    let mut s = delete_setup();
    s.kitty_command(b"a=d,d=C", &ctx_at(101, 20));
    assert!(s.image(1).is_some(), "image 1 still has placement 7");
}

#[test]
fn delete_never_replies() {
    let mut s = delete_setup();
    assert!(cmd(&mut s, "a=d,d=i,i=1").reply.is_none());
}

#[test]
fn delete_dirty_flag() {
    let mut s = delete_setup();
    assert!(cmd(&mut s, "a=d,d=i,i=1").dirty);
    assert!(!cmd(&mut s, "a=d,d=i,i=1").dirty);
}

// ---------------------------------------------------------------- animation

/// Image 1 (2 by 1 pixels, red) with two more frames (green, blue), gaps 10 and 20.
fn animated() -> GraphicsStore {
    let mut s = store();
    send(&mut s, "a=t,i=1,f=32,s=2,v=1,q=2", &solid(2, 1, [255, 0, 0, 255]));
    send(&mut s, "a=f,i=1,f=32,s=2,v=1,z=10,q=2", &solid(2, 1, [0, 255, 0, 255]));
    send(&mut s, "a=f,i=1,f=32,s=2,v=1,z=20,q=2", &solid(2, 1, [0, 0, 255, 255]));
    s
}

/// The pixels of frame `n` (0 based) of image 1.
fn frame_px(s: &GraphicsStore, n: usize) -> Vec<u8> {
    s.image(1).unwrap().frames[n].rgba.to_vec()
}

#[test]
fn frames_are_added_with_gaps() {
    let s = animated();
    let img = s.image(1).unwrap();
    assert_eq!(img.frames.len(), 3);
    assert_eq!(img.frames.iter().map(|f| f.gap_ms).collect::<Vec<_>>(), vec![0, 10, 20]);
    assert_eq!(s.used_bytes(), 24);
    assert_eq!(&frame_px(&s, 1)[..4], &[0, 255, 0, 255]);
}

#[test]
fn frame_default_gap_is_40_and_negative_is_gapless() {
    let mut s = with_image(1);
    send(&mut s, "a=f,i=1,f=32,s=2,v=2,q=2", &solid(2, 2, [1; 4]));
    send(&mut s, "a=f,i=1,f=32,s=2,v=2,z=-1,q=2", &solid(2, 2, [1; 4]));
    let gaps: Vec<i32> = s.image(1).unwrap().frames.iter().map(|f| f.gap_ms).collect();
    assert_eq!(gaps, vec![0, 40, -1]);
}

#[test]
fn frame_background_colour() {
    let mut s = store();
    send(&mut s, "a=t,i=1,f=32,s=2,v=2,q=2", &solid(2, 2, [0; 4]));
    send(&mut s, "a=f,i=1,f=32,s=1,v=1,x=1,y=1,Y=4278190335,q=2", &[0, 255, 0, 255]);
    let px = frame_px(&s, 1);
    assert_eq!(&px[..4], &[255, 0, 0, 255]);
    assert_eq!(&px[12..16], &[0, 255, 0, 255]);
}

#[test]
fn frame_default_background_is_transparent() {
    let mut s = with_image(1);
    send(&mut s, "a=f,i=1,f=32,s=1,v=1,q=2", &[9, 9, 9, 255]);
    let px = frame_px(&s, 1);
    assert_eq!(&px[..4], &[9, 9, 9, 255]);
    assert_eq!(&px[4..8], &[0, 0, 0, 0]);
}

#[test]
fn frame_based_on_previous_frame() {
    let mut s = animated();
    send(&mut s, "a=f,i=1,f=32,s=1,v=1,c=3,x=1,X=1,q=2", &[7, 7, 7, 7]);
    let px = frame_px(&s, 3);
    assert_eq!(&px[..4], &[0, 0, 255, 255]);
    assert_eq!(&px[4..8], &[7, 7, 7, 7]);
}

#[test]
fn frame_alpha_blend_versus_replace() {
    let mut s = with_image(1);
    send(&mut s, "a=t,i=2,f=32,s=1,v=1,q=2", &[0, 0, 0, 255]);
    send(&mut s, "a=f,i=2,f=32,s=1,v=1,c=1,q=2", &[255, 255, 255, 128]);
    let blended = s.image(2).unwrap().frames[1].rgba.to_vec();
    assert!((blended[0] as i32 - 128).abs() <= 1 && blended[3] == 255, "{blended:?}");
    send(&mut s, "a=f,i=2,f=32,s=1,v=1,c=1,X=1,q=2", &[255, 255, 255, 128]);
    assert_eq!(s.image(2).unwrap().frames[2].rgba.to_vec(), vec![255, 255, 255, 128]);
}

#[test]
fn frame_edit_existing() {
    let mut s = animated();
    let r = send(&mut s, "a=f,i=1,f=32,s=1,v=1,r=2,x=1,X=1,z=77", &[1, 2, 3, 4]);
    assert_eq!(body(&r), "i=1;OK");
    assert_eq!(s.image(1).unwrap().frames.len(), 3);
    assert_eq!(&frame_px(&s, 1)[4..8], &[1, 2, 3, 4]);
    assert_eq!(&frame_px(&s, 1)[..4], &[0, 255, 0, 255]);
    assert_eq!(s.image(1).unwrap().frames[1].gap_ms, 77);
}

#[test]
fn frame_edit_root_updates_image_pixels() {
    let mut s = animated();
    send(&mut s, "a=f,i=1,f=32,s=1,v=1,r=1,X=1,q=2", &[5, 6, 7, 8]);
    assert_eq!(&s.image(1).unwrap().rgba[..4], &[5, 6, 7, 8]);
    assert_eq!(&frame_px(&s, 0)[..4], &[5, 6, 7, 8]);
}

#[test]
fn frame_errors() {
    let mut s = animated();
    let cases = [
        ("a=f,i=99,f=32,s=1,v=1", "ENOENT", 4),
        ("a=f,i=1,f=32,s=1,v=1,r=9", "ENOENT", 4),
        ("a=f,i=1,f=32,s=1,v=1,c=9", "ENOENT", 4),
        ("a=f,i=1,f=32,s=1,v=1,x=2", "EINVAL", 4),
        ("a=f,i=1,f=32,s=3,v=1", "EINVAL", 12),
        ("a=f,i=1,I=2,f=32,s=1,v=1", "EINVAL", 4),
    ];
    for (ctrl, code, len) in cases {
        let r = send(&mut s, ctrl, &vec![0; len]);
        assert!(body(&r).contains(code), "{ctrl}: {}", body(&r));
    }
    assert_eq!(s.image(1).unwrap().frames.len(), 3);
    assert!(send(&mut s, "a=f,f=32,s=1,v=1", &[0; 4]).reply.is_none(), "no id, no reply");
}

#[test]
fn frame_by_image_number_and_png() {
    let mut s = store();
    let r = send(&mut s, "a=t,I=3,f=32,s=2,v=2", &solid(2, 2, [0; 4]));
    let id: u32 = body(&r)[2..body(&r).find(',').unwrap()].parse().unwrap();
    let r = send(&mut s, "a=f,I=3,f=100,x=1,y=1", &png_rgba(1, 1, [4, 4, 4, 255]));
    assert!(body(&r).contains(&format!("i={}", id)), "{}", body(&r));
    assert_eq!(s.image(id).unwrap().frames.len(), 2);
}

#[test]
fn animation_control_current_frame_gap_and_errors() {
    let mut s = animated();
    cmd(&mut s, "a=a,i=1,c=3");
    assert_eq!(s.image(1).unwrap().current_frame, 2);
    assert_eq!(s.image(1).unwrap().current_rgba()[..4], [0, 0, 255, 255]);
    cmd(&mut s, "a=a,i=1,r=1,z=33");
    assert_eq!(s.image(1).unwrap().frames[0].gap_ms, 33);
    cmd(&mut s, "a=a,i=1,r=2,z=0");
    assert_eq!(s.image(1).unwrap().frames[1].gap_ms, 10);
    assert!(body(&cmd(&mut s, "a=a,i=1,c=9")).contains("ENOENT"));
    assert!(body(&cmd(&mut s, "a=a,i=1,r=9,z=4")).contains("ENOENT"));
    assert!(body(&cmd(&mut s, "a=a,i=9,c=1")).contains("ENOENT"));
    assert_eq!(body(&cmd(&mut s, "a=a,i=1,c=1")), "i=1;OK");
}

#[test]
fn animation_runs_loops_and_stops() {
    let mut s = animated();
    cmd(&mut s, "a=a,i=1,r=1,z=5");
    cmd(&mut s, "a=a,i=1,s=3,v=2");
    assert_eq!(s.image(1).unwrap().animation.state, AnimationState::Running);
    assert!(!s.advance_animations(1000));
    assert!(!s.advance_animations(1004));
    assert!(s.advance_animations(1005));
    assert_eq!(s.image(1).unwrap().current_frame, 1);
    assert!(s.advance_animations(1015));
    assert_eq!(s.image(1).unwrap().current_frame, 2);
    assert!(!s.advance_animations(1035));
    assert_eq!(s.image(1).unwrap().animation.state, AnimationState::Stopped, "v=2 plays one loop");
    assert_eq!(s.image(1).unwrap().current_frame, 2);
}

#[test]
fn next_animation_due_says_when_to_step() {
    let mut s = animated();
    assert_eq!(s.next_animation_due(), None, "nothing runs before s=3");
    cmd(&mut s, "a=a,i=1,r=1,z=5");
    cmd(&mut s, "a=a,i=1,s=3,v=2");
    assert_eq!(s.next_animation_due(), Some(0), "running but not scheduled: step now");
    s.advance_animations(1000);
    assert_eq!(s.next_animation_due(), Some(1005));
    s.advance_animations(1005);
    assert_eq!(s.next_animation_due(), Some(1015));
    s.advance_animations(10_000);
    assert_eq!(s.next_animation_due(), None, "one loop played and stopped");
}

#[test]
fn animation_two_loops() {
    let mut s = animated();
    cmd(&mut s, "a=a,i=1,r=1,z=5");
    cmd(&mut s, "a=a,i=1,s=3,v=3");
    s.advance_animations(1000);
    s.advance_animations(1005);
    s.advance_animations(1015);
    assert!(s.advance_animations(1035));
    assert_eq!(s.image(1).unwrap().current_frame, 0, "looped once");
    s.advance_animations(1040);
    s.advance_animations(1050);
    s.advance_animations(10_000);
    assert_eq!(s.image(1).unwrap().animation.state, AnimationState::Stopped);
}

#[test]
fn animation_infinite_loop_and_stop_resets() {
    let mut s = animated();
    cmd(&mut s, "a=a,i=1,r=1,z=5,s=3,v=1");
    let before = s.generation();
    s.advance_animations(0);
    for t in (5..2000).step_by(50) {
        s.advance_animations(t);
    }
    assert!(s.generation() > before);
    assert_eq!(s.image(1).unwrap().animation.state, AnimationState::Running);
    cmd(&mut s, "a=a,i=1,s=1");
    assert_eq!(s.image(1).unwrap().animation.state, AnimationState::Stopped);
    assert_eq!(s.image(1).unwrap().animation.loops_done, 0);
    assert!(!s.advance_animations(100_000));
}

#[test]
fn animation_loading_state_waits_at_last_frame() {
    let mut s = animated();
    cmd(&mut s, "a=a,i=1,r=1,z=5,s=2");
    s.advance_animations(0);
    s.advance_animations(100);
    s.advance_animations(200);
    assert_eq!(s.image(1).unwrap().current_frame, 2);
    send(&mut s, "a=f,i=1,f=32,s=2,v=1,z=10,q=2", &solid(2, 1, [1; 4]));
    s.advance_animations(300);
    assert_eq!(s.image(1).unwrap().current_frame, 3);
}

#[test]
fn animation_gapless_frames_are_skipped() {
    let mut s = store();
    send(&mut s, "a=t,i=1,f=32,s=1,v=1,q=2", &[1; 4]);
    send(&mut s, "a=f,i=1,f=32,s=1,v=1,z=-1,q=2", &[2; 4]);
    send(&mut s, "a=f,i=1,f=32,s=1,v=1,z=30,q=2", &[3; 4]);
    cmd(&mut s, "a=a,i=1,r=1,z=10,s=3");
    s.advance_animations(0);
    s.advance_animations(10);
    assert_eq!(s.image(1).unwrap().current_frame, 2, "the gapless frame passes at once");
}

#[test]
fn animation_single_frame_never_changes() {
    let mut s = with_image(1);
    cmd(&mut s, "a=a,i=1,s=3");
    assert!(!s.advance_animations(10_000));
}

#[test]
fn compose_table() {
    let mut s = animated();
    let r = cmd(&mut s, "a=c,i=1,r=2,c=1,w=1,h=1,X=0,Y=0,x=1,y=0,C=1");
    assert_eq!(body(&r), "i=1;OK");
    assert_eq!(&frame_px(&s, 0)[4..8], &[0, 255, 0, 255]);
    assert_eq!(&frame_px(&s, 0)[..4], &[255, 0, 0, 255]);
    assert_eq!(&s.image(1).unwrap().rgba[4..8], &[0, 255, 0, 255]);
    // blend: half transparent onto opaque
    send(&mut s, "a=t,i=2,f=32,s=1,v=1,q=2", &[0, 0, 0, 255]);
    send(&mut s, "a=f,i=2,f=32,s=1,v=1,X=1,q=2", &[255, 255, 255, 128]);
    cmd(&mut s, "a=c,i=2,r=2,c=1");
    let px = &s.image(2).unwrap().frames[0].rgba;
    assert!((px[0] as i32 - 128).abs() <= 1 && px[3] == 255);
}

#[test]
fn compose_errors() {
    let mut s = animated();
    let cases = [
        ("a=c,i=9,r=1,c=2", "ENOENT"),
        ("a=c,i=1,r=9,c=2", "ENOENT"),
        ("a=c,i=1,r=1,c=9", "ENOENT"),
        ("a=c,i=1,r=1,c=0", "ENOENT"),
        ("a=c,i=1,r=1,c=2,w=3,h=1", "EINVAL"),
        ("a=c,i=1,r=1,c=2,w=1,h=1,x=2", "EINVAL"),
        ("a=c,i=1,r=1,c=2,w=1,h=1,X=2", "EINVAL"),
        ("a=c,i=1,r=1,c=1,w=1,h=1,x=0,X=0", "EINVAL"),
        ("a=c,i=1,r=1,c=1,w=2,h=1,x=1,X=0", "EINVAL"),
    ];
    for (ctrl, code) in cases {
        let r = cmd(&mut s, ctrl);
        assert!(body(&r).contains(code), "{ctrl}: {}", body(&r));
    }
    let r = cmd(&mut s, "a=c,i=1,r=1,c=1,w=1,h=1,x=1,X=0");
    assert_eq!(body(&r), "i=1;OK");
}

#[test]
fn delete_frames_selector() {
    let mut s = animated();
    cmd(&mut s, "a=d,d=f,i=1,r=2");
    assert_eq!(s.image(1).unwrap().frames.len(), 2);
    assert_eq!(s.used_bytes(), 16);
    cmd(&mut s, "a=d,d=f,i=1");
    assert_eq!(s.image(1).unwrap().frames.len(), 1);
    assert_eq!(s.used_bytes(), 8);
    cmd(&mut s, "a=d,d=F,i=1");
    assert!(s.image(1).is_some());
    let mut s = animated();
    cmd(&mut s, "a=d,d=f");
    assert_eq!(s.image(1).unwrap().frames.len(), 1);
}

// ---------------------------------------------------------------- quota

#[test]
fn quota_evicts_oldest_first() {
    let mut s = GraphicsStore::new(100);
    for id in 1..=3 {
        add(&mut s, id, 4, 2);
    }
    assert_eq!((ids(&s), s.used_bytes()), (vec![1, 2, 3], 96));
    add(&mut s, 4, 4, 2);
    assert_eq!(ids(&s), vec![2, 3, 4]);
    add(&mut s, 5, 4, 4);
    assert_eq!(ids(&s), vec![4, 5]);
    assert!(s.used_bytes() <= 100);
}

#[test]
fn quota_prefers_images_without_placements() {
    let mut s = GraphicsStore::new(100);
    add(&mut s, 1, 4, 2);
    add(&mut s, 2, 4, 2);
    add(&mut s, 3, 4, 2);
    put(&mut s, 1, "", 100, 0);
    add(&mut s, 4, 4, 2);
    assert_eq!(ids(&s), vec![1, 3, 4]);
    assert_eq!(s.placements().count(), 1);
}

#[test]
fn quota_evicts_placed_images_when_nothing_else_and_drops_their_placements() {
    let mut s = GraphicsStore::new(64);
    add(&mut s, 1, 4, 4);
    put(&mut s, 1, "", 100, 0);
    add(&mut s, 2, 4, 4);
    assert_eq!(ids(&s), vec![2]);
    assert_eq!(s.placements().count(), 0);
}

#[test]
fn quota_never_evicts_the_new_image_and_rejects_oversize() {
    let mut s = GraphicsStore::new(64);
    add(&mut s, 1, 4, 4);
    let r = send(&mut s, "a=t,i=2,f=32,s=5,v=4", &solid(5, 4, [0; 4]));
    assert!(body(&r).contains("ENOSPC"), "{}", body(&r));
    assert_eq!(ids(&s), vec![1]);
    assert_eq!(s.dropped(), 1);
}

#[test]
fn quota_replacing_an_image_frees_its_bytes_first() {
    let mut s = GraphicsStore::new(64);
    add(&mut s, 1, 4, 4);
    add(&mut s, 1, 4, 4);
    assert_eq!(ids(&s), vec![1]);
    assert_eq!(s.used_bytes(), 64);
}

#[test]
fn quota_transient_images_go_first() {
    let mut s = GraphicsStore::new(100);
    add(&mut s, 1, 4, 2);
    send(&mut s, "a=t,i=2,f=32,s=4,v=2,N=1,q=2", &solid(4, 2, [1; 4]));
    add(&mut s, 3, 4, 2);
    add(&mut s, 4, 4, 2);
    assert_eq!(ids(&s), vec![1, 3, 4]);
}

#[test]
fn quota_frames_count_and_evict_others() {
    let mut s = GraphicsStore::new(64);
    add(&mut s, 1, 2, 2);
    add(&mut s, 2, 4, 2);
    for _ in 0..3 {
        send(&mut s, "a=f,i=2,f=32,s=4,v=2,q=2", &solid(4, 2, [1; 4]));
    }
    assert_eq!(ids(&s), vec![2]);
    assert_eq!(s.image(2).unwrap().frames.len(), 2);
    assert!(s.used_bytes() <= 64);
}

#[test]
fn clear_resets_everything() {
    let mut s = delete_setup();
    let g = s.generation();
    s.clear();
    assert_eq!((s.images().count(), s.placements().count(), s.used_bytes()), (0, 0, 0));
    assert!(s.generation() > g);
}

// ---------------------------------------------------------------- prune and queries

#[test]
fn prune_drops_placements_and_display_only_images() {
    let mut s = store();
    send_at(&mut s, "a=T,i=1,f=32,s=1,v=1,q=2", &[0; 4], &ctx_at(10, 0));
    send_at(&mut s, "a=T,i=2,f=32,s=1,v=1,q=2", &[0; 4], &ctx_at(20, 0));
    send(&mut s, "a=t,i=3,f=32,s=1,v=1,q=2", &[0; 4]);
    put(&mut s, 3, "", 10, 0);
    send_at(&mut s, "a=T,f=32,s=1,v=1", &[0; 4], &ctx_at(5, 0));
    s.prune_before(15);
    assert_eq!(pairs(&s), vec![(2, 0)]);
    assert_eq!(ids(&s), vec![2, 3], "a=t data stays, a=T and anonymous data goes");
    s.prune_before(15);
    assert_eq!(s.placements().count(), 1);
    s.prune_before(21);
    assert_eq!(ids(&s), vec![3]);
}

#[test]
fn prune_keeps_image_with_other_placements_and_virtual_ones() {
    let mut s = store();
    send_at(&mut s, "a=T,i=1,f=32,s=1,v=1,q=2", &[0; 4], &ctx_at(10, 0));
    put(&mut s, 1, "", 30, 0);
    put(&mut s, 1, "U=1,c=1,r=1", 0, 0);
    s.prune_before(20);
    assert_eq!(s.placements().count(), 2);
    assert!(s.image(1).is_some());
}

#[test]
fn prune_cascades_relative_placements() {
    let mut s = relative_setup();
    put(&mut s, 2, "p=1,P=1,Q=1,V=100", 0, 0);
    s.prune_before(101);
    assert_eq!(s.placements().count(), 0);
}

#[test]
fn placements_in_rows_range() {
    let mut s = store();
    add(&mut s, 1, 10, 60);
    put(&mut s, 1, "", 100, 0);
    put(&mut s, 1, "", 200, 0);
    assert_eq!(s.placements_in_rows(100, 100).len(), 1);
    assert_eq!(s.placements_in_rows(102, 150).len(), 1);
    assert_eq!(s.placements_in_rows(103, 150).len(), 0);
    assert_eq!(s.placements_in_rows(0, 99).len(), 0);
    assert_eq!(s.placements_in_rows(0, 1000).len(), 2);
    assert_eq!(s.placements_in_rows(50, 100).len(), 1);
}

#[test]
fn generation_bumps_on_visible_changes_only() {
    let mut s = store();
    let g0 = s.generation();
    add(&mut s, 1, 1, 1);
    assert_eq!(s.generation(), g0);
    put(&mut s, 1, "", 100, 0);
    let g1 = s.generation();
    assert!(g1 > g0);
    put(&mut s, 1, "U=1,c=1,r=1", 100, 0);
    assert_eq!(s.generation(), g1);
    cmd(&mut s, "a=d");
    assert!(s.generation() > g1);
}

#[test]
fn images_iterate_oldest_first_and_default_store_has_default_quota() {
    let mut s = GraphicsStore::default();
    add(&mut s, 5, 1, 1);
    add(&mut s, 2, 1, 1);
    assert_eq!(s.images().map(|i| i.id).collect::<Vec<_>>(), vec![5, 2]);
    assert_eq!(DEFAULT_QUOTA, 320 * 1024 * 1024);
}

// ---------------------------------------------------------------- sixel

/// Decodes sixel data with the default context and returns the stored image.
fn sixel_image(params: &[u16], data: &str) -> (GraphicsStore, GraphicsResult) {
    let mut s = store();
    let r = s.sixel(params, data.as_bytes(), &ctx());
    (s, r)
}

/// The only image of a store.
fn only(s: &GraphicsStore) -> &Image {
    s.images().next().expect("an image")
}

/// RGBA of pixel (x, y).
fn pixel(img: &Image, x: usize, y: usize) -> [u8; 4] {
    let at = (y * img.width as usize + x) * 4;
    [img.rgba[at], img.rgba[at + 1], img.rgba[at + 2], img.rgba[at + 3]]
}

#[test]
fn sixel_basic_colours_and_size() {
    let (s, r) = sixel_image(&[], "#1;2;100;0;0!5~");
    let img = only(&s);
    assert_eq!((img.width, img.height), (5, 6));
    assert_eq!(pixel(img, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(img, 4, 5), [255, 0, 0, 255]);
    assert_eq!(s.placements().count(), 1);
    assert!(r.dirty);
}

#[test]
fn sixel_cursor_ends_after_image() {
    // 5 by 6 pixels with 10 by 20 cells: one row
    let (_, r) = sixel_image(&[], "#1;2;100;0;0!5~");
    assert_eq!(r.move_cursor, Some((0, 1)));
    assert!(r.cursor_to_column_zero);
    // 3 bands = 18 rows is still one 20 pixel row; 4 bands = 24 pixels is two
    let (_, r) = sixel_image(&[], "#1~-~-~-~");
    assert_eq!(r.move_cursor, Some((0, 2)));
    let (s, _) = sixel_image(&[], "#1~");
    let p = s.placements().next().unwrap();
    assert_eq!((p.serial, p.col, p.cols, p.rows), (100, 2, 1, 1));
}

#[test]
fn sixel_bits_map_to_rows() {
    let (s, _) = sixel_image(&[], "#1;2;100;100;100@A");
    let img = only(&s);
    assert_eq!((img.width, img.height), (2, 6));
    assert_eq!(pixel(img, 0, 0)[3], 255);
    assert_eq!(pixel(img, 1, 1)[3], 255);
    // '@' is bit 0 (top) and 'A' is bit 1; the rest has the background colour 0 (black, opaque)
    assert_eq!(pixel(img, 0, 1), [0, 0, 0, 255]);
    assert_eq!(pixel(img, 1, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(img, 0, 0), [255, 255, 255, 255]);
}

#[test]
fn sixel_carriage_return_overprints_with_second_colour() {
    let (s, _) = sixel_image(&[], "#1;2;100;0;0@$#2;2;0;100;0A");
    let img = only(&s);
    assert_eq!(pixel(img, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(img, 0, 1), [0, 255, 0, 255]);
}

#[test]
fn sixel_new_line_moves_down_six() {
    let (s, _) = sixel_image(&[], "#1;2;100;0;0~-#2;2;0;0;100~");
    let img = only(&s);
    assert_eq!((img.width, img.height), (1, 12));
    assert_eq!(pixel(img, 0, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(img, 0, 6), [0, 0, 255, 255]);
}

#[test]
fn sixel_repeat_counts() {
    let (s, _) = sixel_image(&[], "#1~!10~!3?~");
    let img = only(&s);
    assert_eq!(img.width, 15);
    assert_eq!(pixel(img, 10, 0)[3], 255);
    let (s, _) = sixel_image(&[], "#1!0~");
    assert_eq!(only(&s).width, 1, "a repeat of 0 draws once");
}

#[test]
fn sixel_default_palette() {
    let (s, _) = sixel_image(&[], "#1~#2~#15~");
    let img = only(&s);
    assert_eq!(pixel(img, 0, 0), [51, 51, 204, 255]);
    assert_eq!(pixel(img, 1, 0), [204, 33, 33, 255]);
    assert_eq!(pixel(img, 2, 0), [204, 204, 204, 255]);
}

#[test]
fn sixel_hls_colours() {
    let cases = [
        ("#0;1;120;50;100~", [255, 0, 0]),
        ("#0;1;240;50;100~", [0, 255, 0]),
        ("#0;1;0;50;100~", [0, 0, 255]),
        ("#0;1;0;100;0~", [255, 255, 255]),
        ("#0;1;0;0;50~", [0, 0, 0]),
    ];
    for (data, rgb) in cases {
        let (s, _) = sixel_image(&[], data);
        let px = pixel(only(&s), 0, 0);
        assert_eq!(&px[..3], &rgb, "{data}");
    }
}

#[test]
fn sixel_rgb_colours_are_percentages() {
    let (s, _) = sixel_image(&[], "#7;2;50;0;100~");
    assert_eq!(pixel(only(&s), 0, 0), [128, 0, 255, 255]);
}

#[test]
fn sixel_redefining_register_affects_later_pixels_only() {
    let (s, _) = sixel_image(&[], "#1;2;100;0;0~#1;2;0;100;0~");
    let img = only(&s);
    assert_eq!(pixel(img, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(img, 1, 0), [0, 255, 0, 255]);
}

#[test]
fn sixel_register_numbers_wrap_to_256() {
    let (s, _) = sixel_image(&[], "#257;2;100;0;0~#1~");
    assert_eq!(pixel(only(&s), 0, 0)[..3], [255, 0, 0]);
}

#[test]
fn sixel_raster_attributes_set_size() {
    let (s, _) = sixel_image(&[], "\"1;1;10;12#1;2;100;0;0!3~");
    let img = only(&s);
    assert_eq!((img.width, img.height), (10, 12));
    assert_eq!(pixel(img, 5, 0), [0, 0, 0, 255], "undrawn pixels take register 0");
    assert_eq!(pixel(img, 2, 5), [255, 0, 0, 255]);
}

#[test]
fn sixel_raster_attributes_crop_extra_data() {
    let (s, _) = sixel_image(&[], "\"1;1;4;3#1~!10~-~");
    let img = only(&s);
    assert_eq!((img.width, img.height), (4, 3));
}

#[test]
fn sixel_aspect_ratio_from_p1_and_raster() {
    let (s, _) = sixel_image(&[1], "#1~");
    assert_eq!((only(&s).width, only(&s).height), (1, 12));
    let (s, _) = sixel_image(&[0], "#1~");
    assert_eq!((only(&s).width, only(&s).height), (1, 6));
    let (s, _) = sixel_image(&[9], "#1~");
    assert_eq!((only(&s).width, only(&s).height), (1, 6));
    let (s, _) = sixel_image(&[], "\"2;1;1;6#1~");
    assert_eq!((only(&s).width, only(&s).height), (1, 12));
    let (s, _) = sixel_image(&[], "\"1;2;1;6#1~");
    assert_eq!((only(&s).width, only(&s).height), (2, 6));
    let (s, _) = sixel_image(&[], "\"80;1;1;6#1~");
    assert_eq!(only(&s).height, 48, "the repeat count is capped at 8");
}

#[test]
fn sixel_background_select_transparency() {
    let (s, _) = sixel_image(&[0, 1], "\"1;1;3;6#1;2;100;0;0~?~");
    let img = only(&s);
    assert_eq!(pixel(img, 1, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(img, 0, 0), [255, 0, 0, 255]);
    let (s, _) = sixel_image(&[0, 0], "\"1;1;3;6#1;2;100;0;0~?~");
    assert_eq!(pixel(only(&s), 1, 0), [0, 0, 0, 255]);
    let (s, _) = sixel_image(&[0, 2], "\"1;1;3;6#1;2;100;0;0~?~");
    assert_eq!(pixel(only(&s), 1, 0)[3], 255);
}

#[test]
fn sixel_background_uses_register_zero_colour() {
    let (s, _) = sixel_image(&[], "\"1;1;3;6#0;2;0;100;0#1;2;100;0;0~");
    assert_eq!(pixel(only(&s), 2, 3), [0, 255, 0, 255]);
}

#[test]
fn sixel_truncated_image_keeps_decoded_part() {
    let (s, r) = sixel_image(&[], "#1;2;100;0;0!10~-!10~-!3");
    let img = only(&s);
    assert_eq!((img.width, img.height), (10, 12));
    assert!(r.dirty);
    let (s, _) = sixel_image(&[], "#1;2;100;0;");
    assert_eq!(s.images().count(), 0);
    let (s, _) = sixel_image(&[], "#1;2;100;0;0~\x1b");
    assert_eq!(only(&s).width, 1);
}

#[test]
fn sixel_ignores_whitespace_and_unknown_bytes() {
    let (s, _) = sixel_image(&[], "#1;2;100;0;0 \n~\r\n~ \x07~");
    assert_eq!(only(&s).width, 3);
}

#[test]
fn sixel_empty_data_gives_no_image() {
    for data in ["", "$", "-", "#1", "!5", "\"1;1;0;0", "???"] {
        let (s, r) = sixel_image(&[], data);
        assert_eq!(s.images().count(), 0, "{data:?}");
        assert!(r.move_cursor.is_none(), "{data:?}");
    }
}

#[test]
fn sixel_question_mark_columns_extend_width() {
    let (s, _) = sixel_image(&[], "#1~!5?");
    assert_eq!(only(&s).width, 6);
}

#[test]
fn sixel_image_is_freed_on_prune_and_has_no_id_reply() {
    let (mut s, _) = sixel_image(&[], "#1~");
    assert!(only(&s).anonymous);
    s.prune_before(1000);
    assert_eq!(s.images().count(), 0);
}

#[test]
fn sixel_over_quota_is_dropped() {
    let mut s = GraphicsStore::new(100);
    let r = s.sixel(&[], b"#1!50~-!50~", &ctx());
    assert_eq!(s.images().count(), 0, "a 50 by 12 image needs 2400 bytes; the decoder stops growing at the budget");
    let _ = r;
}

#[test]
fn sixel_huge_raster_does_not_allocate() {
    let mut s = GraphicsStore::new(1 << 20);
    s.sixel(&[], b"\"1;1;16000;16000#1~", &ctx());
    assert!(s.used_bytes() <= 1 << 20);
}

// ---------------------------------------------------------------- vt340test corpus

/// Splits a `.six` file into (DCS parameters, data) for every DCS that ends in `q`.
fn sixel_blocks(bytes: &[u8]) -> Vec<(Vec<u16>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = if bytes[i] == 0x90 {
            Some(i + 1)
        } else if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'P') {
            Some(i + 2)
        } else {
            None
        };
        let Some(mut j) = start else {
            i += 1;
            continue;
        };
        let params_start = j;
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';' || (0x20..0x30).contains(&bytes[j])) {
            j += 1;
        }
        let is_sixel = bytes.get(j) == Some(&b'q');
        let data_start = j + 1;
        let mut end = data_start;
        while end < bytes.len() && bytes[end] != 0x9c && !(bytes[end] == 0x1b && bytes.get(end + 1) == Some(&b'\\')) {
            end += 1;
        }
        if is_sixel {
            let text = String::from_utf8_lossy(&bytes[params_start..j]).into_owned();
            let params = text.split(';').map(|p| p.trim().parse::<u16>().unwrap_or(0)).collect();
            out.push((params, bytes[data_start.min(bytes.len())..end.min(bytes.len())].to_vec()));
        }
        i = end + 1;
    }
    out
}

/// FNV-1a 64 bit hash.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

#[test]
fn vt340test_images_decode_to_expected_values() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/vt340test");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "six"))
        .collect();
    files.sort();
    assert!(files.len() >= 10, "the corpus has at least 10 files");
    let mut lines = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let blocks = sixel_blocks(&std::fs::read(path).unwrap());
        assert!(!blocks.is_empty(), "{name} has a sixel DCS");
        for (n, (params, data)) in blocks.iter().enumerate() {
            let mut s = store();
            let r = s.sixel(params, data, &ctx());
            let Some(img) = s.images().next() else {
                assert!(!r.dirty, "{name}#{n}");
                lines.push(format!("{}#{} none", name, n));
                continue;
            };
            assert!(r.dirty && r.cursor_to_column_zero, "{name}#{n}");
            assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
            lines.push(format!("{}#{} {}x{} {:016x}", name, n, img.width, img.height, fnv(&img.rgba)));
        }
    }
    let expected_path = dir.join("expected.txt");
    let actual = lines.join("\n") + "\n";
    if std::env::var("TERMINUS_BLESS").is_ok_and(|v| v == "1") {
        std::fs::write(&expected_path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&expected_path).expect("expected.txt; run once with TERMINUS_BLESS=1");
    assert_eq!(actual.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
}

#[test]
fn vt340test_documented_dimensions() {
    // Dimensions stated by the raster attributes of each file (aspect ratio 1:1), reviewed by hand.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/vt340test");
    let cases = [
        ("sixeltests_map8.six", (93, 14)),
        ("sixeltests_8bit.six", (423, 20)),
        ("sixeltests_colorwheel+dither.six", (480, 480)),
        ("mediacopy_samplehardcopy_level2compressed.six", (800, 480)),
        ("vms_declogo_declogo.six", (800, 222)),
        ("jerch_sdmhome.six", (627, 40)),
    ];
    for (name, (w, h)) in cases {
        let blocks = sixel_blocks(&std::fs::read(dir.join(name)).unwrap());
        let mut s = store();
        s.sixel(&blocks[0].0, &blocks[0].1, &ctx());
        let img = s.images().next().unwrap();
        assert_eq!((img.width, img.height), (w, h), "{name}");
    }
}

// ---------------------------------------------------------------- iTerm2

/// An iTerm2 payload (after `File=`) with arguments and a PNG.
fn iterm_payload(args: &str, png: &[u8]) -> Vec<u8> {
    format!("{}:{}", args, b64(png)).into_bytes()
}

#[test]
fn iterm2_basic_inline_png() {
    let mut s = store();
    let png = png_rgba(25, 45, [1, 2, 3, 4]);
    let r = s.iterm2_file(&iterm_payload(&format!("inline=1;size={};name={}", png.len(), b64(b"a.png")), &png), &ctx());
    let img = only(&s);
    assert_eq!((img.width, img.height), (25, 45));
    let p = s.placements().next().unwrap();
    assert_eq!((p.serial, p.col, p.cols, p.rows, p.stretch), (100, 2, 3, 3, false));
    assert_eq!(r.move_cursor, Some((3, 3)));
    assert!(r.dirty);
}

#[test]
fn iterm2_sizes_table() {
    // image 40 by 40 pixels, cells 10 by 20, screen 80 by 24
    let png = png_rgba(40, 40, [1; 4]);
    let cases: &[(&str, (u16, u16), bool)] = &[
        ("inline=1", (4, 2), false),
        ("inline=1;width=8", (8, 4), false),
        ("inline=1;height=3", (6, 3), false),
        ("inline=1;width=100px", (10, 5), false),
        ("inline=1;width=50%", (40, 20), false),
        ("inline=1;height=50%", (24, 12), false),
        ("inline=1;width=auto;height=auto", (4, 2), false),
        ("inline=1;width=10;height=1", (2, 1), false),
        ("inline=1;width=10;height=1;preserveAspectRatio=0", (10, 1), true),
        ("inline=1;width=10;height=5;preserveAspectRatio=1", (10, 5), false),
    ];
    for (args, (cols, rows), stretch) in cases {
        let mut s = store();
        s.iterm2_file(&iterm_payload(args, &png), &ctx());
        let p = s.placements().next().unwrap_or_else(|| panic!("{args}"));
        assert_eq!((p.cols, p.rows, p.stretch), (*cols, *rows, *stretch), "{args}");
    }
}

#[test]
fn iterm2_not_inline_or_not_png_or_malformed() {
    let mut s = store();
    let png = png_rgba(2, 2, [1; 4]);
    assert_eq!(s.iterm2_file(&iterm_payload("inline=0", &png), &ctx()), GraphicsResult::default());
    assert_eq!(s.iterm2_file(&iterm_payload("name=x", &png), &ctx()), GraphicsResult::default());
    assert_eq!(s.images().count(), 0);
    assert_eq!(s.dropped(), 0);
    s.iterm2_file(&iterm_payload("inline=1", b"GIF89a....."), &ctx());
    s.iterm2_file(&iterm_payload("inline=1", b"\xff\xd8\xff\xe0jpeg"), &ctx());
    s.iterm2_file(b"inline=1", &ctx());
    s.iterm2_file(b"inline=1:!!!!", &ctx());
    s.iterm2_file(&iterm_payload("inline=1", &png[..20]), &ctx());
    assert_eq!(s.dropped(), 5);
    assert_eq!(s.images().count(), 0);
}

#[test]
fn iterm2_images_are_display_only() {
    let mut s = store();
    s.iterm2_file(&iterm_payload("inline=1", &png_rgba(2, 2, [1; 4])), &ctx());
    assert!(only(&s).anonymous);
    s.prune_before(1000);
    assert_eq!(s.images().count(), 0);
}
