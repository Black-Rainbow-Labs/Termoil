# Terminus shell integration for Windows PowerShell 5.1 and PowerShell 7.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Terminus started this
# shell and set TERMINUS_SHELL_INTEGRATION=1. Loading it twice installs the hooks once.
# Keep this file ASCII only: Windows PowerShell 5.1 reads a file without a byte order mark as ANSI.

if ($env:TERMINUS_SHELL_INTEGRATION -ne '1') { return }
if ($global:__TerminusIntegrated) { return }
$global:__TerminusIntegrated = $true

# Programs use TERM_PROGRAM to recognise the terminal.
if (-not $env:TERM_PROGRAM) { $env:TERM_PROGRAM = 'terminus' }

$global:__TerminusEsc = [string][char]27
$global:__TerminusBel = [string][char]7
# The id of the newest history entry seen at the last prompt. A change means a command ran.
$global:__TerminusLastHistoryId = $null
$global:__TerminusFirstPrompt = $true
$global:__TerminusLastCwd = $null

# Builds an OSC sequence with a BEL terminator.
function global:__Terminus-Osc([string]$body) {
    return $global:__TerminusEsc + ']' + $body + $global:__TerminusBel
}

# Turns the current directory into an OSC 7 file URL: file://HOST/C:/path with each segment
# percent encoded. Returns an empty string when the location is not a file system path.
function global:__Terminus-CwdUrl {
    $loc = $ExecutionContext.SessionState.Path.CurrentLocation
    if ($loc.Provider.Name -ne 'FileSystem') { return '' }
    $path = $loc.ProviderPath
    $hostName = if ($env:COMPUTERNAME) { $env:COMPUTERNAME } else { [System.Net.Dns]::GetHostName() }
    if ($path.StartsWith('\\')) {
        # A share: \\server\share\dir becomes file://server/share/dir
        $segments = $path.Substring(2).Split('\')
        $hostName = $segments[0]
        $segments = $segments[1..($segments.Length - 1)]
    } elseif ($path.StartsWith('/')) {
        # PowerShell on Linux or macOS: /home/me
        $segments = $path.Substring(1).Split('/')
    } else {
        $segments = $path.Split('\')
    }
    $encoded = foreach ($s in $segments) {
        # A drive such as C: keeps its colon; everything else is encoded as UTF-8.
        if ($s -match '^[A-Za-z]:$') { $s } else { [uri]::EscapeDataString($s) }
    }
    return 'file://' + $hostName + '/' + ($encoded -join '/')
}

# Works out the exit code of the command that just finished from $? and $LASTEXITCODE.
function global:__Terminus-ExitCode([bool]$succeeded, $nativeCode) {
    if ($succeeded) { return 0 }
    if ($nativeCode -is [int] -and $nativeCode -ne 0) { return $nativeCode }
    return 1
}

# Remember the prompt the user (or their profile) already has, and wrap it.
$global:__TerminusOriginalPrompt = $function:prompt

# The prompt: D for the command that just ended, OSC 7, A, the original prompt, then B.
function global:prompt {
    $succeeded = $?
    $nativeCode = $global:LASTEXITCODE
    $out = ''
    $last = Get-History -Count 1 -ErrorAction SilentlyContinue
    $lastId = if ($last) { $last.Id } else { 0 }
    if (-not $global:__TerminusFirstPrompt -and $lastId -ne $global:__TerminusLastHistoryId) {
        $out += __Terminus-Osc ('133;D;' + (__Terminus-ExitCode $succeeded $nativeCode))
    }
    $global:__TerminusFirstPrompt = $false
    $global:__TerminusLastHistoryId = $lastId
    $url = __Terminus-CwdUrl
    if ($url -and $url -ne $global:__TerminusLastCwd) {
        $global:__TerminusLastCwd = $url
        $out += __Terminus-Osc ('7;' + $url)
    }
    $text = & $global:__TerminusOriginalPrompt
    return $out + (__Terminus-Osc '133;A') + $text + (__Terminus-Osc '133;B')
}

# OSC 133;C is sent when the user presses Enter, so the line is read through PSReadLine here and
# the mark is written before PowerShell runs the command. An empty line sends nothing.
# PSReadLine 2.2 and later take the status of the last command as a third argument (it colours
# the prompt character); older versions take two.
if ('Microsoft.PowerShell.PSConsoleReadLine' -as [type]) {
    $global:__TerminusReadLineTakesStatus = $null -ne ([Microsoft.PowerShell.PSConsoleReadLine].GetMethods() |
        Where-Object { $p = $_.GetParameters(); $_.Name -eq 'ReadLine' -and $p.Count -eq 3 -and $p[2].ParameterType -eq [Nullable[bool]] })
    function global:PSConsoleHostReadLine {
        $lastRunStatus = $?
        if ($global:__TerminusReadLineTakesStatus) {
            $line = [Microsoft.PowerShell.PSConsoleReadLine]::ReadLine($Host.Runspace, $ExecutionContext, $lastRunStatus)
        } else {
            $line = [Microsoft.PowerShell.PSConsoleReadLine]::ReadLine($Host.Runspace, $ExecutionContext)
        }
        if ($line -and $line.Trim().Length -gt 0) {
            [Console]::Write((__Terminus-Osc '133;C'))
        }
        return $line
    }
}
