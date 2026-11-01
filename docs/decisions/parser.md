# Decision: the byte parser is Terminus's own, not the vte crate

Written 2026-11-01 during task-2458 (milestone M1). Changes `docs/TDD.md` section 4.3, which said the `vte` 0.15 crate provides the byte level state machine.

## What was found

Reading `vte` 0.15.0's source before building on it:

- **APC payloads are thrown away.** `ESC _` enters `SosPmApcString` and every byte until ST is ignored; `Perform` has no APC callback. The kitty graphics protocol travels in APC, so with `vte` Terminus could not receive a single kitty image.
- **OSC has no size bound with the `std` feature.** `osc_raw` is a `Vec` that grows with the payload; `MAX_OSC_RAW` applies only without `std`. The TDD requires 8 MiB and drop.
- **Text is dispatched one `char` at a time.** `ground_dispatch` calls `print(char)` per code point, so a run of ASCII cannot be written to the grid as one slice.
- **8 bit C1 controls** are passed to `execute` as a byte, so `0x9B` never starts a CSI.
- **DCS is streamed** byte by byte through `put`, with no limit.

## Decision

`terminus-vt::parser` is a state machine written from the vt100.net description (<https://vt100.net/emu/dec_ansi_parser>), with:

- a ground state front end that finds the end of a printable run with an SSE2 scan (`scan.rs`) and validates UTF-8 for the run with `simdutf8`, then emits one `Action::Print(&str)` for the whole run;
- APC, OSC and DCS payloads collected and dispatched whole, with limits of 8 MiB (OSC) and 65 MiB (DCS, APC); a payload over its limit is dropped and counted in `ParserStats`;
- 16 parameters per sequence and 5 sub parameters per parameter (one more than the TDD's 4, because `38:2:<colour space>:r:g:b` needs 5), each clamped at 65535;
- 8 bit C1 controls acted on as their ESC forms when the byte is not part of a valid UTF-8 sequence; inside a valid sequence, 0x80 to 0x9F are text;
- ST recognised as `ESC \` and as a free standing 0x9C; inside a UTF-8 sequence in an OSC payload, 0x9C is a continuation byte;
- `ESC ESC` inside DCS kept as one literal ESC, which is how tmux wraps sequences for the outer terminal.

The scalar path (a plain byte loop and `core::str::from_utf8`) runs every U1 case and the property tests alongside the SIMD path and must produce the same actions.

## Evidence

- U1: `crates/terminus-vt/tests/parser.rs`, 17 table groups, each case run whole, one byte at a time and in seeded random chunks, through both paths.
- Property `simd_and_scalar_agree` in `crates/terminus-vt/tests/properties.rs` over random bytes.
- Throughput: see `docs/performance.md` (P1).

`vte` is no longer a dependency. Nothing else in the TDD changes.
