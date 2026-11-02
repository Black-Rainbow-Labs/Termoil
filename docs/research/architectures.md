# Terminal emulator architectures: research for Termoil

Date of research: 2026-11-01. Termoil is a new terminal emulator written from scratch in Rust.

## Method and sources

I shallow cloned the default branch of six repositories on 2026-11-01 and read the source directly. The commits read were: Ghostty (tip of 2026-11-01, after the 1.4.0 release notes), Alacritty (2026-10-26, alacritty_terminal 0.26.1 dev, alacritty 0.18 dev), WezTerm (2026-10-26), kitty (2026-11-01), Windows Terminal (2026-10-30), Rio (2026-11-01, version 0.5.29). Zed was read through raw GitHub files because the repository is large. Release notes, pull requests and blog posts were read through web fetches. Where a number came from a secondary source or a project's own documentation it is marked as such, and where I could not confirm a claim in source I say so.

Repository base URLs used for file paths below:

- Ghostty: https://github.com/ghostty-org/ghostty (the project announced in April 2026 that it is leaving GitHub, so this URL may later become a mirror)
- Alacritty: https://github.com/alacritty/alacritty
- WezTerm: https://github.com/wez/wezterm (redirects to the wezterm organisation)
- kitty: https://github.com/kovidgoyal/kitty
- Windows Terminal: https://github.com/microsoft/terminal
- Rio: https://github.com/raphamorim/rio
- Zed: https://github.com/zed-industries/zed

Findings that change the framing of the task, stated first:

1. Rio no longer reuses alacritty_terminal. Its tip has its own core crate, `rio-vt`, with a 64 bit packed cell. Only the Zed terminal still wraps alacritty_terminal (a Zed fork pinned at a revision).
2. Ghostty's tip has scrollback compression (LZ4 on cold pages, shipped in 1.4.0), a threaded PTY read and parse split, and a C library `libghostty-vt` that builds for Windows, WebAssembly and freestanding targets. This is the closest existing model for a reusable Rust core.
3. Windows Terminal supports the kitty keyboard protocol and DEC private mode 2027 (grapheme clusters) in its parser, so the claim that only Ghostty, kitty and WezTerm handle these is out of date.
4. kitty's shaders are now written in Slang (`kitty/shaders/*.slang`), and its cell storage is a pair of arrays per line, 12 bytes of CPU cell and 20 bytes of GPU cell.

---

## 1. Ghostty (Zig)

### 1.1 Process and library split

Mitchell Hashimoto describes the structure as a shared core written in Zig with a native GUI per platform, "over 90% of the code shared" (https://mitchellh.com/writing/ghostty-1-0-reflection). In the tree:

- The shared core is `src/` (`Surface.zig`, `App.zig`, `terminal/`, `renderer/`, `termio/`, `font/`, `input/`, `config/`).
- The platform layer is called an apprt (application runtime), in `src/apprt/`: `embedded.zig` (the C API that the macOS Swift app uses, header at `include/ghostty.h`, Swift sources under `macos/Sources`), `gtk/` for Linux, `browser.zig`, and `none.zig`.
- `libghostty-vt` is the terminal core extracted as its own library: parser, screen state, scrollback, selection, search, kitty graphics, tmux control mode parsing. Its Zig root is `src/lib_vt.zig`; its C headers are in `include/ghostty/vt/` (`terminal.h`, `render.h`, `screen.h`, `search.h`, `kitty_graphics.h`, `key.h`, `mouse.h`, `snapshot.h`, `unicode.h`, and others). The announcement says it has zero dependencies and does not even need libc (https://mitchellh.com/writing/libghostty-is-coming). The 1.4.0 notes say it builds for macOS, Linux, Windows (MSVC and GNU), FreeBSD, Android, iOS and WebAssembly, that CI verifies every target, and that the maintainers plan one more ABI break before a 1.0 tag (https://ghostty.org/docs/install/release-notes/1-4-0). `src/terminal/render.zig` carries a developer note that `RenderState` lives in the terminal directory so that it stays generic across renderers, which is how libghostty-vt hands a renderable snapshot to an embedder.
- The libghostty announcement lists the planned siblings: input encoding ("a big one"), GPU rendering where the host supplies the surface, and UI widgets.
- The 1.4.0 notes say the Windows GUI is dropped from the roadmap and downstream projects should use libghostty on Windows.

### 1.2 Terminal core data structures

Files: `src/terminal/PageList.zig` (21,173 lines), `page.zig`, `Screen.zig`, `Terminal.zig`, `style.zig`, `hyperlink.zig`, `ref_counted_set.zig`, `bitmap_allocator.zig`, `hash_map.zig`, `size.zig`.

**Page.** A page is one contiguous block of memory, allocated straight from the operating system with `mmap` (or `VirtualAlloc` on Windows) because the OS guarantees zeroed pages (`page.zig`, `PageAlloc`). Everything the page owns lives inside that block and refers to other parts of it by offset instead of pointer: the rows, the cells, a style set, a grapheme allocator and map, a hyperlink set and map. Because there are no absolute pointers, a page can be copied with one memory copy, and a page can be compressed as raw bytes. The standard capacity is `std_capacity` in `page.zig`: 215 columns, 215 rows, 128 styles and 8192 bytes of grapheme storage.

**Cell and Row.** `Cell` is a `packed struct(u64)`: a 2 bit content tag (a codepoint, a codepoint that is the first of a grapheme cluster, or a background only cell using a palette index or an RGB colour), 24 bits of content, a style id (zero is the default style and needs no lookup), a wide property (narrow, wide, spacer head, spacer tail), a protected flag, a hyperlink flag, and the semantic content of the cell for OSC 133 (output, input, prompt). `Row` is also a `packed struct(u64)`: an offset to its cells plus flags (`wrap`, `wrap_continuation`, `grapheme`, `styled`, `hyperlink`, `semantic_prompt`, `kitty_virtual_placeholder`, `dirty`). The comments explain that `styled` and `hyperlink` may have false positives but never false negatives, and that the `styled` flag alone makes erase operations about 4 times faster for rows that were never styled.

**Style table.** Styles are interned. A cell holds a 16 bit style id; the page holds a reference counted set (`ref_counted_set.zig`) that maps ids to `Style` values and deduplicates. When a page runs out of style slots the page is split or grown (`Capacity`).

**Grapheme storage.** A cell whose content tag is the grapheme tag holds only the first codepoint. The remaining codepoints are in the page's `grapheme_alloc`, a bitmap allocator (`bitmap_allocator.zig`), found through a map from the cell's offset to a slice. The 1.4.0 notes set a limit of 64 combining codepoints per cell.

**Hyperlink storage.** Cells carry a hyperlink flag. The id is in a per page `hyperlink_map` (cell to id) and the URI and optional id string are in `hyperlink_set`, reference counted like styles.

**PageList.** `PageList.zig` is a doubly linked list of pages. Node `data` is a union of `resident` (a normal page) and `compressed` (a page whose raw physical memory has been discarded and whose LZ4 encoded copy is held in `compress/Page.zig`). The virtual mapping is kept so that restoring a page cannot fail for lack of address space. Pages come from a pool of standard sized pages (`PagePool`); a page that needed more capacity than standard (many emoji, styles or hyperlinks) is allocated directly and is released with `munmap` on removal. Pins (tracked references to a row and column) are kept in a pool so that selections, the viewport, the cursor and search results survive scrolling, pruning and reflow.

**Scrollback is memory bounded by bytes.** `PageList.init` takes `max_size`, a byte limit measured on the uncompressed logical size of pages. The configuration key is `scrollback-limit-bytes` (renamed from `scrollback-limit` in 1.4, with `scrollback-limit-lines` added; `src/config/Config.zig`). The 1.4.0 notes raise the default from 10 MB to 50 MB per terminal. Once the limit is reached, the oldest page is reused as the newest. Idle compression (`PageList.compress`, with incremental, drain and full modes) compresses pages that are fully historical and not visible, and the notes report 70% to 90% less physical memory for typical text. Restoring happens transparently when a page is read for scrolling or search. The notes also say a new empty terminal uses about 16 KiB of page memory on macOS (down from 128 KiB), and that alternate screen memory is allocated on demand.

**What was hard in memory management.** Hashimoto's account of the largest memory leak (https://mitchellh.com/writing/ghostty-memory-leak-fix): during pruning, the oldest page was reused as the newest and its metadata was reset to standard size. If that page had been allocated larger than standard, it then looked like a pool page when freed, so `munmap` was never called. Users saw up to 37 GB. The bug existed since 1.0 and showed only when programs produced many multi codepoint graphemes on the primary screen with heavy scrollback. The fix (pull request 10251, shipped in 1.3) stops reusing non standard pages. The same post describes adding Mach VM tags on macOS so that page memory shows under its own label in profiling tools.

**Reflow.** `PageList.resizeCols` and `reflowRow` rewrite rows into new pages when the width changes; `resizeWithoutReflow*` handles the case where reflow is off. The 1.4.0 notes report resizing with large scrollback is about 6 times faster.

### 1.3 VT stream parser

`src/terminal/Parser.zig` implements the vt100.net state machine (ground, escape, CSI entry and parameter states, DCS, OSC, SOS/PM/APC) with a transition table built at compile time (`parse_table.zig`). `stream.zig` sits on top: it decodes UTF-8 (`UTF8Decoder.zig`), uses SIMD routines to find the next escape byte and to process runs of plain text (`src/simd/vt.cpp`, `index_of.cpp`, `codepoint_width.cpp`, based on Highway and simdutf under `pkg/`), and emits a tagged union of actions (`Action` in `stream.zig`: `print`, `print_slice`, `print_repeat`, cursor movements, and so on). `stream_terminal.zig` applies actions to `Terminal`. The 1.4.0 notes report plain ASCII parsing rose from 128 MB/s to 725 MB/s, and the 1.3.0 notes report that AFL++ fuzzing of the escape parser and stream found about 10 crashes and memory safety issues.

### 1.4 Threads

Kinds of thread in one surface, from the source:

- App thread: windowing and input through the apprt.
- IO thread (`src/termio/Thread.zig`): the writer side. It writes bytes to the PTY, coalesces resize events for 25 ms, resets a synchronized output flag after 1000 ms if the program did not, and runs selection auto scroll at 15 ms. Its comment states the goal is to keep work off the reader because the reader is the hot path.
- Reader (`src/termio/Termio.zig`, `Exec.zig`): reads the PTY and parses into terminal state under the terminal mutex. The 1.4.0 notes add a split: "the kernel caps every read from a pty at about 1 KiB no matter how large the read buffer is", so reading on one thread and parsing on another raised macOS throughput by 25% to 55%.
- Renderer thread (`src/renderer/Thread.zig`): a libxev event loop woken by an async handle, plus a draw now handle, a 600 ms cursor blink timer and an animation timer for custom shaders and kitty graphics. Normal rendering is driven by wakeups and not by a fixed timer. On macOS a display link provides vsync; the 1.4.0 notes say the display link pauses when there is nothing to draw and that GPU memory with 20 hidden tabs fell from about 385 MiB to 18 MiB.
- Search thread (`src/terminal/search/Thread.zig`): takes the terminal lock "in small time slices" and copies data to reduce lock time (1.3.0 notes).

The renderer's mailbox is a bounded blocking queue (`BlockingQueue(Message, 64)`).

### 1.5 Renderer

Files: `src/renderer/generic.zig` (shared logic parameterised by graphics API), `Metal.zig`, `OpenGL.zig`, `metal/`, `opengl/`, `shaders/`, `cell.zig`, `cursor.zig`, `image.zig`, `link.zig`, `Overlay.zig`.

`updateFrame` takes the terminal lock for as little as possible, extracts a `RenderState` snapshot (`src/terminal/render.zig`, which records per row dirty bits and an overall `dirty` of false, partial or full), then builds the cell buffers outside the lock. Each frame uses a swap chain of `FrameState` objects (`swap_chain_count` per graphics API), each with a uniform buffer, a text cell buffer and a background cell buffer. The renderer resets its snapshot every 100,000 frames to release memory. The 1.3.0 notes say the renderer was rearchitected so that lock hold time dropped by 2 to 5 times and most frames need no terminal lock, and the 1.4.0 notes report a further 2.7 to 11 times less lock time per frame and, on GTK, EGL rendering on a dedicated thread with DMABUF export and triple buffering so no OpenGL runs on the main thread.

Background colours are one cell per grid position (a buffer indexed by row and column); text cells are emitted per shaped glyph. Shaping runs per row run with a cache (`font/shaper/Cache.zig`, `run.zig`). Custom shadertoy style shaders are supported (`shadertoy.zig`).

### 1.6 Fonts

Files: `src/font/`. `Collection.zig` holds faces by style, `CodepointResolver.zig` maps a codepoint to a face with explicit codepoint maps and fallback discovery, `SharedGrid.zig` and `SharedGridSet.zig` share one font grid and atlases across surfaces with the same font configuration (their comments say font data is memory heavy and read heavy), `Atlas.zig` is a texture atlas using the skyline bin packing algorithm from Jukka Jylänki's paper, always square, with a fixed pixel format per atlas so grayscale and colour glyphs use separate atlases. Faces: `face/freetype.zig`, `face/coretext.zig`, `face/web_canvas.zig`. Shapers: `shaper/harfbuzz.zig` (Linux), `shaper/coretext.zig` (macOS), `shaper/web_canvas.zig`. Discovery: `discovery.zig` over fontconfig on Linux and CoreText on macOS. `sprite/` draws box drawing, block, braille and powerline glyphs procedurally instead of from fonts, and `nerd_font_attributes.zig` holds generated sizing rules for Nerd Font glyphs. Grapheme and width tables come from the uucode library, with a SIMD codepoint width routine in `src/simd/`.

### 1.7 Input, protocols, shell integration, config

- Kitty keyboard: `src/input/kitty.zig` and `key_encode.zig` (encoding from a key event and terminal mode flags to bytes), `mouse_encode.zig`, `paste.zig`.
- Kitty graphics: `src/terminal/kitty/graphics_command.zig`, `graphics_exec.zig`, `graphics_image.zig`, `graphics_storage.zig`, `graphics_animation.zig`. Storage has a total limit of 320 MB (`total_limit` in `graphics_storage.zig`). The 1.4.0 notes say the implementation covers 100% of the published specification, limit OSC payloads to 8 MiB and graphics payloads to 65 MiB, and report SIMD bulk parsing of 64 MiB payloads about 16 times faster. The 1.3.0 notes list a fix for a crash from specially crafted large images.
- Shell integration: `src/shell-integration/{bash,zsh,fish,elvish,nushell}` are injected automatically into the spawned shell. The terminal records OSC 133 marks in `Row.semantic_prompt` and `Cell.semantic_content`, which power jump to prompt, copy last output, resize behaviour and click to move cursor (the 1.3.0 notes list `click-events` support for bash and zsh).
- Config: `src/config/Config.zig` is a flat `key = value` file with the same keys available on the command line, live reload, conditional values (`conditional.zig` for light and dark themes) and a compatibility table that renames old keys across versions. Embedders use `CApi.zig`.
- Quality tooling: a "tripwire" (`src/tripwire.zig`) injects failures at every `try` site to test cleanup paths. The 1.3.0 notes say it found leaks, state corruption and glyph cache corruption in the font subsystem.

### 1.8 Release notes: what was hard

From https://ghostty.org/docs/install/release-notes/1-3-0 and 1-4-0: the renderer lock contention (two rounds of work), the pruning leak, data races (kitty image placements, hyperlink lookup, selection copy), grapheme break correctness (1.3 makes breaks "match the spec exactly" and moves to Unicode 17), Devanagari wide clusters, tmux control mode parsed but not wired to the GUI, and kitty text sizing (OSC 66) and clipboard (OSC 5522) parsed but not implemented in the GUI. The 1.0 reflection post gives no benchmark numbers and names the hard parts as community scale and an incomplete 1.0 feature set. The search I ran did not find a Hashimoto post specifically about grapheme clustering; the 1.3.0 notes and `Terminal.zig` (the mode 2027 branch in `print`) are the sources for that topic.

---

## 2. Alacritty (Rust)

### 2.1 Crates

`alacritty_terminal/` (library: `term/`, `grid/`, `event_loop.rs`, `tty/`, `selection.rs`, `vi_mode.rs`, `index.rs`), `alacritty/` (the binary: `display/`, `renderer/`, `input/`, `config/`, `polling/`, `event.rs`, `window_context.rs`), `alacritty_config` and `alacritty_config_derive` (TOML configuration derive macros), and the VT state machine in the separate `vte` crate (version 0.15, `ansi` feature).

### 2.2 Grid, storage, cell

- `Term<T>` (`alacritty_terminal/src/term/mod.rs`) owns the active `Grid<Cell>`, the inactive grid (primary and alternate swap), tab stops, mode flags, the scroll region as a `Range<Line>`, colours, cursor style, title stack, selection and the vi mode cursor.
- `Grid` (`grid/mod.rs`) wraps `Storage<Row<Cell>>` (`grid/storage.rs`), a ring buffer over a `Vec<Row<T>>` with a `zero` offset. Scrolling is modular addition on `zero` instead of moving rows. The comment in `storage.rs` says the ring buffer exists because `slice::rotate_left` would rearrange memory. `MAX_CACHE_SIZE` is 1,000 lines of spare capacity. History is a line count (`scrolling_history`, default 10,000 in `TermConfig`), so memory is bounded by lines times width, with every row stored at full width.
- `Cell` (`term/cell.rs`) has `c: char`, `fg` and `bg` colours (enum of named, indexed and RGB), `flags` (a 16 bit `bitflags` set: bold, italic, underline variants, `WRAPLINE`, `WIDE_CHAR`, `WIDE_CHAR_SPACER`, `LEADING_WIDE_CHAR_SPACER`, and so on) and `extra: Option<Arc<CellExtra>>`. `CellExtra` holds up to 9 zero width characters (`MAX_ZEROWIDTH_CHARS`, which the source comment calls a denial of service limit), an underline colour and a hyperlink. By my arithmetic a cell is roughly 24 bytes on a 64 bit machine; I did not measure it. A grapheme cluster is therefore a base character plus up to nine combining characters, and Alacritty has no mode 2027 clustering (a search of `term/mod.rs` found no 2027 or grapheme handling). Width comes from the `unicode-width` crate.
- Resize reflow: `grid/resize.rs` (`grow_columns`, `shrink_columns`) uses the `WRAPLINE` flag on the last cell of a row to rejoin and split logical lines.

### 2.3 Parser, handler, event loop, threads

- `vte::ansi::Processor` parses bytes and calls methods of the `Handler` trait, which `Term` implements.
- `event_loop.rs`: the PTY reader runs on its own thread (spawned in `window_context.rs`, whose comment says PTY I/O is kept off the thread that does the rendering work). It reads into a 1 MiB buffer (`READ_BUFFER_SIZE = 0x10_0000`), tries `try_lock_unfair` on the terminal's fair mutex and keeps reading if the lock is busy, forcing a blocking lock only when the buffer is full. Each locked pass parses at most `MAX_LOCKED_READ` (65,535 bytes) so the render thread can get the lock. It sends `Event::Wakeup` to the UI unless all processed bytes were inside a synchronized update (DEC mode 2026, tracked by the vte processor with a timeout).
- The window and render run on the main thread through winit, with `glutin` for the OpenGL context.

### 2.4 Damage tracking and rendering

- `Term::damage()` returns `TermDamage::Full` or `Partial` with a `LineDamageBounds` (left and right column per line). It always damages the old and new cursor cells and forces full damage when scrolling changes every line.
- `alacritty/src/display/damage.rs` (`DamageTracker`) keeps damage per frame and for previous frames (for buffer age), and the display calls `swap_buffers_with_damage` so the compositor receives only changed rectangles.
- Vsync is turned off (`SwapInterval::DontWait` in `display/mod.rs`) and a `FrameTimer` computes a delay from the monitor refresh interval so that Alacritty paces its own frames.
- Renderer (`renderer/`): OpenGL ES 2 and OpenGL 3.3 paths (`renderer/text/gles2.rs`, `glsl3.rs`), instanced quads batched by atlas texture (a batch is flushed on a texture change or when full), a glyph cache keyed by `GlyphKey` (`text/glyph_cache.rs`, which its own comment calls a "naive glyph cache"), atlases of 1024 by 1024 pixels filled row by row (`text/atlas.rs`), box drawing glyphs drawn by `builtin_font.rs`, and rectangles for underline, cursor and selection backgrounds (`renderer/rects.rs`). Rasterisation uses the `crossfont` crate. There is no shaping step, so there are no ligatures and no complex script support.
- Features: regex search over the grid with `regex-automata` DFAs (`term/search.rs`), vi mode (`vi_mode.rs`), selection types (simple, block, semantic, lines; `selection.rs`), regex hints (`display/hint.rs`), a Unix socket IPC for `alacritty msg` (`polling/ipc.rs`; messages such as creating windows and updating config), TOML config with file watching, and kitty keyboard flags as terminal mode bits (`DISAMBIGUATE_ESC_CODES` and related, `term/mod.rs`) with encoding in `alacritty/src/input/keyboard.rs`.

### 2.5 What it leaves out

The README states: "you won't find things like tabs or splits (which are best left to a window manager or terminal multiplexer) nor niceties like a GUI config editor" (https://github.com/alacritty/alacritty/blob/master/README.md). The README says some features are "just not a good fit for Alacritty". Alacritty also has no image protocols, no ligatures and no shell integration marks in `alacritty_terminal`.

---

## 3. WezTerm (Rust)

### 3.1 Crates and layering

A very large workspace: `termwiz` (escape sequence types, terminal capabilities, widgets, line editor), `vtparse` (the byte state machine), `wezterm-escape-parser`, `wezterm-term` (`term/`: `Terminal`, `Screen`, `terminalstate/`), `wezterm-surface` (lines, cells, change log), `wezterm-cell`, `wezterm-char-props` (Unicode properties), `bidi` (a Unicode bidi implementation), `wezterm-font`, `wezterm-gui`, `mux`, `wezterm-mux-server(-impl)`, `wezterm-client`, `wezterm-ssh`, `config`, `lua-api-crates/*`, `window` (windowing and the glyph atlas), `pty` (portable_pty), `codec` (the mux wire protocol).

### 3.2 Screen model

- `Line` (`wezterm-surface/src/line/line.rs`) holds `CellStorage` (`line/storage.rs`), either `V(VecStorage)` or `C(ClusteredLine)`. `ClusteredLine` (`line/clusterline.rs`) stores runs of text that share attributes instead of one cell per column, which reduces memory for lines of plain text. A unit test in `storage.rs` fixes `CellStorage` at 64 bytes and `VecStorage` at 24. Each line also keeps `zones` (semantic zones, used for OSC 133 prompts), a `seqno` (sequence number used for damage and cache invalidation) and `LineBits` flags.
- `Cell` (`wezterm-cell/src/lib.rs`) is a `TeenyString` (a `u64` that stores one or two characters inline and otherwise points to a heap string, with a bit that caches whether the width is two) plus attributes (`CellAttributes`, which keeps rarely used fields such as hyperlinks and images in boxed extras). Grapheme clustering and width: `wezterm-char-props` with a configurable `unicode_version` and `treat_east_asian_ambiguous_width_as_wide` in `config/src/config.rs`.
- Scrollback is a line count, `scrollback_lines`, default 3,500.
- Images: iTerm2 protocol, sixel and kitty graphics (`enable_kitty_graphics`; `wezterm-cell/src/image.rs` has the shared `ImageData` model), and kitty keyboard (`enable_kitty_keyboard`). `docs/features.md` lists iTerm2 images and experimental sixel.

### 3.3 Mux model

`mux/src/lib.rs`, `domain.rs`, `tab.rs`, `pane.rs`, `window.rs`, `localpane.rs`, `client.rs`, `ssh.rs`, `tmux.rs`.

- `Mux` is the process wide registry of windows, tabs, panes and domains. A `Window` holds tabs; a `Tab` holds a binary tree of panes (`bintree` crate, split requests in `tab.rs`); `Pane` is a trait implemented by `LocalPane` (a PTY), client panes (remote), and terminal tab panes for overlays and tmux.
- `Domain` (`domain.rs`) is documented as "an instance of a multiplexer". Implementations: local, client domains that connect to a `wezterm-mux-server` over a Unix socket, SSH or TLS, SSH domains that start a mux server over SSH or use the built in client, exec domains, serial, WSL, and a tmux control mode domain (`mux/src/tmux.rs`, `tmux_commands.rs`).
- Because the terminal state for each pane lives in the mux server, the GUI is a client that can detach and attach. This is the one design in this study where the VT core is deliberately headless and serialisable.

### 3.4 Threads and PTY path

`read_from_pane_pty` in `mux/src/lib.rs` runs one thread per pane that reads the PTY into a 128 KiB buffer (`mux_output_parser_buffer_size`) and forwards bytes over a socket pair to a second thread, `parse_buffered_data`, that runs `termwiz::escape::parser::Parser`, batches actions for `mux_output_parser_coalesce_delay_ms` (default 3 ms), and holds all actions that change the terminal between synchronized output begin and end. Terminal mutation and GUI work then run on the main thread through `promise::spawn::spawn_into_main_thread`. The UI is the one thread that does the work of applying output and painting, which is the likely source of its contention behaviour (my inference from the code layout, not a measurement).

### 3.5 Renderer

`wezterm-gui/src/`: `renderstate.rs`, `quad.rs`, `glyphcache.rs`, `shapecache.rs`, `customglyph.rs`, `termwindow/render/*` (`paint.rs`, `pane.rs`, `screen_line.rs`, `tab_bar.rs`, `split.rs`), `glyph-vertex.glsl`, `glyph-frag.glsl`, `shader.wgsl`.

- Front ends: `FrontEndSelection` in `config/src/frontend.rs` has `OpenGL` (the default), `WebGpu` (wgpu, `termwindow/webgpu.rs`) and `Software`.
- Every frame, the paint code shapes line clusters, looks up glyphs and writes quads into three layers (background, glyph, overlay) through `TripleLayerQuadAllocator` into a rotating set of three vertex buffers (`TripleVertexBuffer`, `vb: RefCell<[TripleVertexBuffer; 3]>`). A heap allocator exists for building quads off the mapped buffer. The default frame rate cap is 60 (`default_max_fps`).
- Glyph atlas: `glyphcache.rs` plus `window::bitmaps::atlas` (a rectangle atlas that raises `OutOfTextureSpace`, after which the renderer grows or clears the atlas).
- Shaping: `wezterm-font/src/shaper/harfbuzz.rs` with `hbwrap.rs`; font location through `locator/` (fontconfig, CoreText, GDI), rasterisation through FreeType (`rasterizer/freetype.rs`) with COLR support (`rasterizer/colr.rs`), a fallback index per glyph (`FallbackIdx`), and custom drawn block and box glyphs (`customglyph.rs`). `GlyphInfo.num_cells` handles ligatures that span cells (issue 1563). `shapecache.rs` caches shaped runs.
- Bidi: `config.bidi_enabled` and `Line::bidi_info`; `screen_line.rs` clusters each line with a bidi direction hint. WezTerm is the only emulator in this study that reorders text for right to left scripts.

### 3.6 Lua config and plugins

`config/` evaluates a Lua file into a `Config` struct built with `wezterm-dynamic` (`FromDynamic`, `ToDynamic`); `lua-api-crates/` exposes `wezterm.mux`, `wezterm.plugin`, `wezterm.procinfo`, time, URL and SSH helpers and window functions; events such as `update-status` run Lua callbacks in the GUI process. Plugins are git repositories loaded by the plugin crate.

### 3.7 Performance pain points

From the GitHub issues surfaced in search: slow rendering of a very long output stream (issue 740: a script printing about 100,000 lines took up to 45 seconds fullscreen against about 5 seconds over SSH), high CPU and stutter during heavy output on X11 (issue 5357, open), a complex `update-status` Lua handler causing input lag (issue 5939, open), software rendering chosen by mistake giving high CPU (issue 249), and idle CPU or memory creep (issue 1833). kitty's own benchmark table (https://sw.kovidgoyal.net/kitty/performance/) measured WezTerm 20230712 at 16.4 MB/s on ASCII against 121.8 for kitty 0.33 and 43.1 for Alacritty 0.13.1, on a Ryzen 7 PRO 5850U under Linux X11. That is a competitor's benchmark of a build more than three years old, so it indicates relative cost and is not a current number.

---

## 4. kitty (C and Python)

### 4.1 Structure

C for the screen, parser, renderer and font code (`kitty/*.c`), Python for configuration, window management, layouts, remote control and kittens (`kitty/boss.py`, `tabs.py`, `layout/`, `rc/`, `kittens/`). The Python interpreter is embedded and the C extension types are exposed to it (`data-types.c`, `fast_data_types.pyi`).

### 4.2 Screen model

Files: `screen.c`, `line-buf.c`, `history.c`, `line.h`, `lineops.h`, `text-cache.c`, `hyperlink.c`, `char-props.c`.

- A line is two parallel arrays, a struct of arrays layout: `CPUCell` (12 bytes: the character or an index into the text cache, a hyperlink id and the multicell fields `scale`, `subscale_n`, `subscale_d`, `valign`, `halign` for the text sizing protocol) and `GPUCell` (20 bytes: foreground, background and decoration colours, `sprite_idx`, and `CellAttrs`, a 32 bit set of bold, italic, reverse, wide width, decoration and mark bits). `GPUCell` is uploaded to the GPU as is. Per line attributes (`LineAttrs`) store `has_dirty_text` and the OSC 133 prompt kind. `static_assert`s in `line.h` check the sizes.
- A cell holds either a single codepoint or an index into `TextCache`, a deduplicating table of codepoint sequences (up to 24 codepoints per cell, `MAX_NUM_CODEPOINTS_PER_CELL`).
- Scrollback (`history.c`, `HistoryBuf`) is allocated in segments of 2,048 lines (`SEGMENT_SIZE`) obtained with `mmap`, sized by line count, with an additional byte ring buffer (`PagerHistoryBuf`, `scrollback_pager_history_size`) that stores the text of lines that scrolled off for the pager, in a form that needs no cells.
- Kitty graphics (`graphics.c`): images are kept in a manager with a default `DEFAULT_STORAGE_LIMIT` of 320 MiB. When over quota, `apply_storage_quota` removes images in sorted order, oldest first. `disk-cache.c` holds data on disk so that large uploads are not kept in memory.

### 4.3 Threads and loop

`child-monitor.c` has three threads: the main thread (GLFW event loop and rendering, and where the Python `Boss` runs), an I/O thread (`io_loop`, which reads child output, runs the parser and writes input; `pthread_create` near line 323), and a talk thread for remote control sockets. Two settings control pacing. `input_delay` (default 3 ms) limits how often the I/O thread wakes the main loop, with a source comment that wakeups are expensive, and `repaint_delay` limits the render rate. The performance page says the low latency settings are `input_delay 0`, `repaint_delay 2`, `sync_to_monitor no`. The first wakeup after quiet is immediate and later ones are rate limited (see the comments in `child-monitor.c` between lines 2068 and 2103).

### 4.4 Parsing

SIMD scanning of UTF-8 and escape bytes in `simd-string.c` with 128, 256 and 512 bit variants. The kitty performance page claims the bytes are parsed with SIMD, glyphs are cached in video RAM, and "a typical screen update needs only a few bytes sent to the GPU". It publishes throughput in MB/s on a Ryzen 7 PRO 5850U: kitty 0.33 averaged 134.55 against Alacritty 0.13.1 at 54.05 and WezTerm at 48.5.

### 4.5 Rendering

`shaders.c`, `shaders/*.slang` (`cell.slang`, `graphics.slang`, `border.slang`, `background.slang`, and so on, compiled by `shaders/slang.py`), `glyph-cache.c`, `fonts.c`, `gl.c`. OpenGL only. Glyphs live in a sprite map (`SpriteMap` in `shaders.c`): a 2D texture array whose size is limited by `GL_MAX_TEXTURE_SIZE` (capped at 8192) and the array layer limit. Each `GPUCell.sprite_idx` selects a sprite, and the cell shader renders the whole grid from a buffer of cells, so per frame work is uploading changed cells. A shaped run that forms a ligature covers several cells, and `sprite_position_for` is called once per cell with a `ligature_index` so each cell draws its slice of the same shaped glyph. Shaping uses HarfBuzz per run (`hb_buffer` calls in `fonts.c`) with `LigatureType` markers for infinite ligatures (fonts such as Fira Code), and `disable_ligatures.py` is a remote control command. Font discovery is in `fonts/fontconfig.py` and `fonts/core_text.py`.

### 4.6 Protocols, remote control, kittens

- Keyboard protocol: https://sw.kovidgoyal.net/kitty/keyboard-protocol/ (progressive enhancement flags; `keys.c` and `keys.py`).
- Graphics protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/ (images transmitted in chunks, placements with z index, unicode placeholder `U+10EEEE`, animation frames).
- Remote control: about 40 commands as Python modules in `kitty/rc/` (`launch.py`, `send_text.py`, `get_text.py`, `set_colors.py`, `ls.py`, and so on), reachable over a Unix socket or through an escape code from the terminal itself; `kitty @` is the client.
- Kittens (`kittens/`): `icat`, `diff`, `hints`, `ssh`, `themes`, `clipboard`, `notify`, `pager`, `panel`, `choose_fonts`, `unicode_input`, `transfer`, `hyperlinked_grep`, `quick_access_terminal`, and others. They run as separate programs using `kittens/tui`, which is how kitty adds features without growing the C core.
- Text sizing protocol (OSC 66) is implemented through the multicell fields above.
- No bidi (no matching code in the C sources).

---

## 5. Windows Terminal

### 5.1 Layers

`src/` of https://github.com/microsoft/terminal:

- `cascadia/TerminalCore` (`Terminal.cpp`, `Terminal.hpp`, `TerminalApi.cpp`, `TerminalSelection.cpp`): the emulator object. It owns a `TextBuffer`, a VT state machine and the selection.
- `terminal/parser` (`stateMachine.cpp`, `OutputStateMachineEngine.cpp`, `InputStateMachineEngine.cpp`) and `terminal/adapter` (`adaptDispatch.cpp`, `SixelParser.cpp`, `PageManager.cpp`, `MacroBuffer.cpp`): the parser calls an `ITermDispatch` implemented by `AdaptDispatch`, the same code used by conhost and by the Terminal control.
- `buffer/out` (`textBuffer.cpp`, `Row.cpp`, `Marks.hpp`, `ImageSlice.cpp`, `search.cpp`).
- `cascadia/TerminalControl` (`ControlCore.cpp`, `ControlInteractivity.cpp`, `TermControl.cpp`): the XAML control. `ControlCore` owns the terminal and the render thread; `TermControl` is the UI and input.
- `cascadia/TerminalApp` (tabs, panes, command palette), `cascadia/WindowsTerminal` (the process), `cascadia/Remoting` (process model across windows).
- `renderer/atlas`, `renderer/gdi`, `renderer/uia`, `renderer/base`.
- `cascadia/TerminalSettingsModel`: the settings model.
- `winconpty/` (the ConPTY client library) and `host/` (conhost, "OpenConsole").

### 5.2 Text buffer

`ROW` (`src/buffer/out/Row.hpp`) stores a row as UTF-16 text in a buffer taken from one large block of virtual memory per `TextBuffer`, a `uint16_t` array `_charOffsets` that maps each column to the index in the text where its character begins (with the top bit marking the trailing half of a wide glyph), a run length encoded vector of attributes (`RowAttributes`), and the column count. The comments give examples: a wide glyph has the same offset in two columns, and an emoji that is a surrogate pair takes two UTF-16 units. A row with more characters than columns spills to a heap string. Multi codepoint grapheme clusters are therefore ordinary text in the row. DEC private mode 2027 is defined in `DispatchTypes.hpp` (`GCM_GraphemeClusterMode`) and the `PSEUDOCONSOLE_GLYPH_WIDTH_*` flags in `winconpty/winconpty.h` choose between grapheme, wcswidth and console width conventions for a ConPTY session. Marks for shell integration are in `Marks.hpp`; the design is in `doc/specs/#11000 - Marks`.

### 5.3 AtlasEngine

`src/renderer/atlas/README.md` has the architecture. The base `Renderer` turns the text buffer into GDI shaped drawing calls (a design inherited from the old GDI renderer), `AtlasEngine.cpp` converts those into DirectWrite glyph runs, and one of two backends draws them: `BackendD3D` (a custom Direct3D 11 renderer with its own glyph cache, a glyph atlas packed with `stb_rect_pack`, instance buffers of quads, and pixel shaders for ClearType and grayscale text, `shader_ps.hlsl` and `dwrite_helpers.hlsl`), and `BackendD2D` (pure Direct2D, for remote desktop and older GPUs). The README says that breaking the buffer into GDI primitives only to rebuild DirectWrite ones "is pretty wasteful" and "incredibly bug prone".

The rewrite (pull request 14959, merged 2023-04-26, https://github.com/microsoft/terminal/pull/14959) replaced the first design, which gave each cell its own glyph texture, with quads that use the glyph indices returned by DirectWrite directly. The stated reasons were that fonts are often not monospace, that fallback collections cannot be made fully monospace, and that glyphs with overhang or very large size broke the cell model. The pull request describes less code and a simpler CPU side that ran much faster on AMD APUs (no figures given), plus dirty rectangle tracking with partial redraw, inverted cursors made by hole punching, and line rendition support. `BackendD3D` splits wide ligatures into chunks (`_drawTextOverlapSplit`) so colour changes inside a ligature work, resets and enlarges the atlas when it is full (`_resetGlyphAtlas`, `_resizeGlyphAtlas`), and draws box and line glyphs itself (`BuiltinGlyphs.cpp`). Text analysis and fallback use DirectWrite (`DWriteTextAnalysis.cpp`).

### 5.4 ConPTY and OpenConsole

`host/` is conhost, built from the same source as the open source OpenConsole. When a Windows console application calls the Console API, conhost translates those calls into VT sequences for a hosting terminal. The design document `doc/specs/#13000 - In-process ConPTY.md` (Leonard Hecker, 2024) lists the problems with the old arrangement: output was rendered into conhost's own buffer, then a separate "VtEngine" on the renderer thread turned buffer changes back into VT, so the stream the terminal saw could be out of sync with what the program wrote, not every Console API call has a VT equivalent, and the code was hard to maintain. Pull request 17510 (merged 2024-08-01, https://github.com/microsoft/terminal/pull/17510) removed VtEngine and translates Console API calls to VT synchronously during the call, with application VT passing through unmodified. It reports about 1.3 to 2 times faster mixed text output in conhost and about 20 times through ConPTY in Windows Terminal on a large text file, lists remaining quirks (delayed end of line wrap emitted as CRLF breaks reflow, focus events injected in the middle of sequences, surrogate pairs dropped on some buffer operations), and notes regressions found in review (doubled or missing cursor position replies). The long term goal in the spec is to ship the server library with Windows and build conhost on top of it.

Passthrough mode: a ConPTY creation flag `PSEUDOCONSOLE_PASSTHROUGH_MODE` (value 0x8 according to a third party crate's documentation, which also says it is honoured only on Windows 11 build 22000 and later) makes ConPTY forward the child's bytes instead of re synthesising output from a virtual screen. I did not find the flag in the `winconpty` headers in this repository, so the behaviour lives in the operating system's conhost, and the claim comes from third party documentation and not from Microsoft. Treat it as unverified until tested on a real machine.

### 5.5 Settings model

`cascadia/TerminalSettingsModel` and `doc/specs/#754 - Cascading Default Settings.md`: a JSON `settings.json` layered over built in defaults, with profile inheritance, profile fragments from installed applications, themes, actions with ids (spec `#6899`), and a settings user interface (`TerminalSettingsEditor`) that edits the same model. `MTSMSettings.h` is a macro list that drives the JSON serialisation and WinRT projection of each setting. The result is a typed model with a schema, validated on load and live reloaded.

### 5.6 Latency

I found no first party latency numbers. A closed pull request (https://github.com/microsoft/terminal/pull/19757) argued that the three buffer swap chain with vsync adds about two frames, and reported 33 ms falling to 15 to 18 ms with two buffers and vsync off in typometer; it was closed, so this is a claim and not an adopted change. A secondary article reports that 1.19 (April 2024) roughly halved latency and that MinTTY led before that; I could not verify it against the original benchmark.

---

## 6. Rio and Zed

### 6.1 Rio

Findings at tip (`frontends/rioterm`, `rio-vt`, `rio-grid`, `sugarloaf`, `rio-grapheme-width`, `rio-unicode`, `rio-graphics`, `librio`, `teletypewriter`, `corcovado`):

- No dependency on alacritty_terminal. `rio-vt/src/crosswords/mod.rs` says it "was originally taken from" a specific Alacritty revision of `term/mod.rs`; `pos.rs`, `search.rs` and `vi_mode.rs` carry the same notes. So the grid logic, search and vi mode descend from Alacritty and then diverged.
- The cell is `Square(u64)` (`rio-vt/src/crosswords/square.rs`), `#[repr(transparent)]` and packed like Ghostty's, with styles and extras interned in `crosswords/style.rs`. This is the major change from Alacritty's cell.
- `rio-vt` README: an embeddable core with "no rendering, GPU, or font shaping dependencies", with parser, grid, scrollback, selection, search and the PTY facing event model. `librio` wraps it as a C ABI. `librio-wasm` targets the browser.
- Grapheme clustering for mode 2027 uses flat lookup tables built at first use by driving a reference implementation (`rio-vt/src/grapheme_lut.rs`: a class table over `U+0000..U+20000` and a transition table indexed by previous class, next class and state), plus `codepoint_width.rs`, `simd_utf8.rs`, `simd_base64.rs`.
- Protocols: kitty graphics, iTerm2 images, sixel and a custom glyph protocol (`rio-vt/src/ansi/`, `specs/glyph-protocol.md`).
- Rendering is `sugarloaf`. `sugarloaf/src/grid/mod.rs` describes a persistent, row indexed GPU cell buffer (one `bg_cells` buffer indexed by row times columns and per row foreground glyph lists), rewriting only dirty rows, with backends Metal (macOS), Vulkan (Linux), wgpu and a CPU path. `rio-grid` converts cells into `CellBg` and `CellText` instances and shapes by runs of cells sharing font and style, so ligatures form; shaping uses CoreText on macOS and `swash` elsewhere; fonts are found with `font-kit`; `sprite/` draws box, braille and geometric glyphs. This is Ghostty's design reimplemented in Rust. The module comment in `grid/mod.rs` still describes phases of a rewrite, so parts may be scaffolding.

### 6.2 Zed

`crates/terminal` (emulator wrapper), `crates/terminal_view` (GPUI view). Source: https://github.com/zed-industries/zed/tree/main/crates/terminal.

- Core: `alacritty_terminal` from `https://github.com/zed-industries/alacritty` pinned at revision `4c129667ce56611becdc82de6e28218c80e2e88f`, and `vte` 0.15 with `ansi`, in the workspace `Cargo.toml`. Zed uses Alacritty's `EventLoop` to read the PTY and a `Term` behind a lock (`AlacrittyTermLock`).
- Event pipeline: backend events arrive on an unbounded channel; the subscriber handles the first event immediately, then collects further events for up to 4 ms or 100 events, keeps only the latest wakeup, applies the batch in one update and yields. Resize events are merged.
- Snapshot: `Content` with `cells: Vec<IndexedCell>`, cursor, selection and display offset, copied from the grid under the lock (`make_content`).
- Rendering (`terminal_view/src/terminal_element.rs`): there is no terminal specific GPU pipeline. `layout_grid` walks visible cells, merges adjacent same colour backgrounds into rectangles (`merge_background_regions`), batches adjacent cells with the same style into `BatchedTextRun`s that are shaped with GPUI's text system using the cell width as a forced advance, draws block elements as quads on a 24 by 8 sub cell grid, then paints backgrounds, selection, text, IME text and cursor. The element rebuilds the layout every paint and caches nothing itself; any glyph and shaping cache is inside GPUI (I read the first 100,000 characters of the file only).
- Additions on top of Alacritty: tasks and terminal modes (`interactive`, `task`), display only terminals (a subprocess piped into the emulator), regex based path and URL detection with a 100 ms throttle and a regex timeout, working directory tracking for resolving relative paths, scrollback limits (default 10,000 lines, maximum 100,000), vi mode keystroke handling, key and mouse encoding in `terminal/src/mappings/`, minimum contrast enforcement (`ensure_minimum_contrast`), and integration with the editor's themes and settings.

---

## 7. Cross cutting design questions

### 7.1 Thread model

| Emulator | Threads |
|---|---|
| Ghostty | App thread; IO writer thread (events, coalesced resize); PTY reader and, since 1.4, a separate parser thread; renderer thread (libxev loop, wakeup driven); search thread. One mutex over terminal state, taken in short slices. |
| Alacritty | Main thread (winit, render); PTY reader and parser thread. A fair mutex with `try_lock_unfair` and a cap of 65,535 bytes per locked parse. |
| WezTerm | Per pane: PTY reader thread and parser thread joined by a socket pair; terminal state changes and rendering on the main thread; mux server in a separate process. |
| kitty | Main thread (render, Python); I/O thread (read, parse); talk thread (remote control). |
| Windows Terminal | UI thread (XAML), a connection thread, and a render thread in `ControlCore`; ConPTY and conhost in a separate process. |
| Rio | Main thread, a machine thread per terminal built on `teletypewriter` and `corcovado` (not read in detail), renderer inside sugarloaf. |
| Zed | PTY thread from Alacritty's event loop; batching task on the UI executor; GPUI paints on the main thread. |

### 7.2 Frame pacing and vsync

- Ghostty: wakeup driven draws, display link on macOS (pauses when idle), swap chain of frame states, draw now for forced redraws; no fixed render timer except animation and cursor blink.
- Alacritty: vsync deliberately disabled; `FrameTimer` computes delay to match refresh; buffer age damage.
- kitty: `repaint_delay` and `input_delay` settings, optional `sync_to_monitor`; first wakeup immediate then rate limited.
- WezTerm: `max_fps` default 60 with a coalescing delay of 3 ms in the parser.
- Windows Terminal: DXGI swap chain with vsync; the pull request cited above claims buffer count adds latency.
- All implement DEC synchronized output (mode 2026) in some form; Alacritty tracks it in the vte processor, WezTerm holds actions in the parser thread, Ghostty resets it after 1,000 ms if the program does not.

### 7.3 Damage tracking versus full redraw

- Ghostty: per row dirty bit in `Row` and a `RenderState` with false, partial or full; the GPU cell buffers are rebuilt from the snapshot.
- Alacritty: per line column ranges; damage rectangles passed to the compositor.
- kitty: `LineAttrs.has_dirty_text`; the GPU cell buffer is updated for changed cells.
- WezTerm: line sequence numbers; rebuilds quads for the visible screen each paint, with caches for shaped lines.
- Rio: rows rewritten when dirty.
- Windows Terminal: dirty rectangles and partial render in AtlasEngine.
- Zed: full rebuild each paint, reduced by viewport culling and merging.

### 7.4 Cell storage layout

| Emulator | Layout | Wide characters | Graphemes | Styles |
|---|---|---|---|---|
| Ghostty | Array of structs of 8 byte cells inside a page, rows with flags | Wide cell followed by spacer cells | Side allocator per page, cell flagged | Reference counted interned table per page, 16 bit id |
| Alacritty | Array of structs, about 24 bytes per cell (my estimate), ring buffer of rows | Flags `WIDE_CHAR` and spacer flags | Up to 9 zero width chars in `Arc<CellExtra>` | Inline colours and flags |
| WezTerm | Per line enum: vector of cells or clustered runs | Cell width in `TeenyString` | Up to two characters inline, otherwise heap | Inline attributes |
| kitty | Struct of arrays: 12 byte CPU cell, 20 byte GPU cell | Width bits in `CellAttrs` | Index into `TextCache` | Colours in the GPU cell |
| Windows Terminal | Per row UTF-16 text and a column to offset table, run length encoded attributes | Repeated offsets and trailer bit | Plain text in the row | Run length encoded attributes |
| Rio | 64 bit `Square` | Flags | Extras table | Interned style table |

### 7.5 Scrollback limits and compression

Ghostty is the only emulator that bounds by bytes and compresses (LZ4, idle, cold pages only). Alacritty, WezTerm and Zed use line counts (10,000, 3,500 and 10,000 defaults). Rio has a configured history limit (`scrollback-history-limit`). kitty uses a line count for history plus a byte limit for pager history, with segments mapped from the OS. WezTerm's clustered lines are a form of in line compression. I did not check how Windows Terminal bounds history beyond the buffer height setting.

### 7.6 Resize reflow

Ghostty reflows pages (`reflowRow`) and reports a 6 times speedup in 1.4. Alacritty reflows using `WRAPLINE`. WezTerm and kitty reflow using a wrapped line flag (not read in detail). Windows Terminal reflows in `textBuffer.cpp`, and the ConPTY layer has known issues where its own buffer disagrees with the host terminal after a resize (pull request 17510). I did not verify how each handles the alternate screen.

### 7.7 Font loading and fallback

- Ghostty: `Collection` plus `CodepointResolver`, discovery by fontconfig or CoreText, explicit codepoint maps, shared across surfaces.
- Alacritty: `crossfont` by style, system fallback by the rasteriser; no shaping.
- WezTerm: `wezterm-font` locators (fontconfig, CoreText, GDI), a fallback index per glyph, harfbuzz.
- kitty: Python font discovery (fontconfig, CoreText), HarfBuzz per run.
- Windows Terminal: DirectWrite font fallback, text analysis in `DWriteTextAnalysis.cpp`.
- Rio: `font-kit` and swash or CoreText.
- Zed: GPUI's platform text system.

### 7.8 Glyph atlas management

Ghostty: skyline packing in square atlases, one per pixel format. Alacritty: row packed 1024 pixel textures, a new texture when full. WezTerm: rectangle atlas that grows or clears. kitty: a 2D texture array sprite map with limits from the GPU. Windows Terminal: `stb_rect_pack`, reset and resize on exhaustion. Rio: atlas slots keyed by glyph. In the sources I read, the atlases reset or grow when full, and I did not see single entry eviction by least recent use.

### 7.9 Ligatures and shaping cost

Alacritty: none. Ghostty: run shaping with a run cache, harfbuzz or CoreText. WezTerm: harfbuzz per cluster with a shape cache. kitty: per run with ligature sprite slices. Windows Terminal: DirectWrite analysis, glyph pair cache. Rio: per run through `text_run_cache.rs`. The shared technique is to shape a run of cells with the same font and style, cache by run content, and emit one glyph instance per output glyph.

### 7.10 Cursor rendering

Ghostty draws the cursor as a cell instance and handles blink on the renderer thread (600 ms). Alacritty draws cursor rectangles in the rects pass and damages old and new cell positions. Windows Terminal draws cursors in two passes (before text for ordinary cursors, after text for inverted cursors, `_drawCursorPart1` and `_drawCursorPart2`). Zed shapes the cursor glyph so a block cursor shows the character inverted.

### 7.11 Selection model

Ghostty uses tracked pins so selection survives scrolling and reflow. Alacritty selections are in grid coordinates, with simple, block, semantic and lines types. Windows Terminal selection is in `TerminalSelection.cpp` with word delimiters and keyboard selection (spec `#4993`). I did not read WezTerm's or kitty's selection code.

### 7.12 URL and hyperlink detection

OSC 8 hyperlinks are stored per cell: Ghostty's page `hyperlink_map` and `hyperlink_set`, Alacritty's `CellExtra.hyperlink`, kitty's `hyperlink_id` and pool. Plain text URL detection: Alacritty regex hints, Ghostty `input/Link.zig` and `renderer/link.zig` (regex over visible text), WezTerm hyperlink rules in Lua config, kitty `hints` kitten, Zed regex searches for paths with a timeout and throttle.

### 7.13 Search over scrollback

Alacritty: regex DFAs on the grid, run on the UI side while holding the lock. Ghostty: a dedicated search thread that holds the lock in short slices and restores compressed pages as needed (1.3 and 1.4 notes). Windows Terminal: `buffer/out/search.cpp`. kitty and WezTerm: not read in detail.

### 7.14 Kitty graphics storage

Ghostty and kitty both cap storage at 320 MB or MiB, evict oldest first, and keep image data separate from the cell grid; cells contain only placeholders or placements reference pins. kitty can spill image data to a disk cache. WezTerm stores image data in `ImageData` attached to cells. Rio parses the protocol in `rio-vt`. Alacritty and Windows Terminal do not support the kitty graphics protocol (Windows Terminal supports sixel).

### 7.15 Unicode width tables and mode 2027

Ghostty: uucode with SIMD width, mode 2027 clustering in `Terminal.print`, Unicode 17 in 1.3. kitty: `char-props.c` with generated data. WezTerm: `wezterm-char-props` with a version setting. Windows Terminal: mode 2027 defined and ConPTY width flags. Rio: lookup tables for width and for grapheme transitions, mode 2027. Alacritty: `unicode-width`, no 2027. A design decision to carry forward is to generate width and break tables at build time and to test them against the Unicode conformance files.

### 7.16 Shell integration marks

Ghostty: OSC 133 in row and cell bits, scripts for five shells, jump to prompt. kitty: `LineAttrs.prompt_kind` and shell integration scripts. WezTerm: semantic zones per line (`Line::zones`). Windows Terminal: `Marks.hpp` and spec `#11000`. Alacritty: none.

### 7.17 Input events to escape sequences

All encode a key or mouse event plus the current mode flags into bytes: Ghostty `input/key_encode.zig` and `kitty.zig`, Alacritty `input/keyboard.rs`, Zed `mappings/keys.rs`, Windows Terminal's input state machine and Win32 input mode, kitty `keys.c`. The kitty keyboard protocol appears in Ghostty, Alacritty, WezTerm (setting `enable_kitty_keyboard`), kitty and Windows Terminal (`OutputStateMachineEngine.cpp`).

### 7.18 Bidi

WezTerm only, through its `bidi` crate and a direction hint when clustering lines. For Ghostty, Alacritty, kitty, Windows Terminal, Rio and Zed I found no bidi reordering in the grid or renderer sources I searched (Ghostty's matches for the word were in the CoreText shaper and `Screen.zig` and are not terminal level reordering). I did not research proposals for terminal bidi modes.

---

## 8. Synthesis: recommended architecture for Termoil

This is a recommendation for a from scratch Rust emulator that has to match Ghostty and kitty on features and Alacritty and kitty on speed. Each choice names its source and the reason.

**Crate layout.** Build the emulator as a headless core with no windowing, GPU or font code, the arrangement of `libghostty-vt` and `rio-vt`. Suggested workspace: `termoil-vt` (parser, pages, screen, modes, selection, search, kitty graphics storage, unicode tables), `termoil-input` (key and mouse encoders), `termoil-pty` (Unix PTY and Windows ConPTY), `termoil-font` (discovery, fallback, shaping, rasterising), `termoil-gpu` (wgpu renderer), `termoil-app` (windows, tabs, config, IPC). The core should compile for WebAssembly and use only the allocator it is given, because a core with no dependencies is what let libghostty ship on Windows and in the browser. A headless core also leaves room for WezTerm's detach and attach multiplexer later without a redesign.

**Cell and page storage.** Use Ghostty's design. A page is one contiguous mapped allocation that holds rows, 8 byte cells, an interned style table with reference counts, a side allocator for grapheme tails, and a hyperlink set and map, with every internal reference an offset. Rows carry flags (wrap, wrap continuation, grapheme, styled, hyperlink, prompt, dirty) set with the rule "may be true when nothing remains, never false when something exists", which gave Ghostty a 4 times speedup on erase paths. This layout makes copying, compression and reflow cheap, and it avoids the per cell `Arc` that makes Alacritty cells large. In Rust this means an arena type with `unsafe` confined to one module and checked by fuzzing and Miri. kitty's 32 byte struct of arrays uploads to a GPU without conversion but uses four times the memory per cell, so Termoil should build GPU cell buffers from the page for each frame.

**Scrollback.** A linked list of pages with a byte limit (default about 50 MB per terminal, as Ghostty chose in 1.4) and a pool of standard sized pages. Compress cold pages with LZ4 on an idle timer, keep the virtual mapping so that restore cannot fail, and do not reuse a non standard page during pruning (the cause of Ghostty's 37 GB leak). Track selections, the viewport, the cursor and search matches with pins that the list updates during pruning and reflow. Reflow works page by page and should be benchmarked against Ghostty's 6 times faster path from the start.

**Parser.** A table driven state machine from the vt100.net model, built at compile time, with a vectorised front end that scans for escape and control bytes and decodes UTF-8 in bulk, and a bulk path for printable ASCII. Ghostty measured 128 MB/s before and 725 MB/s after its parser and threading work. In Rust use `memchr` or `std::simd` style routines and `simdutf8`. Emit typed actions and not trait calls per byte, so the stream layer can be tested alone. Fuzz it from the first day (AFL++ found about 10 crashes in Ghostty's 1.3 cycle) and add failure injection at allocation sites, as Ghostty's tripwire does.

**Threads.** Three on the hot path. A reader thread that reads the PTY with a large buffer. A parser thread that applies actions to the terminal under one mutex, taking the lock in bounded slices of about 64 KiB like Alacritty's `MAX_LOCKED_READ`, so the renderer is never starved. A renderer thread that wakes on an event, locks only long enough to copy a `RenderState` snapshot (dirty rows only), then builds GPU buffers outside the lock. Ghostty needed two rounds of work to cut lock time by 2 to 11 times, so the snapshot should be a first deliverable. A separate search thread takes the lock in small slices (Ghostty 1.3). The UI thread owns windows and input only. Reader and parser can start as one thread, and be split when a benchmark shows the roughly 1 KiB kernel read cap matters on a target platform (Ghostty 1.4 measured 25% to 55% on macOS).

**Pacing.** Render on demand, with no fixed timer except for animation and cursor blink. Present on the display's refresh using the platform mechanism (a display link on macOS, `Fifo` or waitable swap chains elsewhere). Send the first frame after quiet input immediately and rate limit later wakeups, as kitty does with `input_delay` and `repaint_delay`; the Windows Terminal pull request on buffer counts is a reminder to measure present latency as well as parse speed. Implement synchronized output (mode 2026) with a timeout of about one second.

**Rendering.** wgpu over Metal, Vulkan and DirectX 12, with one instanced pipeline: a background buffer indexed by cell and a foreground instance list per row, rewriting only dirty rows (the design Rio's `sugarloaf` grid renderer follows from Ghostty), a small swap chain of frame state objects, and separate grayscale and colour atlases packed with a skyline or shelf allocator. When an atlas fills, reset or grow it, which is what the emulators I read do. Draw box, block, braille and powerline glyphs procedurally (Ghostty `sprite/`, Windows Terminal `BuiltinGlyphs.cpp`, Zed block quads) so they align at any cell size. Add custom shaders after the base renderer is stable.

**Fonts and shaping.** One font grid shared across windows with the same configuration (Ghostty `SharedGrid`). Platform discovery through DirectWrite, CoreText and fontconfig behind one trait. A codepoint to face resolver with an explicit override map and a fallback cache. Shape runs of cells that share a font and style with HarfBuzz (through `rustybuzz` or another maintained Rust port), cache the result by run content (WezTerm `shapecache.rs`, Rio `text_run_cache.rs`), and emit one instance per glyph, with ligature glyphs sliced per cell so colour changes inside a ligature work (kitty `ligature_index`, Windows Terminal `_drawTextOverlapSplit`). Allow ligatures to be disabled per font.

**Unicode.** Generate width and grapheme break tables at build time from the Unicode data files, use flat class and transition tables for the hot path (Rio `grapheme_lut.rs`), turn on mode 2027 by default, cap combining codepoints per cell (Ghostty 64, Alacritty 9), and test against the conformance files. Leave bidi reordering out of the first release and reserve a per row flag and a run level hook for it, since WezTerm is the only emulator with it and the cost falls on the renderer and on selection.

**Protocols.** Implement the kitty keyboard protocol in `termoil-input` as a function from event and mode flags to bytes, kitty graphics with storage separate from the grid, a 320 MB quota and oldest first eviction, OSC 133 marks in row and cell bits with injected shell scripts for bash, zsh, fish, elvish, nushell and PowerShell, OSC 8, OSC 52 with a permission setting, and sixel and iTerm2 images later. Parse the features Ghostty parses but does not render (OSC 66 text sizing, tmux control mode) so real programs do not break.

**Windows.** Use ConPTY through the `windows` crate and request the passthrough flag where the OS supports it, falling back otherwise, after confirming the flag on target builds. Expect the ConPTY quirks listed in Windows Terminal pull request 17510 (reflow disagreement, injected focus sequences) and test with real console programs.

**Config and scripting.** A typed configuration with a key value file, command line parity, live reload and a rename table for old keys (Ghostty). Add the Unix socket remote control with a small set of commands in the manner of `alacritty msg` and `kitty @` early, because scripts and agents depend on them. An embedded scripting language such as WezTerm's Lua should wait until the core settles, since WezTerm's `update-status` handler is a known source of input lag (issue 5939) when it runs on the UI thread.

**Tabs and splits.** Alacritty's position is that these belong to a window manager or multiplexer; Ghostty, kitty, WezTerm and Windows Terminal ship them. Termoil should keep tabs and splits in the app layer on top of a pane object that owns one headless terminal, so a later mux server with detach and attach (WezTerm domains) is a change of transport.

**Targets and verification.** Use Ghostty 1.4's ASCII parse rate of 725 MB/s as the first throughput target, and kitty's input latency with `input_delay 0` as the first latency target. Measure latency with a camera rig or typometer on each platform, because published numbers conflict (a Wayland camera test measured Alacritty at about 16.7 ms, kitty 18.3, WezTerm 30.8 and Ghostty 38.3 under XWayland, which a roundup treats as an artifact: https://github.com/vizenc/terminal-benchmark). Run vtebench, an asciinema replay corpus (Ghostty analysed about 4 GB), fuzzers, and the esctest suite on every change.

---

## Source list

- https://ghostty.org/docs/install/release-notes/1-3-0
- https://ghostty.org/docs/install/release-notes/1-4-0
- https://mitchellh.com/writing/ghostty-memory-leak-fix
- https://mitchellh.com/writing/libghostty-is-coming
- https://mitchellh.com/writing/ghostty-1-0-reflection
- https://sw.kovidgoyal.net/kitty/performance/
- https://sw.kovidgoyal.net/kitty/keyboard-protocol/
- https://sw.kovidgoyal.net/kitty/graphics-protocol/
- https://github.com/microsoft/terminal/pull/14959
- https://github.com/microsoft/terminal/pull/17510
- https://github.com/microsoft/terminal/pull/19757
- https://github.com/vizenc/terminal-benchmark
- https://github.com/wez/wezterm/issues/740, 249, 1833, 5357, 5939
- https://github.com/zed-industries/zed (crates/terminal, crates/terminal_view, Cargo.toml)
- Repository files cited by path in each section, from the clones of 2026-11-01.
