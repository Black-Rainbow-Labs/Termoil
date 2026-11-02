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

Decision taken 2026-11-01. The probe change, the quiet machine numbers, the control experiment result and the camera
check are task-2458's to add to this file and to `docs/performance.md`; until they are, the P4 row in
`docs/performance.md` reads as failing at a median of 148 ms visible.
