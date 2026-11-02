# Terminus shell integration for bash.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Terminus started this
# shell and set TERMINUS_SHELL_INTEGRATION=1, so sourcing it from ~/.bashrc in another terminal
# changes nothing there. Sourcing it twice installs the hooks once.

# Stop quietly when the terminal did not ask for integration or the shell is not interactive.
[ "${TERMINUS_SHELL_INTEGRATION:-}" = "1" ] || return 0 2>/dev/null
case $- in *i*) ;; *) return 0 2>/dev/null ;; esac
[ -z "${__terminus_bash_loaded:-}" ] || return 0 2>/dev/null
__terminus_bash_loaded=1

# Programs use TERM_PROGRAM to recognise the terminal.
: "${TERM_PROGRAM:=terminus}"
export TERM_PROGRAM

# Used by the DEBUG trap (bash older than 4.4 only): set once the first command of a line has
# been announced with OSC 133;C.
__terminus_cmd_started=1
# __terminus_ran is set (to an empty value) when a command ran since the last prompt, so that
# OSC 133;D is only sent after a real command. It is unset while no command has run.
unset __terminus_ran
__terminus_last_pwd=
# The history number at the last prompt (bash older than 4.4 only).
__terminus_last_hist=

# Percent encodes the string in $1 into $REPLY. Letters, digits and / . _ ~ : - stay as they are.
__terminus_urlencode() {
    local LC_ALL=C s=$1 out= c i
    for ((i = 0; i < ${#s}; i++)); do
        c=${s:i:1}
        case $c in
            [a-zA-Z0-9/._~:-]) out+=$c ;;
            *) printf -v c '%%%02X' "'$c"; out+=$c ;;
        esac
    done
    REPLY=$out
}

# Sends OSC 7 with the working directory when it differs from the last one sent.
__terminus_report_cwd() {
    [ "$PWD" = "$__terminus_last_pwd" ] && return 0
    __terminus_last_pwd=$PWD
    local dir=$PWD
    # Git Bash and Cygwin report /c/Users/x; the file URL wants C:/Users/x.
    if type cygpath >/dev/null 2>&1; then dir=$(cygpath -m "$PWD"); fi
    case $dir in /*) ;; *) dir=/$dir ;; esac
    __terminus_urlencode "$dir"
    printf '\e]7;file://%s%s\a' "${HOSTNAME:-localhost}" "$REPLY"
}

# The number of the newest history entry, in $REPLY (empty when history is off).
__terminus_history_number() {
    local h
    h=$(builtin history 1)
    h=${h#"${h%%[![:space:]]*}"}
    REPLY=${h%%[!0-9]*}
}

# Before bash 4.4 the DEBUG trap does not run for a line that is only a subshell, such as
# (exit 7). A new history entry since the last prompt means a line ran; when the trap did not
# announce it, OSC 133;C is sent now so the OSC 133;D that follows has a C before it.
__terminus_note_line() {
    __terminus_has_ps0 && return 0
    __terminus_history_number
    if [ -n "$__terminus_last_hist" ] && [ "$REPLY" != "$__terminus_last_hist" ] && [ -z "${__terminus_ran+x}" ]; then
        printf '\e]133;C\a'
        __terminus_ran=
    fi
    __terminus_last_hist=$REPLY
}

# First entry of PROMPT_COMMAND: reports how the last command ended, then the directory.
__terminus_prompt_begin() {
    local ec=$?
    __terminus_cmd_started=1
    __terminus_note_line
    if [ -n "${__terminus_ran+x}" ]; then printf '\e]133;D;%s\a' "$ec"; fi
    unset __terminus_ran
    __terminus_report_cwd
    return $ec
}

# Last entry of PROMPT_COMMAND: lets the DEBUG trap announce the next command.
__terminus_prompt_end() {
    local ec=$?
    __terminus_cmd_started=
    return $ec
}

# DEBUG trap: before the first command of a line, sends OSC 133;C and remembers that it ran.
__terminus_preexec() {
    [ -n "$__terminus_cmd_started" ] && return 0
    case $BASH_COMMAND in __terminus_*) return 0 ;; esac
    __terminus_cmd_started=1
    __terminus_ran=
    printf '\e]133;C\a'
}

# Bash 4.4 and later print PS0 after a line is read and before it runs, for every line including
# subshells such as (exit 7), which the DEBUG trap does not see. ${__terminus_ran=} sets the
# variable to empty without printing anything. An existing PS0 is kept after ours.
__terminus_install_ps0() {
    PS0='\e]133;C\a${__terminus_ran=}'"${PS0:-}"
}

# True for bash 4.4 or newer. TERMINUS_BASH_NO_PS0=1 takes the older path on any bash; the tests
# use it to run what macOS's bash 3.2 runs.
__terminus_has_ps0() {
    [ "${TERMINUS_BASH_NO_PS0:-}" = "1" ] && return 1
    [ "${BASH_VERSINFO[0]}" -gt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -ge 4 ]; }
}

# Runs the DEBUG trap that was installed before this script, so it keeps working.
__terminus_previous_debug() { :; }

# Adds the hooks around whatever PROMPT_COMMAND already holds (a string or, in bash 5.1, an array).
__terminus_install_prompt_command() {
    if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
        PROMPT_COMMAND=(__terminus_prompt_begin "${PROMPT_COMMAND[@]}" __terminus_prompt_end)
    elif [ -n "${PROMPT_COMMAND:-}" ]; then
        PROMPT_COMMAND="__terminus_prompt_begin;${PROMPT_COMMAND%;};__terminus_prompt_end"
    else
        PROMPT_COMMAND="__terminus_prompt_begin;__terminus_prompt_end"
    fi
}

# Installs the DEBUG trap, chaining to a trap that was already set.
__terminus_install_debug_trap() {
    local old
    old=$(trap -p DEBUG)
    if [ -n "$old" ]; then
        old=${old#"trap -- '"}
        old=${old%"' DEBUG"}
        eval "__terminus_previous_debug() { $old; }"
        trap '__terminus_previous_debug; __terminus_preexec' DEBUG
    else
        trap '__terminus_preexec' DEBUG
    fi
}

# Wraps PS1 once: OSC 133;A (with cl=line, because readline moves the cursor on click) at the
# start and OSC 133;B at the end. The \[ \] pair tells readline the marks take no width.
__terminus_install_ps1() {
    case $PS1 in *'133;A'*) return 0 ;; esac
    PS1='\[\e]133;A;cl=line\a\]'"$PS1"'\[\e]133;B\a\]'
}

__terminus_install_prompt_command
if __terminus_has_ps0; then __terminus_install_ps0; else __terminus_install_debug_trap; fi
__terminus_install_ps1
