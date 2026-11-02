# Termoil shell integration for bash.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Termoil started this
# shell and set TERMOIL_SHELL_INTEGRATION=1, so sourcing it from ~/.bashrc in another terminal
# changes nothing there. Sourcing it twice installs the hooks once.

# Stop quietly when the terminal did not ask for integration or the shell is not interactive.
[ "${TERMOIL_SHELL_INTEGRATION:-}" = "1" ] || return 0 2>/dev/null
case $- in *i*) ;; *) return 0 2>/dev/null ;; esac
[ -z "${__termoil_bash_loaded:-}" ] || return 0 2>/dev/null
__termoil_bash_loaded=1

# Programs use TERM_PROGRAM to recognise the terminal.
: "${TERM_PROGRAM:=termoil}"
export TERM_PROGRAM

# Used by the DEBUG trap (bash older than 4.4 only): set once the first command of a line has
# been announced with OSC 133;C.
__termoil_cmd_started=1
# __termoil_ran is set (to an empty value) when a command ran since the last prompt, so that
# OSC 133;D is only sent after a real command. It is unset while no command has run.
unset __termoil_ran
__termoil_last_pwd=
# The history number at the last prompt (bash older than 4.4 only).
__termoil_last_hist=

# Percent encodes the string in $1 into $REPLY. Letters, digits and / . _ ~ : - stay as they are.
__termoil_urlencode() {
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
__termoil_report_cwd() {
    [ "$PWD" = "$__termoil_last_pwd" ] && return 0
    __termoil_last_pwd=$PWD
    local dir=$PWD
    # Git Bash and Cygwin report /c/Users/x; the file URL wants C:/Users/x.
    if type cygpath >/dev/null 2>&1; then dir=$(cygpath -m "$PWD"); fi
    case $dir in /*) ;; *) dir=/$dir ;; esac
    __termoil_urlencode "$dir"
    printf '\e]7;file://%s%s\a' "${HOSTNAME:-localhost}" "$REPLY"
}

# The number of the newest history entry, in $REPLY (empty when history is off).
__termoil_history_number() {
    local h
    h=$(builtin history 1)
    h=${h#"${h%%[![:space:]]*}"}
    REPLY=${h%%[!0-9]*}
}

# Before bash 4.4 the DEBUG trap does not run for a line that is only a subshell, such as
# (exit 7). A new history entry since the last prompt means a line ran; when the trap did not
# announce it, OSC 133;C is sent now so the OSC 133;D that follows has a C before it.
__termoil_note_line() {
    __termoil_has_ps0 && return 0
    __termoil_history_number
    if [ -n "$__termoil_last_hist" ] && [ "$REPLY" != "$__termoil_last_hist" ] && [ -z "${__termoil_ran+x}" ]; then
        printf '\e]133;C\a'
        __termoil_ran=
    fi
    __termoil_last_hist=$REPLY
}

# First entry of PROMPT_COMMAND: reports how the last command ended, then the directory.
__termoil_prompt_begin() {
    local ec=$?
    __termoil_cmd_started=1
    __termoil_note_line
    if [ -n "${__termoil_ran+x}" ]; then printf '\e]133;D;%s\a' "$ec"; fi
    unset __termoil_ran
    __termoil_report_cwd
    return $ec
}

# Last entry of PROMPT_COMMAND: lets the DEBUG trap announce the next command.
__termoil_prompt_end() {
    local ec=$?
    __termoil_cmd_started=
    return $ec
}

# DEBUG trap: before the first command of a line, sends OSC 133;C and remembers that it ran.
__termoil_preexec() {
    [ -n "$__termoil_cmd_started" ] && return 0
    case $BASH_COMMAND in __termoil_*) return 0 ;; esac
    __termoil_cmd_started=1
    __termoil_ran=
    printf '\e]133;C\a'
}

# Bash 4.4 and later print PS0 after a line is read and before it runs, for every line including
# subshells such as (exit 7), which the DEBUG trap does not see. ${__termoil_ran=} sets the
# variable to empty without printing anything. An existing PS0 is kept after ours.
__termoil_install_ps0() {
    PS0='\e]133;C\a${__termoil_ran=}'"${PS0:-}"
}

# True for bash 4.4 or newer. TERMOIL_BASH_NO_PS0=1 takes the older path on any bash; the tests
# use it to run what macOS's bash 3.2 runs.
__termoil_has_ps0() {
    [ "${TERMOIL_BASH_NO_PS0:-}" = "1" ] && return 1
    [ "${BASH_VERSINFO[0]}" -gt 4 ] || { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -ge 4 ]; }
}

# Runs the DEBUG trap that was installed before this script, so it keeps working.
__termoil_previous_debug() { :; }

# Adds the hooks around whatever PROMPT_COMMAND already holds (a string or, in bash 5.1, an array).
__termoil_install_prompt_command() {
    if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
        PROMPT_COMMAND=(__termoil_prompt_begin "${PROMPT_COMMAND[@]}" __termoil_prompt_end)
    elif [ -n "${PROMPT_COMMAND:-}" ]; then
        PROMPT_COMMAND="__termoil_prompt_begin;${PROMPT_COMMAND%;};__termoil_prompt_end"
    else
        PROMPT_COMMAND="__termoil_prompt_begin;__termoil_prompt_end"
    fi
}

# Installs the DEBUG trap, chaining to a trap that was already set.
__termoil_install_debug_trap() {
    local old
    old=$(trap -p DEBUG)
    if [ -n "$old" ]; then
        old=${old#"trap -- '"}
        old=${old%"' DEBUG"}
        eval "__termoil_previous_debug() { $old; }"
        trap '__termoil_previous_debug; __termoil_preexec' DEBUG
    else
        trap '__termoil_preexec' DEBUG
    fi
}

# Wraps PS1 once: OSC 133;A (with cl=line, because readline moves the cursor on click) at the
# start and OSC 133;B at the end. The \[ \] pair tells readline the marks take no width.
__termoil_install_ps1() {
    case $PS1 in *'133;A'*) return 0 ;; esac
    PS1='\[\e]133;A;cl=line\a\]'"$PS1"'\[\e]133;B\a\]'
}

__termoil_install_prompt_command
if __termoil_has_ps0; then __termoil_install_ps0; else __termoil_install_debug_trap; fi
__termoil_install_ps1

# A remote host has no xterm-termoil entry, so ssh gets TERM=xterm-256color, which every host
# knows. Only when TERM is xterm-termoil and the person has no ssh function or alias of their own.
if ! declare -F ssh >/dev/null && ! alias ssh >/dev/null 2>&1; then
    ssh() {
        if [ "${TERM:-}" = "xterm-termoil" ]; then TERM=xterm-256color command ssh "$@"; else command ssh "$@"; fi
    }
fi
