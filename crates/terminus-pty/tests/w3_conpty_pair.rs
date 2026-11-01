//! W3: the fetched conpty.dll and OpenConsole.exe match installer/windows/conpty.version and each other.
#![cfg(windows)]

mod common;

use common::*;
use std::path::Path;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::core::PCWSTR;

/// Reads the four part file version of a PE file as `a.b.c.d`.
fn file_version(path: &Path) -> String {
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    // SAFETY: `wide` is a nul terminated path; the buffer is sized by the first call; the query result points into the buffer.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        assert!(size > 0, "no version resource in {}", path.display());
        let mut buffer = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, buffer.as_mut_ptr().cast())
            .expect("GetFileVersionInfoW");
        let mut info: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let root: Vec<u16> = "\\".encode_utf16().chain(Some(0)).collect();
        assert!(VerQueryValueW(buffer.as_ptr().cast(), PCWSTR(root.as_ptr()), &mut info, &mut len).as_bool());
        let fixed = &*(info as *const VS_FIXEDFILEINFO);
        format!(
            "{}.{}.{}.{}",
            fixed.dwFileVersionMS >> 16,
            fixed.dwFileVersionMS & 0xffff,
            fixed.dwFileVersionLS >> 16,
            fixed.dwFileVersionLS & 0xffff
        )
    }
}

/// Converts a NuGet version such as `1.25.260930003` into the file version `1.25.2609.30003`.
fn nuget_to_file_version(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 3, "unexpected version shape {version}");
    let third = parts[2];
    let (head, tail) = third.split_at(third.len() - 5);
    format!(
        "{}.{}.{}.{}",
        parts[0],
        parts[1],
        head,
        tail.trim_start_matches('0').parse::<u32>().map_or("0".to_string(), |n| n.to_string())
    )
}

/// The pair on disk reports the version written in conpty.version, and the two files agree.
#[test]
fn bundled_pair_matches_version_file() {
    if !bundled_present() {
        println!(
            "W3 skipped: conpty.dll and OpenConsole.exe not fetched (run pwsh -NoProfile -File tools/fetch-conpty.ps1)"
        );
        return;
    }
    let version_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../installer/windows/conpty.version");
    let wanted = std::fs::read_to_string(version_file).unwrap().trim().to_string();
    assert!(!wanted.is_empty(), "conpty.version is empty");
    let dll = file_version(&bundled_dir().join("conpty.dll"));
    let exe = file_version(&bundled_dir().join("OpenConsole.exe"));
    assert_eq!(dll, exe, "conpty.dll and OpenConsole.exe are from different builds");
    assert_eq!(dll, nuget_to_file_version(&wanted), "the fetched pair does not match installer/windows/conpty.version");
}
