# Termoil: the test suite

Written 2026-11-01 for task-2456. This is the specification of the automated test suite the implementation ticket builds. Each test id here is referenced by `docs/TDD.md`, and each must row of `docs/PRD.md` section 5 maps to at least one id in the table in section 14. Research behind the choices is `docs/research/testing-corpora.md`.

The suite is about 4,300 automated tests at 1.0 plus 14 fuzz targets, 25 properties, and two external conformance runs. Counts are targets, so that each category is written, reviewed and kept fast; the exact number is not the gate, the coverage described is.

## 1. Rules for every test

- Functional and end to end tests are favoured over unit tests. A test proves a behaviour a person or a program sees, not the shape of a struct.
- Every test waits on a marker from the thing under test and a long overall deadline. Never a short sleep. (Contour's lesson: a conformance run that answers on a timer passes on one machine and fails on another.)
- Expected outputs are files beside the test and are changed only by passing an explicit rewrite flag (`TERMOIL_BLESS=1`), never by default. The commit that blesses a change says which rule changed.
- Every bug fix adds a case to the category it belongs to before the fix lands.
- Tests that need a window, a GPU or an input method are tagged and skipped where those are absent, and the skip is printed, so a green run on CI never hides a test that did not run.
- Byte stream inputs are raw files, and every replay runs three times: whole, one byte at a time, and random chunking from a seeded generator. A byte sequential parser must give the same screen wherever a read ends.
- Two grid dump formats: a plain text dump (one string per row) for cases where only characters matter and a full cell dump (character, width, foreground, background, underline colour, attribute flags, hyperlink id, wrap flag, semantic content) where attributes matter. A diff tool prints the row and column of each difference.
- External corpora are vendored only when their licence allows it and the licence file sits beside the data (`corpus/<name>/LICENSE`). GPL material (esctest2, kitty) is fetched and run in CI at a pinned commit and never copied in.

## 2. Parser state machine (U1, about 300 cases)

In house, table driven, in `crates/termoil-vt/tests/parser/`. Each case is a byte sequence and the expected list of typed actions.

- Every state of the vt100.net machine against every byte class: ground, escape, escape intermediate, CSI entry, CSI param, CSI intermediate, CSI ignore, DCS entry, DCS param, DCS intermediate, DCS passthrough, DCS ignore, OSC string, SOS/PM/APC string.
- Sequences split at every byte boundary give the same actions.
- C0 controls inside a sequence (CAN, SUB abort; ESC restarts; BEL, BS, HT, LF, CR execute during CSI).
- C1 controls as 8 bit bytes and as ESC pairs, and the UTF-8 rule that 0x80 to 0x9F inside a valid multi byte sequence are text.
- Over long parameters (more than 16 parameters, more than 4 sub parameters, a parameter over 65535) are clamped, never grow.
- OSC and DCS terminators: BEL, ST as ESC backslash, ST as 0x9C; an OSC over 8 MiB is dropped and counted.
- UTF-8 errors: truncated sequences, overlong encodings, surrogates, bytes over F4, each replaced with U+FFFD at the right position.
- The SIMD front end agrees with the scalar path on every case (both are run).

## 3. Control function behaviour in text scripts (U2, about 1,400 cases)

In house, in a libvterm style text script format, in `corpus/vt/*.test`. Each file is a sequence of steps: bytes in (with escapes), then expected cursor, pen and rows. The runner in `crates/termoil-vt/tests/script.rs` reads the files; `TERMOIL_BLESS=1` rewrites expectations.

Families, each with cases ported from libvterm's 44 `.test` files (MIT, `corpus/libvterm/LICENSE`) and written in house:

- Cursor movement: CUU, CUD, CUF, CUB, CNL, CPL, CHA, CUP, HVP, VPA, HPA, at margins and outside the scrolling region, with origin mode on and off.
- Pending wrap: print at the last column, then BS, CR, CUP, a wide character, a combining mark, and a control; DECAWM off.
- Tab stops: HTS, TBC 0 and 3, CHT, CBT, default stops at 8, a tab past the last stop.
- Insert and delete: ICH, DCH, IL, DL, with margins, with wide characters split, in insert mode (IRM).
- Erase: ED 0, 1, 2, 3 (scrollback), EL 0, 1, 2, ECH, with protected cells (DECSCA, DECSED, DECSEL), and the `styled` row flag fast path giving the same result as the slow one.
- Scrolling: SU, SD, IND, RI, NEL, DECSTBM, DECSLRM with DECLRMM, scrolling region at the bottom and top, scroll with wide characters at a margin.
- Alternate screen: modes 47, 1047, 1048, 1049; cursor save across switch; the alternate screen has no scrollback; resize on the alternate screen.
- Save and restore: DECSC, DECRC, SCOSC, SCORC, including charset, origin mode, pen and wrap state.
- Resets: DECSTR, RIS, what each clears and keeps (tab stops, modes, palette, title stack, kitty keyboard stack).
- Modes: every DEC private mode the terminal knows and every ANSI mode, set, reset, query with DECRQM, the answers 1, 2, 3, 4 and 0, saved and restored with XTSAVE and XTRESTORE.
- Character sets: SCS G0 to G3, SI, SO, SS2, SS3, DEC special graphics line drawing, UK, and the rule that UTF-8 mode ignores charsets for non ASCII.
- Double width and double height lines (DECDWL, DECDHL): a cell count of half, printing past the end, erase on such a line.
- SGR: every attribute on and off, 256 colour and true colour in both colon and semicolon forms, underline colour (58, 59), underline styles (4:0 to 4:5), XTPUSHSGR and XTPOPSGR with a stack of depth 10, reset after a push.
- Hyperlinks: OSC 8 with and without id, nested, closed, across a wrap, the same id across two spans, over long URIs dropped.
- Titles and icons: OSC 0, 1, 2, the title stack (XTWINOPS 22 and 23), UTF-8 in titles, a title over the limit.
- Reports: DA1, DA2, DA3, DSR 5 and 6, DECXCPR, DECRQSS for every settable, XTVERSION, XTGETTCAP for the terminfo keys Termoil ships, CPR with origin mode, each answer byte exact.
- Colours: OSC 4, 10 to 19, 104, 110 to 119, the `?` query form, `rgb:` and `#` syntaxes.
- OSC 7 cwd (valid file URLs, a non file URL ignored), OSC 9 and 99 notifications, OSC 9;4 progress states, OSC 52 write and the read refused by default, OSC 133 A, B, C, D with exit codes and the resulting row and cell semantic bits, OSC 1337 cwd.
- Synchronized output 2026: the frame is held, the end releases it, the one second timeout releases it, DECRQM answers.
- Kitty keyboard protocol stack: push, pop, query, pop past empty resets, a full stack of 16 drops the oldest, separate stacks per screen.
- Bracketed paste wrapping and focus events on and off.
- Mode 2027 on and off: the same bytes give cluster cells or legacy width cells.

## 4. Recorded sessions, grid goldens (C1, C3, about 80 cases)

- **C1**: Alacritty's 45 ref tests (Apache 2.0, `corpus/alacritty-ref/LICENSE`) replayed through `termoil-vt`: the recording file, the size file and the config are inputs; the expected grid JSON is translated to the Termoil full cell dump once and reviewed, then kept. All 45 pass at 100 % with every chunking. Where Alacritty's grid and Termoil's differ on purpose (a grapheme cluster where Alacritty has zero width extras), the case carries a note and a second expected file.
- **C3**: in house recordings in the same layout, made with `termoil +record <file>` from the real binary: vim, neovim with the kitty keyboard protocol on, tmux, htop, less, bat, zsh with powerlevel10k, fish, nushell, PowerShell 7 with oh-my-posh, Claude Code, Codex, a `cargo build` with progress bars, `git log` with colours, a Windows path listing, and xterm.js's 76 `.in` plus `.text` fixture pairs (MIT, `corpus/xtermjs/LICENSE`) as character only cases. About 35 in house recordings at 1.0, growing with every bug.

## 5. External conformance (C2, run in CI, not vendored)

- **esctest2** (Thomas Dickey's fork, GPL 2, fetched at a pinned commit): run against the headless Termoil front end over a PTY on Linux and macOS, and over ConPTY on Windows where ConPTY lets the sequences through. Contour's method: Termoil answers reports on markers, a separate silence timer, and a ratchet file `corpus/esctest/known-failures.txt` with one reason per line. The job fails on any failure not in the ratchet file and on any line in the ratchet file that now passes (so the file only shrinks). The 1.0 gate is every test Ghostty 1.4 passes, which is recorded in `corpus/esctest/ghostty-1.4-pass-set.txt`, generated once by running esctest2 against Ghostty on Linux and kept.
- **vttest** (BSD, fetched): menus 1, 2 and 8 (cursor movements, screen features, VT102 insert and delete), 35 hold screens plus the main menu, turned into reviewed goldens. `tools/vttest/run.sh` drives vttest through `termoil-headless run --script corpus/vttest/vttest.vtscript`, which feeds the menu choices and captures the screen (text and full cell dump) at each hold prompt, and compares with the goldens in `corpus/vttest/`. The checklist in `corpus/vttest/README.md` covers the rest by hand (character sets, double size characters, keyboard; VT52 is documented unsupported).

## 6. Unicode (U3, about 700 cases)

- `GraphemeBreakTest.txt` from the pinned Unicode version (Unicode licence, `corpus/unicode/LICENSE`): every line as a case against the generated break tables.
- `EastAsianWidth.txt` and `emoji-test.txt`: every code point's width against the generated width table, plus the Termoil decisions written as cases: emoji presentation sequences are wide, text presentation (VS15) is narrow, ZWJ sequences are one wide cell, regional indicator pairs are one wide cell, a combining mark at the start of a line is its own narrow cell, a wide character at the last column wraps and leaves a spacer, 64 combining code points per cell and the 65th is dropped, mode 2027 off gives per code point widths.
- The tables generator is itself tested: generating from the data files yields the checked in tables byte for byte (a stale table fails CI).
- The width Termoil reports and the width ConPTY uses with `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` agree on a fixed list of 200 clusters (W2, Windows only).

## 7. Input encoding (I1, I2, I3, about 700 cases)

In house tables in `crates/termoil-input/tests/`, one row per key and modifier combination and one column per mode.

- **I1 legacy**: every printable, control and function key with every modifier combination in legacy xterm mode, application cursor keys on and off, application keypad on and off, `modifyOtherKeys` 1 and 2, with the known cases named: Ctrl+I is Tab, Ctrl+M is Enter, Ctrl+Space is NUL, Alt+[ is the CSI introducer, Shift+Tab is `CSI Z`, Backspace and Ctrl+Backspace per the `kbs` setting.
- **I2 kitty keyboard protocol**: every flag combination the protocol allows (1, 2, 4, 8, 16 and their sums), every key in the protocol's key table, press, repeat and release under flag 2, alternate keys under flag 4, associated text under flag 16, Enter, Tab and Backspace keeping legacy bytes under flag 1, Ctrl+C as an escape under flag 1, the key code always the unshifted lowercase code point, keypad keys as private use codes, lock modifiers not reported for text keys. Expected bytes written from the protocol's own examples and cross checked by running the same keys through Ghostty and Alacritty once (results kept as a note, GPL kitty tables read and not copied).
- **I3 mouse and platform**: X10, 1000, 1002, 1003, 1005, 1006, 1015 and 1016 encodings, the 223 column limit of the legacy form, wheel and extra buttons, modifiers, motion with and without a button, focus events, bracketed paste wrapping including a paste containing the end sequence. Platform normalisation: AltGr arrives as Ctrl+Alt and types the layout's character, a dead key followed by a space or by itself gives the accent character, a dead key followed by a shortcut does not stay pending, the Win and Cmd keys, numpad with NumLock on and off, each as a table over a fake key source.

## 8. Scrollback, resize, reflow, selection, search (U4, about 300 cases, and 25 properties)

- Resize: grow and shrink columns with wrapped lines, shrink to one column and grow back, resize while scrolled, resize on the alternate screen, resize with a wide character at the new last column, resize with a selection and with prompt marks (the pins move), reflow off.
- Scrollback: the byte limit is honoured, the line limit is honoured, oldest pages are pruned first, an oversized page is never reused, compression and restore give the same rows, clear scrollback (ED 3), the alternate screen has none.
- Selection: character, word (with the configured word boundaries), line, block, semantic (one command's output from its marks), across wrapped lines, across wide cells and clusters, copy as text with trailing spaces trimmed and with wraps joined, copy with styles as ANSI.
- Search: regex forward and backward, wrap around, across wrapped lines, across pages including compressed ones, case folding, match count, the result pins survive new output.
- **Properties** (`proptest`, 25): any chunking gives the same grid; the cursor is always inside the grid; every row has exactly the column count; a wide character never leaves half a cell; resize to a new size and back keeps the visible text when nothing was pruned; scrollback never exceeds its limit; no sequence grows memory without bound over 10 000 random operations; pins always point inside the list; interned style counts match the cells that use them; the alternate screen never has history; the kitty keyboard stack never exceeds 16; the text dump of a page equals the text dump of its compressed and restored copy; and others the implementation adds. The oracle for grid properties is a simple `Vec<Vec<char>>` model.

## 9. Graphics (U5, about 180 cases)

- Kitty graphics: transmit direct, file, temporary file and shared memory; PNG, RGB and RGBA; zlib; chunked transfers including an interrupted one; put with every placement option; z index ordering; Unicode placeholders; animation frames, composition and control; relative placements; delete by every selector; quota eviction oldest first at 320 MB; a payload over 65 MiB dropped and counted; malformed commands answered with the right error code; responses suppressed by `q`. Expected results are the store's state and the cell placements.
- Sixel: hackerb9's `vt340test` images (CC0, `corpus/vt340test/LICENSE`) decoded and compared to reference PNGs rendered once and reviewed; raster attributes, palettes, repeats, transparency, a truncated image.
- iTerm2 inline images: a small in house set (should list).

## 10. Rendering (R1 to R4, about 60 image cases)

- **R1 frame builder as data**: given a snapshot, the row records are compared to expected JSON. Cases: every attribute combination, dirty row subsets, selection overlays, cursor shapes and the inverted block cursor, image placements and clipping, the chrome.
- **R2 pixel goldens on the CPU renderer**: `VelloCpuRenderer` renders a fixed scene with a vendored OFL font (JetBrains Mono, `corpus/fonts/LICENSE`) at a fixed size to a PNG and compares cell by cell against the accepted image in `corpus/render/<platform>/`, with a per test tolerance (max per channel difference, fraction of pixels allowed over it, anti aliasing edge allowance). A failure writes `.new.png` and `.diff.png` beside the accepted image, which are gitignored, and the test prints the row and column of the first differing cell. Cases: every SGR attribute, the five underline styles with colours, strike through, each cursor shape focused and unfocused, selection, a wide glyph, emoji (COLR and bitmap), box drawing and powerline sprites joining at three cell sizes, a ligature split across two colours, scroll offset, resize, a kitty image under text, opacity, the tab bar and split borders, the search bar, the progress bar, the software rendering badge.
- **R3 GPU parity**: the same cases through `VelloGpuRenderer` (and the atlas renderer if built) read back from an offscreen texture, compared to the CPU image with a wider tolerance. Runs on this machine with the real adapter and in CI with a software adapter (lavapipe on Linux, WARP with DXC on Windows, the Metal runner on macOS). The test prints the adapter it got; a golden is never compared across adapter types.
- **R2 tab bar** (`crates/termoil-app/tests/tab_bar.rs`, task-2468): the tab bar rendered for each state (the tab that is showing, a tab, a close cross and the new tab button under the pointer, the window without the keyboard) and for each theme taken from Unluminous plus two whose chrome is worked out from their own colours, against `corpus/render/<platform>/tab_bar_*.png`. The same file checks that the bar is hidden with one tab and shown with two (task-2487), and that the new tab button is drawn against the right edge of the bar. The E6 window test hovers and clicks a tab's cross, checks the bar is hidden when that leaves one tab, and clicks the new tab button at the right end of the bar.
- **Looking at the real window on Windows**: capture it with Windows Graphics Capture (the API screen recorders use), never with a GDI screen copy, `PrintWindow` or Desktop Duplication through `ddagrab`. A GPU swapchain is not in what GDI reads, so a GDI copy shows an old frame; that is how the task-2468 resize fault first looked like a capture problem. Drive the window with `SetWindowPos` and posted mouse messages so the person's own pointer is not moved; a posted move is followed by a real `WM_MOUSELEAVE` when the pointer is elsewhere, so hover is tested through `handle::cursor_moved` in E6 and in the R2 tab bar goldens instead.
- **R4 M2 gate**: a 200 by 60 grid full screen scroll of mixed coloured text at 120 Hz, p99 frame time under 8 ms on this machine, recorded with the adapter and commit into `docs/decisions/renderer.md`. Since task-2481 the gate is measured on `RasterRenderer`, the window's default, and on `vello_gpu`; `termoil-bench render` prints both. `RasterRenderer` is also checked against `vello_cpu` on the R2 scenes under the R3 tolerance, and a run of frames drawn with its row damage must give exactly the pixels of the last frame drawn whole. Also the idle test: an unchanged terminal issues zero frames over 10 seconds.

## 11. PTY and end to end (E1 to E6, about 50 cases)

Spawn the real shell through `termoil-pty` on each platform, send bytes through the same encoder the window uses, wait on markers, read the grid.

- **E1 spawn and exit**: the default shell starts, the cwd is honoured, the environment has `TERM`, `COLORTERM`, `TERM_PROGRAM`; exit code 0, non zero, and a signal (Unix) are reported as a typed status; closing the pane ends the child and its tree (job object on Windows, SIGHUP on Unix).
- **E2 size**: the initial size reaches the child (`stty size`, `$Host.UI.RawUI.WindowSize`), resize is delivered (SIGWINCH on Unix, ConPTY resize on Windows), large output (64 MB) arrives complete and in order.
- **E3 shell integration**: for PowerShell, pwsh, bash, zsh, fish and nushell, the installed script emits OSC 133 A, B, C and D with the exit code, OSC 7 after `cd`, and `wait-prompt` returns the exit code of a command that failed.
- **E4 programs**: vim opens and quits, `less` pages and quits, a program that sends DA1 and DSR gets the answers, a program that reads the kitty keyboard flags sees them, `kitten icat` style output places an image (Unix), tmux passthrough reaches the terminal.
- **E5 IPC and CLI**: every catalogue command through `termoil-cli` against a running instance started for the test: `new-tab` with a cwd, `send-text`, `send-key`, `screen` text and JSON, `scrollback`, `wait-prompt`, `list`, `close`, `run-action` for every action, `show-config`, `reload-config` after writing a changed file, `version`. Permission on the socket is owner only.
- **E6 window**: through winit with a fake key and IME source: typing reaches the PTY, IME preedit is drawn at the cursor and never written to the PTY until commit, cancel writes nothing, focus loss during composition commits or cancels per platform convention, paste is bracketed when the mode is on, a tab opens and closes, a split is created and focus moves, the quick terminal toggles, the command palette runs an action, the palette shows the answers of `check-for-updates` and `list-updates` from a release server in the test process and the error of a failed action, Escape closes an answer and the next key reaches the shell, and every palette entry runs from the palette in turn with none failing apart from the ones the step names (task-2586). Skipped and printed where no display is available.

## 12. Fuzzing (F1 to F14, `cargo fuzz`)

Targets: the parser alone; parser plus screen on a small terminal through both the SIMD and scalar paths; OSC payloads with the three terminators; DCS and APC payloads; the kitty graphics command parser; the sixel decoder; the PNG decode path with a size cap; the key encoder; the mouse encoder; resize and reflow with a random operation sequence; the page allocator; the config parser; the IPC message parser; the shell integration OSC 133 handler. Each target checks for crash, hang, sanitizer fault and the invariants from section 8. A 60 second run per target on every pull request, a 2 hour run nightly, corpora checked in under `corpus/fuzz/` and minimised by a script (`tools/fuzz-corpus.cjs`).

## 13. Performance (P1 to P5, gated on this machine)

Each benchmark prints the machine, build flags, commit, sample count, median and p99, and writes a JSON row to `_agent_output/bench/`. CI runs the same suite on shared runners in report only mode and fails on a regression over 30 % against the last published run; the gates below are measured on this machine before every release and recorded in `docs/performance.md` with the configuration.

- **P1 throughput**: vtebench's 12 scenarios unchanged (Apache 2.0 or MIT, `corpus/vtebench/LICENSE`) plus six in house: unicode heavy text, kitty graphics stream, scrollback fill to the limit, a resize storm, the `mixed` 1 GiB stream, and a tmux passthrough stream. The `mixed` stream is the build log generated by `tools/bench` (`streams::mixed`, fixed seed, one 64 MiB block repeated sixteen times): words, an SGR colour on about one word in eight, a UTF-8 word in twenty, one line in sixteen a progress bar redrawn with CR; about 95 bytes per line, 29 escape sequences per KB, 16 % of bytes inside escape sequences. A test pins the block's length and checksum. Gate: the `mixed` stream through `Terminal` (parser plus screen) at 200 columns by 60 rows with the default scrollback budget, fed in 64 KiB slices on one core, at 250 MB/s or more on this machine; display at or above 60 fps while the PTY saturates. The parser alone on the same stream is published, not gated. Each published run also records alacritty_terminal and Ghostty 1.4 on the same bytes on this machine. The reasons, with the comparison that set the number, are in `docs/decisions/throughput.md`.
- **P2 latency**: the Contour method built in: `termoil +bench-latency` spawns an echo child that prints a prompt and echoes bytes wrapping at 60 columns, injects keys 25 to 35 ms apart with random gaps on the UI thread, timestamps the key and the first frame swap that shows the echoed character, with swaps unpaced, and reports p50, p90, p99 and max. Gate: p50 under 8 ms, p99 under 16 ms at 120 Hz on this machine. Once per release the same number is confirmed with a camera at 240 fps against Windows Terminal and Alacritty on the same display, recorded in `docs/performance.md`.
- **P3 memory**: 100 000 and 1 000 000 lines of 200 columns, typical text and dense attribute text, resident set after the idle compression timer; ten idle tabs. Gate: under 150 MB and under 400 MB.
- **P4 startup**: process start to the first frame with the prompt presented to a visible window, cold and warm, through a probe the app itself reports. The interval starts at the start of `main` and ends when the platform confirms the first frame containing shell output has been composed for a visible window (Windows: `DwmFlush` after the first present that follows `ShowWindow`; Wayland: `wp_presentation_feedback.presented`; macOS: the drawable's presented handler). A frame painted into a hidden window does not end it. The probe reports `visible_ms` (the gate), `painted_ms` (the first paint with shell output, visible or not) and the phases between them. Gate: `visible_ms` under 100 ms warm on Windows and Linux, under 150 ms on macOS. Once per release the number is confirmed with the P2 camera against Windows Terminal and Alacritty launched the same way. The reasons are in `docs/decisions/startup.md`.
- **P6 against Ghostty** (task-2481, reported, not gated): `termoil-bench apps` runs Termoil and Ghostty for Windows on the same idle and stream workloads and reports memory and CPU of each terminal's own process, its pseudoconsole host and the shell separately. The goal from task-2481 is 25 % of Ghostty or less on private memory and stream CPU; `docs/decisions/efficiency.md` has the method and the numbers.
- **P5 allocation**: the parser and screen apply path over the `mixed` 1 GiB stream with a counting allocator; zero allocations per byte in the steady state, allocation only at page boundaries.

## 14. Agent reachability, build, Windows and release checks

- **A1**: every catalogue row has a `run_action` arm, a documentation section in `docs/commands.md`, and a test in E5; the test fails naming the missing one.
- **A2**: the MCP tool list is generated from the catalogue and matches the checked in `docs/mcp-tools.json`.
- **A3**: every keybinding in the default config names a catalogue action.
- **A4**: the command palette lists every action.
- **A5** (task-2586, `crates/termoil-app/tests/a5_window_actions.rs`): every action a person runs from the window is run and they are told what it answered. Every catalogue row is in exactly one of `answer::ANSWERS` and `answer::ACTS`; every action runs through `App::run_for_person` in a headless app with a real shell, and an action whose result is information shows it while a failure from the palette is shown with its reason; `check-for-updates`, `list-updates`, `install-update` and `skip-update` answer from a release server in the test process, through the palette keys and directly; an unreachable channel, a wrong SHA-256 and a skip before any check are shown as failures; the palette asks for every required parameter; the answer overlay scrolls and closes on any key. `docs/decisions/window-actions.md` has the design.
- **B1**: `cargo build -p termoil-vt -p termoil-input -p termoil-protocol --target wasm32-unknown-unknown` passes.
- **B2**: the Unluminous proof of concept branch compiles and `cargo test -p unluminous-terminal` passes there (milestone M4).
- **W1**: ConPTY reflow: ten recorded sessions resized through ConPTY on this machine; Termoil's grid after host side reflow equals what ConPTY reports.
- **W2**: the width agreement test from section 6.
- **W3**: the bundled `conpty.dll` and `OpenConsole.exe` versions match `installer/windows/conpty.version` and each other.
- **S2**: the ConPTY graphics passthrough spike: a test that sends an APC and a DCS through the inbox ConPTY and the bundled one and records what arrived, kept as documentation of this machine's behaviour and failing if the behaviour changes.
- **R1 release**: `tools/release.ps1 -WhatIf` lists every step; after a real release the installed binary's `+version` equals the tag and the GitHub release has one asset per platform.
- **D1 licences**: `cargo deny check licenses` with the allow list in `deny.toml`.

## 15. Coverage map

| PRD section 5 row | Test ids |
|---|---|
| VT parser, bounded stacks | U1, F1 to F4, U2 modes and stacks |
| Grapheme cells, Unicode 17, mode 2027 | U3, U2 mode 2027, W2 |
| Colours and SGR | U2 SGR and colours, R2 |
| Scrollback, paging, compression, reflow | U4, P3, properties |
| Alternate screen, regions, resets | U2 |
| Mouse, focus, paste, 2026 | I3, U2 2026, E6 |
| Keyboard legacy and kitty | I1, I2, E4 |
| OSC family | U2 OSC, E3 |
| Reports, terminfo | U2 reports, E4, XTGETTCAP cases |
| Kitty graphics | U5, E4, R2 image case, S2 |
| DCS passthrough | U1 DCS, E4 tmux |
| Rendering, fonts, cursor, pacing | R1 to R4, P2 |
| Platforms and windowing | E6, R3 adapters, CI matrix |
| Tabs, splits, quick terminal, palette | E5, E6, A4, A5 |
| Selection, search, hints, hyperlinks | U4, U2 hyperlinks, E5 screen |
| Shell integration, prompts, notifications | E3, U2 OSC 133 and 9 and 99 |
| Config, themes, keybindings | E5 config, A3, F12 |
| CLI and IPC | E5, A1, A2 |
| Clipboard, drag and drop, IME | E6, I3 |
| Installers and release | R1, W3 |
| Crash reporting | a panic in a test build writes the report file |

## 16. What is built in house for the suite

1. The text script format and runner (section 3) with the explicit bless flag.
2. `termoil +record` and the replay tool with three chunkings.
3. The conformance harness that runs esctest2 and vttest over a PTY against the headless front end, with the ratchet file.
4. The full cell dump, the text dump, the diff tool, and the image comparison tool with per test tolerance and difference images.
5. The latency probe and the benchmark runner that writes the JSON rows and the markdown table.
6. The fake key, fake IME and fake clipboard sources at the windowing boundary.
7. The fuzz corpus script (minimise, replay crashes).
8. The Unicode tables generator and its staleness check.

## 17. Order of work

Parser tables and the text script runner first, then the Alacritty ref tests, then esctest2 in a non gating job so the first failures appear early, then vttest goldens, then the Unicode and input tables, then scrollback and properties. Rendering goldens, the latency probe, PTY end to end and performance come once the renderer and the window exist. A fuzz target is added the day its component lands.
