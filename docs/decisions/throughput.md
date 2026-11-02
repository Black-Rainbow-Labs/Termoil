# Decision: the P1 throughput gate file and number

Written 2026-11-01 during task-2459, answering the design question task-2458 filed from milestone M1. Changes `docs/TEST-SUITE.md` section 13 (P1), the throughput row in `docs/TDD.md` section 2, and the throughput row and functional requirement 3 in `docs/PRD.md`.

## The question

`docs/TEST-SUITE.md` section 13 said: "Gate: the 1 GB file through `terminus-vt` alone at 1 GB/s or more on one core." Nothing said what the 1 GB mixed file contains or whether "`terminus-vt` alone" means the byte parser or the parser plus the screen. task-2458 built the benchmark (`tools/bench`), measured 196 MB/s on an escape dense build log stream, and asked which stream is the gate, which layer is measured, and whether 1 GB/s is the right number.

## Where 1 GB/s came from

`docs/research/architectures.md` says "Use Ghostty 1.4's ASCII parse rate of 725 MB/s as the first throughput target." The PRD rounded that up to 1 GB/s and attached it to "a 1 GB file of mixed text and escapes". The two halves of that sentence came from different places, and the number was never measured against a mixed stream.

What Ghostty's 725 MB/s measured (sources in `_agent_output/task-2459-throughput/ghostty.md`, kept with the retired worktree evidence, and quoted here):

- Ghostty 1.4.0 release notes: "In parser benchmarks, plain ASCII text went from 128 MB/s to 725 MB/s (5.7x)". <https://ghostty.org/docs/install/release-notes/1-4-0>
- The number is the "ascii (no newlines)" row of pull request 13220, a generated 100 MB corpus of plain ASCII with no newlines, run through `ghostty-bench +terminal-stream`, which applies the stream to a real `Terminal` at 120 by 80, timed with hyperfine on an Apple M4 Max. <https://github.com/ghostty-org/ghostty/pull/13220>
- The same pull request's other rows: "csi mix" 1.6x faster, "sgr fire" 1.6x, "TUI redraw" 2.2x. Pull request 13226 reports 276 to 342 MB/s on a 2.6 GB real asciinema recording on the same machine, and 1186 MB/s on ASCII only. <https://github.com/ghostty-org/ghostty/pull/13226>

So the published 725 MB/s is parser plus screen on plain ASCII with no newlines, on a faster core than this machine's, and Ghostty's own real world rate on the same core is 276 to 342 MB/s.

## What was measured

Every number below is from this machine (Intel Core Ultra 9 285, Windows 11 Pro 10.0.26100, rustc 1.95.0, release profile with fat LTO, one thread, 64 KiB slices), on 2026-11-01, on the identical bytes: the `mixed` stream from `tools/bench/src/streams.rs` at commit 711d65a of task-2458 (plus its uncommitted edits to `page.rs` and `csi.rs` at the time of the run), repeated to 1 GiB. Other agents were building on the machine during the run; medians of three samples are reported, and the rows where samples spread more than 30 % are named in `_agent_output/task-2459-throughput/comparison.md`.

The `mixed` stream is a build log: words, an SGR colour on about one word in eight, a UTF-8 word in twenty, one line in sixteen a progress bar redrawn with CR, with a fixed seed. Its composition:

| Property | Value |
|---|---|
| Average line length | 94.8 bytes |
| Escape sequences per KB | 29.2 |
| UTF-8 multibyte sequences per KB | 13.3 |
| Bytes inside escape sequences | 16.4 % |
| Newlines per MB | 10,553 |

Throughput in MB/s (bytes / seconds / 1e6). "Full" means parser plus screen at 200 columns by 60 rows. terminus-vt's default scrollback budget is 50 MB; alacritty_terminal's history is in lines, and 100,000 lines of this stream is about 9.5 MB. Ghostty ran inside WSL2 Ubuntu on the same machine (zig 0.17.0, tag v1.4.0, `-Doptimize=ReleaseFast`, `ghostty-bench +terminal-stream --terminal-cols=200 --terminal-rows=60`), with its default 10 MB scrollback and process start included in the time.

| Stream | vte 0.15 parser only | alacritty_terminal 0.26 full, 100k lines | Ghostty 1.4 full | terminus-vt parser only | terminus-vt full, 50 MB scrollback | terminus-vt full, 2 MB scrollback |
|---|---|---|---|---|---|---|
| mixed (1 GiB) | 502 | 75 | 202 | 866 | 174 | 168 |
| mixed_plain (256 MiB) | 2303 | 41 | 633 | 2540 | 275 | 519 |
| scrolling | 2044 | 80 | | 4557 | 428 | 855 |
| dense_cells | 345 | 216 | | 547 | 160 | 164 |
| unicode | 246 | 87 | | 1767 | 85 | 86 |
| cursor_motion | 293 | 184 | | 465 | 201 | 204 |
| erase_redraw | 1589 | 161 | | 4874 | 1127 | 1127 |

Parser only columns are not like for like: vte calls `print` once per character (1.19 bytes per action on `mixed`), terminus-vt emits one action per printable run (12.0 bytes per action). Ghostty's `+terminal-parser` action steps the state machine one byte at a time and measured about 60 MB/s, so it is left out.

Ceilings on how fast bytes can arrive at all, which bound how much any of this matters:

| Path | Rate | Source |
|---|---|---|
| ConPTY, bundled 1.25, 64 KiB reads | 28.3 MiB/s | `docs/decisions/conpty.md` |
| ConPTY, inbox | 3.8 MiB/s | `docs/decisions/conpty.md` |
| macOS pty into the Ghostty app, after the reader thread split | 114 to 123 MB/s | Ghostty pull request 13209 |

## Decision

**1. The gate file is the escape dense build log stream, `mixed`.** It is what a terminal running builds and agents receives, it exercises the parser, the SGR apply, UTF-8 and the CR redraw path together, and it is the stream where Terminus is slowest. A catted log file is the `mixed_plain` row, which is memory bound on the cold scrollback pages and says little about the parser. vtebench's scenarios stay in P1 as published numbers but are not the gate, because each isolates one path.

The file is not checked in. It is generated by `tools/bench` from `streams::mixed` with the fixed seed, as one 64 MiB block repeated sixteen times to 1 GiB. A test pins the block's length and a checksum, so the gate stream cannot drift without the test saying so, and the composition table above is recorded next to the gate in `docs/performance.md` with each release.

**2. The gate is on the parser plus the screen, not the parser alone.** `Terminal::new(200, 60, TerminalConfig::default())` fed in 64 KiB slices on one core, with the default 50 MB scrollback budget. That is what Ghostty's number measures and what a person waits on. The parser alone is published beside it as a report only line, because no real pipe delivers bytes faster than the screen can take them, and a parser gate would reward emitting fewer actions rather than finishing sooner.

**3. The number is 250 MB/s, not 1 GB/s.** No terminal measured reaches 1 GB/s on an escape dense stream with a screen: Ghostty 1.4 does 202 MB/s on this file on this machine, alacritty_terminal 75, Terminus 174 today. 1 GB/s was a plain ASCII, no newline, M4 Max figure and is the wrong target for this stream. 250 MB/s is chosen because:

- It is above Ghostty 1.4 on the same file and machine (202) with room for run to run noise, and below Ghostty's own real world rate on an M4 Max (276 to 342), so a core at that rate is known to exist.
- It is twice the fastest pipe measured anywhere (123 MB/s on macOS) and nine times ConPTY, so the reader thread never waits on the core and the display requirement (60 fps while the pty saturates) is carried by the renderer and not by the parser.
- Terminus needs 1.44x from today's 174. The `mixed_plain` row shows 275 MB/s at 50 MB scrollback and 519 at 2 MB, so the cold page writes are one lever; the ticket's own split (408 MB/s without SGR, 196 with) shows the SGR apply is the other.

The gate line in `docs/TEST-SUITE.md` section 13 now reads: the `mixed` 1 GiB stream through `Terminal` at 200 by 60 with the default scrollback budget at 250 MB/s or more on one core on this machine; the parser alone on the same stream published, not gated. The published run also records alacritty_terminal and Ghostty 1.4 on the same file, so the comparison is repeated with every release and the number is never read without its context.

**4. Two rows are below alacritty_terminal and are defects for task-2458, not for the gate.**

- `dense_cells` (an SGR per cell): Terminus 160 MB/s against alacritty_terminal 216. The parser is 547 on the same stream, so the cost is in applying a colour change per cell.
- `unicode`: Terminus parses it at 1767 MB/s and applies it at 85, the same as alacritty_terminal's 87. The wide and combining character apply path is twenty times slower than the parser that feeds it.

## What changed in the other documents

- `docs/TEST-SUITE.md` section 13, P1: names the file, the layer, the size, the scrollback budget and the number, and adds the comparison line.
- `docs/TDD.md` section 2, throughput row: "Parser plus screen at 250 MB/s or more per core on the mixed stream".
- `docs/PRD.md` section 2, throughput row, and functional requirement 3: the same.

## Evidence

- `_agent_output/task-2459-throughput/comparison.md` and `comparison.jsonl`: the Windows runs, machine, crate versions, commands, and the noisy samples named.
- `_agent_output/task-2459-throughput/ghostty.md` and `ghostty-runs.txt`: the Ghostty source quotes with URLs, the WSL build and run commands, and the raw runs.
- `_agent_output/task-2459-throughput/harness/`: the Rust harness that drove vte, alacritty_terminal and terminus-vt on the same bytes. The follow up ticket adds it to `tools/bench` so the comparison runs with each release.
