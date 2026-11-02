# Terminus performance

The gates from `docs/TEST-SUITE.md` section 13, measured on 2026-11-02 on the build that became 1.0 (main after the M5
merges), on this machine. Every gate passes.

## Configuration

| Item | Value |
|---|---|
| CPU | Intel Core Ultra 9 285: 8 performance cores and 16 efficiency cores, 24 logical processors |
| GPU | NVIDIA GeForce RTX 5090 (wgpu chose Vulkan); Intel integrated GPU for the integrated R4 row |
| OS and display | Windows 11 24H2 build 26100.6584, one 164 Hz display |
| Build | release profile (`opt-level = 3`, thin LTO, `debug = "line-tables-only"`), Rust 1.95.0, target x86_64-pc-windows-msvc |
| Load | 17 to 29 % of the CPU in use by other programs (a screen recorder and a test harness); nothing else building |
| Benchmarks | `tools/bench` (`terminus-bench throughput`, `render`, `alloc`, `memory`), `terminus-gui --bench-latency` (P2), `TERMINUS_STARTUP_PROBE` (P4) |
| Pinning | P1 pinned to performance core 12 at high priority, and repeated unpinned; the other benchmarks unpinned at high priority |
| Terminal | 200 by 60 cells, default scrollback (50 MB), default configuration; P2 and P4 with the default font (Cascadia Code, 13 points) |

The raw output of this run is in `_agent_output/task-2458-m5/` in the task-2458 worktree.

## Gates

| Gate | Requirement | Measured | Result | Where |
|---|---|---|---|---|
| P1 throughput | the `mixed` 1 GiB stream through `Terminal` (parser plus screen) at 250 MB/s or more on one core | 270.2 MB/s on performance core 12, 262.5 MB/s unpinned (an efficiency core gives 204 to 239) | passes | `docs/decisions/throughput.md` |
| P2 latency | key to the frame that shows it: p50 under 8 ms, p99 under 16 ms | `vello_gpu` on Vulkan, three runs of 800 keys: p50 1.81 to 1.84 ms, p99 4.74 to 7.12 ms | passes | `docs/decisions/renderer.md` |
| P3 memory | 100,000 lines under 150 MB, 1,000,000 lines under 400 MB, 200 columns, after idle compression | 100,000 lines: 18.0 MB typical, 42.0 MB dense attributes; 1,000,000 lines: 142.5 MB typical, 376.7 MB dense; ten idle tabs 21.9 MB (private bytes of the terminal core) | passes | `terminus-bench memory` |
| P4 start up | under 100 ms warm on Windows, to the first frame with the prompt presented to a visible window | median 82.4 ms over 20 warm runs (74.5 to 265.6); earlier batches 76.3 to 78.6; the window is active at a median 90.2 ms | passes | `docs/decisions/startup.md` |
| P5 allocation | no allocation per byte in the steady state | 58 allocations over 1021 MB of steady state output, at page boundaries | passes | `terminus-bench alloc` |
| R4 frame time | p99 under 8 ms on a discrete GPU, under 16 ms on an integrated one | `vello_gpu` with the glyph atlas on the RTX 5090: p50 2.98 ms, p99 6.87 ms; the integrated GPU 10.0 to 10.4 ms (2026-11-01) | passes | `docs/decisions/renderer.md` |

Other rows from the same runs, for comparison and not gated:

| Row | Value |
|---|---|
| `dense_cells` (a colour change per cell) | 259.4 MB/s on core 12; alacritty_terminal measured 216 |
| `unicode` (wide and combining characters) | 204.8 MB/s on core 12; alacritty_terminal measured 87 |
| R4 without the glyph atlas | p99 17.57 ms; the atlas is the configuration Terminus ships |
| R4 on `vello_cpu` (the start up and fallback renderer) | p99 14.64 to 15.76 ms |
| P2 on OpenGL (the fallback backend) | p50 3.83 ms, p99 31.71 ms (200 keys) |

## Open

- The P2 camera check: once per release, the latency confirmed with a camera at 240 frames per second against Windows
  Terminal and Alacritty on the same display. The same session films a launch for P4. This needs a person with a camera
  and has not been done.
- R4 on a laptop's integrated GPU: measured here on a desktop CPU's integrated GPU only.
- P3 measures the terminal core in its own process. The window's own memory (fonts, the GPU device and its buffers) is not
  gated and has not been measured.
- The P1 comparison: alacritty_terminal and Ghostty were measured without a recorded core; task-2461 repeats them pinned to
  a performance core.
