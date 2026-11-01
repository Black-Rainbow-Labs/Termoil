# Startup file Terminus passes to bash with --rcfile. It loads the user's own startup files first
# and then the Terminus integration, so nothing the user configured is skipped.

# Terminus sets TERMINUS_BASH_LOGIN when the user asked for a login shell, which --rcfile would
# otherwise skip. Read the files a login shell reads, in the same order.
if [ -n "${TERMINUS_BASH_LOGIN:-}" ]; then
    unset TERMINUS_BASH_LOGIN
    __terminus_rc_done=1
    [ -f /etc/profile ] && . /etc/profile
    if [ -f "$HOME/.bash_profile" ]; then . "$HOME/.bash_profile"
    elif [ -f "$HOME/.bash_login" ]; then . "$HOME/.bash_login"
    elif [ -f "$HOME/.profile" ]; then . "$HOME/.profile"
    fi
fi

# A login profile usually sources ~/.bashrc itself; a plain interactive shell needs it here.
if [ -z "${__terminus_rc_done:-}" ]; then
    __terminus_rc_done=1
    [ -f "$HOME/.bashrc" ] && . "$HOME/.bashrc"
fi

# Find this file's folder so the integration script next to it can be loaded.
__terminus_here=${BASH_SOURCE[0]%/*}
[ -f "$__terminus_here/terminus.bash" ] && . "$__terminus_here/terminus.bash"
unset __terminus_here
