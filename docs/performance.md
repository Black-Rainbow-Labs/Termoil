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
| `dense_cells` (a colour change per cell) | 259.4 MB/s on core 12; alacritty_terminal 302 in the P1 comparison below |
| `unicode` (wide and combining characters) | 204.8 MB/s on core 12; alacritty_terminal 109 in the P1 comparison below |
| R4 without the glyph atlas | p99 17.57 ms; the atlas is the configuration Terminus ships |
| R4 on `vello_cpu` (the start up and fallback renderer) | p99 14.64 to 15.76 ms |
| P2 on OpenGL (the fallback backend) | p50 3.83 ms, p99 31.71 ms (200 keys) |

## P1 comparison

`docs/TEST-SUITE.md` section 13 asks each published P1 run to record alacritty_terminal and Ghostty 1.4 on the same
bytes. Measured on 2026-11-02 in one session, terminus-vt at commit `f2ff2a4` (the last commit to touch
`crates/terminus-vt`, inside the 1.0.0 release `855b681`), Rust 1.95.0, release profile.

The gate stream is `streams::mixed(64 << 20)` repeated sixteen times: 1,073,743,136 bytes. The block is 67,108,946 bytes
with FNV-1a 64 `0xbcf285359a5a6081`, 94.8 bytes per line, 29.2 escape sequences per KB, 13.3 UTF-8 multibyte sequences per
KB and 16.4 % of bytes inside escape sequences. The test `mixed_gate_stream_is_pinned` in `tools/bench/src/streams.rs`
fails if any of these change.

How each column was measured:

| Column | What runs |
|---|---|
| vte 0.15 parser only | `vte::Parser` with a `Perform` that counts calls |
| alacritty_terminal 0.26 full | `Term` at 200 by 60, `scrolling_history` 100,000 lines (about 9.5 MB of `mixed`), fed through `vte::ansi::Processor` |
| Ghostty 1.4 full | `ghostty-bench +terminal-stream --terminal-cols=200 --terminal-rows=60` on the gate file, Ghostty v1.4.0 built with zig 0.17.0 and `-Doptimize=ReleaseFast` in WSL2 on this machine, default 10 MB scrollback; the time is the whole process, start up included; not pinned (a WSL2 thread cannot be pinned to a host core) |
| terminus-vt parser only | `terminus_vt::parser::Parser` with a handler that counts actions |
| terminus-vt full | `Terminal::new(200, 60, TerminalConfig::default())`, the 50 MB default scrollback |

Every engine is fed 64 KiB slices on one thread. MB/s is bytes / seconds / 1e6, median of 3. `mixed` is fed 1 GiB, the
other streams 256 MiB. The Windows columns ran pinned to performance core 12 at high priority; other programs used 20 to 36
% of the CPU during the run. Parser only columns are not like for like: vte calls `print` once per character, terminus-vt
emits one action per printable run.

| Stream | vte 0.15 parser only | alacritty_terminal 0.26 full | Ghostty 1.4 full | terminus-vt parser only | terminus-vt full |
|---|---|---|---|---|---|
| `mixed` (the gate) | 631 | 82 | 234 | 946 | 267 |
| `scrolling` | 4653 | 110 | | 6006 | 851 |
| `dense_cells` | 507 | 302 | | 853 | 260 |
| `scrolling_in_region` | 4332 | 179 | | 6370 | 832 |
| `unicode` | 348 | 109 | | 5364 | 211 |
| `cursor_motion` | 402 | 241 | | 647 | 238 |
| `erase_redraw` | 2373 | 194 | | 5931 | 1514 |
| `mixed_plain` | 4637 | 75 | | 4064 | 712 |
| `mixed_sgr` | 729 | 81 | | 1237 | 325 |
| `mixed_unicode` | 2273 | 76 | | 2434 | 505 |
| `mixed_progress` | 4294 | 78 | | 4027 | 708 |

Ghostty's three runs were 246.4, 229.9 and 233.9 MB/s. The same comparison unpinned gave terminus-vt full 274 MB/s on
`mixed`, alacritty_terminal 81, vte 619, terminus-vt parser only 975. `terminus-bench throughput 3 mixed` on core 12 in the
same session gave 255.7 MB/s.

On `mixed` terminus-vt full is 14 % faster than Ghostty 1.4 and 226 % faster than alacritty_terminal. Two streams are
slower in terminus-vt full than in alacritty_terminal: `dense_cells` (260 against 302) and `cursor_motion` (238 against
241).

To repeat it:

```
pwsh tools/bench-ghostty.ps1                          # Ghostty: appends a row to _agent_output/bench/ghostty.jsonl
cargo run --release -p terminus-bench -- compare      # every engine; shows the newest Ghostty row beside mixed
```

`compare` writes one row per stream to `_agent_output/bench/p1.jsonl` with `"source": "compare"` and the numbers above
under `comparison`. `compare 3 all` adds the `micro_` streams, and `compare 3 <stream>` runs one.

## Open

- The P2 camera check: once per release, the latency confirmed with a camera at 240 frames per second against Windows
  Terminal and Alacritty on the same display. The same session films a launch for P4. This needs a person with a camera
  and has not been done.
- R4 on a laptop's integrated GPU: measured here on a desktop CPU's integrated GPU only.
- P3 measures the terminal core in its own process. The window's own memory (fonts, the GPU device and its buffers) is not
  gated and has not been measured.
- The P1 comparison runs Ghostty inside WSL2, where its thread cannot be pinned to a host core, and its time includes
  process start up. The Windows engines are pinned to core 12.
