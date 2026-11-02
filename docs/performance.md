# Terminus performance

The gates from `docs/TEST-SUITE.md` section 13, as measured on this machine (Intel Core Ultra 9 285, NVIDIA GeForce RTX
5090, Windows 11 24H2 build 26100.6584, 164 Hz display) on release builds. Each benchmark writes JSON rows to
`_agent_output/bench/`. This file is updated before every release; the M5 report replaces the open items.

| Gate | Requirement | Measured | Result | Where |
|---|---|---|---|---|
| P1 throughput | the `mixed` 1 GiB stream through `Terminal` (parser plus screen, 200 by 60, default scrollback) at 250 MB/s on one core | 263.0 MB/s on a performance core, 255.7 MB/s left to the scheduler (219 to 245 on an efficiency core) | passes | `docs/decisions/throughput.md` |
| P2 latency | p50 under 8 ms, p99 under 16 ms | with `vello_gpu` on Vulkan: p50 1.77 to 1.93 ms, p99 6.39 to 7.55 ms (three runs, 2,400 keys, 2026-11-01); windows drew with `vello_cpu` until that date because of a defect, see `docs/decisions/renderer.md` | passes | `docs/decisions/renderer.md` |
| P3 memory | 100 000 lines under 150 MB, 1 000 000 lines under 400 MB, 200 columns, after the idle compression | 100 000 lines: 18.0 MB typical, 43.4 MB dense attributes; 1 000 000 lines: 144.3 MB typical, 376.3 MB dense; ten idle tabs: 21.9 MB (private bytes of the terminal core) | passes | `terminus-bench memory` |
| P4 start up | under 100 ms warm on Windows, to the first frame with the prompt presented to a visible window | median 76.3, 77.4, 78.4 and 78.6 ms in four batches of 20 warm runs (64.6 to 102.8); the window is active at a median 80 to 82 ms | passes | `docs/decisions/startup.md` |
| P5 allocation | no allocation per byte in the steady state | 58 allocations over 1.07 GB of steady state output, all at page boundaries | passes | `tools/bench alloc` |
| R4 frame time | p99 under 8 ms (discrete), under 16 ms (integrated) | 5.25 to 6.98 ms on the RTX 5090; 10.0 to 10.4 ms on the Intel integrated GPU | passes | `docs/decisions/renderer.md` |

## Open

- P2 camera check: once per release, the latency confirmed with a camera at 240 frames per second against Windows Terminal
  and Alacritty on the same display. This needs a person with a camera and has not been done.
- R4 on a laptop's integrated GPU: measured here on a desktop CPU's integrated GPU only.
- P3 measures the terminal core in its own process. The window's own memory (fonts, the GPU device and its buffers) is
  measured separately once per release with the installed build.
- P1 comparison: alacritty_terminal and Ghostty were measured without a recorded core; task-2461 repeats them pinned to a
  performance core.
