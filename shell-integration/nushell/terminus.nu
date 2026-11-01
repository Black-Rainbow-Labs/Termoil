# Terminus shell integration for nushell.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Terminus started this
# shell and set TERMINUS_SHELL_INTEGRATION=1. Loading it twice installs the hooks once.
# Untested on this machine: nushell was not installed when this was written.

if (($env.TERMINUS_SHELL_INTEGRATION? | default "") == "1") and (($env.__TERMINUS_NU_LOADED? | default "") != "1") {
    $env.__TERMINUS_NU_LOADED = "1"
    if ($env.TERM_PROGRAM? | is-empty) { $env.TERM_PROGRAM = "terminus" }
    $env.__TERMINUS_CMD_RAN = ""
    $env.__TERMINUS_LAST_PWD = ""

    # Nushell can emit OSC 133 and OSC 7 itself. Turn that off so the marks are not sent twice.
    try { $env.config = ($env.config | upsert shell_integration.osc133 false | upsert shell_integration.osc7 false) }

    # Before each prompt: D for the command that ended, OSC 7 when the directory changed, then A.
    let pre_prompt = {||
        let esc = (char escape)
        let bel = (char bel)
        mut out = ""
        if ($env.__TERMINUS_CMD_RAN? | default "") == "1" {
            $out = $"($out)($esc)]133;D;($env.LAST_EXIT_CODE? | default 0)($bel)"
            $env.__TERMINUS_CMD_RAN = ""
        }
        if $env.PWD != ($env.__TERMINUS_LAST_PWD? | default "") {
            $env.__TERMINUS_LAST_PWD = $env.PWD
            let dir = ($env.PWD | str replace --all '\' '/')
            let dir = if ($dir | str starts-with '/') { $dir } else { $"/($dir)" }
            let host = (sys host | get hostname)
            let encoded = ($dir | url encode --all | str replace --all '%2F' '/' | str replace --all '%3A' ':')
            $out = $"($out)($esc)]7;file://($host)($encoded)($bel)"
        }
        print -n $"($out)($esc)]133;A($bel)"
    }

    # Before a command runs: OSC 133;C.
    let pre_execution = {||
        $env.__TERMINUS_CMD_RAN = "1"
        print -n $"(char escape)]133;C(char bel)"
    }

    # B marks where input starts: it is added to the end of the prompt indicator.
    let old_indicator = ($env.PROMPT_INDICATOR? | default "> ")
    $env.PROMPT_INDICATOR = {||
        let base = if ($old_indicator | describe) == "closure" { do $old_indicator } else { $old_indicator }
        $"($base)(char escape)]133;B(char bel)"
    }

    let hooks = ($env.config.hooks? | default {})
    let prompt_hooks = ($hooks.pre_prompt? | default [] | append $pre_prompt)
    let exec_hooks = ($hooks.pre_execution? | default [] | append $pre_execution)
    $env.config = ($env.config | upsert hooks.pre_prompt $prompt_hooks | upsert hooks.pre_execution $exec_hooks)
}
