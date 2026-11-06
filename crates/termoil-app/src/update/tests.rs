//! Unit tests for reading release manifests, comparing versions and the daily stamp.

use super::*;

/// A channel list in the shape unluminous.com publishes, with two versions.
const CHANNEL_LIST: &str = r#"{
  "product": "Termoil", "channel": "nightly", "version": "1.6.0", "date": "2026-11-07",
  "notes": "https://github.com/Black-Rainbow-Labs/Termoil/releases/tag/v1.6.0",
  "files": {
    "windows": { "file": "TermoilSetup-1.6.0-x64.exe", "url": "https://x/TermoilSetup-1.6.0-x64.exe", "bytes": 10, "sha256": "AB" },
    "macos": { "file": "termoil-1.6.0-macos-universal.dmg", "url": "https://x/m.dmg", "bytes": 20, "sha256": "cd" }
  },
  "versions": [
    { "version": "1.6.0", "date": "2026-11-07", "notes": "n6", "files": { "windows": { "file": "TermoilSetup-1.6.0-x64.exe", "url": "https://x/a", "bytes": 10, "sha256": "ab" } } },
    { "version": "1.5.0", "date": "2026-11-06", "notes": "n5", "files": { "windows": { "file": "TermoilSetup-1.5.0-x64.exe", "url": "https://x/b", "bytes": 11, "sha256": "ef" } } }
  ]
}"#;

/// A channel list is read newest first, with the SHA-256 in lowercase.
#[test]
fn reads_a_channel_list() {
    let list = read_versions(CHANNEL_LIST).expect("a list");
    assert_eq!(list.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(), ["1.6.0", "1.5.0"]);
    assert_eq!(list[0].files["windows"].sha256, "ab");
    assert_eq!(list[1].files["windows"].bytes, 11);
    assert_eq!(list[1].notes, "n5");
}

/// The task-2491 `latest.json`, which has no `versions`, reads as a list of one.
#[test]
fn reads_the_old_latest_file() {
    let body = r#"{ "product": "Termoil", "version": "1.4.1", "page": "p", "install": "i",
      "files": { "deb": { "file": "termoil_1.4.1_amd64.deb", "url": "https://x/d", "bytes": 7, "sha256": "00" } } }"#;
    let list = read_versions(body).expect("a list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].version, "1.4.1");
    assert_eq!(list[0].files["deb"].url, "https://x/d");
}

/// A GitHub release reads its files from the asset names, its SHA-256 from `digest`, and leaves out an
/// asset with no digest.
#[test]
fn reads_a_github_release() {
    let body = r#"{ "tag_name": "v1.4.1", "published_at": "2026-11-02T23:07:47Z", "html_url": "https://g/v1.4.1",
      "assets": [
        { "name": "TermoilSetup-1.4.1-x64.exe", "browser_download_url": "https://g/w", "size": 5, "digest": "sha256:AA" },
        { "name": "termoil-1.4.1-x86_64.AppImage", "browser_download_url": "https://g/a", "size": 6 },
        { "name": "notes.txt", "browser_download_url": "https://g/n", "size": 1, "digest": "sha256:bb" }
      ] }"#;
    let list = read_versions(body).expect("a release");
    assert_eq!(list[0].version, "1.4.1");
    assert_eq!(list[0].date, "2026-11-02");
    assert_eq!(list[0].files.keys().collect::<Vec<_>>(), ["windows"]);
    assert_eq!(list[0].files["windows"].sha256, "aa");
}

/// Bodies that are not release lists read as nothing.
#[test]
fn refuses_other_bodies() {
    assert!(read_versions("<html>").is_none());
    assert!(read_versions(r#"{ "version": "not a version", "files": {} }"#).is_none());
    assert!(read_versions(r#"{ "version": "1.0.0" }"#).is_none());
}

/// Versions compare by number, not as text.
#[test]
fn compares_versions() {
    assert!(is_newer("1.4.1", "1.4.2"));
    assert!(is_newer("1.4.1", "1.10.0"));
    assert!(is_newer("1.9.9", "v2.0.0"));
    assert!(!is_newer("1.4.1", "1.4.1"));
    assert!(!is_newer("1.5.0", "1.4.9"));
    assert!(!is_newer("1.4.1", "junk"));
}

/// A daily check is due with no stamp, a day after the stamp, and when the clock went back.
#[test]
fn daily_check_is_due_once_a_day() {
    let day = DAY.as_secs();
    assert!(is_due(None, 1000));
    assert!(!is_due(Some(1000), 1000 + day - 1));
    assert!(is_due(Some(1000), 1000 + day));
    assert!(is_due(Some(1000), 10));
}

/// The stamp is written and read back from the folder.
#[test]
fn stamp_round_trips() {
    let dir = std::env::temp_dir().join(format!("termoil-stamp-{}", std::process::id()));
    note_checked(&dir, 12345);
    assert_eq!(last_checked(&dir), Some(12345));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The stable channel asks the site's list, the old latest file, then GitHub; nightly asks only its list.
#[test]
fn channel_sources_are_in_order() {
    if std::env::var_os("TERMOIL_RELEASES_STABLE").is_none() {
        let stable = sources(UpdateChannel::Stable);
        assert_eq!(stable.len(), 3);
        assert!(stable[0].ends_with("/termoil/stable.json"));
        assert!(stable[1].ends_with("/termoil/latest.json"));
        assert!(stable[2].contains("api.github.com"));
    }
    if std::env::var_os("TERMOIL_RELEASES_NIGHTLY").is_none() {
        assert_eq!(sources(UpdateChannel::Nightly), vec![NIGHTLY.to_string()]);
    }
}

/// Release file names map to the four kinds the way the release script names them.
#[test]
fn file_kinds() {
    assert_eq!(kind_of_file("TermoilSetup-1.4.1-x64.exe"), Some("windows"));
    assert_eq!(kind_of_file("termoil-1.4.1-macos-universal.dmg"), Some("macos"));
    assert_eq!(kind_of_file("termoil_1.4.1_amd64.deb"), Some("deb"));
    assert_eq!(kind_of_file("termoil-1.4.1-x86_64.AppImage"), Some("appimage"));
    assert_eq!(kind_of_file("termoil.exe"), None);
}

/// The helper's arguments read back, and a missing kind is refused.
#[test]
fn helper_arguments() {
    let args: Vec<String> =
        ["f.exe", "--kind", "windows", "--into", "C:/T", "--pid", "42", "--session", "s.json", "--launch"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    let a = apply::parse(&args).expect("parsed");
    assert_eq!(a.kind, "windows");
    assert_eq!(a.pid, Some(42));
    assert!(a.launch);
    assert_eq!(a.session.as_deref(), Some(std::path::Path::new("s.json")));
    assert!(apply::parse(&["f.exe".to_string()]).is_err());
}

/// A file is accepted only with the manifest's size and SHA-256.
#[test]
fn verify_checks_size_and_hash() {
    let dir = std::env::temp_dir().join(format!("termoil-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pkg.bin");
    std::fs::write(&path, b"abc").unwrap();
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string();
    let good = Download { file: "pkg.bin".into(), url: String::new(), bytes: 3, sha256: sha.clone() };
    assert!(install::verify(&path, &good).is_ok());
    assert!(install::verify(&path, &Download { bytes: 4, ..good.clone() }).is_err());
    assert!(install::verify(&path, &Download { sha256: "00".into(), ..good }).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
