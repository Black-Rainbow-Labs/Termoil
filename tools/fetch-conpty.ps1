<#
.SYNOPSIS
Downloads the Microsoft.Windows.Console.ConPTY NuGet package and extracts the x64 conpty.dll and OpenConsole.exe.

.DESCRIPTION
The version is read from installer/windows/conpty.version. The binaries land in installer/windows/conpty/
(gitignored). Runs without prompts: pwsh -NoProfile -File tools/fetch-conpty.ps1
#>
param([string]$Arch = 'x64')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$versionFile = Join-Path $root 'installer/windows/conpty.version'
$version = (Get-Content -LiteralPath $versionFile -TotalCount 1).Trim()
$outDir = Join-Path $root 'installer/windows/conpty'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("conpty-fetch-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
$nupkg = Join-Path $work 'conpty.zip'
$url = "https://www.nuget.org/api/v2/package/Microsoft.Windows.Console.ConPTY/$version"
Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $nupkg -UseBasicParsing

$extract = Join-Path $work 'x'
Expand-Archive -LiteralPath $nupkg -DestinationPath $extract -Force
$dll = Get-ChildItem -Path $extract -Recurse -Filter "conpty.dll" | Where-Object { $_.FullName.Replace("\", "/") -like "*$Arch/*" } | Select-Object -First 1
$exe = Get-ChildItem -Path $extract -Recurse -Filter "OpenConsole.exe" | Where-Object { $_.FullName.Replace("\", "/") -like "*$Arch/*" } | Select-Object -First 1
if (-not $dll -or -not $exe) { throw "conpty.dll or OpenConsole.exe for $Arch not found in the package" }
Copy-Item -LiteralPath $dll.FullName -Destination (Join-Path $outDir 'conpty.dll') -Force
Copy-Item -LiteralPath $exe.FullName -Destination (Join-Path $outDir 'OpenConsole.exe') -Force
Remove-Item -LiteralPath $work -Recurse -Force
Write-Host "Extracted conpty.dll and OpenConsole.exe ($version, $Arch) to $outDir"
