# Termoil commands

Generated from the action catalogue in `crates/termoil-protocol`; do not edit by hand.
Every action runs through `App::run_action`, whether a keybinding, the command palette, `termoil +<command>`
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
| [`close-tab`](#close-tab) | yes | yes | a pane | Closes the tab holding a pane, ending every shell in it; the last tab closes its window. |
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
| [`list-themes`](#list-themes) | yes | no | nothing | Prints every bundled theme name, the themes Unluminous ships first, and the theme in effect. |
| [`set-shader`](#set-shader) | yes | yes | nothing | Turns on a built in shader (crt, retro, neon and the rest) over the window, or turns it off with none, and writes it to the configuration file. It combines with any theme. |
| [`list-shaders`](#list-shaders) | yes | no | nothing | Prints every built in shader with what it looks like, and the shader in effect. |
| [`set-cursor-effect`](#set-cursor-effect) | yes | yes | nothing | Turns on a built in cursor effect (glow, sonar, orbit, magnify and the rest) around the cursor, or turns it off with none, and writes it to the configuration file. It combines with any shader and replaces the shader's own cursor light. |
| [`list-cursor-effects`](#list-cursor-effects) | yes | no | nothing | Prints every built in cursor effect with what it looks like, and the cursor effect in effect. |
| [`choose-theme`](#choose-theme) | no | yes | a window | Lists the bundled themes and, beside them, the built in shaders and cursor effects, previewing each as it is highlighted; Enter keeps all three. |
| [`open-settings`](#open-settings) | no | yes | a window | Opens the settings view: every setting with its help text and value, edited in place and written to the configuration file. |
| [`open-config`](#open-config) | no | yes | nothing | Opens the configuration file in the configured editor. |
| [`activate-key-table`](#activate-key-table) | yes | no | nothing | Makes a key table active: keys are looked up in its bindings first until it is deactivated. |
| [`deactivate-key-table`](#deactivate-key-table) | yes | no | nothing | Deactivates the key table that was activated last. |
| [`deactivate-all-key-tables`](#deactivate-all-key-tables) | yes | no | nothing | Deactivates every active key table. |
| [`install-shell-integration`](#install-shell-integration) | yes | yes | nothing | Installs the shell integration scripts and hooks them into the named shells' startup files. |
| [`version`](#version) | yes | yes | nothing | Prints the version and build date. |
| [`check-for-updates`](#check-for-updates) | yes | yes | nothing | Asks the release channel for the newest version and says whether it is newer than this one. |
| [`list-updates`](#list-updates) | yes | yes | nothing | Lists the newest five versions on the stable and nightly channels, marking the one running. |
| [`install-update`](#install-update) | yes | yes | nothing | Downloads a version, checks its size and SHA-256, then quits, installs it and reopens the same windows. |
| [`skip-update`](#skip-update) | yes | yes | nothing | Stops the automatic check announcing a version, by writing it to updates.skip. |
| [`telemetry-status`](#telemetry-status) | yes | yes | nothing | Says whether anonymous usage statistics are on, why not when they are off, and what has been sent. |
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
| [`save-session`](#save-session) | yes | yes | nothing | Writes the open windows, tabs, splits, working directories and recent output to the session file now. |
| [`quit`](#quit) | no | yes | nothing | Closes every window and exits. |

## new-window

Opens a new window with one tab.

```
termoil +new-window [--cwd <path>] [--command <text>] [--separate <true|false>]
```

Over IPC: `{"id":1,"action":"new-window","args":{...}}`. MCP tool: `termoil_new_window`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cwd` | path | no | Working directory for the shell. |
| `command` | text | no | Program to run instead of the configured shell. |
| `separate` | true|false | no | Start the window in a process of its own, with its own socket; the answer names the socket. |

## new-tab

Opens a new tab in the focused window, in the given directory or the focused pane's.

```
termoil +new-tab [--cwd <path>] [--name <text>] [--command <text>]
```

Over IPC: `{"id":1,"action":"new-tab","args":{...}}`. MCP tool: `termoil_new_tab`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cwd` | path | no | Working directory for the shell. |
| `name` | text | no | Title for the tab. |
| `command` | text | no | Program to run instead of the configured shell. |

## split

Splits the pane, starting a new shell in the new half.

```
termoil +split [--pane <pane id>] [--direction <left|right|up|down>] [--cwd <path>]
```

Over IPC: `{"id":1,"action":"split","args":{...}}`. MCP tool: `termoil_split`.

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
termoil +close [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"close","args":{...}}`. MCP tool: `termoil_close`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## focus

Focuses a pane, or the neighbouring pane in a direction.

```
termoil +focus [--pane <pane id>] [--direction <left|right|up|down>]
```

Over IPC: `{"id":1,"action":"focus","args":{...}}`. MCP tool: `termoil_focus`.

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

## close-tab

Closes the tab holding a pane, ending every shell in it; the last tab closes its window.

```
termoil +close-tab [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"close-tab","args":{...}}`. MCP tool: `termoil_close_tab`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## next-tab

Switches to the next tab.

No parameters.

## move-tab-to-window

Moves the tab holding a pane into a new window of its own.

```
termoil +move-tab-to-window [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"move-tab-to-window","args":{...}}`. MCP tool: `termoil_move_tab_to_window`.

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
termoil +resize-split [--pane <pane id>] --direction <left|right|up|down> [--percent <integer>]
```

Over IPC: `{"id":1,"action":"resize-split","args":{...}}`. MCP tool: `termoil_resize_split`.

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
termoil +send-text [--pane <pane id>] --text <text>
```

Over IPC: `{"id":1,"action":"send-text","args":{...}}`. MCP tool: `termoil_send_text`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `text` | text | yes | The text; \n is Enter. |

## send-key

Presses a key in a pane, encoded for the program's keyboard mode.

```
termoil +send-key [--pane <pane id>] --key <key>
```

Over IPC: `{"id":1,"action":"send-key","args":{...}}`. MCP tool: `termoil_send_key`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `key` | key | yes | A chord such as enter, ctrl+c or shift+tab. |

## screen

Returns the visible screen of a pane as text, or as JSON cells with styles.

```
termoil +screen [--pane <pane id>] [--json <true|false>]
```

Over IPC: `{"id":1,"action":"screen","args":{...}}`. MCP tool: `termoil_screen`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `json` | true|false | no | Return cells with styles instead of text. |

## scrollback

Returns the last lines of a pane's scrollback and screen as text.

```
termoil +scrollback [--pane <pane id>] [--lines <integer>]
```

Over IPC: `{"id":1,"action":"scrollback","args":{...}}`. MCP tool: `termoil_scrollback`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `lines` | integer | no | How many lines; 500 when omitted. |

## wait-prompt

Waits until the command running in a pane finishes and returns its exit code.

```
termoil +wait-prompt [--pane <pane id>] [--timeout <integer>]
```

Over IPC: `{"id":1,"action":"wait-prompt","args":{...}}`. MCP tool: `termoil_wait_prompt`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `timeout` | integer | no | Seconds to wait; 600 when omitted. |

## list

Lists windows, tabs and panes with their ids, titles and working directories.

```
termoil +list
```

Over IPC: `{"id":1,"action":"list","args":{...}}`. MCP tool: `termoil_list`.

No parameters.

## resize

Resizes a pane's window to a number of columns and rows.

```
termoil +resize [--pane <pane id>] --cols <integer> --rows <integer>
```

Over IPC: `{"id":1,"action":"resize","args":{...}}`. MCP tool: `termoil_resize`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `cols` | integer | yes | Columns. |
| `rows` | integer | yes | Rows. |

## set-title

Sets the title of a pane's tab.

```
termoil +set-title [--pane <pane id>] --title <text>
```

Over IPC: `{"id":1,"action":"set-title","args":{...}}`. MCP tool: `termoil_set_title`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |
| `title` | text | yes | The title. |

## run-action

Runs any action by name with JSON arguments, the way a keybinding does.

```
termoil +run-action --name <action> [--args <text>]
```

Over IPC: `{"id":1,"action":"run-action","args":{...}}`. MCP tool: `termoil_run_action`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | action | yes | The action. |
| `args` | text | no | Arguments as a JSON object. |

## show-config

Prints the effective configuration, or the documented default file.

```
termoil +show-config [--default <true|false>]
```

Over IPC: `{"id":1,"action":"show-config","args":{...}}`. MCP tool: `termoil_show_config`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `default` | true|false | no | Print the default file instead. |

## reload-config

Reads the configuration file again and applies it.

```
termoil +reload-config
```

Over IPC: `{"id":1,"action":"reload-config","args":{...}}`. MCP tool: `termoil_reload_config`.

No parameters.

## set-theme

Switches to a bundled colour theme and writes it to the configuration file.

```
termoil +set-theme --name <text>
```

Over IPC: `{"id":1,"action":"set-theme","args":{...}}`. MCP tool: `termoil_set_theme`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | text | yes | The theme's name, as `list-themes` prints it. |

## list-themes

Prints every bundled theme name, the themes Unluminous ships first, and the theme in effect.

```
termoil +list-themes
```

Over IPC: `{"id":1,"action":"list-themes","args":{...}}`. MCP tool: `termoil_list_themes`.

No parameters.

## set-shader

Turns on a built in shader (crt, retro, neon and the rest) over the window, or turns it off with none, and writes it to the configuration file. It combines with any theme.

```
termoil +set-shader --name <text>
```

Over IPC: `{"id":1,"action":"set-shader","args":{...}}`. MCP tool: `termoil_set_shader`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | text | yes | The shader's name, as `list-shaders` prints it, or none. |

## list-shaders

Prints every built in shader with what it looks like, and the shader in effect.

```
termoil +list-shaders
```

Over IPC: `{"id":1,"action":"list-shaders","args":{...}}`. MCP tool: `termoil_list_shaders`.

No parameters.

## set-cursor-effect

Turns on a built in cursor effect (glow, sonar, orbit, magnify and the rest) around the cursor, or turns it off with none, and writes it to the configuration file. It combines with any shader and replaces the shader's own cursor light.

```
termoil +set-cursor-effect --name <text>
```

Over IPC: `{"id":1,"action":"set-cursor-effect","args":{...}}`. MCP tool: `termoil_set_cursor_effect`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | text | yes | The cursor effect's name, as `list-cursor-effects` prints it, or none. |

## list-cursor-effects

Prints every built in cursor effect with what it looks like, and the cursor effect in effect.

```
termoil +list-cursor-effects
```

Over IPC: `{"id":1,"action":"list-cursor-effects","args":{...}}`. MCP tool: `termoil_list_cursor_effects`.

No parameters.

## choose-theme

Lists the bundled themes and, beside them, the built in shaders and cursor effects, previewing each as it is highlighted; Enter keeps all three.

No parameters.

## open-settings

Opens the settings view: every setting with its help text and value, edited in place and written to the configuration file.

No parameters.

## open-config

Opens the configuration file in the configured editor.

No parameters.

## activate-key-table

Makes a key table active: keys are looked up in its bindings first until it is deactivated.

```
termoil +activate-key-table --name <text> [--once <true|false>]
```

Over IPC: `{"id":1,"action":"activate-key-table","args":{...}}`. MCP tool: `termoil_activate_key_table`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | text | yes | The key table, as the `table` field of the keybindings names it. |
| `once` | true|false | no | Deactivate the table after the next key. |

## deactivate-key-table

Deactivates the key table that was activated last.

```
termoil +deactivate-key-table
```

Over IPC: `{"id":1,"action":"deactivate-key-table","args":{...}}`. MCP tool: `termoil_deactivate_key_table`.

No parameters.

## deactivate-all-key-tables

Deactivates every active key table.

```
termoil +deactivate-all-key-tables
```

Over IPC: `{"id":1,"action":"deactivate-all-key-tables","args":{...}}`. MCP tool: `termoil_deactivate_all_key_tables`.

No parameters.

## install-shell-integration

Installs the shell integration scripts and hooks them into the named shells' startup files.

```
termoil +install-shell-integration [--shells <text>] [--dry-run <true|false>]
```

Over IPC: `{"id":1,"action":"install-shell-integration","args":{...}}`. MCP tool: `termoil_install_shell_integration`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `shells` | text | no | Comma separated: powershell, pwsh, bash, zsh, fish, nushell, cmd; the current shell when omitted. |
| `dry-run` | true|false | no | Show what would change without changing it. |

## version

Prints the version and build date.

```
termoil +version
```

Over IPC: `{"id":1,"action":"version","args":{...}}`. MCP tool: `termoil_version`.

No parameters.

## check-for-updates

Asks the release channel for the newest version and says whether it is newer than this one.

```
termoil +check-for-updates [--channel <stable|nightly>]
```

Over IPC: `{"id":1,"action":"check-for-updates","args":{...}}`. MCP tool: `termoil_check_for_updates`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `channel` | stable|nightly | no | The release channel, stable or nightly; updates.channel when omitted. |

## list-updates

Lists the newest five versions on the stable and nightly channels, marking the one running.

```
termoil +list-updates [--channel <stable|nightly>]
```

Over IPC: `{"id":1,"action":"list-updates","args":{...}}`. MCP tool: `termoil_list_updates`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `channel` | stable|nightly | no | List only this channel; both when omitted. |

## install-update

Downloads a version, checks its size and SHA-256, then quits, installs it and reopens the same windows.

```
termoil +install-update [--version <text>] [--channel <stable|nightly>] [--restart <true|false>]
```

Over IPC: `{"id":1,"action":"install-update","args":{...}}`. MCP tool: `termoil_install_update`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `version` | text | no | The version to install, newer or older; the newest on the channel when omitted. |
| `channel` | stable|nightly | no | The release channel, stable or nightly; updates.channel when omitted. |
| `restart` | true|false | no | Quit, install and start the new version; true when omitted. false only downloads and checks the file. |

## skip-update

Stops the automatic check announcing a version, by writing it to updates.skip.

```
termoil +skip-update [--version <text>]
```

Over IPC: `{"id":1,"action":"skip-update","args":{...}}`. MCP tool: `termoil_skip_update`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `version` | text | no | The version to skip; the one the last check found when omitted. |

## telemetry-status

Says whether anonymous usage statistics are on, why not when they are off, and what has been sent.

```
termoil +telemetry-status [--flush <true|false>]
```

Over IPC: `{"id":1,"action":"telemetry-status","args":{...}}`. MCP tool: `termoil_telemetry_status`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `flush` | true|false | no | Send what is waiting now instead of at the next interval. |

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
termoil +clear-scrollback [--pane <pane id>]
```

Over IPC: `{"id":1,"action":"clear-scrollback","args":{...}}`. MCP tool: `termoil_clear_scrollback`.

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
termoil +search [--pane <pane id>] [--pattern <text>] [--case-sensitive <true|false>]
```

Over IPC: `{"id":1,"action":"search","args":{...}}`. MCP tool: `termoil_search`.

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
termoil +quick-terminal
```

Over IPC: `{"id":1,"action":"quick-terminal","args":{...}}`. MCP tool: `termoil_quick_terminal`.

No parameters.

## reset-terminal

Resets the pane's terminal state (RIS), keeping the shell running.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `pane` | pane id | no | The pane to act on; the focused pane when omitted. |

## save-session

Writes the open windows, tabs, splits, working directories and recent output to the session file now.

```
termoil +save-session [--path <path>]
```

Over IPC: `{"id":1,"action":"save-session","args":{...}}`. MCP tool: `termoil_save_session`.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `path` | path | no | File to write; the session file in the data folder when omitted. |

## quit

Closes every window and exits.

No parameters.
