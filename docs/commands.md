# Terminus commands

Generated from the action catalogue in `crates/terminus-protocol`; do not edit by hand.
Every action runs through `App::run_action`, whether a keybinding, the command palette, `terminus +<command>`
or the IPC socket asked for it.

| Action | Command line | Palette | Needs | Summary |
|---|---|---|---|---|
| [`new-window`](#new-window) | yes | yes | nothing | Opens a new window with one tab. |
| [`new-tab`](#new-tab) | yes | yes | a window | Opens a new tab in the focused window, in the given directory or the focused pane's. |
| [`split`](#split) | yes | yes | a pane | Splits the pane, starting a new shell in the new half. |
| [`split-right`](#split-right) | no | yes | a pane | Splits the focused pane with the new pane on the right. |
| [`split-down`](#split-down) | no | yes | a pane | Splits the focused pane with the new pane below. |
| [`close`](#close) | yes | yes | a pane | Closes a pane, ending its shell; the last pane closes its tab and window. |
| [`focus`](#focus) | yes | no | a window | Focuses a pane, or the neighbouring pane in a direction. |
| [`focus-left`](#focus-left) | no | yes | a window | Moves focus to the pane on the left. |
| [`focus-right`](#focus-right) | no | yes | a window | Moves focus to the pane on the right. |
| [`focus-up`](#focus-up) | no | yes | a window | Moves focus to the pane above. |
| [`focus-down`](#focus-down) | no | yes | a window | Moves focus to the pane below. |
| [`next-tab`](#next-tab) | no | yes | a window | Switches to the next tab. |
| [`move-tab-to-window`](#move-tab-to-window) | yes | yes | a pane | Moves the tab holding a pane into a new window of its own. |
| [`previous-tab`](#previous-tab) | no | yes | a window | Switches to the previous tab. |
| [`zoom-pane`](#zoom-pane) | no | yes | a pane | Shows the focused pane alone in its tab, or restores the split. |
| [`resize-split`](#resize-split) | yes | yes | a pane | Moves the divider next to a pane in a direction, growing or shrinking the pane. |
| [`show-hints`](#show-hints) | no | yes | a pane | Labels every link on the screen of a pane; typing a label opens that link. |
| [`send-text`](#send-text) | yes | no | a pane | Types text into a pane's program, as if pasted without bracketing. |
| [`send-key`](#send-key) | yes | no | a pane | Presses a key in a pane, encoded for the program's keyboard mode. |
| [`screen`](#screen) | yes | no | a pane | Returns the visible screen of a pane as text, or as JSON cells with styles. |
| [`scrollback`](#scrollback) | yes | no | a pane | Returns the last lines of a pane's scrollback and screen as text. |
| [`wait-prompt`](#wait-prompt) | yes | no | a pane | Waits until the command running in a pane finishes and returns its exit code. |
| [`list`](#list) | yes | no | nothing | Lists windows, tabs and panes with their ids, titles and working directories. |
| [`resize`](#resize) | yes | no | a pane | Resizes a pane's window to a number of columns and rows. |
| [`set-title`](#set-title) | yes | yes | a pane | Sets the title of a pane's tab. |
| [`run-action`](#run-action) | yes | no | nothing | Runs any action by name with JSON arguments, the way a keybinding does. |
| [`show-config`](#show-config) | yes | no | nothing | Prints the effective configuration, or the documented default file. |
| [`reload-config`](#reload-config) | yes | yes | nothing | Reads the configuration file again and applies it. |
| [`set-theme`](#set-theme) | yes | yes | nothing | Switches to a bundled colour theme and writes it to the configuration file. |
| [`choose-theme`](#choose-theme) | no | yes | a window | Lists the bundled themes, previewing each as it is highlighted; Enter keeps one. |
| [`open-config`](#open-config) | no | yes | nothing | Opens the configuration file in the configured editor. |
| [`install-shell-integration`](#install-shell-integration) | yes | yes | nothing | Installs the shell integration scripts and hooks them into the named shells' startup files. |
| [`version`](#version) | yes | yes | nothing | Prints the version and build date. |
| [`copy`](#copy) | no | yes | a pane | Copies the selection to the clipboard. |
| [`paste`](#paste) | no | yes | a pane | Pastes the clipboard into the pane, bracketed when the program asked for it. |
| [`select-all`](#select-all) | no | yes | a pane | Selects the whole screen and scrollback. |
| [`clear-selection`](#clear-selection) | no | no | a pane | Clears the selection. |
| [`scroll-page-up`](#scroll-page-up) | no | yes | a pane | Scrolls the view up one page. |
| [`scroll-page-down`](#scroll-page-down) | no | yes | a pane | Scrolls the view down one page. |
| [`scroll-line-up`](#scroll-line-up) | no | no | a pane | Scrolls the view up one line. |
| [`scroll-line-down`](#scroll-line-down) | no | no | a pane | Scrolls the view down one line. |
| [`scroll-to-top`](#scroll-to-top) | no | yes | a pane | Scrolls the view to the oldest line. |
| [`scroll-to-bottom`](#scroll-to-bottom) | no | yes | a pane | Scrolls the view back to the live screen. |
| [`clear-scrollback`](#clear-scrollback) | yes | yes | a pane | Erases the scrollback of the pane, keeping the screen. |
| [`previous-prompt`](#previous-prompt) | no | yes | a pane | Scrolls to the previous shell prompt. |
| [`next-prompt`](#next-prompt) | no | yes | a pane | Scrolls to the next shell prompt. |
| [`select-last-output`](#select-last-output) | no | yes | a pane | Selects the output of the last command that finished. |
| [`search`](#search) | yes | yes | a pane | Opens the search bar, or searches for a pattern and selects the first match. |
| [`search-next`](#search-next) | no | yes | a pane | Selects the next search match. |
| [`search-previous`](#search-previous) | no | yes | a pane | Selects the previous search match. |
| [`increase-font-size`](#increase-font-size) | no | yes | a window | Makes the font one point larger. |
| [`decrease-font-size`](#decrease-font-size) | no | yes | a window | Makes the font one point smaller. |
| [`reset-font-size`](#reset-font-size) | no | yes | a window | Restores the configured font size. |
| [`toggle-fullscreen`](#toggle-fullscreen) | no | yes | a window | Enters or leaves full screen. |
| [`command-palette`](#command-palette) | no | no | a window | Opens the command palette, which lists every action with its binding. |
| [`quick-terminal`](#quick-terminal) | yes | yes | nothing | Shows or hides the quick terminal that slides in from a screen edge. |
| [`reset-terminal`](#reset-terminal) | no | yes | a pane | Resets the pane's terminal state (RIS), keeping the shell running. |
| [`quit`](#quit) | no | yes | nothing | Closes every window and exits. |

## new-window

Opens a new window with one tab.

```
terminus +new-window [--cwd <path>] [--command <text>]
```

Over IPC: `{"id":1,"action":"new-window","args":{...}}`. MCP tool: `terminus_new_window`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cwd` | path | no | Working directory for the shell. |
| `command` | text | no | Program to run instead of the configured shell. |

## new-tab

Opens a new tab in the focused window, in the given directory or the focused pane's.

```
terminus +new-tab [--cwd <path>] [--name <text>] [--command <text>]
```

Over IPC: `{"id":1,"action":"new-tab","args":{...}}`. MCP tool: `terminus_new_tab`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cwd` | path | no | Working directory for the shell. |
| `name` | text | no | Title for the tab. |
| `command` | text | no | Program to run instead of the configured shell. |

## split

Splits the pane, starting a new shell in the new half.

```
terminus +split [--pane <pane id>] [--direction <left|right|up|down>] [--cwd <path>]
```

Over IPC: `{"id":1,"action":"split","args":{...}}`. MCP tool: `terminus_split`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `direction` | left|right|up|down | no | Which side the new pane goes on; right when omitted. |
| `cwd` | path | no | Working directory for the shell. |

## split-right

Splits the focused pane with the new pane on the right.

No parameters.

## split-down

Splits the focused pane with the new pane below.

No parameters.

## close

Closes a pane, ending its shell; the last pane closes its tab and window.

```
terminus +close [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"close","args":{...}}`. MCP tool: `terminus_close`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## focus

Focuses a pane, or the neighbouring pane in a direction.

```
terminus +focus [--pane <pane id>] [--direction <left|right|up|down>]
```

Over IPC: `{"id":1,"action":"focus","args":{...}}`. MCP tool: `terminus_focus`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `direction` | left|right|up|down | no | Move focus to the neighbour on this side. |

## focus-left

Moves focus to the pane on the left.

No parameters.

## focus-right

Moves focus to the pane on the right.

No parameters.

## focus-up

Moves focus to the pane above.

No parameters.

## focus-down

Moves focus to the pane below.

No parameters.

## next-tab

Switches to the next tab.

No parameters.

## move-tab-to-window

Moves the tab holding a pane into a new window of its own.

```
terminus +move-tab-to-window [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"move-tab-to-window","args":{...}}`. MCP tool: `terminus_move_tab_to_window`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## previous-tab

Switches to the previous tab.

No parameters.

## zoom-pane

Shows the focused pane alone in its tab, or restores the split.

No parameters.

## resize-split

Moves the divider next to a pane in a direction, growing or shrinking the pane.

```
terminus +resize-split [--pane <pane id>] --direction <left|right|up|down> [--percent <integer>]
```

Over IPC: `{"id":1,"action":"resize-split","args":{...}}`. MCP tool: `terminus_resize_split`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `direction` | left|right|up|down | yes | Which way the divider moves. |
| `percent` | integer | no | How far, as a percent of the split; 5 when omitted. |

## show-hints

Labels every link on the screen of a pane; typing a label opens that link.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## send-text

Types text into a pane's program, as if pasted without bracketing.

```
terminus +send-text [--pane <pane id>] --text <text>
```

Over IPC: `{"id":1,"action":"send-text","args":{...}}`. MCP tool: `terminus_send_text`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `text` | text | yes | The text; \n is Enter. |

## send-key

Presses a key in a pane, encoded for the program's keyboard mode.

```
terminus +send-key [--pane <pane id>] --key <key>
```

Over IPC: `{"id":1,"action":"send-key","args":{...}}`. MCP tool: `terminus_send_key`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `key` | key | yes | A chord such as enter, ctrl+c or shift+tab. |

## screen

Returns the visible screen of a pane as text, or as JSON cells with styles.

```
terminus +screen [--pane <pane id>] [--json <true|false>]
```

Over IPC: `{"id":1,"action":"screen","args":{...}}`. MCP tool: `terminus_screen`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `json` | true|false | no | Return cells with styles instead of text. |

## scrollback

Returns the last lines of a pane's scrollback and screen as text.

```
terminus +scrollback [--pane <pane id>] [--lines <integer>]
```

Over IPC: `{"id":1,"action":"scrollback","args":{...}}`. MCP tool: `terminus_scrollback`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `lines` | integer | no | How many lines; 500 when omitted. |

## wait-prompt

Waits until the command running in a pane finishes and returns its exit code.

```
terminus +wait-prompt [--pane <pane id>] [--timeout <integer>]
```

Over IPC: `{"id":1,"action":"wait-prompt","args":{...}}`. MCP tool: `terminus_wait_prompt`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `timeout` | integer | no | Seconds to wait; 600 when omitted. |

## list

Lists windows, tabs and panes with their ids, titles and working directories.

```
terminus +list
```

Over IPC: `{"id":1,"action":"list","args":{...}}`. MCP tool: `terminus_list`.

No parameters.

## resize

Resizes a pane's window to a number of columns and rows.

```
terminus +resize [--pane <pane id>] --cols <integer> --rows <integer>
```

Over IPC: `{"id":1,"action":"resize","args":{...}}`. MCP tool: `terminus_resize`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `cols` | integer | yes | Columns. |
| `rows` | integer | yes | Rows. |

## set-title

Sets the title of a pane's tab.

```
terminus +set-title [--pane <pane id>] --title <text>
```

Over IPC: `{"id":1,"action":"set-title","args":{...}}`. MCP tool: `terminus_set_title`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `title` | text | yes | The title. |

## run-action

Runs any action by name with JSON arguments, the way a keybinding does.

```
terminus +run-action --name <action> [--args <text>]
```

Over IPC: `{"id":1,"action":"run-action","args":{...}}`. MCP tool: `terminus_run_action`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | action | yes | The action. |
| `args` | text | no | Arguments as a JSON object. |

## show-config

Prints the effective configuration, or the documented default file.

```
terminus +show-config [--default <true|false>]
```

Over IPC: `{"id":1,"action":"show-config","args":{...}}`. MCP tool: `terminus_show_config`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `default` | true|false | no | Print the default file instead. |

## reload-config

Reads the configuration file again and applies it.

```
terminus +reload-config
```

Over IPC: `{"id":1,"action":"reload-config","args":{...}}`. MCP tool: `terminus_reload_config`.

No parameters.

## set-theme

Switches to a bundled colour theme and writes it to the configuration file.

```
terminus +set-theme --name <text>
```

Over IPC: `{"id":1,"action":"set-theme","args":{...}}`. MCP tool: `terminus_set_theme`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | text | yes | The theme's name, as `choose-theme` lists it. |

## choose-theme

Lists the bundled themes, previewing each as it is highlighted; Enter keeps one.

No parameters.

## open-config

Opens the configuration file in the configured editor.

No parameters.

## install-shell-integration

Installs the shell integration scripts and hooks them into the named shells' startup files.

```
terminus +install-shell-integration [--shells <text>] [--dry-run <true|false>]
```

Over IPC: `{"id":1,"action":"install-shell-integration","args":{...}}`. MCP tool: `terminus_install_shell_integration`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `shells` | text | no | Comma separated: powershell, pwsh, bash, zsh, fish, nushell, cmd; the current shell when omitted. |
| `dry-run` | true|false | no | Show what would change without changing it. |

## version

Prints the version and build date.

```
terminus +version
```

Over IPC: `{"id":1,"action":"version","args":{...}}`. MCP tool: `terminus_version`.

No parameters.

## copy

Copies the selection to the clipboard.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## paste

Pastes the clipboard into the pane, bracketed when the program asked for it.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## select-all

Selects the whole screen and scrollback.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## clear-selection

Clears the selection.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## scroll-page-up

Scrolls the view up one page.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## scroll-page-down

Scrolls the view down one page.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## scroll-line-up

Scrolls the view up one line.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## scroll-line-down

Scrolls the view down one line.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## scroll-to-top

Scrolls the view to the oldest line.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## scroll-to-bottom

Scrolls the view back to the live screen.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## clear-scrollback

Erases the scrollback of the pane, keeping the screen.

```
terminus +clear-scrollback [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"clear-scrollback","args":{...}}`. MCP tool: `terminus_clear_scrollback`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## previous-prompt

Scrolls to the previous shell prompt.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## next-prompt

Scrolls to the next shell prompt.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## select-last-output

Selects the output of the last command that finished.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## search

Opens the search bar, or searches for a pattern and selects the first match.

```
terminus +search [--pane <pane id>] [--pattern <text>] [--case-sensitive <true|false>]
```

Over IPC: `{"id":1,"action":"search","args":{...}}`. MCP tool: `terminus_search`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `pattern` | text | no | A regular expression. |
| `case-sensitive` | true|false | no | Match case exactly. |

## search-next

Selects the next search match.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## search-previous

Selects the previous search match.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## increase-font-size

Makes the font one point larger.

No parameters.

## decrease-font-size

Makes the font one point smaller.

No parameters.

## reset-font-size

Restores the configured font size.

No parameters.

## toggle-fullscreen

Enters or leaves full screen.

No parameters.

## command-palette

Opens the command palette, which lists every action with its binding.

No parameters.

## quick-terminal

Shows or hides the quick terminal that slides in from a screen edge.

```
terminus +quick-terminal
```

Over IPC: `{"id":1,"action":"quick-terminal","args":{...}}`. MCP tool: `terminus_quick_terminal`.

No parameters.

## reset-terminal

Resets the pane's terminal state (RIS), keeping the shell running.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## quit

Closes every window and exits.

No parameters.
