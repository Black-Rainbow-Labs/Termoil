# ConPTY: graphics passthrough, flags and throughput (spike S2)

Measured on 2026-11-01 on this machine by the tests in `crates/terminus-pty/tests/` (`s2_graphics_passthrough.rs`,
`e2_size.rs`, `w3_conpty_pair.rs`). The S2 tests assert the behaviour below and fail if a Windows update or a new
bundled pair changes it.

## What was tested

| Item | Value |
|---|---|
| Windows | Windows 11 24H2, `cmd /c ver` reports `Microsoft Windows [Version 10.0.26100.6584]` |
| Inbox ConPTY | `CreatePseudoConsole` exported by `kernel32.dll`, hosted by the inbox `conhost.exe` of that build |
| Bundled ConPTY | NuGet package `Microsoft.Windows.Console.ConPTY` 1.25.260930003 (file version 1.25.2609.30003), x64 `conpty.dll` and `OpenConsole.exe`, fetched by `tools/fetch-conpty.ps1` into `installer/windows/conpty/` |
| Version written in one place | `installer/windows/conpty.version` contains `1.25.260930003`; test W3 checks both files against it and against each other |
| Latest versions on nuget.org that day | stable `1.24.261001001` and `1.25.260930003`; preview `1.26.260930004-preview`. The higher stable build, 1.25.260930003, was chosen |

The bundled `conpty.dll` exports `ConptyCreatePseudoConsole`, `ConptyResizePseudoConsole` and `ConptyClosePseudoConsole`
(also `ConptyClearPseudoConsole`, `ConptyShowHidePseudoConsole`, `ConptyReparentPseudoConsole`, `ConptyReleasePseudoConsole`,
`ConptyPackPseudoConsole`, `ConptyCreatePseudoConsoleAsUser`), not the unprefixed kernel32 names. `terminus-pty` looks up the
prefixed names first and the unprefixed ones second.

## Sequences sent and what arrived at the terminal side

The child is `examples/helper.rs raw <file>`, which writes the bytes of a file to its console output. Each sequence was sent
alone on a line, wrapped in a text label. Console output mode was tried with and without `ENABLE_VIRTUAL_TERMINAL_PROCESSING`
set by the child; the result was identical. The observed bytes (escape written as `ESC`) are in
`_agent_output/task-2458-conpty/s2-inbox.txt` and `s2-bundled.txt`.

| Sequence sent | Inbox ConPTY (kernel32, build 26100.6584) | Bundled ConPTY 1.25.260930003 |
|---|---|---|
| APC `ESC _ G a=q,i=1;AAAA ESC \` (kitty graphics query) | Removed completely. Nothing between the label brackets. | Delivered byte for byte |
| DCS `ESC P q #0;2;0;0;0#0~~ ESC \` (sixel) | Removed completely | Delivered byte for byte |
| OSC `ESC ] 133;A ESC \` | Delivered unchanged, but preceded by `ESC ] 0;<exe path> BEL` (a window title ConPTY writes itself) | Delivered byte for byte |
| OSC 8 `ESC ] 8;;http://example.com ESC \` | Delivered with an id parameter added: `ESC ] 8;id=39188-1;http://example.com ESC \` (the id is `<pid>-<counter>`) | Delivered byte for byte, no id added |

A kitty graphics transfer of 256 APC chunks of 4096 base64 bytes each (1 MiB, first chunk `a=T,f=24,s=512,v=512,m=1`, the rest
`m=1` and a final `m=0`) arrived through the bundled ConPTY with all 256 `ESC _ G` introducers and every payload byte present.
The inbox ConPTY removes all of it.

## Flags

`CreatePseudoConsole` flags 0x2, 0x4, 0x8, 0x10 and 0x20 were each tried on both implementations (plus no flags and an explicit 0).
None changed which sequences arrived.

- No passthrough flag exists in the shipped header. `inc/conpty.h` in the 1.25 package defines `PSEUDOCONSOLE_INHERIT_CURSOR` 0x1,
  `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` 0x08, `PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH` 0x10, `PSEUDOCONSOLE_GLYPH_WIDTH_CONSOLE` 0x18
  (the three width values share the mask 0x18) and `PSEUDOCONSOLE_AMBIGUOUS_IS_WIDE` 0x20. The third party claim that 0x8 is
  `PSEUDOCONSOLE_PASSTHROUGH_MODE` does not match this header: 0x8 is the grapheme width mode here, and it did nothing for APC or DCS
  on the inbox ConPTY.
- The bundled ConPTY needs no flag to pass APC, DCS, OSC 133 and OSC 8 through.
- `terminus-pty` requests 0x08 (grapheme width) on the bundled ConPTY only. The inbox ConPTY does not get a flag. It does not request
  `INHERIT_CURSOR`, `RESIZE_QUIRK` or `WIN32_INPUT_MODE`.

## Start up sequences each ConPTY sends to the terminal

The terminal core has to accept these before the child prints anything.

- Inbox: `ESC [?9001h ESC [?1004h`, then `ESC [?25l ESC [2J ESC [m ESC [H` when the first output arrives.
- Bundled: `ESC [1t ESC [c ESC [?1004h ESC [?9001h`. `ESC [c` is a Primary Device Attributes request. Nothing answered it in these
  tests and the child ran normally, but the terminal should reply.
- `?9001h` asks the terminal to send keys as win32 input mode records. `?1004h` asks for focus events. A terminal that ignores `?9001h`
  keeps sending ordinary VT input and ConPTY accepts it.

## Decision for kitty graphics on Windows

Works with the bundled ConPTY (1.25.260930003 and later builds that keep this behaviour). It does not work with the inbox ConPTY on
Windows 11 build 10.0.26100.6584, which removes APC and DCS before the terminal sees them. The release must therefore ship the
`conpty.dll` and `OpenConsole.exe` pair beside the executable, and graphics are unavailable when the loader falls back to kernel32
(`Pty::conpty_source()` returns `Inbox`). Sixel (DCS) follows the same rule as APC.

Not covered by S2: replies from the terminal back to the child (for example the kitty `a=q` response) travel on the input pipe, which this
spike did not exercise. That is covered when the terminal core exists.

## Output throughput (TDD open question 3)

One thread reading the output pipe in 64 KiB reads, with a child (`helper numbers 5000000`) writing 5,000,000 lines of `LINE 0000001`
(13 bytes each, 62.0 MiB) in 64 KiB writes, console size 120 by 40, test profile of this repository, other test binaries running at the
same time:

| ConPTY | Bytes received | Time to receive the last line | Rate |
|---|---|---|---|
| Inbox | 66.8 MiB | 17.64 s | 3.8 MiB/s received, 3.5 MiB/s of child output |
| Bundled 1.25.260930003 | 66.8 MiB | 2.36 s | 28.3 MiB/s received, 26.3 MiB/s of child output |

The bundled ConPTY is 645% faster than the inbox one on this workload. All 5,000,000 numbered lines arrived complete and in order through both
(escape sequences removed before counting). On Windows the rate at which text can arrive is set by ConPTY and the pipe, at 28.3 MiB/s with
the bundled pair and 3.8 MiB/s with the inbox one.

## Behaviour notes for the implementation

- A child created without `STARTF_USESTDHANDLES` and with invalid standard handles inherits the parent's redirected stdio instead of the
  pseudoconsole. `terminus-pty` sets all three standard handles to `INVALID_HANDLE_VALUE` and the flag.
- The child is put in a kill on close job object through `PROC_THREAD_ATTRIBUTE_JOB_LIST`, so `kill()` and dropping the `Pty` end a
  grandchild too (tested with a ping started by the child).
- `ClosePseudoConsole` is called on its own thread, because it can block until the output pipe is drained.
