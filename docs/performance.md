# Terminus performance

The gates from `docs/TEST-SUITE.md` section 13, as measured on this machine (Intel Core Ultra 9 285, NVIDIA GeForce RTX
5090, Windows 11 24H2 build 26100.6584, 164 Hz display) on release builds. Each benchmark writes JSON rows to
`_agent_output/bench/`. This file is updated before every release; the M5 report replaces the open items.

| Gate | Requirement | Measured | Result | Where |
|---|---|---|---|---|
| P1 throughput | the 1 GB mixed file through `terminus-vt` at 1 GB/s or more on one core | 176 to 196 MB/s on the mixed stream; plain ASCII 900 MB/s; erase and redraw 1.27 GB/s | **fails** | `docs/decisions/parser.md`; what the mixed file contains is task-2459 (Fable) |
| P2 latency | p50 under 8 ms, p99 under 16 ms | p50 1.5 to 1.9 ms, p99 5.6 to 10.0 ms (three runs, 2,300 keys) | passes | `docs/decisions/renderer.md` |
| P3 memory | 100 000 and 1 000 000 lines under 150 MB and 400 MB | not measured yet | M4 gate | |
| P4 start up | under 100 ms warm on Windows | frame with the prompt painted at a median 84 ms; window visible at a median 148 ms | open: task-2462 (Fable) asks which end of the interval counts | `docs/decisions/renderer.md` |
| P5 allocation | no allocation per byte in the steady state | 58 allocations over 1.07 GB of steady state output, all at page boundaries | passes | `tools/bench alloc` |
| R4 frame time | p99 under 8 ms (discrete), under 16 ms (integrated) | 5.25 to 6.98 ms on the RTX 5090; 10.0 to 10.4 ms on the Intel integrated GPU | passes | `docs/decisions/renderer.md` |

## Open

- P2 camera check: once per release, the latency confirmed with a camera at 240 frames per second against Windows Terminal
  and Alacritty on the same display. This needs a person with a camera and has not been done.
- R4 on a laptop's integrated GPU: measured here on a desktop CPU's integrated GPU only.
- P1: the parser alone runs the mixed stream at 655 MB/s; the gap to 1 GB/s is split between the parser and the screen
  apply path. Work continues after the gate's stream is defined.
