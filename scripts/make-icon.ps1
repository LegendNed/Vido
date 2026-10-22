# Renders the 1024x1024 source icon (config/icons/source.png) with System.Drawing.
# Regenerate all icon sizes afterwards with: pnpm tauri icon config/icons/source.png -o config/icons
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$size = 1024
$out = Join-Path (Split-Path -Parent $PSScriptRoot) "config\icons\source.png"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $out) | Out-Null

function New-RoundedRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

$bmp = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.Clear([System.Drawing.Color]::Transparent)

# Background tile
$bg = New-RoundedRect 40 40 944 944 210
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 21, 23, 28))), $bg)

# Back card (the "deck")
$back = New-RoundedRect 250 250 520 380 60
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 58, 63, 74))), $back)

# Front card
$front = New-RoundedRect 190 360 560 420 64
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 232, 234, 237))), $front)

# Play triangle on the front card
$tri = New-Object System.Drawing.Drawing2D.GraphicsPath
$tri.AddPolygon([System.Drawing.PointF[]]@(
        (New-Object System.Drawing.PointF 405, 480),
        (New-Object System.Drawing.PointF 405, 660),
        (New-Object System.Drawing.PointF 560, 570)))
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 21, 23, 28))), $tri)

# Record dot
$g.FillEllipse((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 77, 79))), 660, 200, 190, 190)

$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Host "Wrote $out"
