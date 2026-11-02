//! The action catalogue: one row per action, with its parameters and where it applies.

/// What an action needs to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Runs without a window (version, show-config, install-shell-integration, list, new-window).
    Global,
    /// Needs a window (tabs, fullscreen, the palette).
    Window,
    /// Needs a pane (send-text, screen, copy, scrolling, search).
    Pane,
}

/// The type of a parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// Free text.
    Text,
    /// An integer.
    Int,
    /// true or false.
    Bool,
    /// A pane id from `list` or `new-tab` (defaults to the focused pane).
    Pane,
    /// A file system path.
    Path,
    /// A key chord such as `ctrl+c`.
    Key,
    /// An action name from this catalogue.
    Action,
    /// One of a fixed set of words.
    Choice(&'static [&'static str]),
}

/// One parameter of an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamDef {
    /// The name, as `--name` on the command line and a JSON key over IPC.
    pub name: &'static str,
    /// The type.
    pub kind: ParamKind,
    /// Must be given.
    pub required: bool,
    /// What it means.
    pub help: &'static str,
}

/// One action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionDef {
    /// The name, kebab case: a keybinding's action, `terminus +<name>`, and the MCP tool name with
    /// `terminus_` in front.
    pub name: &'static str,
    /// One sentence saying what it does.
    pub summary: &'static str,
    /// Parameters.
    pub params: &'static [ParamDef],
    /// What it needs to run.
    pub context: Context,
    /// Offered as a `terminus +<name>` command and an MCP tool.
    pub cli: bool,
    /// Shown in the command palette.
    pub palette: bool,
}

/// Shorthand for a parameter row.
const fn p(name: &'static str, kind: ParamKind, required: bool, help: &'static str) -> ParamDef {
    ParamDef { name, kind, required, help }
}

/// Shorthand for an action row.
const fn a(
    name: &'static str, summary: &'static str, params: &'static [ParamDef], context: Context, cli: bool, palette: bool,
) -> ActionDef {
    ActionDef { name, summary, params, context, cli, palette }
}

use Context::{Global as G, Pane as P, Window as W};
use ParamKind::*;

const PANE: ParamDef = p("pane", Pane, false, "The pane to act on; the focused pane when omitted.");
const DIRS: &[&str] = &["left", "right", "up", "down"];

/// Every action, in the order the command palette and `docs/commands.md` list them.
pub const CATALOGUE: &[ActionDef] = &[
    a(
        "new-window",
        "Opens a new window with one tab.",
        &[
            p("cwd", Path, false, "Working directory for the shell."),
            p("command", Text, false, "Program to run instead of the configured shell."),
        ],
        G,
        true,
        true,
    ),
    a(
        "new-tab",
        "Opens a new tab in the focused window, in the given directory or the focused pane's.",
        &[
            p("cwd", Path, false, "Working directory for the shell."),
            p("name", Text, false, "Title for the tab."),
            p("command", Text, false, "Program to run instead of the configured shell."),
        ],
        W,
        true,
        true,
    ),
    a(
        "split",
        "Splits the pane, starting a new shell in the new half.",
        &[
            PANE,
            p("direction", Choice(DIRS), false, "Which side the new pane goes on; right when omitted."),
            p("cwd", Path, false, "Working directory for the shell."),
        ],
        P,
        true,
        true,
    ),
    a("split-right", "Splits the focused pane with the new pane on the right.", &[], P, false, true),
    a("split-down", "Splits the focused pane with the new pane below.", &[], P, false, true),
    a("close", "Closes a pane, ending its shell; the last pane closes its tab and window.", &[PANE], P, true, true),
    a(
        "focus",
        "Focuses a pane, or the neighbouring pane in a direction.",
        &[PANE, p("direction", Choice(DIRS), false, "Move focus to the neighbour on this side.")],
        W,
        true,
        false,
    ),
    a("focus-left", "Moves focus to the pane on the left.", &[], W, false, true),
    a("focus-right", "Moves focus to the pane on the right.", &[], W, false, true),
    a("focus-up", "Moves focus to the pane above.", &[], W, false, true),
    a("focus-down", "Moves focus to the pane below.", &[], W, false, true),
    a("next-tab", "Switches to the next tab.", &[], W, false, true),
    a("move-tab-to-window", "Moves the tab holding a pane into a new window of its own.", &[PANE], P, true, true),
    a("previous-tab", "Switches to the previous tab.", &[], W, false, true),
    a("zoom-pane", "Shows the focused pane alone in its tab, or restores the split.", &[], P, false, true),
    a(
        "resize-split",
        "Moves the divider next to a pane in a direction, growing or shrinking the pane.",
        &[
            PANE,
            p("direction", Choice(DIRS), true, "Which way the divider moves."),
            p("percent", Int, false, "How far, as a percent of the split; 5 when omitted."),
        ],
        P,
        true,
        true,
    ),
    a(
        "show-hints",
        "Labels every link on the screen of a pane; typing a label opens that link.",
        &[PANE],
        P,
        false,
        true,
    ),
    a(
        "send-text",
        "Types text into a pane's program, as if pasted without bracketing.",
        &[PANE, p("text", Text, true, "The text; \\n is Enter.")],
        P,
        true,
        false,
    ),
    a(
        "send-key",
        "Presses a key in a pane, encoded for the program's keyboard mode.",
        &[PANE, p("key", Key, true, "A chord such as enter, ctrl+c or shift+tab.")],
        P,
        true,
        false,
    ),
    a(
        "screen",
        "Returns the visible screen of a pane as text, or as JSON cells with styles.",
        &[PANE, p("json", Bool, false, "Return cells with styles instead of text.")],
        P,
        true,
        false,
    ),
    a(
        "scrollback",
        "Returns the last lines of a pane's scrollback and screen as text.",
        &[PANE, p("lines", Int, false, "How many lines; 500 when omitted.")],
        P,
        true,
        false,
    ),
    a(
        "wait-prompt",
        "Waits until the command running in a pane finishes and returns its exit code.",
        &[PANE, p("timeout", Int, false, "Seconds to wait; 600 when omitted.")],
        P,
        true,
        false,
    ),
    a("list", "Lists windows, tabs and panes with their ids, titles and working directories.", &[], G, true, false),
    a(
        "resize",
        "Resizes a pane's window to a number of columns and rows.",
        &[PANE, p("cols", Int, true, "Columns."), p("rows", Int, true, "Rows.")],
        P,
        true,
        false,
    ),
    a("set-title", "Sets the title of a pane's tab.", &[PANE, p("title", Text, true, "The title.")], P, true, true),
    a(
        "run-action",
        "Runs any action by name with JSON arguments, the way a keybinding does.",
        &[p("name", Action, true, "The action."), p("args", Text, false, "Arguments as a JSON object.")],
        G,
        true,
        false,
    ),
    a(
        "show-config",
        "Prints the effective configuration, or the documented default file.",
        &[p("default", Bool, false, "Print the default file instead.")],
        G,
        true,
        false,
    ),
    a("reload-config", "Reads the configuration file again and applies it.", &[], G, true, true),
    a(
        "set-theme",
        "Switches to a bundled colour theme and writes it to the configuration file.",
        &[p("name", Text, true, "The theme's name, as `choose-theme` lists it.")],
        G,
        true,
        true,
    ),
    a(
        "choose-theme",
        "Lists the bundled themes, previewing each as it is highlighted; Enter keeps one.",
        &[],
        W,
        false,
        true,
    ),
    a("open-config", "Opens the configuration file in the configured editor.", &[], G, false, true),
    a(
        "install-shell-integration",
        "Installs the shell integration scripts and hooks them into the named shells' startup files.",
        &[
            p(
                "shells",
                Text,
                false,
                "Comma separated: powershell, pwsh, bash, zsh, fish, nushell, cmd; the current shell when omitted.",
            ),
            p("dry-run", Bool, false, "Show what would change without changing it."),
        ],
        G,
        true,
        true,
    ),
    a("version", "Prints the version and build date.", &[], G, true, true),
    a("copy", "Copies the selection to the clipboard.", &[PANE], P, false, true),
    a("paste", "Pastes the clipboard into the pane, bracketed when the program asked for it.", &[PANE], P, false, true),
    a("select-all", "Selects the whole screen and scrollback.", &[PANE], P, false, true),
    a("clear-selection", "Clears the selection.", &[PANE], P, false, false),
    a("scroll-page-up", "Scrolls the view up one page.", &[PANE], P, false, true),
    a("scroll-page-down", "Scrolls the view down one page.", &[PANE], P, false, true),
    a("scroll-line-up", "Scrolls the view up one line.", &[PANE], P, false, false),
    a("scroll-line-down", "Scrolls the view down one line.", &[PANE], P, false, false),
    a("scroll-to-top", "Scrolls the view to the oldest line.", &[PANE], P, false, true),
    a("scroll-to-bottom", "Scrolls the view back to the live screen.", &[PANE], P, false, true),
    a("clear-scrollback", "Erases the scrollback of the pane, keeping the screen.", &[PANE], P, true, true),
    a("previous-prompt", "Scrolls to the previous shell prompt.", &[PANE], P, false, true),
    a("next-prompt", "Scrolls to the next shell prompt.", &[PANE], P, false, true),
    a("select-last-output", "Selects the output of the last command that finished.", &[PANE], P, false, true),
    a(
        "search",
        "Opens the search bar, or searches for a pattern and selects the first match.",
        &[
            PANE,
            p("pattern", Text, false, "A regular expression."),
            p("case-sensitive", Bool, false, "Match case exactly."),
        ],
        P,
        true,
        true,
    ),
    a("search-next", "Selects the next search match.", &[PANE], P, false, true),
    a("search-previous", "Selects the previous search match.", &[PANE], P, false, true),
    a("increase-font-size", "Makes the font one point larger.", &[], W, false, true),
    a("decrease-font-size", "Makes the font one point smaller.", &[], W, false, true),
    a("reset-font-size", "Restores the configured font size.", &[], W, false, true),
    a("toggle-fullscreen", "Enters or leaves full screen.", &[], W, false, true),
    a("command-palette", "Opens the command palette, which lists every action with its binding.", &[], W, false, false),
    a("quick-terminal", "Shows or hides the quick terminal that slides in from a screen edge.", &[], G, true, true),
    a("reset-terminal", "Resets the pane's terminal state (RIS), keeping the shell running.", &[PANE], P, false, true),
    a("quit", "Closes every window and exits.", &[], G, false, true),
];

/// Finds a row by name.
pub fn action(name: &str) -> Option<&'static ActionDef> {
    CATALOGUE.iter().find(|a| a.name == name)
}

impl ParamKind {
    /// The JSON schema type for the MCP tool list.
    pub fn json_type(self) -> &'static str {
        match self {
            ParamKind::Int | ParamKind::Pane => "integer",
            ParamKind::Bool => "boolean",
            _ => "string",
        }
    }
}
