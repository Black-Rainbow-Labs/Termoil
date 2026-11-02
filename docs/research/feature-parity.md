# Termoil feature parity research

Date of research: 2026-11-01. Subject: what Termoil must match to reach parity with Ghostty, Alacritty, WezTerm, Kitty and Windows Terminal, with Rio added as a sixth reference.

## How to read this report

Every cell in the matrix is either confirmed from a page fetched during this research, or marked **(u)**. **(u)** means the cell comes from general knowledge of the project and was not confirmed against a document in this session. The team should verify each **(u)** cell before treating it as a requirement. Sources for confirmed cells are in section 2, under each emulator. Where a row has no figure, no published figure was found.

Versions and dates found this session:

| Emulator | Latest version seen | Date | Source |
|---|---|---|---|
| Ghostty | 1.4.0 | 2026-10-27 | https://ghostty.org/docs/install/release-notes |
| Alacritty | 0.17.0 released, 0.18.0 in development | no date on page | https://raw.githubusercontent.com/alacritty/alacritty/master/CHANGELOG.md |
| WezTerm | last tagged release 20240203-110809-5046fc22 | 2024-02-03 | https://api.github.com/repos/wezterm/wezterm/releases |
| Kitty | 0.49.2 | 2026-10-01 | https://sw.kovidgoyal.net/kitty/changelog/ |
| Windows Terminal | 1.25 stable, 1.24 servicing builds | mid 2026 | https://github.com/microsoft/terminal/discussions/20758 |
| Rio | 0.5.29 | 2026-11-01 | https://api.github.com/repos/raphamorim/rio/releases |

## 1. Feature matrix

| Feature | Ghostty | Alacritty | WezTerm | Kitty | Windows Terminal | Rio |
|---|---|---|---|---|---|---|
| Platforms | macOS, Linux. The Windows GUI roadmap was dropped in 1.4.0; libghostty supports Windows and is tested in CI | macOS, Linux, Windows, BSD | Linux, macOS, Windows 10, FreeBSD, NetBSD | macOS, Linux. Windows only through WSL | Windows only (u) | Windows, Linux, FreeBSD, macOS |
| Rendering backend | Metal on macOS, OpenGL on Linux (GTK renderer uses EGL with triple buffering since 1.4) | OpenGL (u); Metal listed by one third party blog | OpenGL, Metal on macOS per a third party blog (u: WezTerm also has a WebGPU front end) | OpenGL | DirectX through the AtlasEngine renderer (u) | wgpu via its Sugarloaf renderer (u) |
| Tabs | Yes, native UI | No | Yes | Yes | Yes | Features page does not confirm tabs |
| Splits and panes | Yes, native | No. Multiple windows only | Yes | Yes, seven layouts including Splits | Yes (panes) | Yes (split panels) |
| Multiplexing and sessions | No built in multiplexer (u) | No. Intended for use with tmux | Built in multiplexer over Unix sockets, SSH or TLS | Layouts, saved sessions, remote control; no persistent server | No multiplexer. Window persistence with or without scrollback added in 1.25 | No (u) |
| Config format and live reload | Plain text key = value; reload action (u) | TOML (TOML 1.1 in 0.17); live reload (u) | Lua file, hot reload | kitty.conf, a single text file; reload by signal or action (u) | JSON settings file with a settings UI; live reload (u) | TOML (u); live reload (u) |
| Themes count | "Hundreds" bundled | None bundled; community repository (u) | Several hundred built in color schemes (u) | Themes kitten with a large catalogue (u) | A small set of built in schemes (u) | A few built in themes (u) |
| Font ligatures | Yes | No | Yes | Yes (u) | Yes (u) | Yes (u) |
| Font fallback | Yes (u) | Yes (u) | Yes | Yes | Yes | Yes (u) |
| Emoji and color fonts | Yes. Multi codepoint emoji (flags, skin tones) render as single characters | Yes (u) | Color emoji | Yes (u) | Yes (u) | Yes (u) |
| True color | Yes (u) | Yes (u) | Yes | Yes | Yes (u) | Yes (u) |
| Unicode width and grapheme clustering (mode 2027) | Grapheme aware; Unicode 17 since 1.3.0. Mode 2027 support (u) | Unicode 17 added in 0.16.0. No grapheme clustering (u) | Unicode aware (u) | Own width handling (u) | Grapheme cluster support since preview 1.22 (Aug 2024). Setting for ambiguous width in 1.25 | (u) |
| Keyboard protocol (kitty CSI u, modifyOtherKeys) | Kitty protocol | Kitty protocol since 0.13.0 | Yes, listed by kitty | Originator | Kitty protocol in 1.25 | Yes, listed by kitty |
| Bracketed paste | Yes (u) | Yes (u) | Yes | Yes (u) | Yes (u) | Yes (u) |
| Mouse reporting (SGR, pixel 1016) | SGR yes (u); pixel (u) | SGR yes (u); pixel no (u) | SGR yes (u) | SGR and pixel (u) | SGR yes (u) | (u) |
| OSC 8 hyperlinks | Yes | Yes since 0.11.0 | Yes | Yes (u) | Yes (u) | Yes |
| OSC 52 clipboard | Yes | Yes. Paste disabled by default since 0.13.0 | Yes (u) | Yes (u) | Yes. A 1.25 fix removed a 5 second hang when OSC 52 write coincided with a title change | (u) |
| OSC 133 shell integration | Yes. 1.3.0 added click to move cursor in the prompt and command finished notifications. 1.4.0 added `click_events=2` | Not documented (u) | Zones, `ScrollToPrompt`, select command output | Yes. Jump between prompts, view last output in pager. A heap underflow in OSC 133 parsing was fixed in 0.49.2 | Yes since 1.18. Marks need `autoMarkPrompts` unless on by default in current builds | Shell integration listed on features page |
| Synchronized output (DEC 2026) | "Synchronized rendering" listed | (u) | (u) | Yes (u) | (u) | (u) |
| Kitty graphics protocol | Yes. 1.4.0 added animation and relative placements and says the published spec is complete | No | Yes | Originator | No (u) | Yes |
| Sixel | No (u) | No | Yes, marked experimental | No (u) | Yes since preview 1.22 | Yes |
| iTerm2 inline images | No (u) | No | Yes, with built in `imgcat` | No (u) | No (u) | Yes |
| Scrollback size and search | Search added in 1.3.0. Default scrollback limit rose from 10 MB to 50 MB in 1.4.0; idle scrollback is compressed; option `scrollback-limit-lines` | Configurable lines (u). Regex search forward and backward, plus vi mode | Searchable scrollback, Copy Mode, Quick Select | Pager based browsing, marks, search through kittens (u) | Search with regex support | (u) |
| URL detection and click to open | Yes (u) | Hints with regex; click to open with configurable modifiers | Yes (hyperlinks, Quick Select) | `detect_url_regex` option added in 0.49.2; hints kitten | Yes (u) | Hints and hyperlinks |
| Selection modes (block, word, line, smart) | Yes (u) | Double click semantic, triple click line, Ctrl for block; vi mode selection | Xterm style selection, Copy Mode | Word, line, column | Yes, including block (u) | Vi mode (u) |
| Copy on select | Option (u) | Option (u) | Option (u) | Option (u) | Option (u) | (u) |
| Quick terminal or dropdown | Quick Terminal on macOS | No | No (u) | Quick access terminal kitten | Quake mode action (u) | No (u) |
| Command palette | Yes (u) | No | Yes (u) | Command palette kitten | Yes | No (u) |
| Keybinding actions | Rich. Key tables, chained binds and `catch_all` added in 1.3.0 | Configurable actions | Lua defined | `map` with remote control commands | Actions editor added in 1.25 | Configurable (u) |
| SSH handling | Documented shell integration and SSH page; terminfo problems reported by users | None | Built in SSH client with native tabs | `ssh` kitten copies terminfo and bootstraps features | None (OpenSSH only) | None (u) |
| Sessions and restore | macOS restore (u) | None | Multiplexer server keeps sessions | Session files and `save_as_session` | Window persistence in 1.25 | None (u) |
| GPU usage and vsync | GPU renderer; hidden tabs release GPU resources (1.4.0) | GPU renderer | GPU renderer | GPU renderer; throughput up 15 to 35 percent in 0.49.0 | GPU renderer | GPU renderer |
| Input latency figures published | None published by project | None | None | Typometer based claim of best latency on Linux in project docs, no absolute figure | None | None |
| Startup time | "Instant" per third party blog | "Fastest" per third party blog, no figure | "Moderate" per third party blog | "Fast" per third party blog | (u) | (u) |
| Cursor styles and blink | Yes (u) | Yes (u) | Yes (u) | Yes (u) | Yes (u) | Yes (u) |
| Opacity and blur | Yes (u) | Opacity yes; blur on some compositors (u) | Opacity; Wayland blur option in nightly | Opacity and blur (u) | Acrylic and opacity (u) | Opacity and blur (u) |
| Shaders | No (u) | No | Not confirmed | Official custom shader support in 0.49.0, including cursor trails | Pixel shader effect (u). A 1.25 fix covers shaders that use `time` | RetroArch shader support |
| Window decorations | Native macOS and GTK | Configurable (u) | Configurable | Configurable, `window_border_radius` added in 0.49.0 | Native | (u) |
| Split focus follows mouse | Option (u) | Not applicable | Not confirmed | Not confirmed | Not confirmed | Not confirmed |
| Bell | Yes; notification on command finish | Yes (u) | Yes (u) | Yes (u) | Yes (u) | Yes (u) |
| Notifications (OSC 9, 99, 777) | OSC 9 yes. Desktop notification on long commands | (u) | (u) | OSC 99 and legacy OSC 9; OSC 777 not confirmed | (u) | (u) |
| XTGETTCAP and terminfo | XTGETTCAP is listed as a DCS use. Ships its own terminfo (xterm-ghostty) | alacritty terminfo (u) | wezterm terminfo (u) | xterm-kitty terminfo | Not applicable (ConPTY) | (u) |
| DECRQM and DECRPM | Yes (u) | Partial (u) | Yes (u) | Yes (u) | Partial (u) | (u) |
| DA1 and DA2 responses | Yes (u) | Yes (u) | Yes (u) | Yes (u) | Yes. ConPTY no longer waits for DA1 at startup (1.25) | (u) |
| Progress bars (OSC 9;4) | Yes | (u) | (u) | Yes since 0.47.0, drawn at the top of the window | (u) | (u) |
| Title and icon (OSC 0, 1, 2) | Yes | Yes (u) | Yes (u) | Yes (u) | Yes (u) | Yes (u) |
| Cwd reporting (OSC 7) | Yes | Not confirmed | Yes. New panes and tabs start in the reported directory | Yes (u) | Not confirmed | (u) |
| Color query (OSC 10, 11, 12 with ?) | Yes (OSC 10 through 19) | (u) | (u) | Yes (u) | (u) | (u) |
| 8 bit vs 7 bit C1 | 7 bit (u) | 7 bit (u) | 7 bit (u) | 7 bit (u) | 7 bit (u) | 7 bit (u) |
| xterm modes supported | Broad. VT reference is still a work in progress | Broad (u) | Broad (u) | Broad (u) | Growing; 1.25 fixed DECSTR behaviour with DRCS fonts | (u) |
| Accessibility (screen reader) | Native toolkit gives partial support (u) | Weak (u) | Weak (u) | Weak (u) | UI Automation support (u) | Weak (u) |
| IME input | Yes (u) | Yes (u) | Yes (u) | Yes (u) | Yes; 1.25 fixed IME candidate window position | Yes |
| Right to left text and bidi | Renders RTL scripts but only left to right layout | No (u) | No (u) | No (u) | No (u) | No (u) |
| Scripting and IPC | AppleScript on macOS (preview in 1.3.0) | `alacritty msg` | Lua API | Remote control over socket, watchers in Python | Command line arguments and actions | (u) |
| Plugins | None | None | Lua | Kittens (Python and Go) | Extensions settings page added in 1.24 | None (u) |
| Native UI vs custom | Native on macOS and GTK | Custom | Custom | Custom, avoids UI toolkits | WinUI (u) | Custom |
| Licence | MIT (u) | Apache 2.0 or MIT (u) | MIT (u) | GPL 3 (u) | MIT (u) | MIT (u) |

## 2. Per emulator summary

### Ghostty
Best at: native UI with tabs and splits on macOS and Linux, a GPU renderer, a simple config format, full Kitty graphics, the Kitty keyboard protocol, mature OSC 133 integration (click to move the cursor, command finished notifications), key tables, and in 1.4.0 compressed idle scrollback with a 50 MB default limit. Gaps: no Windows GUI and the project dropped that roadmap in 1.4.0; right to left layout is not supported; no sixel or iTerm2 images (u); users on Hacker News report high memory use, SSH terminfo problems, and, before 1.3.0, missing search. Sources: https://ghostty.org/docs/features, https://ghostty.org/docs/install/release-notes/1-3-0, https://ghostty.org/docs/install/release-notes/1-4-0, https://ghostty.org/docs/vt/reference, https://news.ycombinator.com/item?id=47207248.

### Alacritty
Best at: small footprint, stability, a TOML config and availability on every major desktop platform. It has vi mode, regex hints, regex search, multi window through `alacritty msg create-window`, the Kitty keyboard protocol and Unicode 17. Gaps by design: no tabs, no splits, no ligatures, no graphics protocols. OSC 52 paste is off by default. Sources: https://raw.githubusercontent.com/alacritty/alacritty/master/docs/features.md, https://raw.githubusercontent.com/alacritty/alacritty/master/CHANGELOG.md, https://www.pistack.xyz/posts/2026-08-10-ghostty-vs-alacritty-vs-wezterm-terminal-emulator-guide/.

### WezTerm
Best at: a built in multiplexer with SSH and TLS domains, Lua configuration and API, serial ports, iTerm2, Kitty and sixel images, and Windows support including ConPTY. Gaps: the last tagged release is dated 2024-02-03 and only nightly builds are visible after that, so maintenance risk is real. I found no 2026 announcement either way. Memory use is higher than Alacritty by third party measures. Sources: https://wezterm.org/features.html, https://wezterm.org/shell-integration.html, https://api.github.com/repos/wezterm/wezterm/releases, https://wezterm.org/changelog.html.

### Kitty
Best at: its own graphics protocol with animation and Unicode placeholders, the keyboard protocol it originated, seven window layouts, the remote control socket, kittens (ssh, icat, themes, hints, command palette, quick access terminal, panel), sessions, OSC 99 notifications, OSC 9;4 progress, the text sizing protocol and, from 0.49.0, official custom shaders. Gaps: no native Windows build; configuration has hundreds of options; the GPL 3 licence (u) matters if Termoil code is shared with it. A fix for a heap underflow in OSC 133 parsing in 0.49.2 shows where parser attack surface sits. Sources: https://sw.kovidgoyal.net/kitty/overview/, https://sw.kovidgoyal.net/kitty/changelog/, https://sw.kovidgoyal.net/kitty/desktop-notifications/, https://sw.kovidgoyal.net/kitty/keyboard-protocol/.

### Windows Terminal
Best at: Windows integration, ConPTY, tabs and panes, a settings UI and actions editor, a command palette, sixel since preview 1.22, grapheme clusters, OSC 133 marks since 1.18, window persistence and the Kitty keyboard protocol in 1.25. Gaps: Windows only; no Kitty graphics (u); no multiplexer; shell integration needs prompt changes in the shell. Sources: https://github.com/microsoft/terminal/discussions/20758, https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release, https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration, https://www.techzine.eu/news/applications/139337/windows-terminal-1-24-focuses-on-extensions-and-search/.

### Rio
Best at: the widest graphics support of the group (iTerm2, Kitty, sixel), the Kitty keyboard protocol, RetroArch shaders, splits, IME, vi mode, hints and a release every few weeks (0.5.27 on 2026-08-30, 0.5.28 on 2026-09-17, 0.5.29 on 2026-11-01). Gaps: the features page does not confirm tabs, ligatures, renderer or licence; its release notes link to an external changelog that I could not read. Sources: https://rioterm.com/docs/features, https://api.github.com/repos/raphamorim/rio/releases.

## 3. Ten features users most often cite when choosing

This ranking is my synthesis. Evidence came from Hacker News threads, comparison blog posts from 2026 and project release notes. I did not retrieve Reddit threads or GitHub issue counts, so the order reflects how often a theme appeared in the sources read, and is not a vote count.

1. Low typing latency and responsiveness. https://news.ycombinator.com/item?id=47207472, https://blog.luminoid.dev/Terminal-Emulator-Comparison-2026/
2. Native tabs and splits without tmux. https://news.ycombinator.com/item?id=47208462, https://www.pistack.xyz/posts/2026-08-10-ghostty-vs-alacritty-vs-wezterm-terminal-emulator-guide/
3. Windows and WSL support. Ghostty's lack of it is the usual objection. https://ghostty.org/docs/install/release-notes/1-4-0
4. Memory use. Complaints about Ghostty memory and praise for Alacritty. https://news.ycombinator.com/item?id=47207248
5. Scrollback search. Cited as the reason people left Ghostty before 1.3.0. https://news.ycombinator.com/item?id=45352035
6. Built in multiplexing and SSH domains (WezTerm). https://www.pistack.xyz/posts/2026-08-10-ghostty-vs-alacritty-vs-wezterm-terminal-emulator-guide/
7. Scripting and configuration model: Lua for WezTerm, remote control and kittens for Kitty, a simple text file for Ghostty. https://blog.luminoid.dev/Terminal-Emulator-Comparison-2026/
8. Inline image support for TUIs and previews (Kitty graphics protocol, sixel). https://sw.kovidgoyal.net/kitty/overview/
9. SSH and terminfo compatibility on remote hosts. https://news.ycombinator.com/item?id=47207248, https://sw.kovidgoyal.net/kitty/overview/
10. Ligatures and font rendering quality. https://www.pistack.xyz/posts/2026-08-10-ghostty-vs-alacritty-vs-wezterm-terminal-emulator-guide/

Latency evidence: no current published millisecond comparison that includes Ghostty was found. The only Typometer style numbers are old (a macOS camera test from 2022: kitty 29.2 ms, Terminal.app 45.4 ms, WezTerm 45.8 ms, Alacritty 57.9 ms) at https://dev.to/lkhrs/measuring-terminal-latency-26m7, plus kitty's own claim at https://sw.kovidgoyal.net/kitty/performance/. Termoil should run its own measurement.

## 4. Specifications Termoil must implement

URLs marked (f) were fetched in this session. The rest are the canonical locations as I know them and were not re-fetched. The freedesktop terminal working group site refused automated access, so check the OSC 133 URL by hand.

| Specification | Canonical URL |
|---|---|
| ECMA-48 (control functions) | https://ecma-international.org/publications-and-standards/standards/ecma-48/ |
| xterm control sequences (also covers SGR, the DECSET list, OSC 52, XTPUSHSGR and XTPOPSGR, bracketed paste, focus events 1004, mouse modes 1000, 1002, 1003, 1006, 1016, XTVERSION, XTGETTCAP, DECRQSS) | https://invisible-island.net/xterm/ctlseqs/ctlseqs.html |
| Kitty keyboard protocol (f) | https://sw.kovidgoyal.net/kitty/keyboard-protocol/ |
| Kitty graphics protocol | https://sw.kovidgoyal.net/kitty/graphics-protocol/ |
| Synchronized output, DEC private mode 2026 (f) | https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036 and https://github.com/contour-terminal/vt-extensions |
| OSC 133 semantic prompts (FinalTerm origin, adopted by iTerm2 and WezTerm) | https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md |
| WezTerm semantic zones usage (f) | https://wezterm.org/shell-integration.html |
| iTerm2 escape codes (proprietary sequences, shell integration marks, inline images protocol) | https://iterm2.com/documentation-escape-codes.html |
| OSC 8 hyperlinks | https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda |
| OSC 52 clipboard | https://invisible-island.net/xterm/ctlseqs/ctlseqs.html (section on Operating System Commands) |
| OSC 7 working directory | No standalone specification. Behaviour is documented at https://wezterm.org/shell-integration.html (f) and https://sw.kovidgoyal.net/kitty/shell-integration/ |
| OSC 9;4 progress | https://conemu.github.io/en/AnsiEscapeCodes.html (ConEmu specific OSC section) |
| Mode 2027 grapheme clustering (Contour terminal unicode core) (f, landing page) | https://github.com/contour-terminal/terminal-unicode-core |
| Sixel | https://vt100.net/docs/vt3xx-gp/chapter14.html |
| DECRQSS, DECRQM, DA1, DA2 | https://vt100.net/docs/vt510-rm/DECRQSS.html and https://vt100.net/docs/vt510-rm/DECRQM.html |
| Kitty notification protocol, OSC 99 (f) | https://sw.kovidgoyal.net/kitty/desktop-notifications/ |
| tmux DCS passthrough (`allow-passthrough`) | https://man.openbsd.org/tmux.1 |
| Kitty text sizing protocol (added in kitty 0.40) | https://sw.kovidgoyal.net/kitty/text-sizing-protocol/ |
| Ghostty VT reference, a list of sequences a peer implements (f) | https://ghostty.org/docs/vt/reference |
| Unicode 16 and 17 | https://www.unicode.org/versions/Unicode16.0.0/ and https://www.unicode.org/versions/Unicode17.0.0/ |
| UAX 29 grapheme clusters | https://unicode.org/reports/tr29/ |
| UAX 11 East Asian width | https://unicode.org/reports/tr11/ |
| UTS 51 emoji | https://unicode.org/reports/tr51/ |

Notes for the PRD:

- Kitty keyboard flags are 1 disambiguate, 2 event types, 4 alternate keys, 8 all keys as escapes, 16 associated text. Alacritty, Ghostty, Rio, WezTerm and Windows Terminal all implement the protocol, as do Vim, Neovim, Helix, Emacs, fish and nushell (f, kitty keyboard protocol page).
- DEC 2026 must answer DECRQM: 0 or no reply means unsupported, 1 set, 2 reset, 4 permanently reset (f, Contour gist).
- Parser security matters. Kitty fixed a heap underflow in OSC 133 parsing in 0.49.2. Alacritty fixed an unbounded Kitty keyboard mode stack and unbounded per cell memory for zero width cells in 0.18.0 development (changelog links above). Termoil needs fixed bounds on every stack and per cell store.
- Windows Terminal 1.25 fixed a bug where DECSTR cleared an uploaded DRCS soft font. DRCS is a lower priority item.
