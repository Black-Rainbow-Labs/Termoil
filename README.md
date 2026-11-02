# Termoil

Termoil is a terminal emulator written from scratch in Rust for Windows, macOS and Linux. Its core is a headless library with no window, GPU or font dependency, so the same core serves the Termoil window, an IDE's terminal tile and an agent's scripted terminal. Rendering goes through vello behind one renderer trait. The target is parity with Ghostty, Alacritty, WezTerm, Kitty and Windows Terminal on features and performance.


## Documents

- [Product requirements](docs/PRD.md): the problem, the users, the feature list with must, should and later, the gates for 1.0.
- [Technical design](docs/TDD.md): crates, threads, the page based terminal core, the renderer trait and the vello decision, PTY and ConPTY, fonts, the application, IPC and the catalogue, risks, milestones.
- [Test suite](docs/TEST-SUITE.md): every category of test the implementation builds, with ids the TDD and PRD refer to.
- Research, dated 2026-11-01:
  - [Feature parity across six emulators and the specifications to implement](docs/research/feature-parity.md)
  - [Rust crates and the vello question](docs/research/rust-crates-and-vello.md)
  - [Architectures of Ghostty, Alacritty, WezTerm, Kitty, Windows Terminal, Rio and Zed](docs/research/architectures.md)
  - [How terminal emulators are tested, and which corpora to use](docs/research/testing-corpora.md)

## Status

Design only. No code yet; the implementation is tracked as a ticket on the AI Service Tasks board filed from task-2456, with the milestones in the TDD section 11 as its phases. Each milestone ends with a release through `tools/release.ps1`.

## Planned layout

```
termoil/
  crates/
    termoil-vt         parser, pages, screen, modes, selection, search, graphics, unicode
    termoil-input      key and mouse encoders
    termoil-pty        openpty, ConPTY, spawn, exit
    termoil-protocol   IPC messages, catalogue
    termoil-font       discovery, fallback, shaping, raster, sprites
    termoil-render     Renderer trait, frame builder, vello_gpu, vello_cpu, atlas
    termoil-app        windows, tabs, splits, config, IPC server
    termoil-cli        termoil +command, termoil ssh, MCP tool list
  shell-integration/    per shell scripts, terminfo source
  corpus/               vendored test corpora with licences, conformance cases, accepted images
  tools/                unicode-tables, release.ps1, bench runners, latency harness
  installer/            windows, macos, linux
  docs/                 PRD, TDD, TEST-SUITE, research, decisions, commands.md
```

## Licence

MIT OR Apache-2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`.
