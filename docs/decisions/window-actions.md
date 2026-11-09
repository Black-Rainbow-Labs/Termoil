# Actions run from the window, and their answers (task-2586)

## The report

task-2586: "When I check for updates or list for updates inside of termoil through CMD+SHIFT+P, nothing happens. We need extensive testing to ensure all commands and functionality work as expected."

## What was wrong

Every place the window runs an action called `App::run_action` and dropped what it returned:

| Call site | Code before task-2586 |
|---|---|
| Palette, Enter | `let _ = app.run_action(...)` in `gui/overlay.rs` `palette_enter` |
| Keybinding | `let _ = app.run_action(...)` in `gui/handle.rs` `binding` |
| Right click copy and paste | `let _ = app.run_action(...)` in `gui/handle.rs` `right_click` |
| Tab bar cross and plus | `let _ = app.run_action(...)` in `gui/handle.rs` |
| Settings view, open the file | `let _ = app.run_action(...)` in `gui/settings_view.rs` |

`run_action` answers an `Outcome`. Three kinds of answer were lost:

1. **`Outcome::Later(job)`.** `check-for-updates`, `list-updates` and `install-update` do their network work in a closure that the caller has to run on a thread. The window dropped the closure, so the request was never made. This is the report: nothing happened, not even the desktop notification `check-for-updates` sends when it is run with `announce = true`.
2. **An answer that is information.** `version`, `list`, `list-themes`, `show-config`, `telemetry-status`, `list-updates`, `save-session`, `screen`, `scrollback` and `install-shell-integration` answer JSON that a person needs to read. The window threw it away, so running them from the palette did nothing visible.
3. **An error.** A failed action from the window looked the same as an action that did nothing. `skip-update` with no earlier check, `set-theme` with a name that is not a bundled theme, `search-next` with no search active and `resize` were all silent failures.

The palette had one more fault: it asked only for the first required parameter. `resize` needs `cols` and `rows`, so the palette ran it with `cols` alone and it always failed with "--rows is required", which nobody saw.

## The design

### One path for an action a person ran

`App::run_for_person(&mut self, call: &ActionCall)` is the function every window call site uses. It calls `run_action`, then deals with each kind of `Outcome`:

| Outcome | What `run_for_person` does |
|---|---|
| `Done(Ok(value))`, action is in `answer::ANSWERS` | Queues `Effect::ShowAnswer` with the answer written as text. |
| `Done(Ok(value))`, any other action | Nothing more. The action already changed something the person can see (a tab opened, the font grew). |
| `Done(Err(error))` | Queues `Effect::ShowAnswer` with the error, marked as a failure. |
| `Later(job)` | Queues `Effect::ShowAnswer` with "Checking for updates" (or the action's own waiting line), then runs the job on a thread with `announce = true`. The job's result goes to `App::answers`, a sink the window installs, which posts `UserEvent::Answer` back to the event loop. |
| `Wait(waiter, timeout)` | The same as `Later`: a thread waits for the prompt and sends the exit code as the answer. |

`run_action` stays the one place an action becomes a change. `run_for_person` only decides what the person is told about the result. The IPC server and the headless runtime keep calling `run_action`, because their callers read the result themselves.

`App::run_named`, which the E6 harness and the quick terminal use, also goes through `run_for_person`, so a job started there is run and answered too.

### Which actions answer

`crate::answer` holds two lists that together name every catalogue row:

- `ANSWERS`: the actions whose result is information. Run from the window, their result is shown.
- `ACTS`: the actions whose result is a change the person can already see. Run from the window, they show nothing unless they fail.

A test (A5, below) fails naming any catalogue row that is in neither list or in both, so a new action has to be put in one of them.

### How an answer is written

`answer::describe(action, &result) -> Answer` turns the JSON into a title and lines a person reads:

| Action | Title | Lines |
|---|---|---|
| `check-for-updates`, newer | `Termoil 1.6.0 is available` | `You have 1.5.6. Run Install Update from the command palette, or termoil +install-update.` and the release notes URL |
| `check-for-updates`, not newer | `Termoil is up to date` | `1.5.6 is the newest version on the stable channel.` |
| `list-updates` | `Termoil versions (you have 1.5.6)` | one line per channel heading, then one line per version: version, date, `running` beside the one running; a channel that could not be read says why |
| `install-update` | `Termoil 1.6.0 is installing` or `Termoil 1.6.0 was downloaded` | what happens next, or the file it wrote |
| `skip-update` | `Termoil 1.6.0 will not be announced` | `updates.skip is now 1.6.0.` |
| `version` | `Termoil 1.5.6` | the build date |
| any error | `<action> failed` | the error text |
| any other answer | the action's name | the JSON pretty printed, or the text when the answer is a string |

The two `check-for-updates` sentences are the ones the automatic check already uses (`update::actions::sentence`), so the words are the same in both places.

### Where an answer is shown

A new overlay, `Overlay::Answer(AnswerUi)`, is drawn where the palette is drawn, centered near the top of the window, at most `ANSWER_ROWS` (20) rows tall beside the palette's `PALETTE_ROWS`, and the palette's width. A failure is drawn with the theme's red as the title color. While a job is running the overlay shows the waiting line, and the job's answer replaces it when it arrives.

- Escape, Enter or any other key closes it. The key is not sent to the shell.
- An answer that arrives while another overlay is open (the person opened the palette again) waits in `WinState::answer_waiting` and is shown when that overlay closes.
- An answer for a window that has closed is shown in the focused window. With no window open it goes to `platform::notify`.
- A failure of an action in `ACTS` is shown only when it was run from the palette (`answer::Asked::Palette`). From a key it stays quiet, so `copy` pressed with nothing selected, or `search-next` pressed with no search active, does not open an overlay.
- Up, Down, Page Up and Page Down scroll; they do not close it.

### The desktop notification

`check-for-updates` and `install-update` used to send a desktop notification when a person ran them from the window. The answer overlay now shows the same text inside the window, so those two notifications are removed. The automatic check at start still sends its notification, because nobody asked and there may be no window in front.

### The palette asks for every required parameter

`PaletteUi::asking` now holds the action, the required parameters still to ask for, and the arguments collected so far. Enter on a parameter stores it and asks for the next one. When none are left the action runs with every argument. `resize` now asks `--cols` and then `--rows`.

`list-updates` had `palette = false` in the catalogue while the palette lists every action (A4). The flag only reaches `docs/commands.md`, and that table said the palette did not offer it. It is now `true`.

## The test suite

The ticket asks for testing that every command works. Two layers, each named in `docs/TEST-SUITE.md`.

### A5: every action from the window, headless (`crates/termoil-app/tests/a5_window_actions.rs`)

No display is needed. A headless `App` with one window running a real shell, the update environment variables pointed at a release server in the test process (the same manifest shape E5 serves), and an answer sink that records what reaches it. The overlays take an `App` and a `KeyEvent`, so the palette and the answer overlay are driven without a window too.

| Case | What it checks |
|---|---|
| `a5_every_action_is_classified` | Every catalogue row is in exactly one of `answer::ANSWERS` and `answer::ACTS`, and every name in those lists is a catalogue row. |
| `a5_answer_overlay_keys` | Up, Down and Page Up scroll an answer longer than `ANSWER_ROWS` and stop at the ends. Escape, Enter and a typed letter each close it. The drawn content has the title, the lines and the closing hint, and a long answer is cut to `ANSWER_ROWS`. |
| `a5_palette_asks_for_every_required_parameter` | `resize` from the palette asks `--cols`, then `--rows`, then runs with both and queues `ResizeWindow` for 70 by 20. |
| `a5_every_action_from_the_window` | One app, one window with a split, and the release server, in the seven steps below. |

The steps of `a5_every_action_from_the_window`:

1. **Palette keys.** `check-for-updates` typed into the palette and Enter: the palette closes, "Checking for updates" is queued, then the job's "Termoil is up to date" arrives through the sink. The same for `list-updates`. This is the report in task-2586. With the old palette code put back, this step fails with "Enter queued []".
2. **Check and list.** The stable channel at the running version answers "Termoil is up to date". The nightly channel answers "Termoil 99.0.0 is available" with its release notes. `list-updates` lists both channels with the running version marked, and `--channel nightly` lists only nightly.
3. **Install and skip.** `install-update --channel nightly --restart false` answers "was downloaded", and the file has the bytes the server sent. A version with a wrong SHA-256 and a version that is not on the channel are shown as failures. `skip-update` with no version skips the version the check found and writes `updates.skip`.
4. **Errors and keys.** `set-theme` with a theme that does not exist and `focus` of a pane that does not exist are shown as failures from the palette. The same `focus` from a key shows nothing. `version` from a key shows its answer, and `run_named("list")` shows its answer.
5. **Every action.** Every catalogue action except `open-config`, `close`, `close-tab` and `quit`, run from the palette with the arguments a person would give. None answers "unknown action". An action in `ANSWERS` shows a non empty answer. An action in `ACTS` shows nothing. Only the actions in the test's `expected_failures` table fail, each for the reason the table gives: no OSC 133 marks arrive without shell integration, and nothing is selected for `copy`.
6. **The rest.** `open-config` queues `OpenPath` for the configuration file, and `close`, `close-tab` and `quit` show nothing.
7. **Failures of the channel.** In a second app with both channels pointed at files the server does not have, `skip-update` before any check fails with "no check has found a version yet", `check-for-updates` is shown as "check-for-updates failed", and `list-updates` says each channel "could not be read".

The answer text for each row of the table in "How an answer is written" is also checked on fixed JSON by the unit tests in `crates/termoil-app/src/answer.rs`.

### E6: the real window (`crates/termoil-app/tests/e6_window.rs`)

The E6 test opens a winit window with a real shell. Its configuration points the update check at a release server in the test process on the nightly channel, and these steps are added:

| Step | What it checks |
|---|---|
| the palette checks for updates and shows the answer | The palette's key, `check-for-updates` typed, Enter: the answer overlay opens with the waiting line, then shows "Termoil 99.0.0 is available". The window draws it, Escape closes it, and the next key reaches the shell again. |
| the palette lists updates | The same for `list-updates`: the overlay lists both channels and 99.0.0. |
| the palette shows a failed action's error | `set-theme` from the palette with a theme that does not exist shows "set-theme failed". |
| every palette action runs from the palette | Each palette entry is typed and run with Enter in turn, with its required arguments typed when the palette asks, and every overlay is closed before the next one. No action may answer "unknown action", an action in `ANSWERS` must show its answer, and none may fail apart from the ones in the step's `MAY_FAIL` table, each with its reason. The entries in `LEFT_TO_A5` are left to A5 because in the real window they would change the machine outside the test or end the window the step uses: `install-shell-integration`, `install-update`, `open-config`, `close`, `close-tab`, `move-tab-to-window`, `wait-prompt` and `quit`. On this machine 62 entries run. |

The E6 test also sets `TERMOIL_DATA_DIR` to its own folder and `TERMOIL_TELEMETRY=off`, so `save-session` and the session saved at quit never touch the real data folder.

## Not changed

- The IPC server and the CLI answer exactly what they answered before. E5 still covers every command through the CLI.
- The automatic update check at start, and its desktop notification.
- `run_action` itself.
