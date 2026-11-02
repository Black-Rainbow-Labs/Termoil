# Termoil: product requirements

Termoil is a terminal emulator written from scratch in Rust, rendered with vello, that matches Ghostty, Alacritty, WezTerm, Kitty and Windows Terminal on features and performance, and runs on Windows, macOS and Linux from one code base.

Written 2026-11-01 for task-2456. The technical design is `docs/TDD.md`. The test suite the implementation must build is `docs/TEST-SUITE.md`. The research behind both is in `docs/research/`.

## 1. Problem statement

Every terminal emulator in daily use here is a compromise.

- Ghostty has the best core (grapheme aware cells, full kitty graphics, shell integration that moves the cursor on click, 50 MB compressed scrollback) and dropped its Windows GUI roadmap in 1.4.0 (2026-10-27). This machine is Windows. The agent terminals the Tasks board opens, the Service Manager consoles and the daily shell all run on Windows, so the best core is the one we cannot run.
- Windows Terminal is the only one of the five built for Windows, and it is Windows only. It has no kitty graphics, no multiplexer, a C++ and WinUI code base nobody here can embed, and ConPTY between it and every shell.
- Alacritty is small and stable, and by design has no tabs, no splits, no ligatures and no images.
- WezTerm has the widest feature list, a Lua API and a built in multiplexer, and its last tagged release is dated 2024-02-03. Only nightly builds exist after that.
- Kitty is GPL 3, has no native Windows build, and its code cannot be reused in a MIT or Apache project.

Underneath the product gap there is an embedding gap. Unluminous, the IDE in `C:/jason/dev/unluminous`, embeds a terminal built on `alacritty_terminal`, and every feature Alacritty does not have (graphics, grapheme clusters, semantic prompts) is a feature the IDE's terminal cannot have either. The Tasks board's agent terminals are Windows Terminal windows driven through ConPTY by `backend/terminal-daemon.cjs`, which has its own 1024 byte ConPTY write limit problems (see memory: typed messages truncate). A terminal core we own fixes both.

Why now. Rust has the pieces: `vello` (0.6, sparse strips, a CPU path for machines with no usable GPU), `parley` and `skrifa` for shaping and rasterising text, `wgpu` on every platform, `winit` with IME, `portable-pty` with ConPTY. Ghostty has published the design of a modern terminal core in detail. Nothing in that design is zig specific. The Windows gap is open and Ghostty just said it will stay open.

## 2. Goals and success metrics

Each goal has a number the test suite measures. The numbers are gates for the 1.0 release, not aspirations.

| Goal | Measure | 1.0 gate |
|---|---|---|
| Correct emulation | esctest pass rate on the xterm profile, vttest menus 1 to 3 and 11 by inspection, Alacritty's ref tests replayed through our core | esctest: every test Ghostty passes; Alacritty ref tests: 100 % |
| Typing latency | Keypress to pixel, measured by the in repo typometer style harness on this machine (RTX 5090, 240 Hz capable display, 120 Hz test mode) | p50 under 8 ms, p99 under 16 ms at 120 Hz |
| Throughput | `vtebench` scenarios, bytes per second through the parser and screen on the `mixed` 1 GiB build log stream, and frames per second during a `cat` of that file | Parser plus screen: 250 MB/s or more on one core on this machine (set by measurement against Ghostty 1.4 and alacritty_terminal on the same bytes, `docs/decisions/throughput.md`); display never drops below 60 fps while input saturates |
| Memory | Resident set with 100 000 lines of 200 column scrollback in one tab; ten tabs idle | Under 150 MB and under 400 MB |
| Startup | Process start to first frame with the shell prompt visible, warm cache | Under 100 ms on Windows and Linux, under 150 ms on macOS |
| Parity | The feature matrix in `docs/research/feature-parity.md`, each row marked done, partial or deferred | Every "must" row in section 5 done |
| Embeddable | Unluminous builds against `termoil-core` with no UI dependency pulled in | A proof of concept branch in Unluminous compiles and runs the headless terminal tests |
| Agent reachable | Every action a person can take has a CLI or IPC form with the same code path | The catalogue test in the TDD passes |

Guardrails. No feature may regress typing latency past the gate. No feature may add a dependency with a licence other than MIT, Apache 2.0, BSD, Zlib, Unicode or OFL for fonts. Nothing from kitty's code base is read or copied; the protocols are reimplemented from their published specifications.

## 3. Users

### Jason, on Windows, running agents

The primary user. Opens a dozen terminals a day: shells, agent sessions that print tens of megabytes of tool output, `nvidia-smi` watches, log tails. Needs: tabs and splits in one window, a terminal that stays responsive while an agent floods it, search over everything that scrolled past, click to open a path or a URL, and shell integration that lets him jump between prompts and see which command failed. Also needs the terminal to be a first class citizen of the Tasks board: an agent terminal opened by a ticket should be a Termoil window or tab.

### An agent driving a terminal

Claude Code, Codex and opencode sessions run inside terminals, and other agents open terminals to run things. The agent needs a scriptable terminal: open a tab in a directory, send text, read the screen, wait for a prompt, close it. Today that is the ConPTY daemon in ai-service. Termoil gives it a CLI and an IPC socket with the same commands a person's keybindings reach.

### Unluminous

The IDE embeds a terminal tile. It needs the core as a library: a screen snapshot the painter reads, key encoding, a PTY session, and no window of its own. This is the same split as libghostty and the Ghostty app.

### Secondary: anyone on macOS or Linux

Termoil must run there with the same features, because an emulator that only works on one platform cannot claim parity with the group, and because macOS is where most Ghostty users are. These users are second in priority for 1.0 and first for the comparison press.

## 4. Jobs to be done

- As Jason, I want to split a tab three ways and have each pane keep up with an agent's output, so that watching four things at once does not need tmux.
- As Jason, I want to press a key and see the character on the next frame, so that the terminal never feels slower than the editor beside it.
- As Jason, I want to search 100 000 lines of scrollback and jump between prompts, so that finding the command that failed takes seconds.
- As Jason, I want to click a Windows path in agent output and have it open in Unluminous, so that the terminal and the editor act as one tool.
- As an agent, I want to open a Termoil tab in a directory, send keystrokes, and read the screen as text, so that I can run and observe tools without a separate PTY daemon.
- As an agent, I want the terminal to tell me when a command finished and with what exit code, so that I do not have to parse prompts.
- As Unluminous, I want to link a terminal core with no window dependency, so that the IDE's terminal gets kitty graphics and grapheme clusters without a second emulator.
- As a Linux or macOS user, I want the same config file and the same features, so that moving between machines costs nothing.

## 5. Scope

Each row is marked must (in 1.0), should (planned, may slip to 1.1) or later (not in 1.0, design must not block it). "Parity" means the row in `docs/research/feature-parity.md`.

### 5.1 Terminal core (must)

- A VT parser implementing ECMA-48, the xterm control sequences, and the DEC private modes the research lists, with every mode stack and per cell store bounded (kitty 0.49.2 and Alacritty 0.18 both fixed unbounded stacks; Termoil never has one).
- Cells that hold a grapheme cluster, not a code point. Unicode 17 widths, UAX 29 clusters, UTS 51 emoji sequences, mode 2027 grapheme clustering on request, DECRQM reporting for it.
- True colour, 256 colour, the 16 ANSI colours, SGR attributes including undercurl and coloured underlines, XTPUSHSGR and XTPOPSGR.
- Scrollback with a byte budget (default 50 MB per tab, the Ghostty 1.4 default), paged, with idle pages compressed. Reflow on resize.
- Alternate screen, scrolling regions, origin mode, insert and replace, tabs stops, soft and hard reset, DECSTR.
- Mouse modes 1000, 1002, 1003, 1005, 1006, 1015, 1016, focus events 1004, bracketed paste 2004, synchronized output 2026 with DECRQM answers.
- Keyboard: legacy xterm encoding, modifyOtherKeys, and the kitty keyboard protocol with all five flags and the push, pop and query sequences.
- OSC 0, 1, 2 (title and icon), OSC 4 and 10 to 19 including the `?` query form, OSC 7 (cwd), OSC 8 (hyperlinks, with id grouping), OSC 9 and OSC 99 (notifications), OSC 9;4 (progress), OSC 52 (clipboard, read disabled by default), OSC 133 (semantic prompts A, B, C, D with exit codes), OSC 1337 (iTerm2 subset: cwd, inline images).
- DA1, DA2, DA3, DSR, CPR, DECRQSS, XTVERSION, XTGETTCAP, DECRPM, and a terminfo entry `xterm-termoil` shipped with the binary.
- The kitty graphics protocol, complete as published: direct, file and shared memory transmission, PNG and raw RGB and RGBA, zlib, chunking, placements, z index, Unicode placeholders, animation, relative placements, deletion.
- DCS passthrough for tmux.
- Bell with visual and audible options.

### 5.2 Rendering (must)

- vello for rendering, over wgpu on Vulkan, Metal, DX12 and OpenGL as the platform allows. The vello project has three renderers in November 2026: `vello_cpu` (the one it calls most mature), `vello_gpu` (the sparse strips GPU renderer it calls its future, formerly `vello_hybrid`) and the compute renderer `vello` now under `research/`. Termoil used `vello_gpu` for the window and `vello_cpu` behind `softbuffer` when no adapter is usable; since task-2481 the window draws with Termoil's own CPU renderer, `RasterRenderer`, and starts no GPU device unless the GPU is asked for or a custom shader is set, because the GPU device alone cost more memory than a whole Ghostty window (`docs/decisions/efficiency.md`). Both sit behind one `Renderer` trait, and milestone M2 measures `vello_gpu` on a 200 by 60 grid at 120 Hz against the latency and frame time gates before anything is built on it. If it fails the gate, the contingency is a Termoil owned wgpu glyph atlas with instanced cell quads behind the same trait, which is what Alacritty, Kitty, Rio and WezTerm do; vello stays for the CPU path and for vector drawing (cursor shapes, underlines, images, the tab bar) either way.
- A glyph cache keyed by font, size, subpixel offset and style so a frame of 80 by 50 cells costs a fixed number of draw calls. Cell advances are forced to whole pixels so one cache slot per glyph per style is enough.
- Font shaping through parley and skrifa: ligatures (on by default, per font toggle), fallback across system fonts (DirectWrite on Windows, CoreText on macOS, fontconfig on Linux), colour emoji (COLRv1, CBDT and sbix bitmaps), hinting choices, subpixel positioning, bold and italic synthesis when a face is missing.
- Cursor styles block, bar, underline, hollow when unfocused, blink, and the colour inversion rule.
- Window opacity, background blur where the compositor supports it, padding, a configurable minimum contrast ratio for foreground against background.
- Synchronized output honoured: a 2026 block paints as one frame.
- Frame pacing: the renderer paints on damage at the display rate and never faster; while output streams and nothing was typed or clicked in the last quarter second it paints at most `window.output_fps` (60) times a second, because every row changes on every frame then (task-2481). An idle terminal uses no GPU and no CPU.
- Images from the kitty graphics protocol composited into the cell grid with correct z order and clipping, and sixel decoded into the same image store.

### 5.3 Application (must)

- Windows, macOS and Linux (Wayland and X11) from one code base, with winit.
- Tabs, splits (horizontal and vertical, nested), a tab bar that can be hidden, focus follows mouse as an option, resizing with the mouse and keyboard, zooming a pane.
- Multiple windows. A tab moves to a new window with its sessions running (`move-tab-to-window`), and a window can be started in a process of its own (`termoil-gui --separate`, `new-window --separate`) so a fault in one process cannot reach the other windows. A running session does not move between processes in 1.0; that is the multiplexer in 5.5 (docs/decisions/detach.md).
- Quick terminal (a dropdown window toggled by a global hotkey) on all three platforms.
- Command palette listing every action with its binding.
- Selection: character, word, line, block, and semantic (the output of one command, from OSC 133). Copy on select as an option. Middle click paste on Linux.
- Search over the screen and scrollback with regex, with match highlighting and navigation, in both directions.
- URL, file path and Windows path detection with click to open and keyboard hints; OSC 8 hyperlinks underline on hover.
- Shell integration shipped for PowerShell, pwsh, cmd (as far as cmd allows), bash, zsh, fish and nushell, installed by a single command, giving OSC 133 marks, OSC 7 cwd, exit codes, and the right terminfo on SSH where possible.
- Jump to previous and next prompt, select the last command's output, notify on command finish when the window is unfocused, exit code shown in the gutter or the tab.
- Configuration in a TOML file with live reload, a documented default file, and a `termoil +show-config` command that prints the effective config. Keybindings with key tables and chained sequences. Hundreds of themes bundled from the iTerm2 colour schemes set (MIT) with a theme switcher.
- A CLI and an IPC socket (named pipe on Windows, Unix socket elsewhere) that expose every action: new window, new tab in a directory, send text, send keys, read the screen as text or JSON, wait for prompt, list sessions, close. The same catalogue generates the documentation and the MCP tool definitions, following the Unluminous pattern.
- Clipboard, drag and drop of files as paths, IME input with the candidate window positioned at the cursor, dead keys, AltGr, and the Windows modifier edge cases the test suite names.
- Desktop notifications, a bell, a window title that follows OSC 2 and the cwd, and an icon.
- Installers: an Inno Setup installer and a winget manifest on Windows, a signed and notarised dmg on macOS, an AppImage and a .deb on Linux. A `tools/release.ps1` modelled on the Unluminous one that bumps, builds, tags, pushes and publishes in one command.
- Crash reporting to a local file, never to a network.

### 5.4 Should

- Sixel input (decode into the image store). Windows Terminal and WezTerm both have it; Ghostty does not.
- iTerm2 inline images (OSC 1337 File=). WezTerm and Rio have it.
- Session restore: windows, tabs, splits, cwd, and scrollback written to disk on exit and reopened on start.
- A settings window that edits the TOML file with validation.
- Custom shaders (a post process pass over the frame, kitty 0.49 style, taking a WGSL file).
- Undercurl, double underline and dotted underline rendering with the colour from SGR 58.
- Right to left scripts rendered in logical order (as Ghostty does). No bidi reordering.
- Accessibility: expose the screen as text to UI Automation on Windows and NSAccessibility on macOS so a screen reader can read the current line.
- `termoil ssh` that copies the terminfo entry to the remote host before connecting, as kitty's ssh kitten does.

### 5.5 Later, and the design must leave room

- A multiplexer server: sessions that survive the window closing, attach from another window or machine over SSH or TLS, as WezTerm does. The core is designed so a session is a value that can live in another process.
- A plugin system. The IPC catalogue is the API; a plugin host is a later layer on it.
- Bidi text reordering.
- Running as the Tasks board's agent terminal in place of Windows Terminal (replace the ConPTY daemon's window with a Termoil window; the daemon itself stays).
- Unluminous switching its terminal tile from `alacritty_terminal` to `termoil-core`.
- Web and mobile: vello and wgpu both run on WebGPU and Android, and the core has no platform code, so a browser build is possible. Not a 1.x goal.

### 5.6 Non goals

- Not a tmux replacement in 1.0. Splits and tabs are in; a session that outlives its window, or moves to another process, is not (docs/decisions/detach.md).
- Not a Lua or scripting runtime inside the process. Scripting is the CLI and IPC.
- No telemetry, no network access of any kind except SSH the user starts and the update check, which is off until the user turns it on.
- No DRCS soft fonts, no VT52 mode, no ReGIS, no Tektronix. DECRQM answers 0 (unsupported) for each.
- No native toolkit widgets (GTK, WinUI, AppKit windows for tabs). The whole window is painted by vello so it looks and behaves the same everywhere. Native menus and the macOS menu bar are used.

## 6. User experience

### First run

Install, launch, see the shell prompt within 100 ms. The default shell is the user's registered shell (PowerShell 7 if present on Windows, else Windows PowerShell; `$SHELL` elsewhere). The default theme is a dark theme with a 4.5:1 minimum contrast; the default font is the first installed of Cascadia Code, JetBrains Mono, Fira Code, Menlo, DejaVu Sans Mono, at 13 pt, with ligatures on. No wizard. A one line hint in the tab bar says how to open the command palette; it disappears after the first use.

### Daily flow

`Ctrl+Shift+T` new tab in the current directory, `Ctrl+Shift+D` split right, `Ctrl+Shift+E` split down, `Ctrl+Shift+W` close, `Ctrl+Tab` next tab, `Alt+Arrow` move focus between panes, `Ctrl+Shift+F` search, `Ctrl+Shift+P` palette, `Ctrl+Shift+Up` previous prompt, `Ctrl+Shift+Down` next prompt, `Ctrl+Shift+Space` select last output. Every binding is a config line and every action is in the palette. On macOS the same bindings use Cmd.

A command that takes longer than ten seconds while the window is unfocused raises a notification naming the command and its exit code. A failing command's prompt mark turns red in the gutter. Clicking a prompt mark selects that command's output. Clicking a path opens it with the configured opener (Unluminous by default if installed).

### Agent flow

`termoil +new-tab --cwd C:/jason/dev/ai-service --name task-2456` prints the tab id. `termoil +send <id> "npm test\n"`, `termoil +wait-prompt <id> --timeout 600` returns the exit code of the command that just finished, `termoil +screen <id> --json` returns the grid, `termoil +scrollback <id> --lines 500` returns text. The same commands are reachable over the IPC socket as JSON lines. The MCP tool list is generated from this catalogue.

### Error states

- No GPU adapter: the window opens on `vello_cpu` and the title bar shows a small "software rendering" badge; the log says why.
- The shell fails to start: the tab shows the error, the command tried, and a button to open the config.
- A broken config line: the previous config stays in effect, a banner names the line and the error, and the banner clears when the file is fixed.
- A font is missing: the fallback chain is used and the log names the missing face.
- Output floods: the renderer keeps painting at the display rate, or `window.output_fps` when nothing is being typed, from whatever state the parser reached; input is never dropped and the parser never blocks the UI thread.
- A render pass fails (device lost): the renderer recreates the device, replays the current grid, and logs it. Never a crash.

## 7. Functional requirements and acceptance criteria

The TDD turns each of these into a test. Done means the test is in the suite and passes on Windows, macOS and Linux in CI.

1. Feeding Alacritty's recorded ref test byte streams into `termoil-core` yields the recorded grids, cursor and modes, for every ref test.
2. esctest on the xterm profile passes every test Ghostty 1.4 passes, run against a Termoil built with the headless PTY front end.
3. The `mixed` 1 GiB build log stream (generated by `tools/bench`, defined in `docs/TEST-SUITE.md` section 13) is fed through the parser and screen at 250 MB/s or more on one core on this machine, measured in the benchmark suite, with no allocation per byte.
4. The typometer harness reports p99 keypress to frame under 16 ms at 120 Hz on this machine.
5. With 100 000 lines of 200 columns of random coloured text in scrollback, resident memory is under 150 MB and resize to any width reflows in under 200 ms.
6. Every row marked must in section 5 has a test named in `docs/TEST-SUITE.md`, and that test exists and passes.
7. The kitty graphics protocol conformance suite (ours, from the spec) and the kitty keyboard protocol tables pass, and `kitten icat` and Neovim with the kitty keyboard protocol work by inspection.
8. The same config file produces the same bindings, fonts and theme on all three platforms, with platform modifiers mapped.
9. Each CLI and IPC command is listed in the catalogue, has a documentation section, has a test, and reaches the same function the keybinding reaches.
10. The release script produces the installers for all three platforms from one command on this machine and publishes them to the GitHub release, and the installed Windows build reports the version just built in its About dialog.

## 8. Risks and open questions

- vello is a general vector renderer, not a text grid renderer, and the part a terminal depends on, the glyph cache in `glifo` and `vello_gpu`, is labelled experimental by its own documentation. Every 2026 vello release changed the API. The only terminal built on vello (CuTTY, an Alacritty fork) runs on the older compute renderer and reports its author's own benchmarks only. The `Renderer` trait, exact version pins and the M2 measurement are the mitigation. The research is in `docs/research/rust-crates-and-vello.md`, section 1.
- Windows ConPTY limits what reaches the terminal. The inbox ConPTY strips sixel and kitty graphics sequences (reported on Windows 11 24H2 builds too), it stopped repainting after a resize in 1.22 so the host must reflow exactly as ConPTY does, and the `conpty.dll` plus `OpenConsole.exe` pair Termoil ships must match each other. The TDD names what Termoil does on each Windows version. A direct, non ConPTY mode for shells that support it is a later option.
- WezTerm's maintenance status is uncertain, so none of its crates are a dependency. `alacritty_terminal` is a reference, not a dependency: the core is written from scratch as the ticket asks, and Alacritty's ref tests are a corpus. The `vte` byte parser is the one piece of terminal code reused.
- Unicode tables change yearly; the width and grapheme tables are generated from the Unicode data files by a script in the repo, with the Unicode version in one place.
- Font fallback on Linux without fontconfig, and on Windows for scripts DirectWrite does not cover, will show tofu. The test suite includes a fallback matrix.
- The latency gate depends on the display and compositor. Measurements are reported with the configuration; the gate is on this machine's configuration.

## 9. Milestones

The TDD section 11 expands these into tickets. Each milestone ends with its tests green and a release through `tools/release.ps1`.

- M1 Core: parser, grid, scrollback, modes, keyboard encoding, headless tests, Alacritty ref tests passing.
- M2 Render spike: vello painting a 4K grid at 120 Hz with the glyph cache, measured; go or no go on vello versus a custom atlas pass.
- M3 Window: winit, PTY, one tab, input, selection, clipboard, config, themes, the CLI catalogue. Usable daily on Windows.
- M4 Parity: tabs, splits, search, prompts, hyperlinks, hints, graphics protocol, notifications, shell integration for every shell, macOS and Linux.
- M5 Release: installers, release script, esctest and vttest sign off, performance gates, the should list that fits, 1.0.
