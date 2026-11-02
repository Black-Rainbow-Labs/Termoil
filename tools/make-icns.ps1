<#
.SYNOPSIS
Generates installer/macos/termoil.icns from the PNG images inside installer/windows/termoil.ico.

.DESCRIPTION
An .icns file is a header followed by chunks, each a four letter type, a big endian length and the
bytes of one PNG image. The .ico written by tools/make-icon.ps1 already holds PNG images at 16, 32,
48, 64, 128 and 256 pixels, so they are copied across without being redrawn. Run:
pwsh -NoProfile -File tools/make-icns.ps1
#>
param([string] $Source, [string] $Output)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Source) { $Source = Join-Path $root 'installer/windows/termoil.ico' }
if (-not $Output) { $Output = Join-Path $root 'installer/macos/termoil.icns' }

<#
.SYNOPSIS
Reads every PNG image out of an .ico file and returns them keyed by pixel width.
.PARAMETER Path
The .ico file.
#>
function Read-IcoImages([string] $Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $count = [BitConverter]::ToUInt16($bytes, 4)
    $images = @{}
    for ($i = 0; $i -lt $count; $i++) {
        $entry = 6 + 16 * $i
        $width = if ($bytes[$entry] -eq 0) { 256 } else { [int] $bytes[$entry] }
        $length = [BitConverter]::ToUInt32($bytes, $entry + 8)
        $offset = [BitConverter]::ToUInt32($bytes, $entry + 12)
        $png = New-Object byte[] $length
        [Array]::Copy($bytes, $offset, $png, 0, $length)
        $images[$width] = $png
    }
    return $images
}

<#
.SYNOPSIS
Returns the bytes of one .icns chunk: the type, the big endian total length and the PNG.
.PARAMETER Type
The four character chunk type.
.PARAMETER Png
The image bytes.
#>
function New-IcnsChunk([string] $Type, [byte[]] $Png) {
    $length = [BitConverter]::GetBytes([uint32] (8 + $Png.Length))
    [Array]::Reverse($length)
    return [byte[]] ([System.Text.Encoding]::ASCII.GetBytes($Type) + $length + $Png)
}

# Chunk types for PNG images: icp4 16, icp5 32, icp6 64, ic07 128, ic08 256 pixels.
$chunkTypes = @(@(16, 'icp4'), @(32, 'icp5'), @(64, 'icp6'), @(128, 'ic07'), @(256, 'ic08'))
$images = Read-IcoImages $Source
$body = [byte[]] @()
foreach ($pair in $chunkTypes) {
    $width = $pair[0]
    if (-not $images.ContainsKey($width)) { throw "$Source has no ${width} pixel image." }
    $body += New-IcnsChunk $pair[1] $images[$width]
}
$total = [BitConverter]::GetBytes([uint32] (8 + $body.Length))
[Array]::Reverse($total)
$icns = [byte[]] ([System.Text.Encoding]::ASCII.GetBytes('icns') + $total + $body)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Output) | Out-Null
[System.IO.File]::WriteAllBytes($Output, $icns)
Write-Host "Wrote $Output ($($icns.Length) bytes)"
