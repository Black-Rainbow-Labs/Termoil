# Detaching a window to its own process (PRD 5.3)

Asked by task-2458 (M4), answered by task-2463 on 2026-11-01. This is a reading of the PRD against what the platforms allow, not a measured decision. The code it refers to is on the task-2458 branch at the time of writing: `move-tab-to-window` in `crates/terminus-app/src/actions.rs`, `hand_to_running` in `crates/terminus-app/src/gui/mod.rs`, `--socket` in `crates/terminus-app/src/main.rs`, and `Session` in `crates/terminus-app/src/session.rs`.

## The question

PRD 5.3 (must) says "Multiple windows; a window can be detached to its own process." PRD 5.6 says "Splits and tabs are in; detaching from a window is not." PRD 5.5 defers a multiplexer server. TDD 4.8 says a pane owns one `Session` (the PTY, the `Terminal` and its three threads) inside the app process. task-2458 asked whether the must row means a running session moves to another process, or whether a new window in a separate process is enough, and whether the TDD should specify a transfer of handles and terminal state.

## Three things are called "detach"

1. **A tab or pane leaves its window for another window of the same process.** kitty calls this `detach_tab` (to a new OS window) and `detach_window` (to another tab). Windows Terminal calls it tab tear out. The sessions keep running because nothing leaves the process. Terminus has this today as `move-tab-to-window`.
2. **The session outlives the window.** tmux `detach`, the WezTerm multiplexer client. The session lives in a server process and every window is a client of it. This is PRD 5.5, and PRD 5.6 says it is not in 1.0.
3. **A window runs in a process of its own**, so a panic, a GPU device loss or a hung render thread in that process cannot reach the other windows. This is what "its own process" in PRD 5.3 asks for.

The must row is meaning 3 with the wording of meaning 1, and that is what made it read as if a running session had to cross a process boundary.

## Answer to question 1: a separate process is enough, and a running session does not move

A running session stays in the process that created it. The reasons:

- **ConPTY binds the child at creation.** The child gets its pseudoconsole through `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` at `CreateProcess` time. There is no API that rebinds a running child to a different pseudoconsole, and `HPCON` is an opaque value owned by the creating process rather than a handle `DuplicateHandle` can pass. The two pipes can be duplicated into another process, but the `HPCON` and the job object must then stay alive in the first process for the life of the child, so the first process cannot exit. A process that must stay alive to hold the pseudoconsole and forward its bytes is a session server. That is the multiplexer the PRD defers.
- **The terminal state would need a wire format.** The `Terminal` holds the pages and scrollback, the modes, the selection, the OSC 133 marks and up to 320 MB of kitty images. Moving it means a snapshot and a replay in the receiving process, which is the multiplexer's protocol. The `screen` and `scrollback` IPC commands already serialise part of this, so the later work is not from zero, but it is not 1.0 work.
- **Windows Terminal made the same choice.** It ran one process per window with the Remoting monarch and peasant model, and could not move a live connection between windows. Its Process Model v3 merged every window into one process so that tabs could be torn out and merged with their connections intact (https://github.com/microsoft/terminal/issues/1256). The cost is visible in https://github.com/microsoft/terminal/issues/17246, where a crash while merging a tab took every window down. The team that owns ConPTY chose moving tabs inside one process over moving sessions between processes.
- **Unix is easier but 1.0 ships one behaviour.** A PTY master descriptor can be sent over `SCM_RIGHTS`, so the handle part is cheap on macOS and Linux, but the state snapshot problem is identical and PRD 5.3 names all three platforms.

So the must row is satisfied by two things together: `move-tab-to-window`, which moves a tab with its running sessions to a new window of the same process, and a way to start a window in a process of its own.

## Answer to question 2: no transfer in 1.0, and the TDD does not specify one

Moving a running session between processes is the 5.5 multiplexer and stays outside 1.0. The TDD does not describe a handle transfer or a `Terminal` snapshot. What it already provides is the room the multiplexer needs: `Session` has no window, GPU or font dependency, a pane holds it through `Arc`, and the multiplexer later puts a transport behind that handle (a local session in process, a client session over a socket). That is the WezTerm arrangement the architecture research recommends (docs/research/architectures.md, "Tabs and splits").

## The 1.0 specification for the PRD 5.3 row

### `terminus-gui --separate`

- Starts an instance that does not hand its window to the running instance. Without the flag, `terminus-gui` keeps its current behaviour: when an instance is running, it asks that instance for a new window through the socket and exits.
- The instance's socket is `\\.\pipe\terminus-<user>-<pid>` on Windows and `terminus-<user>-<pid>.sock` in the same directory as the primary socket elsewhere. `--socket NAME` still overrides the name, as it does today for the tests.
- Every session the instance spawns gets `TERMINUS_SOCKET` set to that name in its environment, so `terminus +<command>` run inside one of its panes reaches the instance that owns the pane. The primary instance keeps spawning sessions without the variable, since `terminus` finds the primary socket by default.
- It reads the same config file and reloads it the same way. Themes, keybindings and shell integration are identical.
- It does not register the quick terminal global hotkey. The primary instance owns the hotkey; a second registration either fails or takes the hotkey from the primary, and both are wrong. The quick terminal is reached from the primary.
- Lifetime: the process exits when its last window closes, as the primary does. When the primary exits, separate instances keep running and the next `terminus-gui` started without `--separate` becomes the new primary. Separate instances do not take over the primary name.

### The catalogue row

- `new-window` gains a parameter `separate` (boolean, default false). With `separate = true`, `App::run_action` starts `terminus-gui --separate` with `--cwd` set to the given cwd or the focused pane's cwd, and `--command` if one was given, and answers with the socket name of the new instance so a caller can address it with `--socket`. `terminus +new-window --separate` and the MCP tool follow from the catalogue as every other parameter does.
- The command palette lists it as "New window in its own process". No default keybinding.
- There is no action that empties an existing window into a new process, because its sessions cannot follow. A person who wants a window in its own process starts it that way. `move-tab-to-window` remains the way to get a tab out of a crowded window.

### Tests

- E (end to end): start a primary instance; run `terminus +new-window --separate`; confirm a second `terminus-gui` process exists with its own socket and that `terminus --socket <name> +list` shows one window with one pane; run `terminus +list` from inside that pane (through `send-text` and `screen`) and confirm it answers with the separate instance's pane, which proves `TERMINUS_SOCKET` reached the child; stop the primary by its pid and confirm the separate instance still answers `+version` and its shell still echoes.
- A (catalogue): the `separate` parameter appears in `docs/commands.md` and `docs/mcp-tools.json` through generation, and the E test above is the test the A3 check requires.

### The feature matrix

The row "Multiple windows; a window can be detached to its own process" is done once `--separate` and the `new-window` parameter are in, with the note "a tab moves between windows with its sessions running; a session moves between processes only with the 5.5 multiplexer".

## Why have `--separate` at all

Isolation is the use. PRD 5.5 names running as the Tasks board's agent terminal; there, one agent's window crashing its renderer must not take the other agents' windows with it, and a window per process is the cheapest way to get that. The same holds for a long render or benchmark in one window beside everyday work in another.

## Document changes made with this decision

- PRD 5.3: the row now names the two mechanisms and says a running session does not move between processes in 1.0.
- PRD 5.6: the non goal now says what is excluded: a session that outlives its window or moves to another process.
- TDD 4.8: describes `move-tab-to-window`, `--separate` and the `new-window` parameter, and says no action moves a running session to another process.
