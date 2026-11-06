<#
.SYNOPSIS
Generates installer/windows/termoil.ico: a dark rounded square with a light ">_" glyph.

.DESCRIPTION
Draws the icon at 16, 32, 48, 64, 128 and 256 pixels with System.Drawing and packs the PNG images
into one .ico file. Run: pwsh -NoProfile -File tools/make-icon.ps1
#>
param([string] $Output)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$root = Split-Path -Parent $PSScriptRoot
if (-not $Output) { $Output = Join-Path $root 'installer/windows/termoil.ico' }

<#
.SYNOPSIS
Draws the icon at one size and returns the PNG bytes.
.PARAMETER Size
Width and height in pixels.
#>
function New-IconPng([int] $Size) {
    $bitmap = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)
    $radius = $Size * 0.22
    $d = $radius * 2
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $r = $Size - 1
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($r - $d, 0, $d, $d, 270, 90)
    $path.AddArc($r - $d, $r - $d, $d, $d, 0, 90)
    $path.AddArc(0, $r - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    # task-2524: drawn to read as the two characters ">_" a terminal prints: thick strokes with square
    # ends, a ">" as tall as a capital letter, and an underscore on the same baseline, near white on near black.
    $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 14, 15, 19))), $path)
    $light = [System.Drawing.Color]::FromArgb(255, 242, 244, 248)
    $stroke = [float]([Math]::Max(1.6, $Size * 0.105))
    $pen = New-Object System.Drawing.Pen $light, $stroke
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Square
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Square
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Miter
    $chevron = [System.Drawing.PointF[]]@(
        (New-Object System.Drawing.PointF ($Size * 0.22), ($Size * 0.27)),
        (New-Object System.Drawing.PointF ($Size * 0.46), ($Size * 0.48)),
        (New-Object System.Drawing.PointF ($Size * 0.22), ($Size * 0.69)))
    $g.DrawLines($pen, $chevron)
    $bar = $Size * 0.69 - $stroke / 2
    $g.FillRectangle((New-Object System.Drawing.SolidBrush $light), [float]($Size * 0.53), [float]$bar, [float]($Size * 0.27), [float]$stroke)
    $g.Dispose()
    $stream = New-Object System.IO.MemoryStream
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
    return ,$stream.ToArray()
}

$sizes = 16, 32, 48, 64, 128, 256
$images = foreach ($s in $sizes) { ,(New-IconPng $s) }
$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $dim = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$images[$i].Length); $w.Write([uint32]$offset)
    $offset += $images[$i].Length
}
foreach ($img in $images) { $w.Write($img) }
$w.Flush()
[System.IO.File]::WriteAllBytes($Output, $out.ToArray())
Write-Host "Wrote $Output"
