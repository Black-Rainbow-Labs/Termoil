# Terminus shell integration for fish.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Terminus started this
# shell and set TERMINUS_SHELL_INTEGRATION=1. fish loads this file from vendor_conf.d because
# Terminus puts the integration folder at the front of XDG_DATA_DIRS. Loading it twice installs
# the hooks once.
# Untested on this machine: fish was not installed when this was written.

if status is-interactive; and test "$TERMINUS_SHELL_INTEGRATION" = 1; and not set -q __terminus_fish_loaded
    set -g __terminus_fish_loaded 1
    set -g __terminus_cmd_ran 0
    set -g __terminus_last_pwd ''

    # Programs use TERM_PROGRAM to recognise the terminal.
    if not set -q TERM_PROGRAM
        set -gx TERM_PROGRAM terminus
    end

    # A remote host has no xterm-terminus entry, so ssh gets TERM=xterm-256color, which every
    # host knows. Only when TERM is xterm-terminus and the person has no ssh function of their own.
    if not functions -q ssh
        function ssh --wraps ssh --description 'ssh with a TERM the remote host knows'
            if test "$TERM" = xterm-terminus
                TERM=xterm-256color command ssh $argv
            else
                command ssh $argv
            end
        end
    end

    # Sends OSC 7 with the working directory when it differs from the last one sent.
    function __terminus_report_cwd
        test "$PWD" = "$__terminus_last_pwd"; and return
        set -g __terminus_last_pwd $PWD
        printf '\e]7;file://%s%s\a' (hostname) (string escape --style=url -- $PWD)
    end

    # Runs when the directory changes.
    function __terminus_on_pwd --on-variable PWD
        __terminus_report_cwd
    end

    # Runs before a command executes: OSC 133;C.
    function __terminus_preexec --on-event fish_preexec
        set -g __terminus_cmd_ran 1
        printf '\e]133;C\a'
    end

    # Runs after a command finished: OSC 133;D with the exit code of that command.
    function __terminus_postexec --on-event fish_postexec
        set -l ec $status
        if test "$__terminus_cmd_ran" = 1
            printf '\e]133;D;%s\a' $ec
        end
        set -g __terminus_cmd_ran 0
    end

    # Sets $status to the number given, so the user's prompt sees the status of the last command.
    function __terminus_restore_status
        return $argv[1]
    end

    # Wrap the prompt: OSC 7 if needed, A (cl=line, the editor moves the cursor on click), the
    # user's prompt, then B.
    functions -c fish_prompt __terminus_original_fish_prompt
    function fish_prompt
        set -l ec $status
        __terminus_report_cwd
        printf '\e]133;A;cl=line\a'
        # Run the user's prompt with the exit status they expect.
        __terminus_restore_status $ec
        __terminus_original_fish_prompt
        printf '\e]133;B\a'
    end
end
