# Start up: P4 ends at the first frame a person can see (M3, task-2462)

`docs/TEST-SUITE.md` section 13 defines P4 as "process start to the first frame with the prompt". task-2458 measured two
candidates for the end of that interval on this machine (release build, `cmd.exe` as the shell, 20 warm runs, medians,
milliseconds from the start of `main`; the full phase table is in `renderer.md`):

| Candidate end point | Median | Against the 100 ms gate |
|---|---|---|
| The first frame with the prompt painted, into a window that is still hidden | 84 | passes |
| The first frame with the prompt after the window is visible (min 121, max 229) | 148 | fails |

This file answers which one P4 means, and whether a gate that includes time spent inside Windows is the right gate.

## Decision

1. **P4 ends when the first frame that contains the shell's output has been presented to a visible window.** The paint
   into the hidden window is reported as a phase and is not the gate.
2. **The gate stays at 100 ms warm on Windows and Linux and 150 ms on macOS, measured to that end point.** It is not
   restated as Terminus's own portion plus a separate report of the window becoming visible.
3. The probe reports both numbers and the phases between them, so a regression in either is visible.

## Why the visible frame

**The PRD says so.** The PRD gate row reads "Process start to first frame with the shell prompt visible, warm cache".
The test suite's shorter wording dropped the word "visible"; the PRD is the requirement.

**The hidden paint never reaches the screen.** Terminus creates the window with `with_visible(false)`, draws the first
frame, then calls `set_visible(true)` (`crates/terminus-app/src/gui/window.rs`, `draw`). On Windows that first frame goes
through softbuffer to the window's device context. A hidden top level window has no redirection surface and an empty
visible region, so GDI draws nothing. When `ShowWindow` makes the window visible, Windows invalidates the client area and
the window is drawn again from the paint message, through winit's `RedrawRequested` and a second `draw`. That second
frame is the first one a person sees. A gate that ends at the first `draw` ends at a frame that was discarded.

**A gate at the hidden paint rewards the wrong thing.** Any work moved after the hidden paint, including showing the
window, shortens the measured number while a person waits the same time or longer. The gate has to end after the point
where delaying the show stops helping.

**The other costs in the interval are not Terminus's either.** The interval already contains ConPTY starting the shell
(23 ms), the shell printing its prompt, the font loader and DirectWrite, and the loader mapping the executable. Nobody
proposed subtracting those. Time spent making the window visible is the same kind of cost. The gate is on what a person
experiences when launching the installed build on this machine, and other terminals launched on the same machine pay the
same operating system costs, which is what makes the number comparable.

## Why the gate is not restated

The question assumed the 60 ms between the painted frame and the visible window is Windows's cost. That is not
established. `ShowWindow` sends messages to the window procedure synchronously before it returns: `WM_SHOWWINDOW`,
`WM_WINDOWPOSCHANGING`, `WM_WINDOWPOSCHANGED`, `WM_NCCALCSIZE`, `WM_NCPAINT`, `WM_ERASEBKGND`, `WM_ACTIVATE`,
`WM_SETFOCUS`, `WM_IME_SETCONTEXT`, and whatever winit's handlers for those do (its `Resized`, `Moved`, `Focused` and
theme handling), and whatever Terminus does in response. All of that is counted "inside `ShowWindow`". The part that
belongs to the desktop window manager is creating the window's redirection surface and composing it, which should cost
one or two compositions, 6 to 12 ms on this 164 Hz display. The P4 runs were also taken with about 30 % of the CPU in
use by a screen recorder and a test harness, which the renderer decision says moves start up times by tens of
milliseconds, and the spread of the visible number (121 to 229 ms) is consistent with that.

So the order is: measure on a quiet machine, attribute the time inside `ShowWindow`, run the control experiment below,
and only then decide whether any of it is a floor Windows sets. If the control experiment shows a floor that makes 100 ms
unreachable, that is a new design question with the measurement attached, and the answer may move the number. It does
not move the end point: the gate never ends before a person could see the prompt.

## What the probe measures, per platform

The probe (`TERMINUS_STARTUP_PROBE=<file>`) reports, in milliseconds from the start of `main`:

| Field | Meaning |
|---|---|
| `painted_ms` | the first `draw` that contains shell output, whether or not the window is visible (today's `first_frame_ms`) |
| `visible_ms` | the first frame containing shell output presented to a visible window, confirmed as below. **This is P4.** |
| `phases` | the phase table, with the messages dispatched during the show each given a phase, so the time inside the show is attributed |

"Presented to a visible window, confirmed" means:

| Platform | Confirmation |
|---|---|
| Windows | after the first present to the window following `ShowWindow`, call `DwmFlush` and take the time when it returns; the compositor has then composed a frame that includes the present |
| Wayland | the `presented` event of `wp_presentation_feedback` for the first buffer committed after the initial configure |
| X11 | the present completion for the first frame drawn after the window is mapped |
| macOS | the drawable's presented handler for the first frame after `makeKeyAndOrderFront` |

Two things the probe cannot see are excluded, stated here so nobody counts them later: the time between `CreateProcess`
and the start of `main`, and the compositor's window open animation, which runs after composition and is the same for
every application on the machine. Both are covered by the once per release camera check.

**Once per release**, the same camera session that confirms P2 at 240 frames per second films a launch of Terminus,
Windows Terminal and Alacritty from the same shortcut, and the frame count from the key press to the prompt is written
next to the probe's number in `docs/performance.md`. The camera is the calibration of the probe, not the gate.

## The control experiment

Before concluding that any of the show cost is Windows's: a minimal program (Rust, `windows-sys`, no winit) that creates a
hidden top level window with the same styles winit uses, fills it with one colour, and times `ShowWindow`, the first
`WM_PAINT`, and `DwmFlush` after it, warm, 20 runs, on a quiet machine. That number is the floor Windows sets on this
machine for making a window visible. The difference between it and Terminus's `visible_ms - painted_ms` is Terminus's and
winit's, and that difference is what the profile has to explain and reduce.

## Levers, in the order to try them

These are for task-2458, which owns the M3 exit criterion "P4 gate". None of them changes the definition above.

1. Re-measure on a quiet machine. No recorder, no harness, nothing else building.
2. Give every window event dispatched between `first_paint` and `shown` a phase with a timestamp, so the 60 ms is
   attributed to named handlers. Profile that span with Windows Performance Recorder if the phases do not explain it.
3. Run the control experiment and record its number here.
4. Reduce what the profile finds. Candidates, unverified: the second full `draw` on the paint message after the show;
   winit's work on `WM_SETFOCUS` and `WM_ACTIVATE`; the non client frame and theme loading on the first show of the
   first window in the process; anything Terminus does on `Resized` or `Focused` during the show.
5. The 84 ms before the show has its own slack: fonts load at 41 to 55 ms on the UI thread and could start on another
   thread at the top of `main`; the 29 ms from fonts loaded to the first paint needs a phase split (`renderer_ready`,
   layout, view update, paint) before anyone guesses at it.

## Status

Decision taken 2026-11-01. Measured and reduced by task-2458 on 2026-11-01, below. P4 passes: the first visible frame
with the prompt comes at a median of 76 to 79 ms. The camera check is still to do; it needs a person.

## Measured (task-2458)

The probe now writes `painted_ms`, `visible_ms`, `active_ms` (below), the phases, and `show_messages`: every window
message handled on the UI thread while the window is shown and activated, with when it entered the window procedure,
how long it took and how deeply it was nested. Two thread hooks (`WH_CALLWNDPROC`, `WH_CALLWNDPROCRET`) record them,
only while the probe is on (`crates/terminus-app/src/gui/showtrace.rs`). The harness is
`_agent_output/task-2458-p4/run-probe.ps1`; each batch is 20 warm runs after one warm up run, with `cmd.exe` as the
shell, on the release build.

Load: the first measurements below had an Android emulator from another task using about 30 % of the CPU. The final
batches had only a screen recorder running, with 16 to 23 % of the CPU in use. Nothing was building.

### Where the 56 ms inside the show went

`show_messages` before any change, three runs: the time inside `ShowWindow` is almost all `WM_ACTIVATE`, 36 to 73 ms.
Inside it, `WM_IME_SYSTEM` (0x287) took 16 to 38 ms and `WM_SETFOCUS` took 18 to 34 ms. None of winit's buffered events
reached Terminus during the show; winit queues them until the handler that called `set_visible` returns.

- `WM_SETFOCUS`: winit's handler calls `update_modifiers`, which reads winit's keyboard layout table, and the first read
  in a process builds it by calling `ToUnicodeEx` for every key and modifier state.
- `WM_IME_SYSTEM`: the text services starting for the UI thread on its first activation.

With the CPU renderer only (`window.renderer = "cpu"`, no GPU thread starting at the same time) the show took 39 ms
instead of 56, so about 17 ms of it was the GPU device starting on another thread at the same moment.

### The control experiment

`_agent_output/task-2458-p4/control/` is the program described above: `windows` crate, no winit, a hidden window with
winit's styles and the dark title bar attribute, filled with one colour through `GetDC` before the show, as Terminus's
first frame is. 20 warm runs, times from the start of `ShowWindow`, final batch:

| Show | `ShowWindow` returns | First `WM_PAINT` | `DwmFlush` returns: median (min to max) | `WM_ACTIVATE` |
|---|---|---|---|---|
| `SW_SHOW` (activates) | 16.0 ms | 16.5 ms | 24.4 ms (14.8 to 611.7) | 10.1 ms |
| `SW_SHOWNOACTIVATE` | 4.1 ms | 4.3 ms | 10.6 ms (5.6 to 16.0) | none |

So the floor Windows sets for making a window visible on this machine is about 24 ms with activation in the show and
about 11 ms without it. Activation alone is about 10 ms even for an empty window.

Before the changes, Terminus took 70 ms from the first paint to the visible frame (median of 20, loaded machine), 46 ms
more than the floor. After them it takes 26 ms (52.8 to 78.6), against the 11 ms floor of the show it now does.

### What changed

1. **The window is shown without activating it, and activated after the first visible frame**
   (`crates/terminus-app/src/gui/show.rs`). The first frame is drawn into the hidden window, the window is shown with
   `SW_SHOWNOACTIVATE`, the paint message that follows presents the frame again, and then `SetForegroundWindow` brings
   the window to the front and IME is allowed. The activation work above still happens, after the frame a person sees.
   `active_ms` in the probe is when it is done: a median of 80 to 82 ms. `SetForegroundWindow` follows the same rules
   Windows applies when the first window of a new process activates in `ShowWindow`: a window started from the Start
   menu, a shortcut, the taskbar or the quick terminal hotkey comes to the front; one started by a process that is not
   in the foreground flashes its taskbar button. Under the probe, which a background shell starts, the call is refused,
   so `active_ms` there does not include the activation work. The camera check is the place to confirm the window comes
   to the front when a person starts it.
2. **Fonts and the first CPU frame are prepared on a thread** (`crates/terminus-app/src/gui/warm.rs`). Loading the font
   grid took 11 to 13 ms and the first CPU frame took 20 ms against 2 to 4 ms for later ones; the difference is the
   glyph atlas and the render threads starting. Both ran on the UI thread before the first paint. A thread started when
   the event loop exists loads the grid at the system DPI and renders printable ASCII once into the CPU renderer the
   first window uses, while the UI thread spawns the shell and creates the window. The window takes them when its
   scale matches the system DPI and otherwise loads its own as before. Only Windows can say the scale before a window
   exists, so other platforms do not use the thread.

Measured after each step, 20 warm runs:

| Build | Visible: median (min to max) | Painted | Load |
|---|---|---|---|
| Before (the M3 runs) | 148 (121 to 229) | 84 | recorder and harness, about 30 % |
| Before, new probe | 146 (116 to 426) | 134 | emulator, about 30 % |
| Show without activating | 92.9 (87.6 to 463.3) | 79.1 | emulator, about 30 % |
| And fonts and first frame on a thread | 84.3 (73.9 to 93.1) | 74.9 | emulator, about 30 % |
| Same build, four batches | 76.3 / 77.4 / 78.4 / 78.6 (64.6 to 102.8) | 60 to 63 | recorder only, 16 to 23 % |

Phases of the last batch (median of 20, ms from the start of `main`):

| Phase | Time |
|---|---|
| Event loop built, warm thread started | 5.8 |
| Shell spawned (before the window, so it starts in parallel) | 18.8 |
| Window created (hidden) | 36.7 |
| Fonts taken from the warm thread | 42.1 |
| First frame rendered on the CPU | 51.8 |
| First frame painted into the hidden window, already showing the prompt | 52.8 |
| Window shown (`ShowWindow` with `SW_SHOWNOACTIVATE` returns) | 59.1 |
| Paint message after the show: frame presented again | 65.0 |
| **First visible frame with the prompt (`DwmFlush` returned)** | **78.6** |
| Window activated, IME allowed | 81.6 |

The 6 to 20 ms from the second present to `DwmFlush` returning is the compositor waiting for its next composition at
164 Hz.

### Still on the critical path

- Spawning the shell takes 12 ms on the UI thread before the window is created. Starting the window first would make
  the frame wait for the shell's first output instead; spawning on another thread needs the session to exist before the
  window does.
- `create_window` takes 13 to 15 ms in winit (window class, icon, the dark title bar, `OleInitialize` for drag and
  drop). The control's `CreateWindowExW` takes under 1 ms.
- `cmd.exe` prints its banner 40 to 50 ms after it is spawned; its first bytes are ConPTY's set up sequences.
