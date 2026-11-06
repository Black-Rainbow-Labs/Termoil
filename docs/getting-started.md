# Getting started

This page shows how to install Termoil, what the first window does, the keys worth learning first, and how to turn
on shell integration. Terms such as PTY, OSC and shell integration are explained in the [glossary](glossary.md).

## Install

Every release is on the [releases page](https://github.com/Black-Rainbow-Labs/Termoil/releases).

### Windows

Download `TermoilSetup-<version>-x64.exe` and run it. It installs for your user into
`%LOCALAPPDATA%\Programs\Termoil` with no administrator rights, adds a Start menu entry, and puts the folder on your
user `PATH` so `termoil` and `termoil-gui` work from any shell. Running the installer of a newer version upgrades in
place. Windows 10 and Windows 11 on x64 are supported.

The install folder holds `conpty.dll` and `OpenConsole.exe`, Microsoft's console host. Termoil uses this copy instead
of the one built into Windows, because the built in one removes image sequences (sixel and kitty graphics) before they
reach the terminal. [The ConPTY decision](decisions/conpty.md) has the measurements.

To remove Termoil, use **Settings > Apps > Installed apps**, or run `unins000.exe` in the install folder. Your
settings file is left in place.

### Linux

Two packages are published for x86_64:

| Package | How to install |
|---|---|
| `termoil-<version>-x86_64.AppImage` | `chmod +x termoil-<version>-x86_64.AppImage` and run it. Nothing is installed |
| `termoil_<version>_amd64.deb` | `sudo apt install ./termoil_<version>_amd64.deb` on Debian, Ubuntu and their derivatives |

Both work on X11 and Wayland. The packages have been tested under WSL and not yet in a full desktop session.

### macOS

Download `termoil-<version>-macos-universal.dmg`, open it and drag Termoil to Applications. The app runs natively on
Apple silicon and on Intel. It is signed with an Apple Developer ID and notarised by Apple, so Gatekeeper opens it
without a warning. The macOS build is cross compiled on Windows and has not yet been run on a Mac.

### From source

Install Rust (the version is pinned in `rust-toolchain.toml`, and `rustup` picks it up), then:

```sh
git clone https://github.com/Black-Rainbow-Labs/Termoil.git
cd Termoil
cargo build --release -p termoil-app -p termoil-cli
```

The two programs are `termoil-gui` (the window) and `termoil` (the command line) in `target/release`.
[Repository and building](repository.md) covers the tests and the installers.

## The two programs

| Program | What it does |
|---|---|
| `termoil-gui` | the terminal window. A second `termoil-gui` hands its window to the Termoil that is already running, so every window lives in one process |
| `termoil` | the command line. `termoil +<command>` runs an action in the running Termoil, `termoil ssh` connects to a remote host with Termoil's terminfo entry, and `termoil +mcp` serves the actions to an AI agent |

## The first window

Termoil opens one window with one tab running your shell:

- On Windows, PowerShell 7 (`pwsh.exe`) when it is on your `PATH`, otherwise Windows PowerShell.
- On macOS and Linux, the shell in `$SHELL`, started as a login shell on macOS.

The window is 120 columns by 34 rows in the theme Unluminous Dark. The font is the first of Cascadia Code, JetBrains
Mono, Fira Code, Menlo and DejaVu Sans Mono that is installed. With one tab there is no tab bar. Open a second tab and
the bar appears, with the new tab button at its right end.

To use another shell, set it in the settings file:

```toml
[shell]
program = "nu.exe"
args = []
```

## Keys to learn first

| Keys on Windows and Linux | Keys on macOS | Action |
|---|---|---|
| `Ctrl+Shift+T` | `Cmd+T` | open a tab |
| `Ctrl+Tab`, `Ctrl+Shift+Tab` | the same | next tab, previous tab |
| `Ctrl+Shift+N` | `Cmd+N` | open a window |
| `Ctrl+Shift+D` | `Cmd+D` | split, new pane on the right |
| `Ctrl+Shift+E` | `Cmd+Shift+D` | split, new pane below |
| `Alt+Arrow` | `Cmd+Alt+Arrow` | move to the pane in that direction |
| `Ctrl+Shift+W` | `Cmd+W` | close the pane |
| `Ctrl+Shift+C`, `Ctrl+Shift+V` | `Cmd+C`, `Cmd+V` | copy, paste |
| `Ctrl+Shift+F` | `Cmd+F` | search the scrollback |
| `Ctrl+Shift+H` | `Cmd+Shift+H` | label every link on screen, then type a label to open it |
| `Ctrl+Shift+Up`, `Ctrl+Shift+Down` | `Cmd+Up`, `Cmd+Down` | previous prompt, next prompt |
| `Ctrl+Shift+Space` | `Cmd+Shift+Space` | select the output of the last command |
| `Ctrl+Shift+P` | `Cmd+Shift+P` | the command palette |
| `Ctrl+,` | `Cmd+,` | the settings view |

The command palette lists every action with its key, so it is the place to look for anything not in this table.
[Configuration](configuration.md#keybindings) shows how to change a key.

The mouse works the way it does in other terminals. A double click selects a word, a triple click a line, and a
fourth click the whole output of one command. Alt and drag selects a block. Ctrl and click opens a link or a file
path. A middle click on a tab closes it.

## Shell integration

Shell integration is a few lines in your shell's startup file that mark where each prompt, command and output begins
(OSC 133) and report the working directory (OSC 7). Termoil uses the marks to:

- jump to the previous or next prompt,
- select the whole output of a command,
- show a failed command's exit code in its tab title,
- open new tabs and splits in the same folder as the pane you are in,
- tell `termoil +wait-prompt` when a command has finished,
- send a notification when a long command finishes while the window is in the background.

It is on by default. When Termoil starts PowerShell, pwsh, bash, zsh, fish, nushell or cmd, it loads the matching
script from `shell-integration/` into that shell, and your startup files are not touched. `shell.integration = false`
turns this off.

A shell that Termoil did not start itself, such as one you start from another shell, only has the marks if its
startup file loads the script. To add that line:

```sh
termoil +install-shell-integration --shells pwsh,bash
```

It copies the scripts into Termoil's data folder and adds one line to each named shell's startup file: the PowerShell
profile, `.bashrc`, `.zshrc`, fish's `config.fish` or nushell's `config.nu`. Without `--shells` it sets up the shell
you run it from. Run it again and it changes nothing, and `--dry-run true` shows what it would change. cmd has no
startup file, so it only gets the marks when Termoil starts it.

## Connecting to other machines

`termoil ssh <host>` runs `ssh` with Termoil's own terminfo entry, `xterm-termoil`. The first time it connects to a
host it copies the entry there, so programs on the remote host know every feature Termoil has. When the copy fails it
connects with `TERM=xterm-256color` instead.

## Settings

The settings file is TOML:

| System | Path |
|---|---|
| Windows | `%APPDATA%\termoil\config.toml` |
| macOS | `~/Library/Application Support/termoil/config.toml` |
| Linux | `$XDG_CONFIG_HOME/termoil/config.toml`, or `~/.config/termoil/config.toml` |

Termoil applies a change to the file as soon as you save it. `Ctrl+,` opens a settings view inside the window that
shows every setting with its help text and writes your changes to the file. [Configuration](configuration.md) covers
the settings people change most.

## When something goes wrong

- A settings file with an error leaves the previous settings in effect and shows a banner at the top of the window
  with the line and the problem.
- `termoil +version` prints the version and build date of the running Termoil.
- A crash writes a report into `%LOCALAPPDATA%\termoil` on Windows, `~/Library/Application Support/termoil` on
  macOS, or `~/.local/share/termoil` on Linux. The report itself is never sent; the usage statistics count only that a crash happened.
- Anonymous usage statistics: which actions you use and how long Termoil is open, never a path, a command, a
  title or anything you type. Turn them off with `enabled = false` under `[telemetry]`, or in the settings view.
