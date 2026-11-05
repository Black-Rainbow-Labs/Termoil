# CLAUDE.md

Guidance for agents working in this repository.

## Start here

Read `docs/TDD.md` first, then the sections your ticket names. The requirements are `docs/PRD.md` and the tests you must build are `docs/TEST-SUITE.md`. The research the design rests on is in `docs/research/`; read the section the TDD cites before changing a decision.

## Conventions

- Rust, the toolchain in `rust-toolchain.toml`, `cargo fmt` and `cargo clippy` clean, `cargo deny` clean.
- License MIT OR Apache-2.0. A dependency must be MIT, Apache 2.0, BSD, Zlib, Unicode or OFL. Nothing is read from Kitty's source; its protocols are implemented from the published specifications.
- Functions have a doc comment stating intent, parameters on one line, and stay under about 50 lines. No `this` style state; pass state in.
- `termoil-vt`, `termoil-input`, `termoil-pty` and `termoil-protocol` have no window, GPU or font dependency. CI builds `termoil-vt` for wasm to enforce it.
- `unsafe` lives in the page arena module and the SIMD scan only, and both are covered by Miri and a fuzz target.
- Every tunable number is in the config struct with a default, never a literal in the code path.
- Agent outputs go under `_agent_output/`, which is gitignored.

## A person and an agent reach the same code

Every action is a row in the catalogue (`termoil-protocol`) and `App::run_action` is the one place an action becomes a change. Keybindings, the command palette, `termoil +command` and the IPC socket all go through it. The MCP tool list and `docs/commands.md` are generated from the catalogue and tested; never write a tool definition by hand. A feature with a control and no catalogue row is not finished.

## Tests

The suite is specified in `docs/TEST-SUITE.md`. Expected outputs change only with `TERMOIL_BLESS=1`, and the commit says which rule changed. Every bug fix adds a case first. Tests wait on markers, never on sleeps. Pixel goldens live under `corpus/render/<platform>/`; a failure writes `.new.png` and `.diff.png` beside the accepted image, which are gitignored.

## Verification

Functional verification runs here: the end to end suite spawns the real shell on this machine. Visual judgment of the window is done by looking at a release build installed on this machine, not at screenshots of a dev build.

Each milestone ends with `pwsh tools/release.ps1`, which bumps, builds, installs here, tags, pushes and publishes the GitHub release.

## Design questions

If the TDD does not answer a design question, file a ticket on the Tasks board titled `Fable: <question>` with model `claude-fable-5-1[1m]`, assignee `claude`, and agentProjectId `termoil`. Then continue with the parts of your ticket that do not depend on the answer. Measured decisions (the renderer at M2, ConPTY passthrough at M1) are written to `docs/decisions/<topic>.md` with the numbers, and the TDD is updated to match.
