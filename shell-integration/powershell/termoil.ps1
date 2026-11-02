# Termoil shell integration for Windows PowerShell 5.1 and PowerShell 7.
#
# Emits OSC 133 prompt marks (A prompt start, B input start, C output start, D command finished
# with the exit code) and OSC 7 (the working directory). Does nothing unless Termoil started this
# shell and set TERMOIL_SHELL_INTEGRATION=1. Loading it twice installs the hooks once.
# Keep this file ASCII only: Windows PowerShell 5.1 reads a file without a byte order mark as ANSI.

if ($env:TERMOIL_SHELL_INTEGRATION -ne '1') { return }
if ($global:__TermoilIntegrated) { return }
$global:__TermoilIntegrated = $true

# Programs use TERM_PROGRAM to recognise the terminal.
if (-not $env:TERM_PROGRAM) { $env:TERM_PROGRAM = 'termoil' }

$global:__TermoilEsc = [string][char]27
$global:__TermoilBel = [string][char]7
# The id of the newest history entry seen at the last prompt. A change means a command ran.
$global:__TermoilLastHistoryId = $null
$global:__TermoilFirstPrompt = $true
$global:__TermoilLastCwd = $null

# Builds an OSC sequence with a BEL terminator.
function global:__Termoil-Osc([string]$body) {
    return $global:__TermoilEsc + ']' + $body + $global:__TermoilBel
}

# Turns the current directory into an OSC 7 file URL: file://HOST/C:/path with each segment
# percent encoded. Returns an empty string when the location is not a file system path.
function global:__Termoil-CwdUrl {
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
function global:__Termoil-ExitCode([bool]$succeeded, $nativeCode) {
    if ($succeeded) { return 0 }
    if ($nativeCode -is [int] -and $nativeCode -ne 0) { return $nativeCode }
    return 1
}

# Remember the prompt the user (or their profile) already has, and wrap it.
$global:__TermoilOriginalPrompt = $function:prompt

# The prompt: D for the command that just ended, OSC 7, A, the original prompt, then B.
function global:prompt {
    $succeeded = $?
    $nativeCode = $global:LASTEXITCODE
    $out = ''
    $last = Get-History -Count 1 -ErrorAction SilentlyContinue
    $lastId = if ($last) { $last.Id } else { 0 }
    if (-not $global:__TermoilFirstPrompt -and $lastId -ne $global:__TermoilLastHistoryId) {
        $out += __Termoil-Osc ('133;D;' + (__Termoil-ExitCode $succeeded $nativeCode))
    }
    $global:__TermoilFirstPrompt = $false
    $global:__TermoilLastHistoryId = $lastId
    $url = __Termoil-CwdUrl
    if ($url -and $url -ne $global:__TermoilLastCwd) {
        $global:__TermoilLastCwd = $url
        $out += __Termoil-Osc ('7;' + $url)
    }
    $text = & $global:__TermoilOriginalPrompt
    return $out + (__Termoil-Osc '133;A') + $text + (__Termoil-Osc '133;B')
}

# OSC 133;C is sent when the user presses Enter, so the line is read through PSReadLine here and
# the mark is written before PowerShell runs the command. An empty line sends nothing.
# PSReadLine 2.2 and later take the status of the last command as a third argument (it colours
# the prompt character); older versions take two.
if ('Microsoft.PowerShell.PSConsoleReadLine' -as [type]) {
    $global:__TermoilReadLineTakesStatus = $null -ne ([Microsoft.PowerShell.PSConsoleReadLine].GetMethods() |
        Where-Object { $p = $_.GetParameters(); $_.Name -eq 'ReadLine' -and $p.Count -eq 3 -and $p[2].ParameterType -eq [Nullable[bool]] })
    function global:PSConsoleHostReadLine {
        $lastRunStatus = $?
        if ($global:__TermoilReadLineTakesStatus) {
            $line = [Microsoft.PowerShell.PSConsoleReadLine]::ReadLine($Host.Runspace, $ExecutionContext, $lastRunStatus)
        } else {
            $line = [Microsoft.PowerShell.PSConsoleReadLine]::ReadLine($Host.Runspace, $ExecutionContext)
        }
        if ($line -and $line.Trim().Length -gt 0) {
            [Console]::Write((__Termoil-Osc '133;C'))
        }
        return $line
    }
}
