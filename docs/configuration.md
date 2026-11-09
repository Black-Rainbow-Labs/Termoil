# Configuration

Termoil reads one TOML file. Every setting has a default, so the file only needs the settings you want to change, and
an empty file or no file at all gives you the defaults.

| System | Path |
|---|---|
| Windows | `%APPDATA%\termoil\config.toml` |
| macOS | `~/Library/Application Support/termoil/config.toml` |
| Linux | `$XDG_CONFIG_HOME/termoil/config.toml`, or `~/.config/termoil/config.toml` |

`TERMOIL_CONFIG` names a different file.

## Three ways to change a setting

- **The settings view.** `Ctrl+,` (`Cmd+,` on macOS) opens every setting inside the window, with its help text and
  current value. A change is checked and written to the file when you make it.
- **The file.** Edit it in any editor. `open-config` in the command palette opens it in your configured editor.
  Termoil reads the file again a moment after you save it, so you never restart.
- **The command line.** `termoil +set-theme --name "Gruvbox Dark"` changes the theme and writes it to the file.
  `termoil +reload-config` reads the file again straight away.

A file with a mistake in it does not stop Termoil. The settings from before the change stay in effect, and a banner
across the top of the window names the line and the problem until the file is fixed. An unknown key is reported as a
warning on its line and the rest of the file still applies.

To see the settings in effect:

```sh
termoil +show-config                 # the settings in effect, as TOML
termoil +show-config --default true  # the default file, with a comment on every key
```

The default file is the full reference: every section and key, its default value and a sentence saying what it does.

## A starting file

```toml
theme = "Catppuccin Mocha"

[font]
family = ["JetBrains Mono", "Cascadia Code"]
size = 14.0
ligatures = true

[window]
columns = 140
rows = 40
padding = [8, 6]
opacity = 0.95

[cursor]
style = "bar"
blink = true

[shell]
program = "pwsh"
cwd = "~/src"

[scrollback]
limit_bytes = 100_000_000

[keybindings]
bindings = [
  { keys = "ctrl+shift+r", action = "reload-config" },
  { keys = "ctrl+a>c", action = "new-tab" },
]
```

## Themes

Termoil bundles 894 themes: the Unluminous themes and the iTerm2 colour scheme collection. `termoil +list-themes`
prints every name. The easiest way to choose one is **choose-theme** in the command palette: move through the list and
each theme is applied to the window as you reach it. Enter keeps it and Escape puts back the one you had.

```toml
theme = "Tokyo Night"                # one theme, always

[theme]                              # or a theme for each system appearance
dark = "Unluminous Dark"
light = "Unluminous Light"
```

`[colors]` overrides single colours on top of the theme: `foreground`, `background`, `cursor`,
`selection_background`, `selection_foreground`, and `palette` for the 16 ANSI colours.

`window.minimum_contrast` (4.5 by default, the WCAG level for body text) lifts any text colour that falls below that
contrast ratio against its background, so a program's dim grey on a dark theme stays readable. Set it to 1 to draw
every colour exactly as the program sent it.

## Fonts

| Key | Default | What it does |
|---|---|---|
| `font.family` | Cascadia Code, JetBrains Mono, Fira Code, Menlo, DejaVu Sans Mono | faces to try in order; the first one installed is used |
| `font.size` | 13 | size in points |
| `font.ligatures` | true | draw programming ligatures such as `=>` and `!=` |
| `font.ligatures_off` | none | families to draw without ligatures even when they are on |
| `font.line_height` | 1.0 | line height as a multiple of the font's own |
| `font.fallback` | none | extra faces to try for characters the main face does not have |
| `font.hinting` | on | fit glyphs to the pixel grid |

After `font.fallback`, Termoil asks the system for a face that has the character, so emoji and text in other
scripts appear even when no font for them is named. Box drawing, block, braille and powerline characters are drawn by
Termoil, so they join at any size whatever the font.

## The window

| Key | Default | What it does |
|---|---|---|
| `window.columns`, `window.rows` | 120, 34 | the size of a new window, in cells |
| `window.padding` | [4, 4] | space around the text, in pixels, horizontal then vertical |
| `window.opacity` | 1.0 | below 1 the window is translucent |
| `window.blur` | false | blur what is behind a translucent window, where the system supports it |
| `window.decorations` | true | draw the system's title bar and borders |
| `window.renderer` | auto | `auto` draws on the processor and starts the graphics card only for a custom shader; `gpu` uses the graphics card; `cpu` never does |
| `window.shader` | none | a WGSL file run over every frame; see [Custom shaders](shaders.md) |
| `window.quick_terminal_hotkey` | ``ctrl+` `` | a system wide key that slides a terminal in from `window.quick_terminal_edge` |

## The tab bar

The bar is hidden while a window has one tab. When a second tab opens, it appears above the terminal: the tab that is
showing is drawn in the terminal's own background and joins the terminal below it, the other tabs sit on a darker
bar, and the new tab button is at the right end. A tab shows its close cross while it is showing or under the
pointer, and a middle click closes any tab.

| Key | Default | What it does |
|---|---|---|
| `window.show_tab_bar` | true | false hides the bar even with many tabs |
| `tab_bar.show_with_one_tab` | false | true keeps the bar, and its new tab button, with one tab |
| `tab_bar.min_columns`, `tab_bar.max_columns` | 12, 26 | the narrowest and widest a tab is drawn, in columns |
| `tab_bar.bar_shade` | 0.3 | how much darker the bar is than the terminal, from 0 to 1; a light theme uses a third of it |
| `tab_bar.padding` | 8 | pixels above and below the titles |
| `tab_bar.corner_radius` | 8 | the radius of a tab's corners, in pixels |
| `tab_bar.inset` | 5 | pixels between the top of the bar and the tabs, and between neighbouring tabs |
| `tab_bar.underline` | 2 | the thickness of the accent line along the top of the tab that is showing; 0 draws none |

A tab's title is the title its program sets. Until it sets one, the tab shows the working directory when shell
integration reports it (`window.title_follows_cwd`), and otherwise the program's name, such as `pwsh`. A failed
command's exit code and a program's progress are added to the title. `termoil +set-title` gives a tab a fixed title.

## Keybindings

Your bindings are added to the defaults, and a binding for keys that already have one replaces it.

```toml
[keybindings]
bindings = [
  { keys = "ctrl+shift+t", action = "new-window" },   # replace a default
  { keys = "ctrl+shift+k", action = "none" },         # remove a default
  { keys = "ctrl+a>c", action = "new-tab" },          # a sequence: ctrl+a, then c
]

[keybindings.windows]                                  # bindings for one system only
bindings = [ { keys = "alt+left", action = "none" } ]
```

`action` is any action name from [Commands](commands.md). Keys are written as modifiers and a key joined by `+`:
`ctrl`, `shift`, `alt`, `cmd` (also `super`), then a letter, a digit, a named key such as `enter`, `tab`, `left` or
`f11`, or a symbol such as `,` or `plus`. The command palette shows the binding of every action.

A binding names the key, not the finger work. A symbol that needs Shift matches either way it is written:
on a US keyboard the plus sign is Shift and `=`, so `ctrl+plus` and `ctrl+shift+=` are both Ctrl, Shift and `=`. When
both are bound, the one written with `shift` wins. `ctrl+=` is Ctrl and `=` without Shift; the defaults bind it, and
`cmd+=` on macOS, to make the font larger, because that is the key people press for "Ctrl +" and "Cmd +".

A Cmd or Win key with no binding sends nothing to a program that has not turned on the kitty keyboard protocol or
modifyOtherKeys, because such a program cannot read it.

**Key tables** give a set of keys a meaning only while the table is active, the way a tmux prefix works. A binding with
`table = "pane"` is only looked up while `activate-key-table` has made `pane` active, and `keys = "catch_all"` in a
table takes every key the table does not bind.

## Everything else

| Section | What it holds |
|---|---|
| `[cursor]` | `style` (block, bar or underline), `blink`, `blink_interval_ms` |
| `[scrollback]` | `limit_bytes` (50 MB per pane by default), `limit_lines` |
| `[session]` | `restore` reopens your windows, tabs, splits, folders and recent output at start; off by default |
| `[shell]` | `program`, `args`, `cwd`, `env`, and `integration` for the shell integration scripts |
| `[clipboard]` | `copy_on_select`, `paste_confirm_newline`, `word_separators`, and `allow_osc52_read`, off by default |
| `[notify]` | `after_seconds` and `command_finished` for a notification when a long command ends, `on_bell` |
| `[mouse]` | `focus_follows_mouse`, `hide_while_typing`, `scroll_multiplier` |
| `[openers]` | the commands run when you open a path or a URL; `{path}`, `{line}`, `{col}` and `{url}` are filled in |
| `[accessibility]` | `enabled` exposes the screen to screen readers through UI Automation, NSAccessibility and AT-SPI |
| `[telemetry]` | `enabled` sends anonymous usage statistics (on by default; never a path, a command or anything typed), `interval_minutes` |
| `[log]` | `level` |

`termoil +show-config --default true` lists every key in these sections with its default.
