# NexusLedger icon generator — run once to create placeholder icon files.
# Usage: powershell -ExecutionPolicy Bypass -File generate-icons.ps1

Add-Type -AssemblyName System.Drawing

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$pngPath   = Join-Path $scriptDir "icon.png"
$icoPath   = Join-Path $scriptDir "icon.ico"

# Create a 256x256 bitmap with a solid brand colour (#2D4A8A) and
# a simple "NL" text overlay so it is visually identifiable.
$size = 256
$bmp  = New-Object System.Drawing.Bitmap($size, $size)
$g    = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode    = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$g.Clear([System.Drawing.Color]::FromArgb(45, 74, 138))

# Draw "NL" centred on the bitmap.
$font       = New-Object System.Drawing.Font("Segoe UI", 96, [System.Drawing.FontStyle]::Bold)
$brush      = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
$stringFmt  = New-Object System.Drawing.StringFormat
$stringFmt.Alignment     = [System.Drawing.StringAlignment]::Center
$stringFmt.LineAlignment = [System.Drawing.StringAlignment]::Center
$rect       = New-Object System.Drawing.RectangleF(0, 0, $size, $size)
$g.DrawString("NL", $font, $brush, $rect, $stringFmt)

$g.Dispose()

# Save PNG.
$bmp.Save($pngPath, [System.Drawing.Imaging.ImageFormat]::Png)

# Save ICO (32x32 subset extracted from the bitmap).
$iconBmp = New-Object System.Drawing.Bitmap(32, 32)
$ig      = [System.Drawing.Graphics]::FromImage($iconBmp)
$ig.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$ig.DrawImage($bmp, 0, 0, 32, 32)
$ig.Dispose()

$icon = [System.Drawing.Icon]::FromHandle($iconBmp.GetHicon())
$fs   = [System.IO.File]::Create($icoPath)
$icon.Save($fs)
$fs.Close()

$iconBmp.Dispose()
$bmp.Dispose()

Write-Host "Generated: $pngPath"
Write-Host "Generated: $icoPath"
