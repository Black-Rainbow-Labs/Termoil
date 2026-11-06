//! The update check and installing an update (`docs/decisions/updates.md`).
//!
//! unluminous.com publishes `/termoil/stable.json` and `/termoil/nightly.json`, each naming the newest
//! version on its channel and the newest ten before it, with the four release files and their sizes and
//! SHA-256. This module reads those lists, compares versions, keeps the once a day stamp, downloads and
//! checks a file ([`install`]) and, in the helper process `termoil --apply-update`, installs it and starts
//! the new version ([`apply`]). Requests go through the system's curl ([`fetch`]).

pub mod actions;
pub mod apply;
pub mod fetch;
pub mod install;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::UpdateChannel;

/// The stable channel's list.
const STABLE: &str = "https://unluminous.com/termoil/stable.json";
/// The newest stable version in the shape task-2491 published, for a site not yet rebuilt.
const LATEST: &str = "https://unluminous.com/termoil/latest.json";
/// The nightly channel's list.
const NIGHTLY: &str = "https://unluminous.com/termoil/nightly.json";
/// GitHub's newest full release, which is never a prerelease and so is the newest stable version.
const GITHUB: &str = "https://api.github.com/repos/Black-Rainbow-Labs/Termoil/releases/latest";
/// The page a person downloads from when Termoil cannot install the file itself.
pub const RELEASES_PAGE: &str = "https://unluminous.com/termoil#install";
/// How many versions of each channel `list-updates` shows.
pub const SHOWN: usize = 5;
/// The stamp file in the data folder that counts the day across processes.
const CHECKED: &str = "update-checked.txt";
/// One day, the interval of `updates.check = "daily"`.
pub const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// One release file: where it is, and the size and SHA-256 it must have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub file: String,
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
}

/// One version on a channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub notes: String,
    /// The files by kind: `windows`, `macos`, `deb`, `appimage`.
    pub files: BTreeMap<String, Download>,
}

impl Release {
    /// The file this platform installs.
    pub fn download(&self) -> Option<&Download> {
        self.files.get(platform_kind())
    }

    /// The release as JSON for an action's answer.
    pub fn to_json(&self) -> Value {
        let file = self.download();
        json!({
            "version": self.version,
            "date": self.date,
            "notes": self.notes,
            "file": file.map(|d| d.file.clone()),
            "url": file.map(|d| d.url.clone()),
            "bytes": file.map(|d| d.bytes),
            "sha256": file.map(|d| d.sha256.clone()),
        })
    }
}

/// The channel's name as the settings and the manifests write it.
pub fn channel_name(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Nightly => "nightly",
    }
}

/// Reads a channel name; None for anything else.
pub fn parse_channel(name: &str) -> Option<UpdateChannel> {
    match name.trim().to_ascii_lowercase().as_str() {
        "stable" => Some(UpdateChannel::Stable),
        "nightly" => Some(UpdateChannel::Nightly),
        _ => None,
    }
}

/// The URLs a channel's check asks, in order. `TERMOIL_RELEASES_STABLE` and `TERMOIL_RELEASES_NIGHTLY`
/// replace a channel's list with one URL, which is how the tests point Termoil at a local server.
pub fn sources(channel: UpdateChannel) -> Vec<String> {
    let var = match channel {
        UpdateChannel::Stable => "TERMOIL_RELEASES_STABLE",
        UpdateChannel::Nightly => "TERMOIL_RELEASES_NIGHTLY",
    };
    if let Some(url) = std::env::var(var).ok().filter(|v| !v.trim().is_empty()) {
        return vec![url.trim().to_string()];
    }
    match channel {
        UpdateChannel::Stable => vec![STABLE.into(), LATEST.into(), GITHUB.into()],
        UpdateChannel::Nightly => vec![NIGHTLY.into()],
    }
}

/// The kind of file this platform installs: `windows`, `macos`, `appimage` when running as an AppImage,
/// otherwise `deb`.
pub fn platform_kind() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if std::env::var_os("APPIMAGE").is_some_and(|v| !v.is_empty()) {
        "appimage"
    } else {
        "deb"
    }
}

/// Reads the versions in a manifest body: a site channel list (newest first), the task-2491
/// `latest.json`, or a GitHub release. None when the body is none of these.
pub fn read_versions(body: &str) -> Option<Vec<Release>> {
    let value: Value = serde_json::from_str(body).ok()?;
    if value.get("tag_name").is_some() {
        return github_release(&value).map(|r| vec![r]);
    }
    let top = site_release(&value)?;
    let mut list: Vec<Release> = value
        .get("versions")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(site_release).collect())
        .unwrap_or_default();
    if !list.iter().any(|r| r.version == top.version) {
        list.insert(0, top);
    }
    Some(list)
}

/// One version in the site's shape: `version`, `date`, `notes` and `files`.
fn site_release(value: &Value) -> Option<Release> {
    let version = value.get("version")?.as_str()?.trim().to_string();
    parse_version(&version)?;
    let mut files = BTreeMap::new();
    for (kind, f) in value.get("files")?.as_object()? {
        let download = Download {
            file: f.get("file").and_then(Value::as_str).unwrap_or_default().to_string(),
            url: f.get("url")?.as_str()?.to_string(),
            bytes: f.get("bytes")?.as_u64()?,
            sha256: f.get("sha256")?.as_str()?.to_ascii_lowercase(),
        };
        files.insert(kind.clone(), download);
    }
    let text = |k: &str| value.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    Some(Release { version, date: text("date"), notes: text("notes"), files })
}

/// A GitHub release, whose assets carry their SHA-256 in `digest`. An asset without one is left out,
/// because nothing unchecked is installed.
fn github_release(value: &Value) -> Option<Release> {
    let version = value.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    parse_version(&version)?;
    let mut files = BTreeMap::new();
    for asset in value.get("assets").and_then(Value::as_array).into_iter().flatten() {
        let name = asset.get("name").and_then(Value::as_str).unwrap_or_default();
        let (Some(kind), Some(digest)) = (kind_of_file(name), asset.get("digest").and_then(Value::as_str)) else {
            continue;
        };
        let Some(sha) = digest.strip_prefix("sha256:") else { continue };
        let (Some(url), Some(bytes)) =
            (asset.get("browser_download_url").and_then(Value::as_str), asset.get("size").and_then(Value::as_u64))
        else {
            continue;
        };
        files.insert(
            kind.into(),
            Download { file: name.into(), url: url.into(), bytes, sha256: sha.to_ascii_lowercase() },
        );
    }
    let text = |k: &str| value.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let date = text("published_at").chars().take(10).collect();
    Some(Release { version, date, notes: text("html_url"), files })
}

/// The kind of a release file from its name, the way `tools/release.ps1` names them.
pub fn kind_of_file(name: &str) -> Option<&'static str> {
    if name.starts_with("TermoilSetup-") && name.ends_with("-x64.exe") {
        Some("windows")
    } else if name.ends_with("-macos-universal.dmg") {
        Some("macos")
    } else if name.ends_with("_amd64.deb") {
        Some("deb")
    } else if name.ends_with("-x86_64.AppImage") {
        Some("appimage")
    } else {
        None
    }
}

/// Asks a channel's sources in order and answers the first list that reads, newest first.
pub fn fetch_channel(channel: UpdateChannel, timeout: Duration) -> Result<Vec<Release>, String> {
    let mut problems = Vec::new();
    for url in sources(channel) {
        match fetch::get_text(&url, timeout) {
            Ok(body) => match read_versions(&body) {
                Some(list) if !list.is_empty() => return Ok(sorted(list)),
                _ => problems.push(format!("{url} did not answer with a release list")),
            },
            Err(e) => problems.push(format!("{url}: {e}")),
        }
    }
    Err(problems.join("; "))
}

/// The list newest first by version number, each version once.
fn sorted(mut list: Vec<Release>) -> Vec<Release> {
    list.sort_by_key(|r| std::cmp::Reverse(parse_version(&r.version)));
    list.dedup_by(|a, b| a.version == b.version);
    list
}

/// A version's numbers, `1.4.1` as `[1, 4, 1]`. A leading `v` is allowed.
pub fn parse_version(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches('v');
    let parts: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty() && p.len() <= 4)
}

/// Whether `found` is a later version than `current`.
pub fn is_newer(current: &str, found: &str) -> bool {
    match (parse_version(current), parse_version(found)) {
        (Some(c), Some(f)) => f > c,
        _ => false,
    }
}

/// The version of this build.
pub fn current_version() -> &'static str {
    termoil_protocol::VERSION
}

/// Whether a daily check is due: never checked, or checked a day or more ago, or the clock went back.
pub fn is_due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(t) => now < t || now - t >= DAY.as_secs(),
    }
}

/// The time the last automatic check started, from the stamp in `folder`.
pub fn last_checked(folder: &Path) -> Option<u64> {
    std::fs::read_to_string(folder.join(CHECKED)).ok()?.trim().parse().ok()
}

/// Writes the stamp before the request goes, so two windows starting together do not both ask.
pub fn note_checked(folder: &Path, now: u64) {
    let _ = std::fs::create_dir_all(folder);
    let _ = std::fs::write(folder.join(CHECKED), now.to_string());
}

/// Seconds since the Unix epoch.
pub fn now_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests;
