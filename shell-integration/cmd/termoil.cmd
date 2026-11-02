@rem Termoil shell integration for cmd.exe.
@rem
@rem cmd cannot run code before or after a command, so the only hook it has is the PROMPT
@rem string. This sets PROMPT to send OSC 7 (the working directory) and OSC 133;A and 133;B
@rem around the normal prompt. There is no way to get the exit code or the start of output.
@rem Does nothing unless Termoil started this shell and set TERMOIL_SHELL_INTEGRATION=1.
@rem Running it twice leaves the prompt as it was after the first run.
@rem Termoil starts cmd as: cmd.exe /k <folder>\cmd\termoil.cmd
@if not "%TERMOIL_SHELL_INTEGRATION%"=="1" goto :eof
@if defined TERMOIL_CMD_INTEGRATED goto :eof
@set "TERMOIL_CMD_INTEGRATED=1"
@if not defined TERM_PROGRAM set "TERM_PROGRAM=termoil"
@rem $E is the escape character and $E\ ends a string. $P is the current drive and folder.
@rem The path keeps its backslashes and is not percent encoded: PROMPT cannot do either.
@if not defined PROMPT set "PROMPT=$P$G"
@set "PROMPT=$E]133;A$E\$E]7;file://%COMPUTERNAME%/$P$E\%PROMPT%$E]133;B$E\"
