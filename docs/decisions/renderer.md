# Renderer: vello_gpu with the glyph atlas, CPU first at start up (M2, TDD question 1)

Measured on 2026-11-01 on this machine. Every number below is from `tools/bench` (R4, `terminus-bench render`),
`terminus-gui --bench-latency` (P2) and the start up probe in `terminus-gui` (P4), on release builds. The JSON rows are in
`_agent_output/bench/` (`r4.jsonl`, `p2-run*.json`, `p4-runs.jsonl`).

## Machine

| Item | Value |
|---|---|
| CPU | Intel Core Ultra 9 285 |
| Discrete GPU | NVIDIA GeForce RTX 5090, Vulkan backend |
| Integrated GPU | Intel Graphics (the CPU's), Vulkan backend |
| Display | 164 Hz |
| Windows | 11 24H2, build 26100.6584 |
| Font | Cascadia Code at 13 pt (17.33 px), cells of 10 by 21 px |
| Load | The R4 runs were on a quiet machine. The P4 runs had about 30 % of the CPU in use by other work (a screen recorder and a test harness), which moves start up times by tens of milliseconds |

## R4: frame time, 200 by 60 cells, a full screen scroll every frame

Each frame scrolls the whole screen by one line of mixed coloured text, so every row is rebuilt and every glyph drawn
(about 8,700 glyphs). 1,200 frames, three runs. Time is from the start of encoding to the GPU finishing the frame.

| Renderer | p50 | p90 | p99 | max |
|---|---|---|---|---|
| vello_gpu, RTX 5090, glyph atlas (run 1) | 3.07 ms | 3.80 ms | **6.98 ms** | 12.66 ms |
| vello_gpu, RTX 5090, glyph atlas (run 2) | 2.98 ms | 3.74 ms | **5.25 ms** | 9.45 ms |
| vello_gpu, RTX 5090, glyph atlas (run 3) | 3.37 ms | 4.21 ms | **6.90 ms** | 11.62 ms |
| vello_gpu, RTX 5090, no atlas | 12.3 to 15.2 ms | 15.1 to 22.8 ms | 19.5 to 35.3 ms | up to 44.6 ms |
| vello_gpu, Intel integrated, glyph atlas (two runs) | 6.4 to 7.1 ms | 7.5 to 8.4 ms | **10.0 to 10.4 ms** | 11.5 to 53.2 ms |
| vello_gpu, Intel integrated, no atlas | 15.9 to 21.2 ms | 20.0 to 25.5 ms | 30.7 to 44.4 ms | up to 68.8 ms |
| vello_cpu, glyph atlas, one thread | 8.4 to 10.8 ms | 10.7 to 12.1 ms | 13.2 to 14.1 ms | up to 19.2 ms |
| vello_cpu, glyph atlas, four extra threads | 8.1 to 9.1 ms | 10.2 to 11.5 ms | 12.5 to 13.9 ms | up to 17.7 ms |

Building the frame (the frame builder, outside the terminal lock) takes 0.24 to 0.36 ms of each frame. Encoding the vello
scene is most of the rest: 2.0 to 2.3 ms at p50 with the atlas.

**Gate: p99 under 8 ms on the RTX 5090 passes** (5.25 to 6.98 ms) with the glyph atlas on. Without the atlas it fails by
a wide margin, so the atlas is always on. The margin is about one millisecond at p99, and it is spent in scene encoding on
the CPU, not on the GPU.

**Gate: p99 under 16 ms on an integrated GPU passes** (10.0 to 10.4 ms). This was measured on the desktop CPU's integrated
GPU with `TERMINUS_GPU_POWER=low`, not on a laptop; a laptop part is slower and remains to be measured on one.

## P2: input latency

`terminus-gui --bench-latency <keys> <out.json>`: the pane runs `terminus --echo-child`, which prints a prompt line and
echoes every byte it reads, wrapping at 60 columns. Keys are injected on the UI thread 25 to 35 ms apart through the same
path a typed key takes (normaliser, bindings, encoder, PTY). Each sample is the time from injection to the end of the first
paint whose frame shows the echoed character. Presents are unpaced (immediate mode) for the benchmark.

| Run | Keys | p50 | p90 | p99 | max |
|---|---|---|---|---|---|
| 1 | 300 | 1.92 ms | 3.62 ms | 9.54 ms | 11.91 ms |
| 2 | 1,000 | 1.90 ms | 4.14 ms | 9.99 ms | 13.21 ms |
| 3 | 1,000 | 1.50 ms | 2.21 ms | 5.64 ms | 11.70 ms |

**Gate: p50 under 8 ms and p99 under 16 ms passes.** The camera check against Windows Terminal and Alacritty that TEST-SUITE
asks for once per release is not done by an agent; it is listed in `docs/performance.md` as open.

**Re-measured on 2026-11-01 after a defect was found.** From the change that moved the GPU start to a background thread until
this date, a window never switched to `vello_gpu`: the thread also created the window surface, and winit on Windows gives out
a window's handle only on the thread that owns the window, so surface creation failed with "the underlying handle is not
available" and every window, including the 0.1.1 release, drew with `vello_cpu`. The thread now starts the device and builds
vello's pipelines (the slow part), and the UI thread creates the surface and attaches it
(`VelloGpuRenderer::new_detached_in`, `attach_window`). The E6 window test now waits for the switch on a machine with a GPU.
P2 with the GPU renderer in use, the renderer named in each JSON row:

| Run | Renderer | Keys | p50 | p90 | p99 | max |
|---|---|---|---|---|---|---|
| 1 | `vello_gpu` on the RTX 5090 (Vulkan) | 800 | 1.77 ms | 2.51 ms | 6.39 ms | 118.48 ms |
| 2 | `vello_gpu` on the RTX 5090 (Vulkan) | 800 | 1.91 ms | 2.73 ms | 7.55 ms | 13.64 ms |
| 3 | `vello_gpu` on the RTX 5090 (Vulkan) | 800 | 1.93 ms | 2.71 ms | 7.54 ms | 52.53 ms |
| GL | `vello_gpu` on the RTX 5090 (OpenGL, `TERMINUS_GPU_BACKEND=gl`) | 200 | 3.83 ms | 6.22 ms | 31.71 ms | 576.35 ms |

The gate still passes on the primary backend. OpenGL is the fallback when no primary backend has an adapter
(`GpuContext::for_display`), and its p99 is over the gate on this machine; it is there so a machine without Vulkan, DX12 or
Metal still gets a GPU renderer, not for speed.

## P4: start up

`TERMINUS_STARTUP_PROBE=<file>` makes `terminus-gui` write the time from the start of `main` to the first frame that shows
the shell's output, with the start up phases. Twenty warm runs with `cmd.exe` as the shell:

| Phase (median of 20, ms from the start of `main`) | Time |
|---|---|
| Event loop built | 8 |
| Shell spawned (before the window, so it starts in parallel) | 23 |
| First shell output read | 38 |
| Window created (hidden) | 41 |
| Fonts loaded | 55 |
| First frame painted, already showing the prompt | **84** |
| Window shown (`ShowWindow` returns) | 146 |
| First visible frame with the prompt: min / median / max | 121 / **148** / 229 |

Starting a GPU device takes most of a second here, whichever backend:

| wgpu backend | Instance | Adapter | Device | vello pipelines | Total |
|---|---|---|---|---|---|
| Vulkan | 455 to 942 ms | 5 to 6 ms | 76 to 96 ms | 24 to 27 ms | 584 to 1,048 ms |
| DX12 | 12 to 13 ms | 402 to 455 ms | 84 to 117 ms | 329 to 785 ms | 828 to 1,369 ms |
| GL | 174 ms | 0.3 ms | 285 ms | 284 ms | 744 ms |

So the window starts on `vello_cpu` presented through `softbuffer` (ready in a few milliseconds) while a background thread
starts the GPU device and builds `vello_gpu` for the window; the window switches to it on the first frame after it is ready.
Every later window shares the same device, so only the first pays for it. Before this change the first frame came at
1.6 to 2.2 s.

**Gate: under 100 ms warm on Windows does not pass as written.** Terminus paints a frame that shows the prompt at about
84 ms; Windows then takes about 60 ms to show the window, and that time is spent inside `ShowWindow`. Creating the window
visible moves the same cost into window creation. Turning off the compositor's open animation made no measurable
difference and was not kept. Allowing IME is moved after the window is shown. P4 ends at the first frame presented to a
visible window, and the work that brought it under 100 ms is in `docs/decisions/startup.md`.

## Text

Grayscale antialiasing only. `glifo` 0.4 has no LCD subpixel path, and the TDD's question 1 asks whether one is reachable:
it is not without writing one. The pixel goldens (R2, R3) are grayscale. A reviewer comparison against Windows Terminal at
the same font is a visual judgement on the installed release build and is not done here.

## Decisions

1. `VelloGpuRenderer` with `glifo`'s glyph atlas is the window renderer. The atlas renderer contingency is **not built**:
   the gate passes.
2. `VelloCpuRenderer` draws the first frames of every window while the GPU starts, and stays in use when no GPU can draw to
   the window or `window.renderer = "cpu"`.
3. One GPU device for the process (`GpuContext`), on the primary backends only (Vulkan, DX12, Metal); GL is skipped, which
   saves about 170 ms of instance start up and is not needed by vello.
4. Present mode is mailbox when the surface offers it (lowest latency without tearing), FIFO otherwise, with a frame latency
   of one. The TDD said FIFO with a two frame swap chain; mailbox measured lower latency and the TDD is updated.
5. Rendering happens on the UI thread for now. The renderer thread in the TDD's data flow is not needed to meet R4 or P2:
   building a frame takes under 0.4 ms and the terminal lock is held only to copy the dirty rows.
