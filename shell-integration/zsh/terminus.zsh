# Terminus shell integration for zsh.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Terminus started this
# shell and set TERMINUS_SHELL_INTEGRATION=1. Sourcing it twice installs the hooks once.
# Untested on this machine: zsh was not installed when this was written.

[[ -o interactive ]] || return 0
[[ ${TERMINUS_SHELL_INTEGRATION:-} == 1 ]] || return 0
(( ${+__terminus_zsh_loaded} )) && return 0
typeset -g __terminus_zsh_loaded=1

# Programs use TERM_PROGRAM to recognise the terminal.
: ${TERM_PROGRAM:=terminus}
export TERM_PROGRAM

typeset -g __terminus_cmd_ran=
typeset -g __terminus_last_pwd=

# Prints the string in $1 percent encoded. Letters, digits and / . _ ~ : - stay as they are.
__terminus_urlencode() {
    emulate -L zsh
    local LC_ALL=C s=$1 out= c i
    for (( i = 1; i <= ${#s}; i++ )); do
        c=${s[i]}
        case $c in
            [a-zA-Z0-9/._~:-]) out+=$c ;;
            *) out+=$(printf '%%%02X' "'$c") ;;
        esac
    done
    print -rn -- "$out"
}

# Sends OSC 7 with the working directory when it differs from the last one sent.
__terminus_report_cwd() {
    [[ $PWD == $__terminus_last_pwd ]] && return 0
    __terminus_last_pwd=$PWD
    printf '\e]7;file://%s%s\a' "${HOST:-localhost}" "$(__terminus_urlencode "$PWD")"
}

# Runs before each prompt: D for the command that ended, OSC 7, then A. B is added to PS1.
__terminus_precmd() {
    local ec=$?
    if [[ -n $__terminus_cmd_ran ]]; then printf '\e]133;D;%s\a' "$ec"; fi
    __terminus_cmd_ran=
    __terminus_report_cwd
    printf '\e]133;A;cl=line\a'
    # The %{ %} pair tells zsh the mark takes no width. Added once, even if a theme resets PS1.
    [[ $PS1 == *'133;B'* ]] || PS1="$PS1%{"$'\e]133;B\a'"%}"
}

# Runs after a command line is read and before it executes: OSC 133;C.
__terminus_preexec() {
    __terminus_cmd_ran=1
    printf '\e]133;C\a'
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __terminus_precmd
add-zsh-hook preexec __terminus_preexec

# A remote host has no xterm-terminus entry, so ssh gets TERM=xterm-256color, which every host
# knows. Only when TERM is xterm-terminus and the person has no ssh function or alias of their own.
if (( ! ${+functions[ssh]} && ! ${+aliases[ssh]} )); then
    ssh() {
        if [[ ${TERM:-} == xterm-terminus ]]; then TERM=xterm-256color command ssh "$@"; else command ssh "$@"; fi
    }
fi
