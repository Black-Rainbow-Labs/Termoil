# Terminus starts zsh with ZDOTDIR pointing at this folder so this file runs first. It puts
# ZDOTDIR back to what the user had, runs the user's own .zshenv, and arranges for the
# integration script to load after every other startup file has run.

if [[ -n ${TERMINUS_ORIG_ZDOTDIR+x} ]]; then
    if [[ -n $TERMINUS_ORIG_ZDOTDIR ]]; then ZDOTDIR=$TERMINUS_ORIG_ZDOTDIR; else unset ZDOTDIR; fi
    unset TERMINUS_ORIG_ZDOTDIR
else
    unset ZDOTDIR
fi

[[ -f ${ZDOTDIR:-$HOME}/.zshenv ]] && source "${ZDOTDIR:-$HOME}/.zshenv"

# One shot hook: on the first prompt, load the integration and remove this hook. The integration
# script's own precmd is not in the list yet for this prompt, so it is run by hand.
if [[ -o interactive && ${TERMINUS_SHELL_INTEGRATION:-} == 1 && -n ${TERMINUS_SHELL_INTEGRATION_DIR:-} ]]; then
    __terminus_bootstrap() {
        add-zsh-hook -d precmd __terminus_bootstrap
        unfunction __terminus_bootstrap
        source "$TERMINUS_SHELL_INTEGRATION_DIR/zsh/terminus.zsh"
        (( ${+functions[__terminus_precmd]} )) && __terminus_precmd
    }
    autoload -Uz add-zsh-hook
    add-zsh-hook precmd __terminus_bootstrap
fi
