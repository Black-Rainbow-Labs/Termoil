# Termoil shell integration for nushell.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Termoil started this
# shell and set TERMOIL_SHELL_INTEGRATION=1. Loading it twice installs the hooks once.
# Untested on this machine: nushell was not installed when this was written.

if (($env.TERMOIL_SHELL_INTEGRATION? | default "") == "1") and (($env.__TERMOIL_NU_LOADED? | default "") != "1") {
    $env.__TERMOIL_NU_LOADED = "1"
    if ($env.TERM_PROGRAM? | is-empty) { $env.TERM_PROGRAM = "termoil" }
    $env.__TERMOIL_CMD_RAN = ""
    $env.__TERMOIL_LAST_PWD = ""

    # Nushell can emit OSC 133, OSC 7, OSC 9;9 (the directory as a bare path, on Windows) and
    # OSC 633 itself. Turn them off so the marks and the directory are not sent twice.
    try {
        $env.config = ($env.config
            | upsert shell_integration.osc133 false
            | upsert shell_integration.osc7 false
            | upsert shell_integration.osc9_9 false
            | upsert shell_integration.osc633 false)
    }

    # Before each prompt: D for the command that ended, OSC 7 when the directory changed, then A.
    let pre_prompt = {||
        let esc = (char -u '1b')
        let bel = (char bel)
        mut out = ""
        if ($env.__TERMOIL_CMD_RAN? | default "") == "1" {
            $out = $"($out)($esc)]133;D;($env.LAST_EXIT_CODE? | default 0)($bel)"
            $env.__TERMOIL_CMD_RAN = ""
        }
        if $env.PWD != ($env.__TERMOIL_LAST_PWD? | default "") {
            $env.__TERMOIL_LAST_PWD = $env.PWD
            let dir = ($env.PWD | str replace --all '\' '/')
            let dir = if ($dir | str starts-with '/') { $dir } else { $"/($dir)" }
            let host = (sys host | get hostname)
            let encoded = ($dir | url encode --all | str replace --all '%2F' '/' | str replace --all '%3A' ':' | str replace --all '%2D' '-' | str replace --all '%2E' '.' | str replace --all '%5F' '_' | str replace --all '%7E' '~')
            $out = $"($out)($esc)]7;file://($host)($encoded)($bel)"
        }
        print -n $"($out)($esc)]133;A($bel)"
    }

    # Before a command runs: OSC 133;C.
    let pre_execution = {||
        $env.__TERMOIL_CMD_RAN = "1"
        print -n $"(char -u '1b')]133;C(char bel)"
    }

    # B marks where input starts: it is added to the end of the prompt indicator.
    let old_indicator = ($env.PROMPT_INDICATOR? | default "> ")
    $env.PROMPT_INDICATOR = {||
        let base = if ($old_indicator | describe) == "closure" { do $old_indicator } else { $old_indicator }
        $"($base)(char -u '1b')]133;B(char bel)"
    }

    let hooks = ($env.config.hooks? | default {})
    let prompt_hooks = ($hooks.pre_prompt? | default [] | append $pre_prompt)
    let exec_hooks = ($hooks.pre_execution? | default [] | append $pre_execution)
    $env.config = ($env.config | upsert hooks.pre_prompt $prompt_hooks | upsert hooks.pre_execution $exec_hooks)
}

# A remote host has no xterm-termoil entry, so ssh gets TERM=xterm-256color, which every host
# knows. A command defined inside the block above would not be visible after it, so this one is
# defined here; it runs ssh unchanged unless TERM is xterm-termoil.
def --wrapped ssh [...rest] {
    if ($env.TERM? | default "") == "xterm-termoil" {
        with-env { TERM: "xterm-256color" } { ^ssh ...$rest }
    } else {
        ^ssh ...$rest
    }
}
