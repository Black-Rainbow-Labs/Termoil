# Termoil documentation

This folder holds Termoil's documentation. Each page can be read on its own.

The tables list the pages in reading order: first how to use Termoil, then how it performs and what it supports, then
how it is built, then the design documents and the research behind them.

## Using Termoil

| | Page | What it covers |
|---|---|---|
| 1 | [Getting started](getting-started.md) | Installing on Windows, macOS and Linux, the first window, the keys to learn first, shell integration and `termoil ssh` |
| 2 | [Configuration](configuration.md) | The settings file, live reload, themes, fonts, the window, the tab bar, keybindings and key tables |
| 3 | [Commands](commands.md) | Every action, with its key, its command line form, its IPC message and its MCP tool. Generated from the catalogue |
| 4 | [Scripting and AI agents](scripting-and-agents.md) | The action catalogue, the `termoil` command line, the IPC socket, the MCP server and separate instances |
| 5 | [Custom shaders](shaders.md) | Writing a WGSL shader that runs over every frame, with two examples in [shaders/](shaders/) |
| 6 | [Glossary](glossary.md) | The terminal terms these pages use, each in a sentence or two |

## Measurements and support

| | Page | What it covers |
|---|---|---|
| 7 | [Performance](performance.md) | Throughput, latency, memory, start up and frame time, measured against the gates, and the whole application against Ghostty |
| 8 | [Feature matrix](feature-matrix.md) | Every feature from the parity study, where it lives in the code, the test that covers it, and what is partial or deferred |

## How it is built

| | Page | What it covers |
|---|---|---|
| 9 | [Architecture overview](architecture-overview.md) | The crates, the threads, how output becomes pixels and how a key reaches the program, in one page with no Rust |
| 10 | [Repository and building](repository.md) | Each crate, building, the tests and pixel goldens, the rules the code follows, and how a release is made |

## Design documents

| | Page | What it covers |
|---|---|---|
| 11 | [Product requirements](PRD.md) | The problem, the users, the feature list with must, should and later, and the gates for 1.0 |
| 12 | [Technical design](TDD.md) | Crates, threads, the page based terminal core, the renderers, PTY and ConPTY, fonts, IPC, security, risks and milestones |
| 13 | [Test suite](TEST-SUITE.md) | Every category of test the implementation builds, with ids the other documents refer to |

## Decisions

Each decision was settled by a measurement, and each file has the numbers, the machine and the commit.

| | Page | What it covers |
|---|---|---|
| 14 | [Decisions index](decisions/README.md) | Which milestone wrote which decision |
| 15 | [Throughput](decisions/throughput.md) | The P1 gate stream and number, from a comparison with Ghostty 1.4 and alacritty_terminal |
| 16 | [Parser](decisions/parser.md) | Why Termoil has its own parser instead of `vte` |
| 17 | [Renderer](decisions/renderer.md) | The latency and frame time measurements behind the renderer choice |
| 18 | [ConPTY](decisions/conpty.md) | What reaches the terminal through the bundled and the built in ConPTY |
| 19 | [Fonts](decisions/fonts.md) | The Windows font fallback matrix |
| 20 | [Start up](decisions/startup.md) | Where the start up measurement ends on each platform |
| 21 | [Efficiency](decisions/efficiency.md) | Memory and processor time against Ghostty, and why the window draws on the processor |
| 22 | [Detaching a window](decisions/detach.md) | What `termoil-gui --separate` does and why |

## Research

Written on 1 November 2026, before the first line of code.

| | Page | What it covers |
|---|---|---|
| 23 | [Feature parity](research/feature-parity.md) | Six terminal emulators feature by feature, and the specifications to implement |
| 24 | [Rust crates and vello](research/rust-crates-and-vello.md) | The crates considered for each part, and the vello question |
| 25 | [Architectures](research/architectures.md) | How Ghostty, Alacritty, WezTerm, Kitty, Windows Terminal, Rio and Zed are built |
| 26 | [Testing corpora](research/testing-corpora.md) | How terminal emulators are tested, and which corpora to use |
