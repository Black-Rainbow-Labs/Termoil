<#
.SYNOPSIS
Generates installer/windows/terminus.ico: a dark rounded square with a light ">_" glyph.

.DESCRIPTION
Draws the icon at 16, 32, 48, 64, 128 and 256 pixels with System.Drawing and packs the PNG images
into one .ico file. Run: pwsh -NoProfile -File tools/make-icon.ps1
#>
param([string] $Output)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$root = Split-Path -Parent $PSScriptRoot
if (-not $Output) { $Output = Join-Path $root 'installer/windows/terminus.ico' }

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
    $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 24, 26, 33))), $path)
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 230, 233, 240)), ([float]($Size * 0.075))
    $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $chevron = [System.Drawing.PointF[]]@(
        (New-Object System.Drawing.PointF ($Size * 0.24), ($Size * 0.32)),
        (New-Object System.Drawing.PointF ($Size * 0.44), ($Size * 0.50)),
        (New-Object System.Drawing.PointF ($Size * 0.24), ($Size * 0.68)))
    $g.DrawLines($pen, $chevron)
    $g.DrawLine($pen, [float]($Size * 0.52), [float]($Size * 0.70), [float]($Size * 0.76), [float]($Size * 0.70))
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
