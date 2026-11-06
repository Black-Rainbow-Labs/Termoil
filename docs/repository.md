# Repository and building

This page is for people who want to build Termoil, run its tests or change it. It lists the crates, the commands to
build and test, how the pixel tests work, and how a release is made.

## The crates

| Crate | What it does | Depends on a window, GPU or font |
|---|---|---|
| `termoil-vt` | the terminal: parser, pages, screen, scrollback, modes, selection, search, the image store, Unicode tables | no |
| `termoil-input` | turns key and mouse events into the bytes a program expects, in every keyboard and mouse mode | no |
| `termoil-pty` | starts a program on a pseudo terminal: openpty on macOS and Linux, ConPTY on Windows | no |
| `termoil-protocol` | the action catalogue and the IPC messages | no |
| `termoil-font` | finds fonts, falls back, shapes text, rasterizes glyphs, and draws box and block characters | yes |
| `termoil-render` | the `Renderer` trait, the frame builder, and the CPU and GPU renderers | yes |
| `termoil-app` | the `termoil-gui` program: windows, tabs, splits, settings, the IPC server | yes |
| `termoil-cli` | the `termoil` program: `+commands`, `termoil ssh` and the MCP server | through `termoil-app` |

`tools/` holds the programs that are not shipped: `bench` (the benchmarks in [Performance](performance.md)),
`headless` (a terminal with no window, which the conformance suites drive), `unicode-tables` (generates the width
and grapheme tables), `themes` (converts the theme collections), and the release and installer scripts.

The first four crates have no window, GPU or font dependency. CI builds them for `wasm32-unknown-unknown` to keep it
that way. [Architecture overview](architecture-overview.md) shows how the crates fit together.

## Building

The Rust version is pinned in `rust-toolchain.toml`, and `rustup` installs it on the first build.

```sh
cargo build --release -p termoil-app -p termoil-cli
```

`target/release` then holds `termoil-gui` and `termoil`. On Linux the window and font crates need the development
packages for fontconfig, freetype, xkbcommon, Wayland and X11:

```sh
sudo apt-get install -y libfontconfig1-dev libfreetype-dev libxkbcommon-dev libwayland-dev libx11-dev \
  libxcursor-dev libxrandr-dev libxi-dev pkg-config
```

On Windows, `pwsh tools/fetch-conpty.ps1` downloads the ConPTY pair that the installer ships. Without it Termoil uses
the console host built into Windows, which removes image sequences.

## Tests

```sh
cargo test --workspace
```

runs the Rust tests. vttest and esctest2 run through `tools/vttest/run.sh` and `tools/esctest/run.sh` on Linux, and
the fuzz targets through `node tools/fuzz-corpus.cjs run 60`; CI runs all three. The suite is specified in
[the test suite](TEST-SUITE.md), where each test has an id the other documents refer to. The main groups:

| Group | What it checks |
|---|---|
| Conformance | Alacritty's reference tests, the esctest2 cases Ghostty 1.4 passes, vttest screens, and Termoil's own scripts in `corpus/vt/*.test` |
| Unicode | the width and grapheme tables against the Unicode data files |
| Input | every key in every kitty keyboard flag combination, and every mouse mode and encoding |
| Rendering | the frame builder's records as data (R1), and pixel goldens rendered on the CPU (R2) |
| End to end | real shells started on a PTY, the command line against a running instance, and a real window driven through its event handlers (E6) |
| Fuzzing | twelve `cargo fuzz` targets under `fuzz/`, from the parser to the settings file |
| Benchmarks | throughput, allocation, memory, latency and start up gates, run by `tools/bench` |

Tests wait on markers in the output, never on a fixed sleep.

### Pixel goldens

The R2 tests render a scene with the CPU renderer and the vendored JetBrains Mono, and compare it with the accepted
image in `corpus/render/<platform>/`. A failure writes `<name>.new.png` and `<name>.diff.png` beside the accepted
image; both are gitignored. To accept a change on purpose, run the test with `TERMOIL_BLESS=1` and say in the commit
which rule changed. CI uploads the candidate images from Linux and macOS, so those platforms' goldens are taken from
the CI run.

## Rules the code follows

- Every action is a row in the catalogue in `termoil-protocol`, and `App::run_action` is the only place an action
  becomes a change. A control with no catalogue row is not finished.
- `docs/commands.md` and `docs/mcp-tools.json` are generated from the catalogue, and a test fails when they differ.
- `unsafe` is allowed only in the page arena and the SIMD scan, and both are covered by Miri and a fuzz target.
- Every number a person might want to tune is a setting with a default.
- Dependencies must be MIT, Apache 2.0, BSD, Zlib, Unicode or OFL licensed, and `cargo deny` checks it.
- Functions have a doc comment that says what they are for and stay under about 50 lines.
- Every bug fix adds a test that fails before the fix.

## Releases

`pwsh tools/release.ps1` makes a release from a clean checkout: it runs the tests, bumps the version in
`Cargo.toml`, builds, makes the Windows installer, installs it on the build machine and checks the installed version,
builds the Linux AppImage and `.deb` in WSL and the macOS dmg, tags, pushes, publishes the GitHub release with every
package attached, downloads each one back to compare its SHA256, writes the winget manifest, and publishes
unluminous.com/termoil. `-Part minor` makes a minor release and `-WhatIf` prints the plan without changing anything.

Every release is a **nightly**: a GitHub prerelease, listed in `unluminous.com/termoil/nightly.json`. The page and
`stable.json` stay on the stable version. After a nightly has been used, `pwsh tools/promote.ps1 [-Version x.y.z]`
makes it **stable**: it publishes the same files on the stable channel and marks the GitHub release as the latest full
release. Nothing is built to promote. Termoil checks `updates.channel` (stable by default) once a day and offers a newer
version with a notification; `install-update` downloads it, checks its size and SHA-256, and restarts into it with the
same windows. `docs/decisions/updates.md` is the design.

## The design documents

| Document | What it holds |
|---|---|
| [PRD](PRD.md) | the problem, the users, the features with must, should and later, and the gates for 1.0 |
| [TDD](TDD.md) | the technical design: crates, threads, the terminal core, rendering, PTY, fonts, IPC, risks |
| [Test suite](TEST-SUITE.md) | every category of test, with ids |
| [Decisions](decisions/README.md) | each measured decision, with its numbers |
| [Research](research/feature-parity.md) | the feature parity study, the crate survey, other terminals' architectures, testing corpora |
