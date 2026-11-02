# Scripting and AI agents

Everything you can do in Termoil with a key or the mouse, a script or an AI agent can do too, through the same code.
This page explains how that works and shows the three ways in: the `termoil` command line, the IPC socket and the
MCP server.

## One catalogue of actions

Every action Termoil has, from `new-tab` to `screen`, is a row in one catalogue in the `termoil-protocol` crate. A row
has the action's name, its parameters, a one line description, and what it needs to run (a window, a pane, or
nothing). `App::run_action` is the only function that turns an action into a change. Keybindings, the command palette,
the command line, the IPC socket and the MCP server all call it.

That arrangement has three results:

- A script and a person get the same behaviour, because there is no second implementation for scripts to drift from.
- [Commands](commands.md) and the MCP tool list are generated from the catalogue. A test fails if either one differs
  from it, so the documentation is never out of date.
- A new feature is finished only when it has a catalogue row. A control that a person can reach and a script cannot
  is treated as a bug.

The catalogue has 65 actions. 47 of them have a command line form, and 30 of those are MCP tools. The other 18, such
as `scroll-page-up`, act on what a person is looking at; a script can still run them by name through `run-action`.

## The command line

`termoil +<action> [--parameter value ...]` runs one action in the running Termoil and prints its answer as JSON.

```sh
termoil +list                                  # windows, tabs and panes with their ids, titles and folders
termoil +new-tab --cwd ~/src/app --name app    # a tab in a folder, with a title
termoil +split --direction down                # split the focused pane
termoil +send-text --pane 4 --text "npm test"  # type into pane 4
termoil +send-key --pane 4 --key enter         # press a key in pane 4, encoded for its keyboard mode
termoil +wait-prompt --pane 4 --timeout 300    # wait for the command to finish; prints its exit code
termoil +screen --pane 4                       # the visible text
termoil +screen --pane 4 --json true           # every cell with its character, colours and style
termoil +scrollback --pane 4 --lines 2000      # the last 2000 lines of history and screen
termoil +run-action --name zoom-pane           # any action, with --args '{"key": "value"}'
```

An action that needs a pane uses the focused pane when `--pane` is left out. A shell started by Termoil has
`TERMOIL_SOCKET` set, so a `termoil` command run inside a Termoil pane always reaches the Termoil it runs in.

`wait-prompt` and the exit code it returns come from the prompt marks shell integration adds (see
[Getting started](getting-started.md#shell-integration)). Without them Termoil cannot tell when a command has ended.

The command line exits with 0 when the action succeeded. When it did not, it prints the reason on standard error and
exits with 1, or with 2 when the arguments were wrong. When no Termoil is running it says so; `termoil` with no
arguments, or `termoil-gui`, starts one.

## The IPC socket

The command line is a thin client of a socket the running Termoil serves:

| System | Socket |
|---|---|
| Windows | the named pipe `\\.\pipe\termoil-<user>` |
| macOS and Linux | `$XDG_RUNTIME_DIR/termoil-<user>.sock`, or the same name in the temporary folder |

Only your user can open it: it has an owner only DACL on Windows and mode 0600 elsewhere. Termoil never listens on a
network port.

The protocol is one JSON object per line. A request names the action and its arguments, and the reply carries the
same `id`:

```json
{"id": 1, "action": "new-tab", "args": {"cwd": "C:\\src\\app", "name": "app"}}
{"id": 1, "ok": true, "result": {"window": 2, "tab": 5, "pane": 4}}

{"id": 2, "action": "focus", "args": {"pane": 99}}
{"id": 2, "ok": false, "error": "<the reason>"}
```

Any language that can open a named pipe or a Unix socket can drive Termoil this way.

## The MCP server

`termoil +mcp` is a Model Context Protocol server over standard input and output. Each tool is an action from the
catalogue, named `termoil_<action>` with the action's parameters as its input schema, and a call is forwarded to the
running Termoil. To add it to an agent:

```json
{
  "mcpServers": {
    "termoil": {
      "command": "termoil",
      "args": ["+mcp"]
    }
  }
}
```

For Claude Code, `claude mcp add termoil -- termoil +mcp` does the same. The tool list is in
[mcp-tools.json](mcp-tools.json).

A typical agent loop:

1. `termoil_new_tab` with a `cwd` and a `name`, which returns the new pane's id.
2. `termoil_send_text`, then `termoil_send_key` with `enter`, to run a command in that pane.
3. `termoil_wait_prompt` to wait for the command, which returns its exit code.
4. `termoil_screen` or `termoil_scrollback` to read what it printed.

Because the agent drives a real terminal, a person can watch the same tab while the agent works, and take over with
the keyboard at any point.

## A second, separate Termoil

`termoil-gui --separate`, or `termoil +new-window --separate true`, starts a Termoil in a process of its own with its
own socket, `termoil-<user>-<pid>`. Its answer names the socket, and `termoil --socket <name> +<action>` talks to it.
Use it to keep an agent's windows apart from your own, or to give a test its own instance.

## Embedding the terminal

The terminal itself is the `termoil-vt` crate: a parser, the screen, scrollback, selection, search and the graphics
store, with no window, font or GPU dependency. It builds for WebAssembly, and CI checks that it still does. A program
that wants a terminal without Termoil's window can feed `termoil-vt` the bytes from `termoil-pty` and draw the cells
however it likes. [Architecture overview](architecture-overview.md) shows how the crates fit together.
