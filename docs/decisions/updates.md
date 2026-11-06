# Updates: nightly and stable channels, the update check, and installing an update

task-2524. This is a design reading, not a measurement: it says what is built and why, and the TDD (sections 4.10 and 7) and the PRD (section 6) are updated to match.

## The ask

> For termoil, we want auto check for updates and a release system similar to Unluminous. That way users can be notified when updates are available. We also want to ensure Unluminous always pulls in the latest during its builds. We also want to ensure that Unluminous.com/termoil is updated during release so it has the latest release.

## What happens before this ticket

- `tools/release.ps1` tests, bumps, builds the Windows installer, the macOS dmg and the Linux AppImage and .deb, installs on this machine, tags, pushes and creates a GitHub release. Every release is a GitHub prerelease unless `-Stable` is passed. Only 1.0.0 was ever a full release.
- unluminous.com/termoil shows whatever `node scripts/termoil/sync-termoil-release.mjs [tag]` was last run against, by hand, in the black-rainbow-labs-sites checkout. It wrote `src/data/termoil-release.json` (the page) and `public/termoil/latest.json` (the file the agent install prompt reads). Nothing ran it during a release, so the page stays on an old version until somebody remembers.
- Termoil has a `[updates] check` key, a boolean, off, with no code behind it. Nobody running Termoil is ever told a newer one exists.
- Unluminous takes `termoil-vt`, `termoil-pty` and `termoil-input` from git with `rev = "<commit>"` in its workspace `Cargo.toml`. The commit only moves when somebody edits all three lines.

## Design summary

The design copies Unluminous's task-2328 and task-2063 designs (`tasks/task-2328-nightly-and-stable-releases-tdd.md` and the update section of Unluminous's CLAUDE.md), with changes where Termoil differs: it has no menu bar, it has a command line and an MCP server for every action, it ships four packages instead of two, and its packages are served from GitHub instead of from the site.

| Part | What it does |
|---|---|
| Channels | `nightly` holds every release `tools/release.ps1` cuts. `stable` holds only the versions somebody promoted with `tools/promote.ps1` after using them. |
| unluminous.com | `/termoil/nightly.json`, `/termoil/stable.json` and `/termoil/latest.json` (the newest stable, the shape it always had). The page offers the stable version and names the newest nightly beside it. |
| Termoil | Checks its channel once a day by default, shows a desktop notification when a newer version is there, and installs a chosen version with a check of its size and SHA-256 before anything runs. Every part is a catalogue action. |
| Release | `tools/release.ps1` publishes to nightly and then publishes the site. `tools/promote.ps1` makes a nightly stable, building nothing. |
| Unluminous | Its three Termoil dependencies follow Termoil's `main` branch, and both Unluminous release scripts run `cargo update` for them before the suite, so each Unluminous release builds against the newest Termoil. |

## Channels

| Channel | What it holds | Who is offered it |
|---|---|---|
| `nightly` | Every release `tools/release.ps1` cuts, which is every finished ticket. | A Termoil whose `updates.channel` is `nightly`. |
| `stable` | Versions promoted with `tools/promote.ps1`. | Every other Termoil, which is everybody by default. |

Versions stay plain numbers. A stable version is a nightly that was promoted, with the same bytes and the same SHA-256, so `termoil +version` means the same build whichever channel delivered it. Promotion builds nothing: rebuilding for stable would ship bytes nobody ran.

On GitHub a nightly is a prerelease and a stable version is a full release marked latest. GitHub's `releases/latest` never answers with a prerelease, so it also means the newest stable version.

## What unluminous.com publishes

All three files are written by `scripts/termoil/sync-termoil-release.mjs` in the site and served from `public/termoil/`.

```json
{
  "product": "Termoil",
  "channel": "nightly",
  "version": "1.5.0",
  "date": "2026-11-06",
  "notes": "https://github.com/Black-Rainbow-Labs/Termoil/releases/tag/v1.5.0",
  "page": "https://unluminous.com/termoil",
  "install": "https://unluminous.com/termoil/install.md",
  "files": {
    "windows":  { "file": "TermoilSetup-1.5.0-x64.exe", "url": "https://github.com/...", "bytes": 7901030, "sha256": "..." },
    "macos":    { "file": "termoil-1.5.0-macos-universal.dmg", "url": "...", "bytes": 0, "sha256": "..." },
    "deb":      { "file": "termoil_1.5.0_amd64.deb", "url": "...", "bytes": 0, "sha256": "..." },
    "appimage": { "file": "termoil-1.5.0-x86_64.AppImage", "url": "...", "bytes": 0, "sha256": "..." }
  },
  "versions": [ { "version": "1.5.0", "date": "2026-11-06", "notes": "...", "files": { } } ]
}
```

- The top level is the newest version on the channel. `versions` lists the newest ten on it, newest first, each in the same shape as the top level without `versions`.
- `latest.json` keeps the shape it had in task-2491 (`product`, `version`, `page`, `install`, `files`) and means the newest stable version. The agent install prompt reads it, so an agent following the prompt installs stable.
- The channel history is `src/data/termoil-channels.json`, committed in the site repository beside `termoil-release.json`, which was already committed. The first time it is missing it is seeded from the GitHub releases: the five newest releases with all four files go on both channels, because those were the public releases up to now.
- `--channel nightly` adds the version to the top of the nightly list and leaves the page and `latest.json` on stable. `--channel stable` moves the version from the nightly list (the same measured files) to the top of the stable list and rewrites the page data and `latest.json`. A version is never in a list twice; adding one again moves it to the top.
- The files are measured from the bytes GitHub serves, as task-2491 did, and a size that differs from GitHub's own is refused.
- `scripts/termoil/publish-termoil.ps1 -Channel nightly|stable -Version x.y.z` runs the sync, builds the site, deploys it with `tools/deploy.mjs unluminous --skip-build`, and then asks `https://unluminous.com/termoil/<channel>.json` what it says, failing when the version is not the one published.

## The release scripts

- `tools/release.ps1` creates the GitHub release as a prerelease (as it already did), then runs `publish-termoil.ps1 -Channel nightly -Version <new>` in the site checkout (`$env:TERMOIL_SITE` or `C:/jason/dev/black-rainbow-labs-sites/sites/unluminous`). The GitHub release is already public when the site step runs, so a site failure does not fail the release: the script prints the exact command to run again and exits 1 at the end. `-SkipSite` leaves the site alone. `-Stable` is kept and now means "release and promote in one go", which runs the site with `-Channel stable`.
- `tools/promote.ps1 [-Version x.y.z]` is new. With no version it takes the newest nightly from `https://unluminous.com/termoil/nightly.json`. It refuses a version that is not on the nightly channel, runs `publish-termoil.ps1 -Channel stable -Version x.y.z`, clears `prerelease` on the GitHub release and marks it latest, and checks that `stable.json` and `latest.json` both name the version. Nothing is built. An agent does not promote unless Jason asks.

The working rule: a ticket ships a nightly, Jason runs the nightly, and when it has held up `pwsh tools/promote.ps1` makes it stable.

## Termoil

### Settings

| Key | Values | Default | Meaning |
|---|---|---|---|
| `updates.check` | `daily`, `start`, `off` | `daily` | `daily` asks at most once a day, counted across every Termoil process on the machine. `start` asks every time the first window opens. `off` sends nothing. The old `true` and `false` are read as `daily` and `off`. |
| `updates.channel` | `stable`, `nightly` | `stable` | Which channel every check asks. |
| `updates.skip` | a version or empty | empty | The daily check does not announce this version. A later one is announced. |

The day is counted in `update-checked.txt` in the data folder, written before the request so two windows starting together do not both ask. What is sent is one unauthenticated `GET` of a static file; it says nothing about the person or the machine beyond what any HTTP request says.

`updates.check` defaults to `daily` because the ticket asks for an automatic check. That reverses the PRD's "off until the user turns it on", which is updated to say so.

### Where a check asks

| Channel | Sources, in order |
|---|---|
| `stable` | `https://unluminous.com/termoil/stable.json`, then `https://unluminous.com/termoil/latest.json` (a site not yet rebuilt), then `https://api.github.com/repos/Black-Rainbow-Labs/Termoil/releases/latest`, whose asset `digest` fields give the SHA-256. |
| `nightly` | `https://unluminous.com/termoil/nightly.json` only. GitHub's latest release is the stable one. |

`TERMOIL_RELEASES_STABLE` and `TERMOIL_RELEASES_NIGHTLY` replace a channel's sources with one URL, which is how the tests point a Termoil at a local server.

The request is made with the system's `curl` (`%SystemRoot%\System32\curl.exe` on Windows 10 and 11, `/usr/bin/curl` on macOS, `curl` on the path on Linux) with a 15 second limit and no console window. That keeps an HTTP client and a TLS stack out of the build: the TLS crates either pull in OpenSSL on Linux, which the AppImage would then have to carry, or have certificate root licenses outside the allowed list. A machine without curl gets "curl was not found" as the answer to a check, and nothing else changes.

### Which file a platform installs

| Platform | File | How it is installed |
|---|---|---|
| Windows | `windows` | The Inno Setup installer, run silently into the folder Termoil is running from. |
| macOS | `macos` | The dmg is attached, `Termoil.app` is copied beside the running bundle, the two are swapped, and the dmg is detached. |
| Linux, running as an AppImage (`$APPIMAGE` is set) | `appimage` | The new AppImage replaces the file `$APPIMAGE` names. |
| Linux, installed from the .deb | `deb` | Not installed: it needs root. The answer names the URL and the release page is opened. |

A build that no installer put where it is (`target/release`, or any folder without the installer's uninstaller beside it on Windows) is not updated in place either: the answer names the download and the release page is opened, because installing would put the new version somewhere else and start that one.

### Actions

Every part is a catalogue row, so the palette, a keybinding, `termoil +<name>` and the MCP server reach the same code.

| Action | Parameters | What it does |
|---|---|---|
| `check-for-updates` | `channel` | Asks the channel and answers `{ current, channel, latest, newer, notes, file, bytes }`. Run from the palette, it shows a notification with the answer either way. |
| `list-updates` | `channel` | The newest five versions on each channel (or the one named), newest first, the running one marked. |
| `install-update` | `version`, `channel`, `restart` | Downloads the version (the newest on the channel when none is named), checks its size and SHA-256, and with `restart` (the default) saves the session, closes Termoil, installs, and starts the new version with the same windows. With `restart` false it answers the path of the checked file and does nothing else. |
| `skip-update` | `version` | Writes `updates.skip`, the version the last check found when none is named. |

The work that waits on the network runs on a thread. A new `Outcome::Later` carries it: the IPC server runs it on the connection's thread, as it already does for `wait-prompt`, and the palette and keybindings run it on a thread of its own. The window's thread never waits on the network.

`check-for-updates`, `list-updates` and `install-update` with `restart` false also answer from `termoil +<name>` when no Termoil is running, like `version` does.

### Installing and restarting

Nothing unchecked is run. The file is downloaded into `termoil-update-<version>` in the temporary folder and compared with the manifest's size and SHA-256; a file that does not match is deleted and the action fails with both values.

With `restart`:

1. The window's thread writes the session to `update-session.json` in the data folder with `save-session`, whatever `session.restore` says.
2. `termoil.exe` is copied into the download folder, because the installer replaces the one in the install folder, and the copy is started as `termoil --apply-update <file> --pid <this process> --into <install folder>`.
3. Termoil quits.
4. The helper waits for the process to end, installs (the table above), and starts the new `termoil-gui` with `TERMOIL_RESTORE_SESSION=<the session file>` in its environment. `termoil-gui` restores that file once at start when the variable is set, then deletes it. An environment variable is used rather than an argument because an older Termoil, installed by choosing an older version, does not know the argument and would refuse to start.
5. The helper writes `apply-update.log` beside the downloaded file, because it has no window and no console.

On Windows the installer runs as `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /DIR=<install folder>`, with `/CURRENTUSER` for an install under the user's profile and `/ALLUSERS` for one under `Program Files`. Any version can be installed over any other, newer or older: `termoil.iss` marks every file `ignoreversion`.

### The notice

When a daily or start check finds a version newer than the running one, and it is not `updates.skip`, Termoil shows a desktop notification: "Termoil 1.5.0 is available" with "Run Install Update from the command palette, or termoil +install-update." The notification goes through `platform::notify`, which already shows command finished notices. Termoil has no menu bar or toast of its own to put buttons on, so the palette is the button.

## Unluminous

- The workspace `Cargo.toml` names the three crates with `branch = "main"` instead of `rev`. Cargo.lock still records the exact commit, so a build is reproducible until the lock moves.
- `tools/release.ps1` and `tools/release.sh` run `cargo update -p termoil-vt -p termoil-pty -p termoil-input` beside the existing `cargo update -p inillucent-driver`, before the suite, so what is tested is what ships, and Cargo.lock goes into the version commit.
- The Unluminous CLAUDE.md rule "taken here by moving the `rev`" becomes "taken by the next Unluminous release".

`main` is used instead of the newest Termoil release tag because Termoil merges a ticket and releases it in the same run, so `main` and the newest nightly differ only while a release is in progress, and following a tag would need the release script to rewrite three lines of `Cargo.toml`.

## What is left out

- Buttons on the desktop notification. Windows toast buttons need a COM activator registered by the installer, and macOS and Linux each need their own. The palette action is the button.
- Delta updates. The installer is under 20 MB.
- Automatic promotion after a number of days. Promotion is where a person decides a build is good.
- Installing the .deb. It needs root, and a terminal emulator asking for a password to update itself is the wrong place for that.

## Tests

- `update` module unit tests: reading a channel manifest with and without `versions`, the GitHub release shape with `digest`, version comparison, the daily stamp, each channel's sources and the environment overrides, the file for each platform.
- `crates/termoil-app/tests/updates.rs`: a local HTTP server serves a stable and a nightly manifest and a fake package. A headless instance answers `check-for-updates` and `list-updates` for both channels, `install-update --restart false` downloads and checks the package, refuses a package whose SHA-256 is wrong and deletes it, and refuses a version that is on no channel. `skip-update` writes `updates.skip`.
- Config: `true`, `false`, `daily`, `start`, `off` and a bad value for `updates.check`.
- The site: `node --test scripts/termoil/channels.test.mjs` over seeding, adding to a channel, moving a version to the top, cutting to ten and the three manifests agreeing.
- End to end, once, on this machine: release N with this ticket, release N+1 with `-SkipInstall`, and have the installed N run `termoil +install-update --channel nightly`, which must leave N+1 installed and running with the same windows.
