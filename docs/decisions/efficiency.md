# Decision: memory and CPU against Ghostty, and the CPU renderer that came out of it

Written 2026-11-02 during task-2481. The ticket: "use 75 % less memory and CPU than Ghostty. Research online, build an
eval, and hill climb." This file records the eval, what was measured before and after each change, which changes were
kept, and what is still above the goal and why. It changes `docs/TDD.md` sections 4.2 and 4.7, `docs/PRD.md` section 5.2,
`docs/TEST-SUITE.md` sections 10 and 13, and adds a section to `docs/decisions/renderer.md`.

## Result

Median of three runs of each terminal, alternating, on the machine in `docs/performance.md`, commit of this change.
"Termoil/Ghostty" is Termoil's number as a share of Ghostty's; the goal is 25 % or less.

| Workload | Metric | Termoil before | Termoil now | Ghostty | Termoil/Ghostty |
|---|---|---|---|---|---|
| idle | private bytes, MB | 353.1 | 13.2 | 207.9 | 6.4 % |
| idle | private working set, MB | 127.7 | 11.9 | 55.2 | 21.5 % |
| idle | working set, MB | 174.7 | 31.5 | 85.9 | 36.7 % |
| idle | CPU over 10 seconds, s | 0.031 | 0.000 | 0.344 | 0 % |
| idle | CPU to start, s | 1.125 | 0.094 | 0.922 | 10.2 % |
| stream | CPU per 100 MB of output, s | 91.5 | 15.5 | 89.8 | 17.2 % |
| stream | pseudoconsole host CPU per 100 MB, s | 35.3 | 25.6 | 40.5 | 63.2 % |
| stream | peak private bytes, MB | 430.8 | 67.0 | 303.6 | 22.1 % |
| stream | private bytes after the output, MB | 372.7 | 24.0 | 303.6 | 7.9 % |
| stream | private working set after the output, MB | 147.3 | 22.5 | 72.2 | 31.2 % |
| stream | time to print 16 MiB, s | 14.0 | 11.0 | 14.2 | 77.6 % |

The "before" column is the 1.2.1 build (commit `5a62061`), measured by the same eval in the same session.

Three rows are above 25 %:

- **Working set** (36.7 %) counts pages shared with other processes. Of Termoil's 31.5 MB, 11.9 MB is its own; the rest
  is system DLLs (user32, combase, shell32, UI Automation, DirectWrite, the text input framework) and the font files,
  which every Windows program with a window and text maps. Termoil's own pages, the private working set, are 21.5 %.
- **The pseudoconsole host** (63.2 %) is Microsoft's `OpenConsole.exe` 1.25 for Termoil and the inbox `conhost.exe` for
  Ghostty. It is listed because it is part of what a terminal on Windows costs, but neither terminal's code runs in it.
  Termoil's host uses 37 % less CPU than Ghostty's, mostly because Termoil reads its output in fewer, larger reads.
- **Private working set after the output** (31.2 %) holds the history. Termoil keeps 50 MB of scrollback by default and
  Ghostty 10 MB, so after 16 MiB of output Termoil holds about five times more lines. With `scrollback.limit_bytes` set to
  Ghostty's 10 MB the same run settles at 17.0 MB, 23.5 % of Ghostty, with a peak of 26.4 MB (8.7 %). The default stays
  50 MB.

## The eval: `termoil-bench apps`

`tools/bench/src/apps.rs`. It starts each terminal inside a Windows job object, so every process the terminal creates is
found, and samples each process every 50 to 100 ms. Processes are split into three classes: the terminal's own
executable, the pseudoconsole host (`conhost.exe` or `OpenConsole.exe`), and the shell with whatever it runs. Both
terminals run the same shell and the same command, so the shell is reported but left out. The goal is stated on the
terminal's own executable; the host is reported beside it.

| Workload | What runs | Measured |
|---|---|---|
| `idle` | the window at a `cmd.exe` prompt | memory after 6 s, CPU over the next 10 s, CPU used to start |
| `stream` | a batch file waits until the window has settled, marks the start, `type`s 16 MiB of the `mixed` gate stream (`streams::mixed`), marks the end | CPU between the two marks per 100 MB, peak memory, memory 3 s after the end |

Memory is three numbers: private bytes (commit), the working set, and the private working set (resident pages no other
process shares, what Task Manager's Memory column shows). Memory the GPU driver keeps on the card is in none of them.
Both windows are 120 by 34 cells with Cascadia Code at 13 points. Termoil reads a configuration file written by the
harness (`TERMOIL_CONFIG`) and a data folder of its own; Ghostty runs with `--config-default-files=false` and a config file
written by the harness.

To repeat it:

```
cargo build --release -p termoil-bench -p termoil-app -p termoil-cli
# conpty.dll and OpenConsole.exe must sit beside termoil-gui.exe (tools/fetch-conpty.ps1, or copy them from an install)
$env:TERMOIL_BENCH_GHOSTTY = '<path to ghostty.exe>'
target/release/termoil-bench apps 3 16        # runs, MiB of output
```

`TERMOIL_BENCH_APPS_ONLY=termoil` runs one terminal, `TERMOIL_BENCH_RENDERER` measures another renderer and
`TERMOIL_BENCH_SCROLLBACK_BYTES` another history budget. Rows go to `_agent_output/bench/apps.jsonl`; every stream row
carries the CPU seconds of each named thread of the terminal's process, which is how the changes below were found.

### Which Ghostty

Ghostty has no Windows release. The comparison uses the native Windows port `shiweis/ghostty-windows` 1.3.0 (September
2026), a fork of ghostty-org/ghostty with a Win32 application runtime and OpenGL rendering, so both terminals run on the
same OS, display, ConPTY and shell. The same Ghostty core runs on macOS and Linux. Two other ways were tried and dropped:

- Ghostty 1.4's GTK AppImage in WSLg. It cannot create an OpenGL context there (`failed to create EGL context
  err=error.BadMatch` with every `GDK_BACKEND` and `LIBGL_ALWAYS_SOFTWARE` setting tried), and installing GTK
  development packages needs a password the agent does not have.
- Published figures. The ones found are secondary and give no method: Ghostty idle at 28 MB to under 60 MB on an M3
  MacBook, and 60 to 100 MB on Linux, in reviews on dev.to and dasroot.net. They agree with the 55 MB private working set
  measured here, and are not used.

## What was measured, in order

Each step changed one thing and was measured with `apps` on Termoil alone before it was kept. CPU is stream CPU per 100 MB
of the terminal's own process; the starting point was 91.5 (three runs) and 81 in the single runs below.

### 1. Read coalescing in the pty reader: CPU 81 to 38. Kept.

ConPTY delivered the 16 MiB as 228,583 reads averaging 77 bytes. Each read cost a `Vec` allocation, a channel send, a
parser thread wake and a UI wake (67,162 UI wakes in 13 s). Applying the bytes took 0.42 s of the parser thread's 3.5 s;
the rest was handing chunks between threads. The reader now waits `read_coalesce` (1 ms) before its next read when a
read came within `stream_gap` (3 ms) of the one before, so the program's small writes collect in the pipe. A read after a
quiet period is sent at once, so typing is not delayed.

| `read_coalesce` | reads | UI wakes | CPU per 100 MB |
|---|---|---|---|
| 0 (before) | 222,013 | 58,400 | 81.0 |
| 1 ms | 7,409 | 5,847 | 37.7 |
| 2 ms | 4,551 | 3,993 | 36.0 |

1 ms was kept: 2 ms saves little more and waits longer. The pseudoconsole host's CPU fell too, from 36 to 26 per 100 MB.

### 2. No accessibility capture until a client asks. Kept.

The window built the screen's text as a `String` per cell on every frame for the accessibility adapter, which sends
nothing until a screen reader asks for the tree. The capture now starts with the first `InitialTreeRequested`. It was 0.5
of 0.9 ms after each paint; the change is inside the run to run spread of the total.

### 3. A frame rate for streaming output: CPU 38 to 25 (GPU renderer). Kept as `window.output_fps`.

Measured by raising the frame interval while `type` ran: 164 Hz (the display) 38, 60 Hz 25.4, 30 Hz 20.0. At about 110 new
lines per frame on a 34 row window, every frame redraws every row, and frames above 60 per second show nothing anyone can
read. A cap cannot apply while someone types without adding up to a frame of latency, so it applies only when nothing was
typed, clicked or scrolled in the last `window.input_grace_ms` (250 ms), and the first frame after a quiet period is never
held back. `output_fps = 0` draws at the display rate as before.

### 4. The renderer: idle private memory 353 to 18 MB. Kept as `RasterRenderer`, the default.

A wgpu device with vello's pipelines commits 306 MB (Vulkan) to 335 MB (DX12) on this NVIDIA driver and 102 MB on OpenGL
(`termoil-bench gpu-init`), before a frame is drawn. With `renderer = "cpu"` (`vello_cpu`) the window idled at 89 MB; a
counting allocator showed 67 MB of that was one allocation: glifo's glyph atlas page, fixed at 4096 by 4096 pixels by
`DEFAULT_GLYPH_ATLAS_SIZE` in vello_cpu 0.3 and not configurable. Without the atlas, `vello_cpu` was 11 MB but 5.0 ms per
frame at 120 by 34 instead of 2.75.

`RasterRenderer` (`crates/termoil-render/src/raster/`) draws through the same `Canvas` calls as both vello renderers, so the
frame builder and the drawing order are shared:

- Each glyph is rasterized once by vello_cpu into a small scratch pixmap and kept as a coverage mask cropped to its ink (or
  as premultiplied pixels for a colour face), keyed by face, glyph, size, style, hinting and quarter pixel offset. About a
  kilobyte per glyph; `DEFAULT_GLYPH_CAPACITY` (4096) glyphs before the cache starts again.
- Rectangles are filled directly, with coverage on fractional edges; paths (box drawing, curly underlines, rounded tabs)
  through a signed area accumulation rasterizer of about 60 lines.
- It remembers each layer's rows, cursor and geometry, redraws only rows that differ, clipped to their band, and when the
  rows moved up k lines it moves the pixels and draws the k new rows.
- It draws straight into the window's softbuffer, so the pixels are not held twice, and presents only the changed bands.

| Renderer, 120 by 34, a full scroll every frame | private after 300 frames | frame p50 | frame p99 |
|---|---|---|---|
| `vello_cpu` with the glyph atlas | 80.4 MB | 2.75 ms | 8.84 ms |
| `vello_cpu` without the atlas | 11.3 MB | 5.01 ms | 8.81 ms |
| `RasterRenderer`, first version | 9.5 MB | 5.21 ms | 7.75 ms |
| `RasterRenderer` with scroll moves, integer rectangles and a cheaper glyph cache hash | 9.2 MB | 0.72 ms | 1.09 ms |

(`termoil-bench cpu-mem`, frame time includes copying the state and building the frame.)

`window.renderer = "auto"` now draws with `RasterRenderer` and never starts a GPU device unless a custom shader is set,
which only the GPU runs. `gpu` starts on the CPU and switches to `vello_gpu` when the device is ready, as `auto` did
before. `cpu` is `RasterRenderer` alone. `vello_cpu` stays as the pixel tests' reference and in `termoil-bench`.

With the renderer and the frame rate, idle private memory was 18.2 MB and stream CPU 22.1.

### 5. Drawing into the window's buffer, and the flags font: idle private 18.2 to 13.4 MB. Kept.

The renderer had a framebuffer of its own (3.5 MB at this size) that was copied into softbuffer's each frame; it now draws
into softbuffer's buffer, which holds the last frame (Windows' softbuffer buffer reports age 1), and redraws everything
when it does not. The warm start thread drew its ASCII strip into a buffer of the window's size and kept it; it now
draws into one of the strip's size and frees it. The bundled flags font (886 KB) was copied into the heap at start; it is
now read where it lies in the executable.

### 6. Holding wakes while a frame is scheduled: CPU 21.5 to 17.7. Kept.

After step 1 the UI still handled about 440 wakes a second while drawing 60 frames. When a window already has a frame
scheduled, a wake from one of its panes now leaves the pane's wake flag set, so the session sends no more, until that frame
is drawn; the pane's events are applied then.

### 7. A 1 MiB output pipe, so the pause in step 1 does not limit throughput (task-2485)

Step 1 left the ConPTY output pipe at the system default buffer, about 4 KiB. While the reader pauses for
`read_coalesce`, the pseudoconsole can put only one pipe's worth of output in it, so a program writing large escape
sequences was held to about 4 KiB per millisecond, roughly 4 MB/s. The Termoil and Ghostty video found it: a Mandelbrot
zoom sent as 800 by 520 kitty graphics frames at 30 a second (about 15 MB/s of base64) showed 77 frames in Termoil and 148
in Ghostty over the same seconds, and Termoil's process used 1.7 s of CPU in 6 s, so it was waiting, not working.

`ConptyConfig.output_pipe_bytes` (default 1 MiB) is now the pipe's buffer. With it both terminals show all 205 frames.
`crates/termoil-pty/tests/w4_output_pipe.rs` streams 4 MB through `type` with a reader that pauses 1 ms after each read:
the largest read was 9,975 bytes with the old pipe and the run took 5.2 s, against more than 16 KiB and 3.1 s now.

The `apps` stream run is unchanged by it, because `type` writes small chunks: three runs each in the same session gave
16.6 and 17.4 s of CPU per 100 MB for this build and 17.4 for 1.3.0 (the 15.5 in the table above was a quieter machine),
and the same memory to within 0.1 MB.

### Tried and not kept

- Compacting the process heap (`HeapCompact`) after the idle scrollback compression: no change in any number. The memory
  after a stream is the history it holds, as the 10 MB row above shows.
- More threads for `vello_cpu`: 0 and 4 extra threads drew a 120 by 34 frame in the same time.

## The gates on the new default

| Gate | Requirement | `RasterRenderer` | Before (`vello_gpu`) |
|---|---|---|---|
| P2 latency, 800 keys, three runs | p50 under 8 ms, p99 under 16 ms | p50 1.74 to 1.84 ms, p99 4.16 to 4.63 ms | p50 1.77 to 1.93 ms, p99 6.39 to 7.55 ms |
| P4 start up, 20 warm runs, `cmd.exe` | median under 100 ms | median 79.3 ms (67.0 to 108.3, one run at 632 while other agents were building) | median 82.4 ms |
| R4 frame time, 200 by 60, a full scroll every frame | p99 under 8 ms | p50 3.06 ms, p99 4.67 ms | p50 3.30 ms, p99 6.40 ms (glyph atlas) |

R4 on `RasterRenderer` at 200 by 60 is mostly moving 10 MB of pixels up one line per frame. The latency benchmark used to
wait for the GPU renderer before recording; it now waits only while a switch to the GPU is pending, and its rows name the
renderer.

## What this costs

- Grayscale antialiasing only, as before (`docs/decisions/renderer.md`, Text). Glyph shapes are vello_cpu's.
- A custom shader (`window.shader`) still needs the GPU, so a window with one starts the GPU device and costs what it did.
- Translucent windows (`window.opacity` below 1) still draw on the GPU through DirectComposition.
- Kitty graphics images are drawn with the nearest pixel when scaled; vello filtered them. Most placements are drawn at
  their own size.
- `RasterRenderer` matches `vello_cpu` on the R2 scenes under the tolerance the GPU renderer is held to (`r3_raster_matches_cpu`),
  not under the CPU golden tolerance, so the R2 goldens are still rendered by `vello_cpu`.

## Evidence

`_agent_output/task-2481/` in the task-2481 worktree, moved to `_agent_output/retired-worktrees/task-2481/` when it is
retired: `apps-baseline.txt` and `apps-final.txt` (the three run comparisons, every row), `p2-raster-*.json`,
`p4c-*.json`.
