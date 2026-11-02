# How terminal emulators are tested: corpora, harnesses and a plan for Termoil

Prepared 2026-11-01 for the Termoil test suite specification. Termoil is a terminal emulator written from scratch in Rust.

Method. I cloned (treeless, shallow) the repositories for Alacritty, vtebench, Ghostty, WezTerm, kitty, xterm.js, Windows Terminal, Contour, foot, libvterm and esctest2 on 2026-11-01 and read the test directories, CI workflows and documentation directly. Test counts below come from searching those clones, so they are counts of what the search pattern matched on that date, and they are lower bounds where a project writes tests in more than one style. Pages I could read are cited. Pages I could not read are listed in the last section, so nothing in this report is quoted from memory without saying so.

---

## 1. Conformance suites and shared corpora

### vttest

vttest is the classic interactive VT100 conformance program. Thomas Dickey has maintained it since 1996 and it now covers VT220 through VT520 features, ISO 6429 colour, background colour erase, VT420 rectangle operations and xterm specific window operations. The page states that the patch date is the effective version number. Its licence is BSD; the original noncommercial licence was changed in 2007 with the original author's agreement. Source: https://invisible-island.net/vttest/ and https://invisible-island.net/datafiles/release/vttest.tar.gz . Manual page: https://invisible-island.net/vttest/manpage/vttest.html .

What it tests: cursor movement, screen features (132 column mode, scrolling regions, origin mode, tab stops), double sized characters, keyboard, terminal reports, VT52 mode, VT102 insert and delete, "known bugs" of the original VT100, then the extended menus for VT220 character sets, ECH, VT420 rectangles, colour and xterm features.

How to run it without a person. vttest asks a human to look at each screen and press Return, so it cannot assert anything by itself. The options in the manual page are `-l` (write a log, `vttest.log`), `-s` (add delays), `-p` (padding), `-8` (8 bit controls), `-f file` (load a soft character set file) and a geometry argument in the form `24x80.132`. Dickey's page says he changed vttest so it can read its own log file back as a command script, which he used with lcov to measure which tests had been exercised. There are three ways projects run it unattended:

1. Alacritty recorded real vttest sessions in a PTY and turned them into ref tests: `vttest_cursor_movement_1`, `vttest_insert`, `vttest_origin_mode_1`, `vttest_origin_mode_2`, `vttest_scroll` and `vttest_tab_clear_set` are in https://github.com/alacritty/alacritty/tree/master/alacritty_terminal/tests/ref .
2. libvterm converted vttest screens to its text test format. The directory https://github.com/neovim/libvterm/tree/master/t holds files named `90vttest_01-movement-1.test` and `90vttest_02-screen-1.test` and so on, which use `PUSH` lines with `$SEQ` loops to replay the same escape sequences vttest sends and then check the screen.
3. Contour built a harness, `src/vtconformance`, that runs the real vttest binary under a PTY against a headless terminal engine, answers each prompt when the bytes of the prompt appear on the wire (not on a timer), and compares each captured screen to a blessed golden dump. Its design document is the best description of the pitfalls and is at https://github.com/contour-terminal/contour/blob/master/docs/internals/vt-conformance.md . The key points are listed under Contour in section 2.

### esctest and esctest2

esctest is a Python suite of automatic tests that drive a terminal and read the results back through terminal reports. Its README describes the ideal as "xterm, but without bugs in George's opinion" and says the reference documents are the xterm control sequences page, the DEC VT510 reference manual and DEC STD 070. The original is by George Nachman (iTerm2). The maintained successor is Thomas Dickey's https://github.com/ThomasDickey/esctest2 . The GitHub original at https://github.com/gnachman/esctest now holds only a README that says the project moved to https://gitlab.freedesktop.org/terminal-wg/esctest (I could not read that page because it is behind an Anubis bot check; the search result listed it as GPL 2.0 or later).

How it drives a terminal. esctest runs inside the terminal under test. It writes escape sequences to its stdout and reads the replies from its stdin. It reads the screen back with DECRQCRA (checksum of a rectangle), cursor position reports (CPR), device status reports and XTWINOPS reports, so it needs no screen scraping. The README states that colours cannot be tested because a cell's colour cannot be read back; characters, cursor position and window attributes can. Arguments in `esctest/escargs.py` include `--include` (regex of tests), `--expected-terminal` (xterm or iTerm2 or iTerm2beta), `--xterm-checksum`, `--xterm-reverse-wrap`, `--max-vt-level`, `--stop-on-failure`, `--timeout`, `--logfile`, `--options` and `--force`.

Size. The esctest2 clone has 81 test modules under `esctest/tests` (apc, cbt, cha, cub, cud, cuf, cup, da, da2, dch, decaln, decbi, deccra, decdsr, decera, decfi, decfra, decic, decrqm, decrqss, decscl, decsed, decsel, decset, decstbm, decstr, dl, ech, ed, el, hpa, ich, il, ind, nel, rep, ri, ris, sgr, sm, su, tbc, vpa, xterm_winops and others) and 559 `def test_` functions.

Licence. The `LICENSE` file in esctest2 is the GNU General Public License version 2. Termoil should run esctest2 as an external program in CI and must not copy its code into the Termoil repository.

Who uses it. Ghostty runs it against libghostty-vt in a CI job that reports results and never fails the build (https://github.com/ghostty-org/ghostty/tree/main/test/esctest ). Contour runs it and gates the build on it (`src/vtconformance/EscTestSuite.cpp`). iTerm2 and xterm are the two terminals the README configures it for.

How Ghostty wires it up, which is the model to copy. The runner in `test/esctest/src/main.zig` spawns esctest under a PTY, feeds everything it writes into a `Terminal`, writes the terminal's replies back, and copies esctest's log to stdout when it exits. A full run takes about a minute. It passes `--expected-terminal=xterm --xterm-checksum=411 --xterm-reverse-wrap=411`, sets an 80x25 screen, enables DECRQCRA with xterm's checksum extension and answers primary device attributes as a VT520. The README states two known causes of failures: libghostty-vt does not implement DECSTR (which esctest uses to reset between tests, so state leaks between tests), and the terminal never resizes, so the XTWINOPS and DECCOLM tests fail.

### xterm test scripts and DEC references

Dickey's xterm distribution carries the "ctlseqs" document used as the specification by esctest (https://invisible-island.net/xterm/ctlseqs/ctlseqs.html ). The xterm source also holds shell and perl scripts for checking sequences (the `vttests` directory in the xterm tarball, https://invisible-island.net/xterm/ ), which I did not read. Windows Terminal ships a small Python set in `src/tools/vttests` (`common.py`, `burrito.py`, `test-unicode.py`, `template.py`), which print sequences for a person to inspect (https://github.com/microsoft/terminal/tree/main/src/tools/vttests ). hackerb9's VT340 repository (section 1, sixel) records how a real DEC VT340 answers a large number of probes, which is useful as evidence of what the hardware does when the xterm documentation is silent.

### terminal-wg

The freedesktop terminal working group hosts the maintained esctest, a specifications repository and a parsing repository: https://gitlab.freedesktop.org/terminal-wg/esctest , https://gitlab.freedesktop.org/terminal-wg/specifications , https://gitlab.freedesktop.org/terminal-wg/terminal-parsing , https://gitlab.freedesktop.org/terminal-wg/bidi . All of these returned an access denied page to my fetch (Anubis), so I list them as pointers and have not verified their contents or counts. The Contour project's Terminal Unicode Core specification, which is aimed at fixing grapheme cluster and width handling across terminals, is at https://github.com/contour-terminal/terminal-unicode-core ; the repository page names no test suite and I could not read the spec text.

### Unicode width and grapheme tests

The Unicode Consortium publishes GraphemeBreakTest.txt, currently version 18.0.0 dated 2026-06-12, in https://www.unicode.org/Public/UCD/latest/ucd/auxiliary/GraphemeBreakTest.txt . Each line is a list of code points separated by symbols for "break here" and "no break here", with a comment naming the rule that decides it. It is thousands of lines. Projects that use it:

- kitty vendors a JSON conversion, `kitty_tests/GraphemeBreakTest.json` (https://github.com/kovidgoyal/kitty/tree/master/kitty_tests ).
- Ghostty takes its Unicode properties from the `uucode` library (`src/unicode/grapheme.zig`, `props_uucode.zig`, `symbols_uucode.zig`), has 7 tests in `grapheme.zig`, and benchmarks `CodepointWidth.zig`, `GraphemeBreak.zig` and `IsSymbol.zig` in `src/benchmark`.
- WezTerm runs the Unicode bidi conformance files `bidi/data/BidiTest.txt` and `bidi/data/BidiCharacterTest.txt` through `bidi/tests/conformance.rs`.
- Contour uses the libunicode library for widths and graphemes, and the grapheme width behaviour is what the Terminal Unicode Core specification describes.

The width of a cell is a terminal decision and no Unicode file gives it, so a width corpus has to be built in house from EastAsianWidth.txt, emoji-test.txt and the behaviour of the terminals Termoil wants to agree with. The GraphemeBreakTest file is only the segmentation half.

### Kitty keyboard protocol corpus

The specification is https://sw.kovidgoyal.net/kitty/keyboard-protocol/ . It defines five progressive enhancement flags (disambiguate, report event types, report alternate keys, report all keys as escape codes, report associated text), the form `CSI unicode-key-code:alternate-key-codes ; modifiers:event-type ; text-as-codepoints u`, modifier bits (shift 1, alt 2, ctrl 4, super 8, hyper 16, meta 32, caps lock 64, num lock 128, sent as 1 plus the sum), event types (press 1, repeat 2, release 3), and private use area codes for F13 to F35, keypad and media keys. It has no test corpus of its own. The test material that exists is:

- kitty's `kitty_tests/keys.py`, with `test_encode_key_event` (a very large table of modifiers, actions, alternate keys and text through a `csi()` helper) and `test_encode_mouse_event` (https://github.com/kovidgoyal/kitty/blob/master/kitty_tests/keys.py ). The kitty repository is GPL 3 (the header of `test.py` says so), so Termoil can read these tables and write its own, not copy them.
- Ghostty `src/input/key_encode.zig` with 105 tests, `mouse_encode.zig` with 24, `key_mods.zig` with 23 and `paste.zig` with 14.
- Windows Terminal `src/terminal/adapter/ut_adapter/kittyKeyboardProtocol.cpp` (7 test methods), plus `inputTest.cpp` and `MouseInputTest.cpp`.
- libvterm `t/25state_input.test`, which checks ordinary keys, for example Ctrl+A is sent as the escape sequence for codepoint 65 with modifier 5 and Ctrl+Alt+A is sent with modifier 7.

### Kitty graphics protocol tests

- kitty `kitty_tests/graphics.py`: 36 test methods covering the disk cache, PNG and RGB loads, put with pixel offsets, image parents, animation frames, Unicode placeholders (several variants including scroll and history cases), deletion and reset.
- Ghostty `src/terminal/kitty/graphics_storage.zig` (76 tests), `graphics_exec.zig` (68 tests) and `testdata` holding `dog.png` and four raw image data files (RGB and PNG, with and without zlib deflate).
- WezTerm `term/src/test/image.rs` (inline image protocol tests).
- Contour `src/vtbackend/graphics/KittyGraphics_test.cpp`, `SixelParser_test.cpp`, `GoodImageProtocol_test.cpp` and ReGIS tests.

### Sixel test images

- hackerb9's https://github.com/hackerb9/vt340test is CC0 licensed and holds `sixeltests`, `mediacopy`, `colormap`, `regis` and other directories, with sixel samples and screenshots captured from a real VT340, many contributed by j4james and jerch. It is a corpus of expected pictures for edge cases (transparency, colour maps, raster attributes, repeat counts) and is the closest thing to a shared sixel test suite.
- The Jexer sixel page (https://jexer.sourceforge.io/sixel.html ) lists captured sixel sessions with raw output and an xterm screenshot for comparison, and describes a stress test with a very large repeat counter that a terminal must survive without drawing anything.
- GNOME VTE issue 258 (https://gitlab.gnome.org/GNOME/vte/-/issues/258 ) lists what sixel tests should cover, including malformed input.

I did not verify the licence of the libsixel sample images.

---

## 2. How each emulator tests itself

### Alacritty (recorded byte streams and grid JSON)

Where: `alacritty_terminal/tests/ref.rs` and `alacritty_terminal/tests/ref/<name>/` (https://github.com/alacritty/alacritty/tree/master/alacritty_terminal/tests ). There are 45 test directories, each registered by name in a `ref_tests!` macro that generates one `#[test]` per name. Each directory holds four files: `alacritty.recording` (the raw bytes the shell and programs wrote to the PTY), `size.json` (terminal dimensions), `config.json` (only `history_size`) and `grid.json` (the expected grid serialized with serde). The largest recording is 350,752 bytes (`vim_24bitcolors_bce`).

How they are recorded. `CONTRIBUTING.md` says to build a release binary of the patched Alacritty, run it with `--ref-test`, do the thing that exposes the bug, then close the window or kill the process (`exit` and Ctrl+D do not work), after which the four files are written to a new directory under `tests/ref`, and the test is enabled by adding its name to the macro. It also says that when fixing a bug the new ref test should be checked to fail without the fix.

How they are replayed. `ref_test` reads the files, builds a `Term` with a no op event listener and the recorded scrollback size, feeds the whole recording to the `vte` ANSI processor in one call, truncates the invisible lines, and requires the grid to equal the stored grid. On a mismatch it prints every differing cell as `[row][col] expected => actual` and panics.

What is covered. Names include `alt_reset`, `csi_rep`, `decaln_reset`, `deccolm_reset`, `delete_lines`, `erase_chars_reset`, `fish_cc`, `history`, `hyperlinks`, `indexed_256_colors`, `region_scroll_down`, `saved_cursor`, `saved_cursor_alt`, `selective_erasure`, `sgr`, `tab_rendering`, `tmux_git_log`, `tmux_htop`, `underline`, `vim_24bitcolors_bce`, `vim_large_window_scroll`, `vim_simple_edit`, six `vttest_*` captures, `wrapline_alt_toggle`, `zerowidth` and `zsh_tab_completion`. The set mixes small hand made cases with real sessions of vim, tmux, htop, zsh and fish.

Limits. The expected grid is whatever Alacritty produced at recording time, so a recorded test says "this is what we do today" and a wrong behaviour can be recorded as correct. There is no fuzzing in the repository: the `vte` parser repository (https://github.com/alacritty/vte ) has `src`, `tests`, `examples` and `doc` and no fuzz or benches directory, and the Alacritty tree has no fuzz directory. The CI workflow I read (`.github/workflows/ci.yml`) runs `cargo test` on windows-latest and macos-latest, a test with no default features, an oldstable toolchain run and clippy.

### Ghostty (Zig unit tests, snapshots, AFL++, benchmarks)

Unit tests. A search for `test "` blocks finds 4,594 in `src` and 3,460 in `src/terminal` (a lower bound, since unnamed tests are not counted). The biggest files are `Terminal.zig` (472), `PageList.zig` (298), `Screen.zig` (230), `stream_terminal.zig` (149), `c/terminal.zig` (145), `formatter.zig` (125), `stream.zig` (93), `kitty/graphics_storage.zig` (76), `kitty/graphics_exec.zig` (68), `osc/parsers/semantic_prompt.zig` (65), `osc/parsers/osc9.zig` (62) and `page.zig` (55). Tests sit next to the code they test, in the same file. The page and style tests are small, direct tests of memory layout: `page.zig` checks that a cell is zero by default, that a page layout with maxed capacity works, and sweeps `adjust capacity` for columns down to 1 and up past the limit; `PageList.zig` holds the scrollback, resize and reflow tests, which is where the largest share of a terminal's bugs sit.

Binary snapshots. `src/terminal/snapshot` contains a serialization format for a whole terminal (`page.zig`, `style.zig`, `screen.zig`, `history.zig`, `kitty.zig`, `hyperlink.zig`, `grid.zig`, a Kaitai definition `snapshot.ksy`, and `fixture.zig`). Its AGENTS.md says to validate on encode and degrade gracefully on decode. This gives Ghostty a way to store and compare full terminal state.

Comparison with xterm. I found no automated xterm comparison harness in the repository tree. There is a comment in `Terminal.zig` about "verifying the behavior with xterm", which means the comparison is done by hand when a test is written. The automated comparison is esctest, described above, which is calibrated to xterm's answers.

Fuzzing. `test/fuzz-libghostty` uses AFL++ with three targets: `osc` (OSC parser with an allocator), `parser` (VT parser one byte at a time) and `stream` (a small `Terminal` driven through its stream handler, where the first input byte chooses between the SIMD slice path and the scalar path). The checked in corpora are `stream-cmin` (3,271 files), `parser-cmin` (616), `parser-initial` (50), `stream-initial` (24), `osc-initial` (20) and `osc-cmin` (20). A script, `replay-crashes.nu`, replays crashes. Source: https://github.com/ghostty-org/ghostty/tree/main/test/fuzz-libghostty .

Benchmarks. `src/benchmark` has two roles. `ghostty-gen` generates input data and `ghostty-bench` consumes it. The benchmark list is `ApcParser`, `OscParser`, `TerminalParser`, `TerminalStream`, `TerminalFormatter`, `TerminalResize`, `TerminalSnapshot`, `ScreenClone`, `ScrollbackCompression`, `PageCompression`, `HyperlinkMap`, `ShaperRows`, `CodepointWidth`, `GraphemeBreak`, `IsSymbol` and a C API benchmark. The AGENTS.md rules are useful for Termoil: generate the data first and benchmark it later, reuse the same files across revisions, use fixed seeds, use `hyperfine` with warmups and medians, build with ReleaseFast, and never run two benchmarks at once.

CI. The workflow (`.github/workflows/test.yml`) uses self provisioned runners (`namespace-profile-ghostty-*`) and has jobs that build the libghostty-vt library for macOS, Windows, Android, WebAssembly and freestanding targets, run `zig build test-lib-vt` on Windows, and run esctest with `continue-on-error`.

### WezTerm (termwiz and wezterm-term)

The terminal model tests are in `term/src/test` (`mod.rs`, `c0.rs`, `c1.rs`, `csi.rs`, `image.rs`, `selection.rs`). `mod.rs` defines a `TestTerm` wrapper with a fixed scrollback configuration, a local clipboard and helper methods `print`, `set_mode` and `soft_reset`, and a helper `assert_visible_contents` that compares the visible lines against an array of strings and reports the file and line of the failing assertion. The selection tests are commented out in `mod.rs` with a note to port them to the render layer. A search finds 134 `#[test]` functions across `term/src` (58), `termwiz/src` (53) and `vtparse/src` (23). `termwiz` also has a benchmark (`benches/cell.rs`) and a key tester example (`examples/key_tester.rs`). CI is generated per platform (`gen_windows.yml`, `gen_macos.yml`, many Linux distributions) by https://github.com/wezterm/wezterm/tree/main/.github/workflows .

### kitty (Python suite over a C and Python screen model)

`./test.py` runs `kitty_tests/`, which holds about 60 Python modules and 706 test methods (the biggest: `dnd.py` 151, `screen.py` 67, `tab_drop.py` 66, `layout.py` 50, `graphics.py` 36, `datatypes.py` 28, `fonts.py` 23, `parser.py` 21). The screen model is tested directly: `BaseTest.create_screen(cols, lines, scrollback, cell_width, cell_height)` builds a `Screen` with a callbacks object and no window, then tests write text and escape sequences to it and assert on lines, cursor and history. `parser.py` checks the parser on byte level, including parser threading, SIMD UTF-8 decode, `test_csi_codes`, `test_osc_codes`, `test_dcs_codes`, `test_graphics_command` and malformed CSI subparameter cases. Tests that need a child process use a `PTY` class and `create_pty`. Fonts for ligature tests are bundled (Fira Code, Cascadia Code, Iosevka and others). CI (`.github/workflows/ci.yml`) runs on ubuntu-latest across Python versions and compilers, with `sanitize` on and off, on macos-latest, and runs a type check. Source: https://github.com/kovidgoyal/kitty/tree/master/kitty_tests .

### xterm.js (unit tests, Playwright, fixtures, benchmarks)

- 105 `*.test.ts` unit test files across `src` and `addons`, run in CI on ubuntu, macos and windows with Node 22.
- `test/playwright` holds `CharWidth`, `InputHandler`, `MouseTracking`, `Parser`, `RenderService`, `Renderer`, `SharedRendererTests` and `Terminal` tests, and each addon has its own `playwright.config.ts`. The browser jobs in CI run on ubuntu 22.04, with a comment that macos is flaky.
- `test/fixtures/escape_sequence_files` holds 76 pairs of `.in` (input bytes) and `.text` (expected screen). The NOTES file says the `.text` files were produced by running the input in a real xterm resized to 80x25 and pasting the window output. That is a recorded golden from a reference terminal, and the screen shape is fixed at 80x25.
- Benchmarks: `test/benchmark` (`EscapeSequenceParser`, `Terminal`, `DecorationService`, `Event`) plus serialize and Unicode graphemes addon benchmarks, using `RuntimeCase` for milliseconds and `ThroughputRuntimeCase` for MB per second.

Source: https://github.com/xtermjs/xterm.js .

### Windows Terminal and conhost

Tests use TAEF (the Windows Test Authoring and Execution Framework, `TEST_METHOD` macros). Counts of `TEST_METHOD` found: `src/terminal/adapter/ut_adapter` 74 (the adapter that turns parsed escape sequences into console operations, including `kittyKeyboardProtocol.cpp`, `inputTest.cpp` and `MouseInputTest.cpp`), `src/terminal/parser/ut_parser` 98 (output and input state machines), `src/cascadia/UnitTests_TerminalCore` 54 (the terminal core without UI: buffer, scroll, selection, input, API), `src/host/ut_host` 337 (conhost unit tests), `src/host/ft_host` 138 (functional tests against the real console API), `src/buffer/out/ut_textbuffer` 14. There are UI Automation smoke tests (`src/cascadia/WindowsTerminal_UIATests`). Fuzzing is in `src/host/ft_fuzzer` with a libFuzzer entry point, built under a `Fuzzing` configuration and run in CI through OneFuzz (`doc/fuzzing.md`, `build/pipelines/fuzz.yml`). Source: https://github.com/microsoft/terminal . The licence is MIT.

### foot

foot has one C test file (`tests/test-config.c`, configuration parsing) and relies on vtebench and its own `scripts/benchmark.py`, which writes a file to stdout a number of times (default 20 iterations) and records the time per iteration. `doc/benchmark.md` holds result tables for foot, Alacritty, urxvt and xterm from 2022 on an i9 9900 and an i5 8250U (for example "dense cells" 29.58 ms for foot with PGO against 97.45 ms for Alacritty 0.10.1 and 2323 ms for xterm 372). Source: https://codeberg.org/dnkl/foot .

### Contour

Contour has the largest in repo test culture of the non Ghostty projects: a search finds 3,186 `TEST_CASE` or `TEST_METHOD` entries in `src/vtbackend`, `src/vtpty` and `src/vtconformance`, with separate test files for the Sixel parser, Kitty graphics, ReGIS, search, images and color. Three parts matter to Termoil.

1. `src/vtconformance` runs vttest and esctest against a headless `vtbackend::Terminal` (https://github.com/contour-terminal/contour/tree/master/src/vtconformance ). The design document says esctest gates the build and vttest is judged by golden screen dumps. It records these rules:
   - The harness answers a vttest prompt when the bytes of the prompt arrive and captures a golden only at the "Push RETURN" banner, so a capture never happens in the middle of a drawn screen. A separate timer exists only to detect a program that has gone silent.
   - `EAGAIN` means "no data right now" and must not be read as silence.
   - vttest's `*` runs every menu item, and some items are toggles or submenus, so reading it as "run every test" silently measures nothing. Chapter 11.6 turned colour off for the colour tests until this was found.
   - A golden is "recorded" until someone reviews it against vttest's own claim for that screen, and both states gate. Blessing a golden is a separate explicit command, never a side effect of a failing run.
   - Known failures are kept in `known-gaps.txt` and `esctest-known-failures.txt` with a justification per entry. The suite fails on a new gap and also reports entries that no longer fail.
   - 143 goldens across 18 visual scenarios have been reviewed.
2. `src/vthost/latency` is a latency harness (section 5).
3. The Qt `offscreen` platform draws no frames, so a probe under it times out; Contour uses a headless weston compositor through `scripts/run-display-tests.sh` for display tests on Linux.

### libvterm and its text test format

libvterm (https://github.com/neovim/libvterm , a fork of https://www.leonerd.org.uk/code/libvterm ; neovim now carries its own copy in `src/nvim/vterm`) has 44 `.test` files in `t/` and a C harness `t/harness.c`, driven by `t/run-test.pl`, about 3,460 lines in all. The licence is MIT. The format is line based. `INIT` creates a terminal, `WANTSTATE`, `WANTSCREEN`, `WANTPARSER` and `WANTENCODING` choose which callbacks to print, `PUSH "..."` feeds bytes (with `\e`, `\x01` and similar escapes), `RESIZE`, `INKEY` and `INCHAR` send input, `DAMAGEMERGE` and `DAMAGEFLUSH` test damage reporting, `$SEQ a b: ...` loops, a line starting with `!` names the case, and lines indented by two spaces are the expected output from the harness (for example `?cursor = 0,3` and `output "\e[65;5u"`). The files are grouped by number: parser (02), encoding (03), state (10 to 40: putglyph, move cursor, scroll, edit, encoding, mode, resize, mouse, termprops, wrapping, tab stops, save, input, query, reset, double width and height, fallback, pen, rep, flow, selection), screen (60 to 69: ascii, unicode, damage, resize, pen, protect, extent, double width, termprops, pushline, reflow) and vttest derived scenes (90). Because the expectations are plain text, they are readable by a person and portable to any implementation, which is why other projects reuse them as a source of cases.

### vtebench

vtebench (https://github.com/alacritty/vtebench ) measures only how quickly a terminal reads from its PTY. Its README says it lacks frame rate and latency and tells readers not to draw broad conclusions. Each benchmark is a directory with an executable `benchmark` whose stdout is the payload and an optional `setup`. The default scenarios are `cursor_motion`, `dense_cells`, `light_cells`, `medium_cells` (a recorded escape heavy vim session), `scrolling`, `scrolling_bottom_region`, `scrolling_bottom_small_region`, `scrolling_fullscreen`, `scrolling_top_region`, `scrolling_top_small_region`, `sync_medium_cells` (the same session wrapped in synchronized update sequences) and `unicode`. The runner takes a number of warmup runs, a maximum duration in seconds and an optional maximum sample count, records each sample in milliseconds and reports the median and spread; `--dat` output feeds gnuplot scripts. The licence is Apache 2.0 or MIT.

### Typometer, Zutty and published benchmark sets

- Typometer (https://github.com/pavelfatin/typometer , Apache 2.0, Java 8 or later). It types a dot at a chosen interval using operating system key events, reads one pixel per character from the screen and times the gap between key press and the pixel changing. A synchronous mode waits for each character before typing the next and is the most accurate. It warns that compositors add delay (at least one frame under Windows Aero) and that gradient backgrounds and low contrast text break its metric detection. The page does not mention terminals or Wayland.
- Dan Luu's 2017 terminal latency study (https://danluu.com/term-latency/ ) sent the "." key at about 10.3 characters per second, 10,000 keypresses per measurement, on a 2014 MacBook Pro, idle and with a Rust compile running. The figures at the 50th and 99.9th percentile were, idle: alacritty 31 and 36 ms, terminal.app 6 and 25, st 25 and 63, iterm2 44 and 60, emacs-eshell 5 and 17. The suckless project disputes the rigour of the macOS setup.
- kitty's performance page (https://sw.kovidgoyal.net/kitty/performance/ ) describes `kitten __benchmark__`, which sends large data of ASCII, Unicode, CSI mix and Image types to the tty and times the parse, in MB per second, with rendering suppressed unless `--render` is given. Its table (kitty 0.33 average 134.55 MB/s, alacritty 0.13.1 54.05, wezterm 48.5, xterm 389 30.72, run on a Ryzen 7 PRO 5850U under X11) comes from the author. Latency is referred to third party Typometer runs on Linux and a hardware tester on macOS. It also measured CPU use while scrolling `less` as a proxy for energy.
- Zutty (https://github.com/alanswanson/zutty ). A search says its README carries its own latency comparison against X terminals and that outside rankings put it behind xterm. I could not read the figures or the method, so I do not cite numbers.
- Contour's latency work (section 5) is the most complete published method I read.

---

## 3. Golden and snapshot approaches, rendering and CI

### Byte stream in, grid state out

This is the dominant approach and the cheapest. The forms in use are: Alacritty's recording plus `grid.json` (full cell structure including attributes), xterm.js's `.in` plus `.text` (characters only, from xterm), WezTerm's `assert_visible_contents` (an array of expected lines), libvterm's text script (cursor, pen and screen callbacks) and Ghostty's inline Zig tests plus a binary snapshot format. Termoil should keep two grid forms: a plain text dump (one string per row, easy to review in a diff) and a full cell dump (character, width, foreground, background, underline colour, attribute flags, hyperlink, wrap flag) for the cases where attributes matter. A recording should be a raw byte file so one input can be replayed with every chunking of the bytes (whole, one byte at a time, random splits); Contour's note that a byte sequential parser must give the same screen wherever a read ends is the property to test.

### Screenshot comparison with tolerance

The pattern used by wgpu is the best documented. Its GPU test job uploads `*-actual.png` and `*-difference.png` as artifacts when a comparison fails, and its test documentation says tests compare with `nv-flip` and require outputs to be within tolerance of the expected image, with no tolerance number stated (https://github.com/gfx-rs/wgpu/blob/trunk/docs/testing.md ). Glyph level comparison needs a pinned font file, pinned size and no subpixel positioning drift: render each glyph from a fixed string into a small image and compare each cell's pixels, which is how a font or atlas bug shows up as one cell and not as a whole frame difference. Comparing at cell granularity also lets a test say which row and column differ. The three tolerance settings to record per test are the maximum per channel difference, the fraction of pixels allowed to exceed it, and whether anti aliasing pixels at glyph edges are excluded.

### Rendering headless with wgpu

The wgpu repository's own CI job (`.github/workflows/ci.yml`, job `gpu-test`) runs on windows-2022, macos-14 and ubuntu-24.04, with steps that install DXC, WARP, the Agility SDK, Mesa and the Vulkan SDK on Windows, the Vulkan SDK on macOS, and the Vulkan SDK and Mesa on Linux. It runs `cargo --locked xtask test --llvm-cov`, prints the GPU configurations it found at the end, and sets `LVP_POISON_MEMORY=true` on Linux, which the testing documentation says makes the software Vulkan driver fill new memory with non zero values to catch uninitialized memory. It uses `WGPU_DX12_COMPILER=dxc` on Windows. The workflow does not mention Xvfb. Source: https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/.github/workflows/ci.yml . From this, the working set for Termoil is:

- Linux: Mesa's software Vulkan driver (lavapipe) through wgpu's Vulkan backend. Rendering to an offscreen texture needs no display server.
- Windows: WARP through the DX12 backend with a DXC shader compiler.
- macOS: the Metal backend on the runner (wgpu runs its GPU tests there), which I could not confirm is hardware backed or paravirtual from GitHub's documentation.
- Select the adapter by environment variable (`WGPU_ADAPTER_NAME`, `WGPU_BACKEND`) and have the test print which adapter it got, so a golden is not compared across adapters that round differently. Keep one golden set per adapter type, or a tolerance wide enough for the differences, and mark the software adapter as the reference for goldens.
- A wgpu issue (https://github.com/gfx-rs/wgpu/issues/1551 ) records an older failure where EGL initialization failed on a headless llvmpipe setup in CI and lavapipe crashed on an old Mesa, so pin the Mesa version.

### Testing a PTY end to end

Spawn the real program in a PTY, feed what it writes into an in process emulator, and assert on the emulator's grid. Rust crates in this space, from a search (not verified by running them): `portable-pty` from the WezTerm project (one API across Windows ConPTY, Unix PTY and SSH), the `vt100` crate as the emulator, `expectrl` and `rexpect` for matching on the byte stream, and newer wrappers `termlens`, `ttry` and `testty` that combine them. Termoil owns its emulator, so the Termoil core can be the screen model for these tests: a test harness spawns `sh` or `cmd` through ConPTY, types keystrokes as bytes through the same input encoder the GUI uses, waits for a marker that arrives in the byte stream, and reads the grid. The wait must be on a marker from the program and a long overall deadline, never on a short sleep, which is the lesson in Contour's document. kitty's `create_pty` and Contour's `TerminalHarness` are the two worked examples.

### CI on Windows, macOS and Linux

GitHub's table for standard runners in public repositories (https://docs.github.com/en/actions/reference/runners/github-hosted-runners ): Linux x64 4 vCPU 16 GB (`ubuntu-latest`, `ubuntu-24.04`, `ubuntu-26.04`), Linux arm64 4 vCPU 16 GB, Windows x64 4 vCPU 16 GB (`windows-latest`, `windows-2025`, `windows-2022`), Windows arm64 4 vCPU 16 GB, macOS Intel 4 vCPU 14 GB (`macos-15-intel`, `macos-26-intel`) and macOS arm64 3 vCPU (M1) 7 GB (`macos-latest`, `macos-14`, `macos-15`, `macos-26`). Each has 14 GB of storage. The page lists no GPU for standard runners and mentions GPUs only for larger runners on paid plans. It states that nested virtualization is not supported on macOS runners. There is a one vCPU Linux runner, `ubuntu-slim`, with 5 GB, which is too small for builds.

Consequences. Performance numbers from these shared runners vary with the neighbours on the host, so use them to catch large regressions (for example 30 percent) and not to publish figures. xterm.js moves its browser tests to ubuntu 22.04 because macOS was flaky. For Linux display tests use a virtual X server (Xvfb) for an X11 build and a headless Wayland compositor such as weston for a Wayland build, since Contour reports that Qt's offscreen platform draws no frames; wgpu rendering to a texture needs neither. On Windows the PTY tests run against ConPTY, which the Windows runners provide.

---

## 4. Property based and fuzz testing

What exists. Among the repositories I searched for files with `fuzz`, `proptest` or `quickcheck` in the path, only Ghostty (AFL++ harnesses with checked in corpora) and Windows Terminal (libFuzzer and OneFuzz, `ft_fuzzer`) have fuzzing, and none of the others (Alacritty, vte, kitty, WezTerm, xterm.js, foot, libvterm, Contour) has any in its tree. That does not prove they never fuzz outside the repository. Outside the repositories, the dgl.cx article on terminal security (https://dgl.cx/2023/09/ansi-terminal-security ) found ten CVEs in terminal emulators through escape sequence handling, and an older oss-security thread records AFL use on rxvt, tmux, screen and mosh (https://www.openwall.com/lists/oss-security/2017/05/01/20 ).

What Ghostty's targets check. The three targets only check that the code does not crash, hang or trip a sanitizer on any input: OSC with large payloads and the three terminator variants, the bare VT parser, and the full stream into a small terminal with both the fast slice path and the scalar path. The corpora are seeds minimized by `afl-cmin` (the `cmin` suffix).

Differential testing. I found two published patterns. Termless (https://termless.dev/emulator-differences.html ) sends the same escape sequences to several emulator backends with xterm.js as the reference, compares screen text and scrollback, logs known divergences instead of failing, and lets the reference backend gate CI. The tuios blog post (https://tuios.dev/blog/the-fuzzer-that-found-nothing ) fuzzes with structured sequences and hostile parameters, checks invariants after every step, and replays emitted frames to confirm they redraw the same screen. I did not find a published harness that compares grids against VTE or libvterm directly. Differential testing against libvterm is practical because it is a small C library with `vterm-dump` in `bin/`, and against xterm through esctest's DECRQCRA readback.

Recommendation for Termoil. Use `cargo fuzz` (https://rust-fuzz.github.io/book/cargo-fuzz.html ) for these targets: parser alone, parser plus screen, OSC and APC and DCS payloads with allocation limits, kitty graphics command parser, sixel decoder, key encoder, mouse encoder, resize and reflow with a random sequence of operations. Use `proptest` (https://proptest-rs.github.io/proptest/ ) for properties that need no oracle: the same bytes in any chunking give the same grid; the cursor is always inside the grid; every row has exactly the column count; wide characters never leave half a cell; resize to a new size and back keeps the text of the visible rows when no content was lost; scrollback never exceeds its limit; no sequence makes memory grow without bound.

---

## 5. Performance tests

Throughput. Write a large file to the terminal and time how long the terminal takes to read it, which is what vtebench does for twelve patterns, and what `kitten __benchmark__` does for four data types. Ghostty's `ghostty-bench` times the parser and the stream with no GPU involved, which isolates the parser and the screen model from rendering. The result vtebench prints is median milliseconds for a fixed payload, so lower is better.

Latency. Three methods. (1) Typometer style: send real key events, read a pixel (Pavel Fatin's tool). (2) Hardware: a camera or photodiode (kitty's page points to a hardware tester on macOS). (3) Instrumented: Contour's `bench-latency` timestamps the key event on the GUI thread and the frame swap that first draws the echoed cursor column, using the system wide steady clock, with a tiny echo program as the child that prints a prompt and echoes every byte and wraps after 60 columns, and keys typed 25 to 35 ms apart with random gaps so the key presses do not line up with the display refresh. It leaves swaps unpaced because a 60 Hz vsync rounds every sample to 16.7 ms. It reports p50, p90, p99 and max in microseconds and gates on a budget (the daemon path may cost at most 1 ms at the median for a keystroke and 10 ms at the median for launch to first prompt). Source: https://github.com/contour-terminal/contour/blob/master/docs/internals/latency.md . Dan Luu's study is the template for the field method: 10,000 keypresses per figure, idle and under load, 50th and 99.9th percentile.

Frame time. wgpu timestamp queries or the swap to swap interval under a scripted workload (scrolling, full screen redraw, a cursor blink with nothing changing). Report the p99 as well as the mean, because dropped frames show up there.

Memory. Fill 100,000 and 1,000,000 lines of scrollback with typical text and with dense attribute text, then record resident memory. Ghostty has benchmarks for `ScrollbackCompression`, `PageCompression` and `ScreenClone` and a compact page layout, which show what the question looks like for a mature implementation.

Startup. Time from process launch to the first frame that shows the prompt, measured as Contour does with a probe reported by the GUI itself, with cold and warm cases kept apart.

How emulators publish. foot commits dated tables in `doc/benchmark.md` with the machine, geometry, font and scrollback listed. kitty publishes a table with the hardware and a statement that the same font, font size and window size were used. Contour publishes a method document and a gate, and no numbers. Ghostty publishes rules for running benchmarks and comparing branches and no numbers. Termoil should publish the machine, the build flags, the number of samples, median and p99, and the commit.

---

## 6. Input testing

Keyboard protocol encoding tables. The most complete tables to build from are kitty's `test_encode_key_event` (read, not copied, because of the GPL 3 licence), Ghostty's `key_encode.zig` tests, the Windows Terminal adapter tests and libvterm's `25state_input.test`. A table has one row per key and modifier combination and one column per mode: legacy, xterm modifyOtherKeys is not needed (the kitty page recommends against it), application cursor keys on and off, application keypad on and off, and each of the kitty flag combinations (1, 3, 5, 9, 25 and the others the program can push).

Known tricky cases, from the kitty specification and its notes:
- Ctrl+I is Tab, Ctrl+M is Enter, Ctrl+R equals Ctrl+Shift+R, and Ctrl+Space is NUL in legacy mode. Alt+[ produces the CSI introducer in legacy mode.
- Shift+Tab is `CSI Z` in legacy mode; Ctrl+Shift+Tab is `CSI 9;6u` with the kitty flags.
- Enter, Tab and Backspace keep their legacy bytes under flag 1 so a person can type `reset`, and have no release events unless flag 8 is set.
- Ctrl+C stops being SIGINT under flag 1 and arrives as an escape sequence.
- The key code is always the unshifted lowercase codepoint (Ctrl+Shift+A is 97 with modifier 6).
- Keypad keys report as ordinary keys in legacy mode and as private use codes under the flags.
- `CSI R` for F3 was dropped because it collides with the cursor position report.
- Main and alternate screens keep separate flag stacks; popping an empty stack resets the flags; a full stack drops its oldest entry.
- Some terminals swap Backspace and Ctrl+Backspace; the `kbs` terminfo entry tells programs which.
- Lock modifiers are not reported for text keys under flag 1.

Mouse encoding: X10, normal (1000), button event (1002), any event (1003), SGR (1006), urxvt (1015) and SGR pixel (1016) encodings, with the coordinate limits of the legacy form (223 columns). Ghostty's `mouse_encode.zig` has 24 tests and Windows Terminal has `MouseInputTest.cpp`.

Windows. Ctrl+Alt is how AltGr arrives from the keyboard, so a rule that treats Ctrl+Alt as a shortcut breaks layouts where AltGr types characters (the Blender commit https://developer.blender.org/rB8ef05d3180b1b54c7647d059a1f8a6c610937bc9 documents this for another application). `ToUnicodeEx` changes the kernel keyboard buffer, so calling it to inspect a key can clear a pending dead key; Microsoft's documentation describes a flag that leaves the state unchanged on Windows 10 version 1607 and later (https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-tounicodeex ). Windows Terminal issue 5784 (https://github.com/microsoft/terminal/issues/5784 ) records a dead key that stays pending after the key was used as a shortcut, and discussion 12130 (https://github.com/microsoft/terminal/discussions/12130 ) argues a console program cannot tell a dead key from an accelerator.

IME and dead keys. I did not find an IME test corpus in any of the projects I read. The practical test is at the boundary between the windowing library and the terminal: a scripted sequence of preedit start, preedit update, commit and cancel events with a fake IME, checked for what the terminal writes to the PTY (only the committed text, never the preedit), where the preedit is drawn (at the cursor), what happens when the cursor moves or the focus is lost during composition, and that a dead key followed by a space or the same dead key produces the accent character. This part has to be built in house and then confirmed by hand with real Japanese, Chinese and Korean input methods and a European dead key layout on each operating system, because no CI runner has an input method.

---

## Recommended test suite structure for Termoil

Counts are targets for the first release, chosen so each category can be written, reviewed and kept fast. The total is about 4,300 automated tests plus 14 fuzz targets and property tests, and a conformance run on top.

| Category | Target count | Source |
|---|---|---|
| 1. Parser state machine | 300 | In house. Table driven: every state and every byte class, split sequences, C1 controls in UTF-8 mode, aborted sequences (CAN, SUB, ESC inside a sequence), over long parameters, subparameters, OSC and DCS terminators (BEL and ST), UTF-8 errors. |
| 2. Control function behaviour, text script format | 1,400 | In house, in a libvterm style `.test` file format (text in, expected cursor and rows out), written by hand and by porting cases from libvterm (MIT). Covers cursor movement, margins, origin mode, wrap and the pending wrap flag, tab stops, insert and delete, erase, scrolling regions, alternate screen, save and restore, DECSTR and RIS, modes, character sets, double width and height lines, SGR, hyperlinks, titles, reports. |
| 3. Recorded sessions, grid golden | 80 | In house recordings in Alacritty's layout (recording, size, config, expected grid), including real vim, tmux, htop, less, zsh and fish sessions, each replayed with whole, byte at a time and random chunking. Alacritty's recordings can be reused under Apache 2.0 as inputs, with expected grids regenerated and reviewed. |
| 4. External conformance, run not vendored | esctest2 (559 test functions) plus vttest (18 scenarios) | esctest2 is GPL 2 and is fetched at a pinned commit in CI. vttest is BSD and is fetched or built in CI. Use Contour's method: esctest gates after the first pass, vttest through reviewed goldens, with a ratchet file of known failures that fails on new entries. |
| 5. Unicode | 700 | GraphemeBreakTest (Unicode licence, check the header of the file), plus an in house width table test (EastAsianWidth, emoji presentation, variation selectors, ZWJ sequences, regional indicator pairs, combining marks at line start, wide character at the last column), as the width behaviour is a Termoil decision. |
| 6. Input encoding | 700 | In house tables: legacy xterm keys with modifiers, application modes, kitty keyboard flag combinations, mouse encodings, bracketed paste, focus events. Cases cross checked against kitty, Ghostty and libvterm behaviour. |
| 7. Scrollback, resize, reflow, selection, search | 300 | In house. Resize with wrapped lines, shrink to one column and grow back, resize on the alternate screen, selection across wrapped lines and wide cells, scrollback limit, clear scrollback. |
| 8. Graphics | 180 | In house kitty graphics tests (transmit, put, delete, animation, placeholders, limits, malformed), a sixel decoder test set built on the CC0 vt340test images, and a smaller inline image set. |
| 9. Rendering golden images | 60 | In house. A fixed font, a software adapter (lavapipe on Linux, WARP on Windows, Metal on macOS), cell level comparison with per test tolerance, difference images saved as CI artifacts. Cases: every attribute combination, underline styles, cursor shapes, selection, wide glyphs, emoji, box drawing, ligatures if supported, scroll offset, resize. |
| 10. PTY end to end | 50 | In house. Spawn a shell through the PTY layer on each OS (ConPTY on Windows), type, run commands, resize, exit codes, signals, large output, a program that sends reports and one that reads them. Wait on markers, not sleeps. |
| 11. Fuzz and property | 14 fuzz targets, 25 properties | `cargo fuzz` and `proptest` as listed in section 4. A short fuzz run (60 seconds per target) on every pull request and a long run nightly, with the corpus checked in. |
| 12. Performance | 12 vtebench scenarios plus 6 in house scenarios | vtebench's scenarios run unchanged (Apache 2.0 or MIT), plus unicode heavy, kitty graphics, scrollback fill, resize storm, latency probe and startup. A regression gate at a wide margin on CI, publishing runs on a fixed machine. |
| 13. Input method and dead keys | 60 automated plus a manual script | In house with a fake IME event source, plus a written manual checklist per operating system. |

### Which external corpora to vendor and with what licence

Copy into the repository (the licence allows it, keep the licence file beside the data):
- vtebench scenario scripts: Apache 2.0 or MIT.
- libvterm test files: MIT. Rewrite expectations as Termoil needs, and keep a note of origin.
- Alacritty recordings used as input bytes: Apache 2.0. Regenerate and review the expected grids.
- hackerb9 vt340test sixel images: CC0.
- Unicode data files (GraphemeBreakTest.txt, BidiTest.txt, BidiCharacterTest.txt, EastAsianWidth.txt, emoji-test.txt): the Unicode licence, which permits redistribution with the notice. Pin the Unicode version and update on a schedule.
- xterm.js `.in` and `.text` fixtures: MIT. They are characters only at 80x25, from xterm.

Fetch and run in CI, never copy into the repository:
- esctest2: GPL 2.
- vttest: BSD, fetch to avoid carrying its source and its menu.
- kitty and its tests: GPL 3. Read the tables, write your own.

Check the licence before use: libsixel sample images, any Windows Terminal data (MIT, so usable) and Ghostty test data (verify the licence in its repository before copying).

### What to build in house

1. The text script format and its runner (category 2). It should use plain text files, one expected output per step, and a flag to rewrite expectations that must be passed explicitly, following Contour's rule that blessing is a deliberate act.
2. The recording tool: a flag in the Termoil binary that saves the PTY bytes, size and the resulting grid, as Alacritty's `--ref-test` does, plus a replay tool that applies every chunking.
3. The conformance harness: runs esctest2 and vttest against the headless core under a PTY, with Contour's rules (answer on markers, a separate silence timer, goldens only at the hold prompt, a ratchet file of known failures with a reason on each line).
4. A cell dump format and a diff tool that prints row and column of each difference, and an image comparison tool with per test tolerance and a difference image.
5. A latency probe in the application, as Contour has, with an echo child, key times read on the GUI thread and the frame swap time, and a benchmark runner that publishes p50, p90, p99 and max.
6. A fake IME and a fake keyboard source for the windowing boundary.
7. A corpus management script for the fuzz corpora (minimize, replay crashes), as Ghostty has.

### Order of work

First the parser tables and the text script runner (categories 1 and 2), then recordings (3), then esctest2 in a non gating job so the first failures appear early, then vttest goldens, then Unicode and input tables. Rendering goldens, PTY end to end and performance come once a renderer exists. Fuzz targets are added as each component lands, because a parser fuzz target costs an hour to write and finds the first crash within minutes.

---

## Things I could not verify

- The contents of the terminal-wg repositories (esctest, specifications, terminal-parsing, bidi): the site returned an access denied page.
- Zutty's latency method and numbers: the GitHub repository path I tried returned 404 and the documentation page I guessed returned 404. The repository that a search returned is https://github.com/alanswanson/zutty .
- The Casey Muratori refterm and termbench repositories: searches returned nothing about them, so I make no claim.
- Whether libvterm runs under OSS-Fuzz, and whether Alacritty or kitty fuzz outside their own repositories.
- Whether the macOS runners provide a hardware or paravirtual Metal device: GitHub's page does not say, though wgpu's CI runs its GPU tests on macos-14.
- Any IME test corpus.
- Test counts are search pattern counts and are lower bounds for Ghostty (named tests only), Windows Terminal (`TEST_METHOD` only, so data driven cases are not counted) and Contour (`TEST_CASE` and `TEST_METHOD` at the start of a line).

## URL index

- vttest: https://invisible-island.net/vttest/ , https://invisible-island.net/vttest/manpage/vttest.html
- xterm control sequences: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
- esctest2: https://github.com/ThomasDickey/esctest2 ; original https://github.com/gnachman/esctest ; terminal-wg https://gitlab.freedesktop.org/terminal-wg/esctest
- Alacritty ref tests: https://github.com/alacritty/alacritty/tree/master/alacritty_terminal/tests ; vte https://github.com/alacritty/vte ; vtebench https://github.com/alacritty/vtebench
- Ghostty: https://github.com/ghostty-org/ghostty (`src/terminal`, `src/benchmark`, `test/esctest`, `test/fuzz-libghostty`, `.github/workflows/test.yml`)
- WezTerm: https://github.com/wezterm/wezterm (`term/src/test`, `termwiz`, `vtparse`, `bidi`)
- kitty: https://github.com/kovidgoyal/kitty/tree/master/kitty_tests ; keyboard protocol https://sw.kovidgoyal.net/kitty/keyboard-protocol/ ; performance https://sw.kovidgoyal.net/kitty/performance/
- xterm.js: https://github.com/xtermjs/xterm.js
- Windows Terminal: https://github.com/microsoft/terminal
- foot: https://codeberg.org/dnkl/foot
- Contour: https://github.com/contour-terminal/contour (`docs/internals/vt-conformance.md`, `docs/internals/latency.md`, `src/vtconformance`, `src/vthost/latency`) ; https://github.com/contour-terminal/terminal-unicode-core
- libvterm: https://github.com/neovim/libvterm ; upstream https://www.leonerd.org.uk/code/libvterm
- Unicode: https://www.unicode.org/Public/UCD/latest/ucd/auxiliary/GraphemeBreakTest.txt
- Sixel: https://github.com/hackerb9/vt340test , https://jexer.sourceforge.io/sixel.html , https://gitlab.gnome.org/GNOME/vte/-/issues/258
- Latency: https://github.com/pavelfatin/typometer , https://danluu.com/term-latency/ , https://github.com/alanswanson/zutty
- wgpu CI and testing: https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/.github/workflows/ci.yml , https://github.com/gfx-rs/wgpu/blob/trunk/docs/testing.md , https://github.com/gfx-rs/wgpu/issues/1551
- GitHub runners: https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- Fuzz and differential: https://rust-fuzz.github.io/book/cargo-fuzz.html , https://proptest-rs.github.io/proptest/ , https://termless.dev/emulator-differences.html , https://tuios.dev/blog/the-fuzzer-that-found-nothing , https://dgl.cx/2023/09/ansi-terminal-security , https://www.openwall.com/lists/oss-security/2017/05/01/20
- Windows input: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-tounicodeex , https://github.com/microsoft/terminal/issues/5784 , https://github.com/microsoft/terminal/discussions/12130
