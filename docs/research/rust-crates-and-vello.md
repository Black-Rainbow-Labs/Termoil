# Termoil: Rust crates and the vello question

Research date: 2026-11-01. Version numbers, release dates and licences come from the crates.io API queried on that date. Open issue counts come from the GitHub API and include open pull requests. Where a fact could not be confirmed from a primary source, the text says so. Statements marked "recommendation" are judgement, not sourced fact.

## 0. Corrections to the brief, found while researching

1. **vello_hybrid no longer exists under that name.** Sparse Strips v0.3.0 (2026-10-02) renamed `vello_hybrid` to `vello_gpu`. The crate `vello_sparse_strips` was never published; "sparse strips" is the name of the architecture and of the release tag (`sparse-strips-v0.3.0`). Source: https://github.com/linebender/vello/releases/tag/sparse-strips-v0.3.0
2. **The crate named `vello` is now the experimental compute renderer.** Its source moved under `research/` in the vello repository. The README says it "remains an experimental implementation for compute capable GPUs". Source: https://github.com/linebender/vello/blob/main/README.md and https://github.com/linebender/vello/blob/main/ARCHITECTURE.md
3. **Warp is no longer closed.** The `warpdotdev/Warp` repository is public. The UI framework crates `warpui_core` and `warpui` are MIT, and the rest is AGPL 3.0. Source: https://github.com/warpdotdev/Warp
4. **Somebody has built a terminal on vello.** CuTTY is an Alacritty fork that replaced the OpenGL renderer with wgpu, vello 0.7.0 and parley 0.7.0. Details in section 1.6.
5. **`wezterm-term` is not on crates.io.** Only `termwiz` is published (0.23.3). `wezterm-term` exists in the WezTerm repository at version 0.1.0 and is consumed by git dependency.
6. **`rustybuzz` is archived.** Its README says it is unmaintained and recommends HarfRust. Parley, cosmic-text and the Linebender stack all moved to `harfrust`.
7. **winit has no stable 0.31 yet.** The latest release is 0.31.0-beta.4 (2026-10-24). The latest stable is 0.30.13 (2026-03-02).

## 1. Rendering

### 1.1 What the vello repository contains now

| Crate | Version | Released | Licence | What it is |
|---|---|---|---|---|
| `vello_cpu` | 0.3.0 | 2026-10-02 | Apache-2.0 OR MIT | CPU only renderer, SIMD and optional multithreading. Release notes call it "broadly usable"; no API stability guarantee. |
| `vello_gpu` (was `vello_hybrid`) | 0.3.0 | 2026-10-02 | Apache-2.0 OR MIT | Path preprocessing on the CPU, rasterisation and compositing on the GPU through wgpu or WebGL2. No compute shaders. Release notes say it is "in early stages and lacks feature parity with Vello CPU". |
| `vello_hybrid` | 0.2.0 | 2026-08-07 | Apache-2.0 OR MIT | The old name. No compatibility re-export exists, so new code depends on `vello_gpu`. |
| `vello_common` | 0.3.0 | 2026-10-02 | Apache-2.0 OR MIT | Shared geometry, tiling, strips, paint. |
| `glifo` | 0.4.0 | 2026-10-02 | Apache-2.0 OR MIT | Glyph run drawing and glyph caching shared by `vello_cpu` and `vello_gpu`. README: "under rapid development. Consider it experimental for now." Renamed from `parley_draw`. |
| `vello` | 0.11.0 | 2026-10-02 | Apache-2.0 OR MIT | The original compute shader renderer, now under `research/`. Requires wgpu 30 and compute shader support. |
| `kurbo` | 0.13.1 | 2026-05-13 | Apache-2.0 OR MIT | 2D geometry. |
| `peniko` | 0.6.1 | 2026-05-15 | Apache-2.0 OR MIT | Brushes, colours, blend modes, font data types. |
| `fearless_simd` | 1.2.0 | 2026-10-25 | Apache-2.0 OR MIT | SIMD abstraction used by `vello_cpu`. |

Repository health: `linebender/vello` pushed 2026-10-30, 215 open issues and pull requests, 4,458 stars. MSRV is Rust 1.89. https://github.com/linebender/vello

The vello README now sorts the project into three renderers and recommends `vello_cpu` as "the most mature choice" and `vello_gpu` as "intended to become the primary renderer for production GPU use cases as it matures". The `vello_gpu` crate docs say it "does not match the compute centric vello renderer's raw performance on dynamic and vector heavy workloads" but "provides excellent performance on workloads that benefit from GPU acceleration, such as images, gradients, and filters". https://docs.rs/vello_gpu

### 1.2 The sparse strips architecture

From ARCHITECTURE.md: paths are flattened on the CPU, binned into tiles, and represented as sparse horizontal strips. Coverage data is kept only where it is needed around path edges, and fully covered spans are stored compactly. `vello_cpu` rasterises and composites strips into a CPU pixmap. `vello_gpu` uploads the scheduled strips and paint data and rasterises them with ordinary vertex and fragment passes. The design descends from the "Potato" hybrid CPU/GPU design document and Laurenz Stampfl's ETH master's thesis on Vello CPU. https://github.com/linebender/vello/blob/main/ARCHITECTURE.md

Consequence for a terminal: the CPU work per frame scales with the number of distinct paths drawn, and glyph outlines are paths. Without a glyph cache, 10,000 glyphs per frame means 10,000 flatten, tile and strip operations per frame. That is why the glyph cache matters (section 1.4).

### 1.3 wgpu requirement

- `vello_gpu` 0.3.0 and `vello` 0.11.0 both require wgpu 30 (the vello_gpu changelog lists "Updated wgpu to v30"; the `vello` README badge shows wgpu v30.0.0). Current wgpu is 30.0.1 (2026-08-22), MIT OR Apache-2.0, 1,315 open issues and pull requests, last push 2026-10-31. https://crates.io/crates/wgpu
- wgpu backends per its README: Vulkan on Windows and Linux/Android, Metal on macOS and iOS, DX12 on Windows, OpenGL (GL 3.3+, or GLES 3.0) as a lower tier, MoltenVK on Apple as an option, WebGL2 and WebGPU on wasm. https://github.com/gfx-rs/wgpu
- `vello_gpu` also has a separate `webgl` feature with GLSL shaders for browsers that lack WebGPU. https://docs.rs/vello_gpu
- The compute renderer needs compute shaders. For the web it relies on WebGPU only. https://github.com/linebender/vello/blob/main/research/vello_research/README.md
- A vello dependency pins the whole application to a wgpu major version. wgpu 30 is the version every vello crate here uses, so any other wgpu user in Termoil must also be on 30.

### 1.4 Text path and glyph caching

The stack for text is: `fontique` (font enumeration and fallback), then `parley` (layout), then `harfrust` (shaping), then `skrifa` (outlines, hinting, COLR painting), then `glifo` (cache and draw), then `vello_cpu` or `vello_gpu` (`glyph_run`). Parley's README names its four dependencies as Fontique, HarfRust, Skrifa and ICU4X. https://github.com/linebender/parley

What exists today:

- `glifo` exports `GlyphAtlas`, `GlyphCacheConfig`, `AtlasCacher`, `HintCache`, `OutlineCache` and a COLR module that paints COLR glyphs through skrifa's `ColorPainter`, including gradients. https://github.com/linebender/vello/blob/main/glifo/src/lib.rs
- `vello_cpu` has a glyph atlas used by `RenderContext::glyph_run`. Issue #2041 (closed 2026-10-30) describes it: rasterised glyphs are cached in glifo's atlas, keyed by glyph, size, the linear part of the transform and a quantised subpixel offset. A downstream measurement in that issue shows that without the atlas `fill_path` takes 57 to 75 percent of samples for repeated glyphs. https://github.com/linebender/vello/issues/2041
- `vello_gpu` needs the `full_image_support` feature (default on) for "glyph atlas caching". The crate docs list glyph caching among exposed features that "remain experimental and are not recommended for use". https://docs.rs/vello_gpu
- The Linebender Q1 2026 update says "First cut at glyph caching – more work is needed." https://linebender.org/blog/tmil-25/
- Open issues touching the text path: #204 "Plan for glyph rendering" (open since 2022), #2019 "Glyphs with monochrome EBDT bitmaps are dropped instead of drawn from their outlines" (2026-10-23), #1931 "API: Remove backend from GlyphRunBuilder?" (API churn), #1948 images render fully transparent after a frame with no late bound resources (persistent image atlas freed while residency survives, 2026-09-24). https://github.com/linebender/vello/issues

The 0.3.0 release notes list breaking changes to the glyph API (`fill_glyphs` and `stroke_glyphs` now return `Result`, `HybridGlyphRunBackend` renamed, atlas dimensions moved to `u16`). Expect further breaks before 1.0.

### 1.5 Known limits for a grid redrawn at 120 Hz

No source I found measures vello on a terminal sized grid at 120 Hz. The points below are inferences from the documented architecture and should be treated as hypotheses for a spike to test.

- A terminal grid of 200 by 60 cells is 12,000 cells, nearly all of them glyphs of one font at one size on a flat background. This is the case where a texture atlas plus one instanced draw call is the cheapest possible design. Vello's strength is arbitrary vector paths, gradients and layers. A terminal uses almost none of that for the grid.
- On `vello_cpu` and `vello_gpu`, every frame the scene is rebuilt and every glyph becomes an atlas lookup. Even with a working glyph atlas, per frame CPU work grows with glyph count because the scene is re-encoded each frame. Terminals avoid this by redrawing only damaged rows. Whether the vello scene API can reuse retained scene fragments per row is not documented.
- `vello_gpu` glyph caching is flagged experimental by its own docs, and `glifo` is flagged experimental. That puts the exact feature a terminal depends on in the least stable part of the project.
- Hinting for crisp small text is available in skrifa (section 2). The atlas keys glyphs by quantised subpixel offset, which suits a fixed cell grid if the cell advance is an integer pixel count. A terminal should force integer cell advances, and then one atlas slot per glyph per style is enough.
- Subpixel LCD text: I found no confirmation that `glifo` rasterises LCD (per colour channel) coverage into the atlas. Alacritty's renderer does LCD text through OpenGL. Treat LCD as unproven on vello.
- Memory: no vello memory figures exist in the sources I read. For wgpu in general, a Zed pull request author measured an empty window at about 100 MB with wgpu against about 10 MB with GPUI's native renderer on Windows 11 (https://github.com/zed-industries/zed/pull/46758, taken from a search summary; I did not open the PR body). Ratty, a Bevy based terminal using parley and vello, reports about 300 MB of RAM. https://blog.orhun.dev/introducing-ratty/
- Presentation: `vello_cpu` renders to a `Pixmap` that must then be presented through softbuffer or uploaded to a texture. At 120 Hz on a 4K window that is roughly 33 MB of pixel data per frame. Multithreading helps only if the `multithreading` feature is enabled.

### 1.6 Has anyone built a terminal or editor on vello?

- **CuTTY** (`gold-silver-copper/CuTTY`): Alacritty fork, "replaces the old OpenGL renderer with a wgpu/vello rendering stack and a parley text pipeline", about 4,000 lines of renderer and font code removed, Apache-2.0, beta quality, 78 commits, one open issue. Its Cargo manifest pins `vello = "0.7.0"` (the compute renderer, an older release than 0.11.0), `parley = "0.7.0"` and `winit = "0.30.9"`. The README reports benchmarks run on 2026-03-18 on an unspecified machine: vtebench dense_cells 8.04 ms against Alacritty 10.23 ms, Kitty 17.99 ms and Ghostty 9.57 ms; scrolling_fullscreen 187.54 ms against Alacritty 194.91 ms. In Kitty's suite CuTTY lost "long escape codes" (4.82 s against Kitty 2.42 s). These are the author's own numbers, with no memory, latency or power data. https://github.com/gold-silver-copper/CuTTY and https://crates.io/crates/cutty
- **Ratty**: terminal with inline 3D graphics, parley for text and vello for drawing, `vt100` for parsing, `portable-pty`, rendered to a texture inside Bevy. Experimental; the author says it is "not the lightest terminal emulator". https://blog.orhun.dev/introducing-ratty/
- **Xilem and Masonry** (Linebender's own GUI toolkit) render with vello. A pull request there says `imaging_vello` remains the default and Masonry can use Vello CPU when no GPU is available. https://github.com/linebender/xilem/pull/1847. Xilem has 161 open issues and pull requests, last push 2026-10-29. Masonry's Android view is described as "not yet generally usable".
- **Zed does not use vello.** GPUI uses its own renderers: Metal on macOS, DirectX on Windows, and wgpu on Linux (the Blade renderer was removed by PR #46758). https://github.com/zed-industries/zed/pull/46758
- Bevy switched its text stack to Parley, per the Linebender Q1 2026 update. Blitz (Dioxus) also uses Parley and vello.

### 1.7 Alternatives a Rust terminal can use

| Option | Version and licence | What it is | Fit for a terminal grid |
|---|---|---|---|
| wgpu plus a custom glyph atlas | wgpu 30.0.1, MIT OR Apache-2.0 | You rasterise glyphs to an atlas texture and draw instanced quads, one per cell. | The standard design. Cheapest per frame, and you control damage tracking, subpixel mode and cell snapping. You write and maintain the atlas packer, eviction and shaders. |
| `glyphon` | 0.12.0 (2026-07-09), MIT OR Apache-2.0 OR Zlib, 21 open issues | Text renderer for wgpu: cosmic-text shapes and rasterises, etagere packs the atlas, wgpu draws. https://github.com/grovesNL/glyphon | Works out of the box for text, but it is a text layout renderer and has no notion of a cell grid. Per cell backgrounds and attributes need separate quad passes. Used by the Oxidite terminal (small project). |
| `cosmic-text` | 0.19.0 (2026-04-22), MIT OR Apache-2.0, 125 open | Layout, shaping (HarfRust), swash rasterisation, custom font fallback, bidi. https://github.com/pop-os/cosmic-text | Reasonable if you want an integrated stack. cosmic-term uses it with the `monospace_fallback` and `shape-run-cache` features. |
| `swash` | 0.2.10 (2026-07-17), Apache-2.0 OR MIT | Shaping, introspection, scaling. | Glyph rasteriser used by cosmic-text and sugarloaf. |
| Rio `sugarloaf` | 0.5.29 (2026-11-01), MIT, 91,950 downloads | Rio's renderer. Manifest shows `skrifa 0.37`, `swash 0.2.10`, `zeno`, `tiny-skia`, `guillotiere`, native Metal on macOS (`baremetal` crate), optional wgpu enabled for Windows and wasm, and its own `rio-fonts` and `rio-unicode`. https://github.com/raphamorim/rio | Built for a terminal, but tied to Rio's internals and changing quickly. Useful as a reference, not an obvious dependency. |
| Alacritty's OpenGL renderer | `crossfont` 0.9.0 (Apache-2.0), `glutin` 0.32.3, `winit` 0.30 | Texture atlas, instanced draws, GL 3.3 or GLES 2. https://github.com/alacritty/alacritty | The proven fast and minimal design. Alacritty 0.17.0 released 2026-04-06. OpenGL on macOS is deprecated by Apple but still works. Alacritty's changelog lists a crash on OpenGL context reset fixed in 0.17.0. |
| Zed GPUI | not published as a standalone stable crate | GPUI renders with Metal, DirectX and wgpu. | Not a reusable dependency for Termoil. |
| WezTerm renderer | repository only | OpenGL through `glium` in the `window` crate, and a wgpu front end (`wezterm-gui` depends on `wgpu`). Fonts: freetype, harfbuzz, fontconfig, `dwrote`, `core-text`. https://github.com/wezterm/wezterm | A reference. Not a reusable library. |
| Skia via `skia-safe` | 0.156.0 (2026-10-26), MIT, 56 open | Full 2D library with GPU backends. https://github.com/rust-skia/rust-skia | Large native build and binary, and the same problem as vello: a general engine used for a grid. |
| `tiny-skia` | 0.12.0 (2026-02-02), BSD 3 Clause | CPU rasteriser, no GPU. https://github.com/linebender/tiny-skia | Adequate CPU fallback, single threaded, now a Linebender project. `vello_cpu` is the faster option. |
| `softbuffer` | 0.4.8 (2025-12-13), MIT OR Apache-2.0 | Puts a CPU pixel buffer on a winit window. | Pairs with `vello_cpu` or `tiny-skia` for a fallback with no GPU. |

### 1.8 Recommendation on vello

**Vello is the wrong primary renderer for the cell grid in November 2026, and is a suitable choice for a secondary role.** Reasons:

1. The grid is a texture atlas problem. Nothing else in the surveyed set beats instanced atlas quads for a monospace grid, and Alacritty, Rio, WezTerm and Kitty all use that approach.
2. The component Termoil would depend on, the vello glyph cache, is labelled experimental in two places (the `vello_gpu` docs and the `glifo` README), and `vello_gpu` itself is "in early stages" per its release notes.
3. The only terminal on vello (CuTTY) uses the older compute renderer (0.7.0), which the vello project has moved to `research/` and calls experimental. The project's stated direction is `vello_gpu`, which has no terminal in the wild.
4. Every vello release in 2026 has broken the API (rename, wgpu major bump, `u16` sizes, glyph API changes). A terminal that has to follow wgpu upgrades on every vello release takes on maintenance cost.

**Recommended layering:**

- Define a `Renderer` trait in Termoil (cells, cursor, selection, images, damage rectangles).
- Primary implementation: wgpu 30 with a Termoil owned glyph atlas and instanced quads. Glyph rasterisation by `skrifa` (outlines and hinting) or `swash`, shaping by `harfrust`, through parley or directly.
- CPU fallback implementation: `vello_cpu` with `softbuffer`, drawing glyphs through glifo's atlas. This is the most mature vello component, it needs no GPU, and it gives remote desktop, VM and driver bug cases a working path.
- Optional experiment: a `vello_gpu` backend behind the same trait, to decide with measurements whether to replace the custom atlas later. Gate on p99 frame time at 120 Hz for a full screen scroll of a 200 by 60 grid, LCD text availability, and memory.
- If the TDD keeps vello as the primary against this recommendation, choose `vello_gpu` (not the compute `vello`) because it is the project's declared future and works without compute shaders, and keep the custom atlas renderer as the fallback in place of `vello_cpu`.

Spike items needed before the TDD is final: a measurement of `vello_gpu` glyph atlas frame cost at 12,000 glyphs, whether scene reuse per row is possible, and whether LCD subpixel coverage is reachable.

## 2. Text stack

| Crate | Version, date | Licence | Role | Notes |
|---|---|---|---|---|
| `parley` | 0.12.0, 2026-10-09 | Apache-2.0 OR MIT | Rich text layout: line breaking, bidi, alignment | 122 open issues. 0.12.0 introduced `parley_engine` and makes `Cluster` span whole extended grapheme clusters. Built for CSS like layout, which is more than a terminal needs. https://github.com/linebender/parley |
| `fontique` | 0.12.0, 2026-10-09 | Apache-2.0 OR MIT | Font enumeration and fallback | System backends in its source: DirectWrite on Windows, Core Text on Apple, fontconfig on Linux and FreeBSD (optional `fontconfig-dlopen`), and an Android backend. Memory maps font files. https://github.com/linebender/parley/tree/main/fontique |
| `harfrust` | 0.15.0, 2026-10-22 | MIT | Shaping (HarfBuzz port) | Replaces `rustybuzz`. Applies ligatures and resolves emoji sequences. |
| `skrifa` | 0.49.0, 2026-10-21 | MIT OR Apache-2.0 | Outlines, hinting, metrics, colour glyph painting | From Google Fonts' fontations, used as Chrome's font backend on Android and Linux. Hints with the font's own TrueType instructions or an automatic hinter, with an LCD target option. 265 open issues in the fontations repository. https://github.com/googlefonts/fontations/tree/main/skrifa |
| `read-fonts` | 0.46.0, 2026-10-21 | MIT OR Apache-2.0 | Low level font table parser | Dependency of skrifa and fontique. |
| `swash` | 0.2.10, 2026-07-17 | Apache-2.0 OR MIT | Own shaper, scaler and rasteriser | Per its README: GSUB and GPOS shaping, variable fonts, asymmetric vertical hinting, horizontal subpixel rendering and fractional positioning, sbix, CBLC/CBDT and COLR/CPAL emoji. COLR v1 gradients are not listed in the README, so treat COLRv1 as absent in swash. https://github.com/dfrg/swash |
| `cosmic-text` | 0.19.0, 2026-04-22 | MIT OR Apache-2.0 | Layout, shaping (HarfRust), rasterisation (swash), own fallback | Its README says Linux, macOS and Windows have the full feature set. Fallback is a custom list modelled on browsers. https://github.com/pop-os/cosmic-text |
| `rustybuzz` | 0.20.1, 2024-11-12 | MIT | Shaping | Archived and unmaintained. Do not use. |
| `harfbuzz_rs` | 3.0.1, 2026-10-21 | MIT | Binding to the C HarfBuzz library | Needs the C library. Only relevant if exact HarfBuzz behaviour is required. |
| `ab_glyph` | 0.2.32, 2025-09-28 | Apache-2.0 | Outline loading and rasterisation, no shaping | No ligatures and no fallback. Too limited. |
| `fontdb` | 0.24.0, 2026-07-29 | MIT | Font database for system fonts | Used by cosmic-text. No fallback logic of its own. |
| `font-kit` | 0.14.3, 2025-05-26 | MIT OR Apache-2.0 | Native font loading (DirectWrite, Core Text, FreeType, fontconfig) | Servo project. Last release over a year ago. |
| `ttf-parser` | 0.25.1, 2024-11-29 | MIT OR Apache-2.0 | Parser | Superseded in the Linebender stack by read-fonts. |

Capability summary:

- **Shaping and ligatures:** `harfrust` (through parley or cosmic-text) or swash's own shaper. Terminal ligatures (for example `=>` in Fira Code) need shaping of runs of cells with the font's `calt` feature, which both support. A terminal then has to map the shaped glyphs back to cells and break the run at cursor, selection and style boundaries. Neither crate does that mapping for you.
- **Rasterisation:** skrifa outlines plus a rasteriser (vello, swash, or `zeno`, which sugarloaf uses), or swash directly.
- **Fallback:** `fontique` (system fonts per script and locale on all three platforms) or cosmic-text's custom lists. A terminal usually wants a user configured fallback list first, then the system fallback.
- **Colour emoji:** COLRv1 through skrifa's `ColorPainter` (implemented in glifo's `colr.rs`), bitmap strikes (sbix, CBDT) in swash and glifo (its `png` feature), and SVG in OpenType. I did not find SVG in OpenType support in these crates, so assume it is missing. I did not verify which COLR version the Windows emoji font ships.
- **Hinting:** skrifa (interpreter and automatic hinter), swash (vertical only).
- **Subpixel positioning:** swash, and skrifa based glifo, quantise a fractional offset. A terminal on integer cell advances does not need it.
- **Platform state:** fontique has native DirectWrite, Core Text and fontconfig backends. cosmic-text on Windows and macOS uses fontdb to scan system font directories, with its own fallback tables.

Recommendation: `fontique` + `harfrust` + `skrifa`, used directly or through parley, because they are one maintained family with consistent versions. If Termoil avoids parley, those three crates work without it. If a lower risk integrated stack is preferred, `cosmic-text` 0.19 is the alternative, and it is what cosmic-term ships.

## 3. Terminal core

### 3.1 Candidates

| Crate | Version, date | Licence | What it provides | Stability |
|---|---|---|---|---|
| `alacritty_terminal` | 0.26.0, 2026-04-06 | Apache-2.0 | Grid with scrollback, VT state machine on `vte`, selection, regex search, vi mode, PTY and event loop on Unix and Windows (ConPTY), `Term` with damage tracking, kitty keyboard mode flags. | Pre 1.0. Breaking changes are listed in bold in its CHANGELOG. 0.26.0 changed `ChildEvent::Exited` and `Event::ChildExit` to carry `ExitStatus` instead of `i32`; 0.25.0 replaced `Options::hold` with `drain_on_exit`. 0.26.1 development fixes a PTY non blocking panic, unbounded per cell memory for zero width cells and an unsound `Row::new` with zero columns. Used by Zed (`crates/terminal`) and cosmic-term. https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/CHANGELOG.md |
| `vte` | 0.15.0, 2025-02-02 | Apache-2.0 OR MIT | Paul Williams state machine parser and an `ansi` module with a `Handler` trait. 13 open issues. Last push 2026-10-25. | Parser only, no screen. `Parser::advance` takes byte slices since 0.14. |
| `termwiz` | 0.23.3, 2025-03-20 | MIT | WezTerm's escape parser, surface, cell and terminal abstraction, widgets. | Published, but the last release was 2025-03. |
| `wezterm-term` | 0.1.0, not on crates.io | MIT | WezTerm's terminal emulator core: screen, scrollback, kitty and iTerm2 and sixel images, hyperlinks. | Git dependency only. No stability promise. https://github.com/wezterm/wezterm/tree/main/term |
| `vt100` | 0.18.1, 2026-10-19 | MIT | Parser and in memory screen, aimed at programs like tmux and screen. 12 open issues. | Active. Not designed for a GUI terminal, and I saw no kitty keyboard or graphics support in its README or changelog. https://github.com/doy/vt100-rust |
| `rio-vt` | 0.5.29, 2026-11-01 | MIT | Rio's terminal core extracted as a library: VT state machine, grid with scrollback, PTY driver, selection, search, kitty graphics and sixel, damage tracking. No rendering dependencies. | Very new, 115,973 downloads, docs suggest depending on `rio-vt = "0.5"` with no stated API guarantee. https://docs.rs/crate/rio-vt/latest |
| `libghostty-vt` | 0.2.2, 2026-09-28 | MIT OR Apache-2.0 | Rust binding of the Ghostty terminal library: `Terminal`, `RenderState`, `KeyEncoder`, `MouseEncoder`, with a PTY write back handler. | Pre 1.0. The build needs Zig 0.17.x and fetches a pinned Ghostty source. wasm32 is supported. https://github.com/Uzaaft/libghostty-rs |

C libraries for comparison (from general knowledge, not re-verified in this session): `libvterm` (used by Neovim, a common conformance reference) and Contour's terminal library (C++, supports sixel and grapheme cluster mode 2027). Both add a build step and an FFI boundary. Neither is a candidate for a Rust design, but both are sources of test cases.

### 3.2 Build or reuse?

Recommendation: reuse `vte` as the byte level parser and write the screen and grid layer in Termoil, with `alacritty_terminal` as the first reference. Reasons:

- `alacritty_terminal` is the most proven reusable core (Alacritty 0.17.0, Zed, cosmic-term). Its limits: it has no kitty graphics or sixel in its dependency list (no image crates appear in its manifest), and grapheme handling is a `char` per cell plus zero width extras (hence the changelog fix for "unbounded per cell memory usage for zero width cells"). If Termoil wants kitty graphics, mode 2027 grapheme clusters or its own scrollback format, those are changes to the core grid, which is cheaper in your own code than in a fork.
- The VT parser is the smallest and best understood part: `vte` 0.15 is MIT or Apache and used by Alacritty and Zed.
- `rio-vt` already ships kitty graphics and sixel, but it is new and its API is unstable. `libghostty-vt` has the most modern feature set but pulls Zig into the build.
- The cost of writing the grid, scrollback, reflow on resize, alt screen, scroll regions, tab stops, charsets and the long tail of xterm sequences is roughly the size of the Alacritty `term` module, plus conformance testing (vttest, esctest, kitty's tests). That is weeks of work.

Decision for the TDD: either (a) depend on `alacritty_terminal` 0.26 and accept its limits for version 1, or (b) write the grid and use `vte`. The research supports (b) only if kitty graphics, grapheme clusters or a custom scrollback are version 1 requirements.

### 3.3 Unicode crates

| Crate | Version, date | Licence | Unicode version |
|---|---|---|---|
| `unicode-width` | 0.2.3, 2026-10-28 | MIT OR Apache-2.0 | 18.0.0 (constant `UNICODE_VERSION` in `src/tables.rs`) |
| `unicode-segmentation` | 1.13.3, 2026-06-01 | MIT OR Apache-2.0 | The version is a constant in `src/tables.rs`; I did not capture the value. |
| `icu_properties` | 2.4.0, 2026-10-22 | Unicode-3.0 | The ICU4X changelog lists "Updated to Unicode 18" in a recent release. I did not confirm that this is 2.4.0. |
| `icu_segmenter` | 2.4.0, 2026-10-22 | Unicode-3.0 | Same. Also supports line, word, sentence and grapheme segmentation with compiled data. |

Width rules differ between terminals. `unicode-width` follows East Asian Width plus emoji presentation rules. ConPTY has `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES`, `PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH`, `PSEUDOCONSOLE_GLYPH_WIDTH_CONSOLE` and `PSEUDOCONSOLE_AMBIGUOUS_IS_WIDE`, defined in the current `winconpty.h`, so the terminal and ConPTY must agree on how wide a cluster is or the cursor drifts. https://github.com/microsoft/terminal/blob/main/src/winconpty/winconpty.h

## 4. PTY and process spawning

### 4.1 Crates

| Crate | Version, date | Licence | Platforms | Notes |
|---|---|---|---|---|
| `portable-pty` | 0.9.0, 2025-02-11 | MIT | Unix and Windows ConPTY | WezTerm's PTY crate. Last release 20 months ago, though the repository is active. |
| `pty-process` | 0.5.4, 2026-10-19 | MIT | Unix only | Wraps `std::process::Command` or the tokio Command. |
| `rustix-openpty` | 0.2.0, 2025-03-06 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Unix | Thin `openpty` on rustix. What `alacritty_terminal` uses on Unix. |
| `nix` | 0.31.3, 2026-05-11 | MIT | Unix | `openpty`, `forkpty`, termios, signals. |
| `conpty` | 0.7.0, 2024-09-23 | MIT | Windows | Small wrapper from the expectrl author. |
| `windows` | 0.62.2, 2025-10-06 | MIT OR Apache-2.0 | Windows | Official Microsoft bindings, including `CreatePseudoConsole`. |
| `windows-sys` | 0.61.2, 2025-10-06 | MIT OR Apache-2.0 | Windows | Raw bindings. Alacritty's tty uses 0.59. |
| `teletypewriter` | 2.0.1, 2026-11-01 | MIT | Unix and Windows | Rio's PTY crate. |
| `signal-hook` | 0.4.6, 2026-10-31 | MIT OR Apache-2.0 | Unix | SIGCHLD and SIGWINCH handling. |

Recommendation: own a thin `Pty` trait. On Unix use `rustix-openpty` (or `nix`) and spawn with `std::process::Command` and `pre_exec` for `setsid` and `TIOCSCTTY`. On Windows call `CreatePseudoConsole` through `windows-sys` or `windows`, loading the Windows Terminal `conpty.dll` and `OpenConsole.exe` pair when present. `portable-pty` is a workable shortcut for a prototype, but its release age is a risk.

### 4.2 ConPTY facts

- `CreatePseudoConsole` needs Windows 10 1809 or later. The documented flag in Microsoft's reference page is `PSEUDOCONSOLE_INHERIT_CURSOR` only. https://learn.microsoft.com/en-us/windows/console/createpseudoconsole
- Current `winconpty.h` in microsoft/terminal main defines `PSEUDOCONSOLE_INHERIT_CURSOR` (0x1), glyph width mode flags (mask 0x18) and `PSEUDOCONSOLE_AMBIGUOUS_IS_WIDE` (0x20). It does not define a passthrough flag. Third party projects claim a `PSEUDOCONSOLE_PASSTHROUGH_MODE` flag with value 0x8 on Windows 11 22H2 and later; that value would collide with `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` in the current header, so I could not confirm it and Termoil should not rely on it. https://github.com/wudream813/super-termux/releases/tag/v2.3.6 (third party claim)
- The inbox ConPTY (kernel32 and conhost) renders child output into its own virtual screen buffer and re-emits it as VT. Several projects report that sixel (DCS) and kitty graphics (APC) sequences are stripped before the terminal sees them, including on Windows 11 build 10.0.26100.x (the 24H2 line). Reports: https://github.com/openwong2kim/wmux/pull/1964, https://github.com/flotilla-org/cleat/pull/235, and an unresolved case at https://github.com/D0n9X1n/SonicTerm/issues/1561. I found no Microsoft statement that 24H2 changed this.
- Microsoft ships updated ConPTY as the NuGet package `Microsoft.Windows.Console.ConPTY`, latest listed 1.25.260930003, which contains `conpty.dll` plus `OpenConsole.exe`, works on Windows 10.0.17763 and later, and silently falls back to the system conhost if `OpenConsole.exe` is missing beside the DLL. https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY
- Alacritty tries `LoadLibraryW("conpty.dll")` and falls back to kernel32 (`alacritty_terminal/src/tty/windows/conpty.rs`). WezTerm ships the pair beside `wezterm.exe`, and its changelog says the DLL and EXE must be the matched pair. WezTerm's tracker shows version drift and a crash fixed by updating both files to 1.24. https://github.com/wezterm/wezterm/issues/7774
- Resize: since ConPTY 1.22, ConPTY no longer repaints the screen after a resize, and the host terminal must reflow its own buffer to match ConPTY's reflow. https://github.com/microsoft/terminal/issues/16911 and https://github.com/yeroo/agwinterm/issues/351 (third party summary). Termoil on Windows has to implement reflow identical to Windows Terminal's, or pin an older ConPTY.
- Windows Terminal itself is adding the kitty graphics protocol (PR #20872, seen open), and the PR says conhost passes APC replies back through ConPTY. https://github.com/microsoft/terminal/pull/20872
- ConPTY's pipes are synchronous only, so use a dedicated reader thread and writer thread per session.

### 4.3 Shell spawn

- Unix: allocate the PTY pair, call `setsid`, set the controlling terminal, set the initial window size with `TIOCSWINSZ`, and `exec` the shell. Set `TERM`, `COLORTERM=truecolor`, `TERM_PROGRAM` and `PWD`, and clean up variables inherited from the launcher. For a login shell, Alacritty on macOS runs the shell through `login` (its changelog notes passing `-q` when `~/.hushlogin` exists). On Linux the usual convention is argv[0] prefixed with `-`. Termoil must decide whether it launches login shells by default. These details come from knowledge of those programs and were not re-fetched.
- Windows: `CreateProcessW` with `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, `EXTENDED_STARTUPINFO_PRESENT` and careful command line quoting. `alacritty_terminal` 0.26 added `escape_args` in `tty::Options` for shell argument escaping control.
- Working directory: pass it explicitly. Tracking the shell's current directory needs OSC 7 or process inspection.
- Exit codes: on Unix `waitpid` returns an `ExitStatus` that distinguishes exit codes from signals, which is why `alacritty_terminal` 0.26 changed its events to carry `ExitStatus`. On Windows `GetExitCodeProcess` returns a u32 (NTSTATUS values such as 0xC000013A for Ctrl+C).
- Signals: send SIGHUP to the foreground process group when closing the window, handle SIGCHLD to reap, and SIGWINCH is delivered by the kernel after `TIOCSWINSZ`. On Windows, closing the pseudoconsole ends the attached processes.

## 5. Windowing, input, clipboard

| Crate | Version, date | Licence | Notes |
|---|---|---|---|
| `winit` | 0.30.13 stable (2026-03-02); 0.31.0-beta.4 (2026-10-24) | Apache-2.0 | 731 open issues and pull requests, last push 2026-11-01. |
| `tao` | 0.37.2, 2026-10-23 | Apache-2.0 | Tauri's winit fork with menus and tray. |
| `sdl3` | 0.21.0, 2026-11-01 | MIT | Rust binding of SDL3. |
| `glutin` | 0.32.3, 2025-04-30 | Apache-2.0 | OpenGL context creation. Alacritty uses it with winit. Not needed with wgpu. |
| `raw-window-handle` | 0.6.2, 2024-05-17 | MIT OR Apache-2.0 | Bridge between winit and wgpu or softbuffer. |
| `arboard` | 3.7.0, 2026-10-15 | MIT OR Apache-2.0 | Text and image clipboard on Windows, macOS and Linux (X11 by default; the `wayland-data-control` feature for Wayland, which needs compositor support for the data control protocols). https://github.com/1Password/arboard |
| `copypasta` | 0.10.2, 2025-04-25 | MIT / Apache-2.0 | Alacritty's clipboard crate. |

winit facts:

- 0.31.0-beta.1 removed the `KeyEventExtModifierSupplement` trait and made `text_with_all_modifiers` and `key_without_modifiers` public fields of `KeyEvent`. It added `Window::request_ime_update`, `Ime::DeleteSurrounding`, more `ImePurpose` values and `ImeHints`, and deprecated `set_ime_allowed`, `set_ime_cursor_area` and `set_ime_purpose`. https://github.com/rust-windowing/winit/releases/tag/v0.31.0-beta.1
- 0.31.0-beta.4 added clipboard get and set on `ActiveEventLoop` (Wayland, X11, macOS, Windows), primary selection on Wayland and X11, a mouse wheel `source` field, and fixes for IME preedit cursor offsets on Windows, IME after seat changes on Wayland, and unfinished IME composition on macOS. It also stops sending an empty `Ime::Preedit` before `Ime::Commit`. https://github.com/rust-windowing/winit/releases/tag/v0.31.0-beta.4
- Key events carry a physical key (`KeyCode`), a logical key, `text`, `location`, `repeat` and the modifier supplement fields. Alacritty's input code (`alacritty/src/input/keyboard.rs`) reads `text_with_all_modifiers()` and `key_without_modifiers()`, and works around a Windows case where Ctrl+Alt mangles `logical_key`. It is the best worked example of turning winit events into terminal bytes. https://github.com/alacritty/alacritty/blob/master/alacritty/src/input/keyboard.rs
- Known quirks: winit issue #3038 (Wayland sets `text` with Ctrl held) and #2898 (a Windows Dvorak case with Ctrl+0). Both are old and may be fixed; I did not retest them.
- High DPI is handled through `scale_factor` and `ScaleFactorChanged`. Multiple windows are supported through the event loop. X11 and Wayland are both supported (Wayland client side decorations are the application's concern; Alacritty enables `wayland-csd-adwaita`).
- IME status per platform is not summarised in any single document that I found. The beta changelogs show active IME fixes on Windows, macOS and Wayland, so treat IME as a test matrix item and not a solved problem.

Kitty keyboard protocol (https://sw.kovidgoyal.net/kitty/keyboard-protocol/): the application sets flags (disambiguate escape codes, report event types, report alternate keys, report all keys as escape codes, report associated text) and the terminal then encodes each event. The encoder needs the unshifted key code (`key_without_modifiers`), the shifted and base layout alternates, modifiers including Super, Hyper, Meta, CapsLock and NumLock, the event type (press, repeat, release), and the text produced. winit supplies most of these, but the base layout key is not obviously available on every platform. Alacritty's `SequenceBuilder` in the same file implements the protocol with `TermMode` flags and is the closest reference. Alacritty's 0.18 development changelog fixes an unbounded kitty keyboard mode stack, so enforce a stack limit.

Alternatives: `sdl3` has a mature IME and gamepad stack but weaker integration with native menus and Wayland decorations. `tao` adds menus but follows winit. `glutin` is only for OpenGL contexts. Recommendation: `winit`, starting on 0.30.13 behind a small window abstraction, and moving to 0.31 when it leaves beta, unless the 0.31 IME API is needed sooner. Alacritty (0.30.9 and later) and CuTTY (0.30.9) are both on 0.30.

wgpu: 30.0.1 covers Vulkan, Metal, DX12 and GL as described in section 1.3. Create surfaces through `raw-window-handle` 0.6. wgpu's MSRV is 1.87.

## 6. Configuration and miscellany

| Need | Crate | Version, date | Licence | Note |
|---|---|---|---|---|
| Config format | `toml` | 1.1.8 (spec 1.1.0), 2026-10-09 | MIT OR Apache-2.0 | Alacritty 0.17 added TOML 1.1 syntax support. `toml_edit` for editing in place. |
| Serialisation | `serde` | 1.0.229, 2026-07-18 | MIT OR Apache-2.0 | |
| Config reload | `notify` | 8.2.1, 2026-10-27 | CC0-1.0 | Alacritty uses 8.0. Watch the parent directory and debounce, because editors replace files. |
| Logging | `tracing` | 0.1.44, 2025-12-18 | MIT | |
| Channels | `crossbeam` | 0.8.5, 2026-09-05 | MIT OR Apache-2.0 | |
| Async runtime | `tokio` | 1.54.1, 2026-10-21 | MIT | |
| I/O polling | `mio` | 1.2.4, 2026-10-03 | MIT | |
| I/O polling | `polling` | 3.11.0, 2025-09-14 | Apache-2.0 OR MIT | Alacritty and `alacritty_terminal` use it for the PTY event loop. |
| URL detection | `regex` | 1.13.1, 2026-07-15 | MIT OR Apache-2.0 | `alacritty_terminal` uses `regex-automata` directly for grid search. |
| Image decode | `image` | 0.25.10, 2026-03-10 | MIT OR Apache-2.0 | PNG for kitty graphics. Raw RGB and RGBA frames need no decoder. |
| zlib | `flate2` | 1.1.10, 2026-08-28 | MIT OR Apache-2.0 | Kitty graphics `o=z` compression. |
| base64 | `base64` | 0.23.1, 2026-08-04 | MIT OR Apache-2.0 | Kitty graphics payloads and OSC 52 clipboard. |

Tokio against a thread per PTY: Alacritty and `alacritty_terminal` use a dedicated PTY thread with `polling` and no async runtime. A terminal reads PTY bytes, parses them and publishes a snapshot, which is a single producer and single consumer flow that a plain thread per PTY with a bounded channel handles well. Tokio adds value only for features that talk over the network (remote sessions, an SSH client). Recommendation: one OS thread per PTY using `polling` or blocking reads, `crossbeam` channels to the UI thread, and no tokio in the core.

## 7. Existing Rust terminals

| Terminal | Core | Renderer | Font stack | PTY | Notes |
|---|---|---|---|---|---|
| Alacritty 0.17.0 (2026-04-06), Apache-2.0, 66k stars, 344 open | `alacritty_terminal` 0.26 on `vte` 0.15 | OpenGL 3.3 or GLES 2 through `glutin` with a glyph atlas | `crossfont` 0.9 (FreeType, DirectWrite, Core Text) | In `alacritty_terminal` (`rustix-openpty`, ConPTY through `windows-sys`, optional `conpty.dll`) | No ligatures, no shaping, no images, no tabs by design. Fixed in 0.17: crash on OpenGL context reset, IME crash on macOS. https://github.com/alacritty/alacritty |
| WezTerm, MIT, 29k stars, 1,945 open | `wezterm-term` and `termwiz` | OpenGL (`glium`) and a wgpu front end | FreeType, HarfBuzz (C), fontconfig, `dwrote`, `core-text` | `portable-pty` | Last tagged release 20240203; the repository still receives pushes (2026-10-26) and nightly builds. Bundles `conpty.dll` and `OpenConsole.exe`. Single maintainer risk. https://github.com/wezterm/wezterm |
| Rio 0.5.29 (2026-11-01), MIT, 7.7k stars, 338 open | In house `rio-vt` | `sugarloaf`: native Metal on macOS, wgpu on Windows and web, plus librashader filters | `rio-fonts`, `skrifa` 0.37, `swash`, `zeno`, `core-text` | In house `teletypewriter` | Fast release cadence and fast structural change (the terminal core moved into `rio-vt` during 0.5). The workspace manifest declares `rust-version` 1.99.1. https://github.com/raphamorim/rio |
| cosmic-term (COSMIC desktop), 601 stars, 276 open | `alacritty_terminal` 0.25.1 | `libcosmic` (iced on winit) | `cosmic-text` 0.19 with `monospace_fallback` and `shape-run-cache` | `alacritty_terminal` | Tied to COSMIC libraries by git dependency. https://github.com/pop-os/cosmic-term |
| Warp | In house | `warpui` (MIT) on GPU | In house | In house | Client open source under AGPL 3.0 (UI crates MIT), 65.8k stars. I did not confirm its current renderer from a primary source (Metal originally). https://github.com/warpdotdev/Warp |
| Zed terminal | `alacritty_terminal` and `vte` | GPUI (Metal, DirectX, wgpu) | GPUI text system | `alacritty_terminal` | `crates/terminal` in the Zed repository. Zed has 3,014 open issues and pull requests. |
| CuTTY | Alacritty fork (`cutty_terminal` 0.26.4) | wgpu plus vello 0.7 | parley 0.7 | `alacritty_terminal` | Beta, one maintainer, Apache-2.0. |
| Tattoy | `shadow-terminal` (headless terminal) | None. Draws UTF8 half blocks inside a host terminal | None | `portable-pty` | A compositor for an existing terminal and not a GUI terminal. Last push 2025-09-14. https://github.com/tattoy-org/tattoy |
| Helix | None | | | | A terminal application, not an emulator. Listed because the brief names it. |
| Ratatui 0.30.2 | None | | | | A library for text UIs, not a terminal emulator. |

Other terminals mentioned for reference but not studied: Ghostty (Zig; its VT core is available to Rust as `libghostty-vt`), Kitty (C and Python, the reference for kitty protocols), Contour (C++).

## 8. Recommended crate list for Termoil

Versions as of 2026-11-01. "Pin" means pin an exact version because the crate is pre 1.0 and makes breaking changes in minor releases.

**Core**
- `vte` 0.15.0 (parser). Or `alacritty_terminal` 0.26.0 if section 3.2 option (a) is chosen. Pin.
- `unicode-width` 0.2.3, `unicode-segmentation` 1.13.3. Optionally `icu_segmenter` 2.4.0 for line breaking only.
- `base64` 0.23.1, `flate2` 1.1.10, `image` 0.25.10 (PNG feature only).
- `regex` 1.13.1 for hyperlinks and URL detection, `serde` 1.0.229, `toml` 1.1.8.
- `tracing` 0.1.44, `crossbeam` 0.8.5, `polling` 3.11.0.
- `notify` 8.2.1.

**PTY**
- Unix: `rustix-openpty` 0.2.0 with `rustix` 1.1.6 and `signal-hook` 0.4.6. Or `nix` 0.31.3.
- Windows: `windows` 0.62.2 (or `windows-sys` 0.61.2) calling `CreatePseudoConsole`, plus the Microsoft ConPTY NuGet pair (1.25.260930003 at last check) shipped beside the executable.
- Prototype shortcut: `portable-pty` 0.9.0.

**Window and input**
- `winit` 0.30.13 (move to 0.31 after it leaves beta), `raw-window-handle` 0.6.2.
- `arboard` 3.7.0 with the `wayland-data-control` feature, or the winit 0.31 clipboard API.

**Rendering**
- `wgpu` 30.0.1 with a Termoil glyph atlas (instanced quads). Primary.
- `softbuffer` 0.4.8 plus `vello_cpu` 0.3.0 and `glifo` 0.4.0 for the CPU fallback. Pin.
- Optional spike: `vello_gpu` 0.3.0 behind the same renderer trait. Pin.

**Text**
- `fontique` 0.12.0 (font discovery and fallback), `harfrust` 0.15.0 (shaping), `skrifa` 0.49.0 (outlines, hinting, COLR), `read-fonts` 0.46.0. Add `parley` 0.12.0 only if parley's layout is wanted. Alternative single crate: `cosmic-text` 0.19.0.

## 9. Risks

1. **Vello API instability.** `vello_gpu` was renamed in October 2026, bumped wgpu majors, and changed glyph and atlas APIs. `glifo` is experimental and `vello_gpu` glyph caching is flagged experimental. Mitigation: a renderer trait, exact pins, and the atlas renderer as primary.
2. **No evidence for vello at terminal scale.** The only vello terminal (CuTTY) is beta, has one maintainer, runs on the older compute renderer, and has self reported benchmarks only. Mitigation: the spike in section 1.8 before committing.
3. **wgpu major version churn.** vello and wgpu move together. Every wgpu major release (30 now) can force vello and surface changes. Mitigation: isolate wgpu use in the renderer crate.
4. **wgpu memory and startup.** One report measured about 10 times the memory of a native renderer for an empty window on Windows. Mitigation: measure in a Termoil prototype and offer the CPU path.
5. **ConPTY on Windows.** The inbox ConPTY drops sixel and kitty graphics sequences and re-renders output. Resize needs host side reflow matching ConPTY since 1.22. The bundled `conpty.dll` and `OpenConsole.exe` must be a matched pair and must be updated deliberately; WezTerm shipped old pairs and crashed. The passthrough flag claim is unverified. This is the largest platform risk.
6. **winit 0.31 is still beta.** Staying on 0.30 means the older IME and key APIs. Moving to 0.31 means API changes at the end of the beta. IME correctness across Windows, macOS, X11 and Wayland needs a test matrix.
7. **Terminal core choice.** `alacritty_terminal` has no kitty graphics or sixel and no grapheme cluster cells, and it breaks its API between minor versions. Writing the grid yourself costs weeks and needs a conformance suite. `rio-vt` and `libghostty-vt` are new with no stability promise, and the latter needs Zig.
8. **Terminal width disagreements.** Termoil, applications and ConPTY can disagree on cluster width (emoji, ZWJ sequences, ambiguous width). `unicode-width` 0.2.3 already has Unicode 18 data, and other crates update at different times.
9. **Font features.** Ligature to cell mapping, COLRv1 and SVG emoji, LCD subpixel text and fallback performance are not solved by any single crate. Fontique's Windows and macOS fallback behaviour needs testing on real machines.
10. **Stale or unmaintained crates.** `rustybuzz` (archived), `font-kit` (last release 2025-05), `portable-pty` (last release 2025-02), `ttf-parser` (superseded), `glutin` (last release 2025-04), `copypasta` (2025-04). Mitigation: avoid or wrap.
11. **Small maintainer teams.** WezTerm has had no tagged release since 2024-02, Rio and CuTTY are mostly one person, and `vte` has few maintainers. Reference code is fine, dependence needs vendoring or forking plans.
12. **Licence.** Warp's client is AGPL 3.0 and cannot be copied into a permissive project. Alacritty is Apache-2.0, WezTerm and Rio are MIT, skia-safe is MIT, tiny-skia is BSD 3 Clause, `dwrote` is MPL-2.0 and `notify` is CC0. Termoil can stay Apache or MIT with the recommended choices, but check `dwrote` before using it.
13. **MSRV.** vello needs Rust 1.89, parley and fontique 1.88, wgpu 1.87, glyphon 1.92, and Rio's workspace declares 1.99.1. Pick a toolchain policy early.
14. **Facts I could not confirm.** Android and iOS status of `vello_gpu`, memory figures for vello, LCD text in glifo, the Unicode version of `unicode-segmentation` and `icu_properties` 2.4.0, current Windows 11 24H2 ConPTY behaviour from a Microsoft source, SVG in OpenType support, and Warp's current renderer. These belong on the spike list.

## 10. Source index

- vello repository and README: https://github.com/linebender/vello
- vello architecture: https://github.com/linebender/vello/blob/main/ARCHITECTURE.md
- Sparse Strips 0.3.0 release notes: https://github.com/linebender/vello/releases/tag/sparse-strips-v0.3.0
- vello_gpu docs: https://docs.rs/vello_gpu
- glifo: https://github.com/linebender/vello/tree/main/glifo
- vello issue 2041 (glyph atlas): https://github.com/linebender/vello/issues/2041
- Linebender Q1 2026: https://linebender.org/blog/tmil-25/
- Parley and fontique: https://github.com/linebender/parley
- fontations and skrifa: https://github.com/googlefonts/fontations
- swash: https://github.com/dfrg/swash
- cosmic-text: https://github.com/pop-os/cosmic-text
- glyphon: https://github.com/grovesNL/glyphon
- rustybuzz (archived): https://github.com/harfbuzz/rustybuzz
- wgpu: https://github.com/gfx-rs/wgpu
- winit: https://github.com/rust-windowing/winit
- Alacritty: https://github.com/alacritty/alacritty
- alacritty_terminal changelog: https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/CHANGELOG.md
- vte: https://github.com/alacritty/vte
- WezTerm: https://github.com/wezterm/wezterm
- Rio and rio-vt: https://github.com/raphamorim/rio and https://docs.rs/crate/rio-vt/latest
- cosmic-term: https://github.com/pop-os/cosmic-term
- CuTTY: https://github.com/gold-silver-copper/CuTTY
- Ratty: https://blog.orhun.dev/introducing-ratty/
- libghostty-rs: https://github.com/Uzaaft/libghostty-rs
- Warp: https://github.com/warpdotdev/Warp
- Zed GPUI wgpu PR: https://github.com/zed-industries/zed/pull/46758
- vt100: https://github.com/doy/vt100-rust
- Tattoy: https://github.com/tattoy-org/tattoy
- CreatePseudoConsole: https://learn.microsoft.com/en-us/windows/console/createpseudoconsole
- ConPTY flag header: https://github.com/microsoft/terminal/blob/main/src/winconpty/winconpty.h
- ConPTY NuGet package: https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY
- Kitty keyboard protocol: https://sw.kovidgoyal.net/kitty/keyboard-protocol/
- Crate pages: https://crates.io/crates/NAME for every crate named above (versions and dates queried through the crates.io API on 2026-11-01).
