<#
.SYNOPSIS
Measures Ghostty 1.4 on the P1 gate file in WSL and records the median for `terminus-bench compare`.

.DESCRIPTION
Builds terminus-bench, writes the 1 GiB `mixed` gate file with `terminus-bench write-stream mixed`,
runs tools/bench-ghostty.sh inside WSL on it (which fetches zig 0.17.0 and Ghostty v1.4.0 into a
scratch folder, builds ghostty-bench, runs it, and deletes the scratch folder), and appends the
JSON row it prints to _agent_output/bench/ghostty.jsonl with the machine and the date. Run it before
`cargo run --release -p terminus-bench -- compare`, which shows the newest row beside `mixed`.
The gate file is deleted when the script finishes.

.PARAMETER Runs
How many times ghostty-bench runs on the file. The median is reported.
#>
param([int] $Runs = 3)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

<#
.SYNOPSIS
Builds terminus-bench in release and writes the mixed gate file to `$Path`.
#>
function Write-GateFile([string] $Path) {
    & cargo run --release -q -p terminus-bench --manifest-path (Join-Path $repo 'Cargo.toml') -- write-stream mixed $Path
    if ($LASTEXITCODE -ne 0) { throw "terminus-bench write-stream failed ($LASTEXITCODE)" }
}

<#
.SYNOPSIS
Converts a Windows path to the path WSL sees it at.
#>
function ConvertTo-WslPath([string] $Path) {
    $p = (Resolve-Path $Path).Path -replace '\\', '/'
    (& wsl.exe -- wslpath -a $p).Trim()
}

<#
.SYNOPSIS
Runs tools/bench-ghostty.sh in WSL on `$DataFile`; returns the JSON row it prints last.
#>
function Invoke-GhosttyBench([string] $DataFile, [int] $Runs) {
    $script = ConvertTo-WslPath (Join-Path $PSScriptRoot 'bench-ghostty.sh')
    $data = ConvertTo-WslPath $DataFile
    $lines = & wsl.exe -- bash $script $data $Runs
    if ($LASTEXITCODE -ne 0) { throw "bench-ghostty.sh failed ($LASTEXITCODE)" }
    ($lines | Where-Object { $_ -like '{*' } | Select-Object -Last 1)
}

<#
.SYNOPSIS
Adds the machine and the date to the row and appends it to _agent_output/bench/ghostty.jsonl.
#>
function Add-GhosttyRow([string] $Json) {
    $row = $Json | ConvertFrom-Json
    $row | Add-Member -NotePropertyName date -NotePropertyValue (Get-Date).ToUniversalTime().ToString('o')
    $row | Add-Member -NotePropertyName cpu -NotePropertyValue $env:PROCESSOR_IDENTIFIER
    $row | Add-Member -NotePropertyName host -NotePropertyValue $env:COMPUTERNAME
    $dir = Join-Path $repo '_agent_output/bench'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Add-Content -Path (Join-Path $dir 'ghostty.jsonl') -Value ($row | ConvertTo-Json -Compress) -Encoding utf8
    $row
}

$scratch = Join-Path ([IO.Path]::GetTempPath()) "terminus-bench-ghostty-$PID"
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
$gate = Join-Path $scratch 'mixed-1gib.bin'
try {
    Write-GateFile $gate
    $row = Add-GhosttyRow (Invoke-GhosttyBench $gate $Runs)
    "Ghostty 1.4 full, mixed 1 GiB at 200x60: {0} MB/s (median of {1}: {2})" -f $row.mb_per_s, $Runs, ($row.runs_mb_per_s -join ', ')
}
finally {
    Remove-Item -LiteralPath $gate -Force -ErrorAction SilentlyContinue -Confirm:$false
    Remove-Item -LiteralPath $scratch -Force -ErrorAction SilentlyContinue -Confirm:$false
}
