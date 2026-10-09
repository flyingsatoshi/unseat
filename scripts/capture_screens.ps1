# Captures README screenshots from a real Unseat test instance (UNSEAT_TEST_HOOKS=1).
# The test instance uses its own settings folder and window titles, so it runs beside a
# normal Unseat without touching it.
#
#   powershell -ExecutionPolicy Bypass -File scripts/capture_screens.ps1 [-Exe path] [-Out dir]
param(
  [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\unseat.exe"),
  [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) "assets\screenshots")
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "unseat_test_lib.ps1")
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force $Out | Out-Null

function Backdrop([int]$w, [int]$h, [bool]$dark) {
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $top = if ($dark) { [System.Drawing.Color]::FromArgb(255, 44, 52, 74) } else { [System.Drawing.Color]::FromArgb(255, 222, 228, 238) }
  $bottom = if ($dark) { [System.Drawing.Color]::FromArgb(255, 74, 88, 122) } else { [System.Drawing.Color]::FromArgb(255, 186, 198, 218) }
  $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.Rectangle 0, 0, $w, $h), $top, $bottom, 60.0)
  $g.FillRectangle($brush, 0, 0, $w, $h)
  $brush.Dispose()
  return @($bmp, $g)
}

function Save-Overlay($inst, [string]$path, [bool]$dark, [switch]$Hover) {
  $bmp = Get-OverlayBitmap $inst -Hover:$Hover
  $pad = 28
  $canvas, $g = Backdrop ($bmp.Width + 2 * $pad) ($bmp.Height + 2 * $pad) $dark
  $g.DrawImage($bmp, $pad, $pad, $bmp.Width, $bmp.Height)
  $g.Dispose(); $bmp.Dispose()
  $canvas.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $canvas.Dispose()
  "wrote $path"
}

function Save-Window([IntPtr]$hwnd, [string]$path, [bool]$dark) {
  $r = Get-Rect $hwnd
  $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
  $shot = New-Object System.Drawing.Bitmap $w, $h
  $sg = [System.Drawing.Graphics]::FromImage($shot)
  $sg.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $h))
  $sg.Dispose()
  $pad = 20
  $canvas, $g = Backdrop ($w + 2 * $pad) ($h + 2 * $pad) $dark
  # Clip to the window's rounded corners so no desktop pixels are captured.
  $radius = [int](8 * $w / 380.0) * 2
  $clip = New-Object System.Drawing.Drawing2D.GraphicsPath
  $l = $pad + 1; $t = $pad + 1; $r2 = $pad + $w - 1; $b2 = $pad + $h - 1
  $clip.AddArc($l, $t, $radius, $radius, 180, 90)
  $clip.AddArc($r2 - $radius, $t, $radius, $radius, 270, 90)
  $clip.AddArc($r2 - $radius, $b2 - $radius, $radius, $radius, 0, 90)
  $clip.AddArc($l, $b2 - $radius, $radius, $radius, 90, 90)
  $clip.CloseFigure()
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.SetClip($clip)
  $g.DrawImage($shot, $pad, $pad, $w, $h)
  $g.ResetClip()
  $g.Dispose(); $shot.Dispose()
  $canvas.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $canvas.Dispose()
  "wrote $path"
}

foreach ($theme in @("dark", "light")) {
  $dark = $theme -eq "dark"
  $inst = Start-TestInstance $Exe @{
    settings_version = 2; theme = $theme; widget_size = "medium"; sitting_limit_secs = 1200
    window_x = 240; window_y = 240; stats_enabled = $true
    history = @(
      @{ date = (Get-Date).AddDays(-6).ToString("yyyy-MM-dd"); sitting_secs = 14400 },
      @{ date = (Get-Date).AddDays(-5).ToString("yyyy-MM-dd"); sitting_secs = 19800 },
      @{ date = (Get-Date).AddDays(-4).ToString("yyyy-MM-dd"); sitting_secs = 11200 },
      @{ date = (Get-Date).AddDays(-3).ToString("yyyy-MM-dd"); sitting_secs = 22300 },
      @{ date = (Get-Date).AddDays(-2).ToString("yyyy-MM-dd"); sitting_secs = 16100 },
      @{ date = (Get-Date).AddDays(-1).ToString("yyyy-MM-dd"); sitting_secs = 9800 }
    )
  }
  try {
    Advance $inst 428
    Save-Overlay $inst (Join-Path $Out "overlay-sitting-$theme.png") $dark
    # Hover reveals pause, reset and hide controls.
    Save-Overlay $inst (Join-Path $Out "overlay-hover-$theme.png") $dark -Hover
    Advance $inst (1200 - 428 + 7 * 60 + 20)
    Save-Overlay $inst (Join-Path $Out "overlay-overdue-$theme.png") $dark
    Send-Command $inst $ID_BREAK
    Advance $inst 41
    Save-Overlay $inst (Join-Path $Out "overlay-break-$theme.png") $dark
    Send-Command $inst $ID_RESET
    Advance $inst 300
    Send-Command $inst $ID_PAUSE
    Save-Overlay $inst (Join-Path $Out "overlay-paused-$theme.png") $dark
    Send-Command $inst $ID_PAUSE

    $dlg = Open-Settings $inst
    $tabs = "timer", "breaks", "alerts", "general"
    for ($i = 0; $i -lt $tabs.Count; $i++) {
      Click-SettingsTab $dlg $i
      Save-Window $dlg (Join-Path $Out "settings-$($tabs[$i])-$theme.png") $dark
    }
    Close-Window $dlg
  } finally {
    Stop-TestInstance $inst
  }
}
# One sheet with every overlay state, dark above light.
$states = "sitting", "hover", "overdue", "break", "paused"
$first = [System.Drawing.Image]::FromFile((Join-Path $Out "overlay-sitting-dark.png"))
$cw = $first.Width; $ch = $first.Height; $first.Dispose()
$sheet = New-Object System.Drawing.Bitmap ($cw * $states.Count), ($ch * 2)
$g = [System.Drawing.Graphics]::FromImage($sheet)
for ($row = 0; $row -lt 2; $row++) {
  $theme = @("dark", "light")[$row]
  for ($i = 0; $i -lt $states.Count; $i++) {
    $img = [System.Drawing.Image]::FromFile((Join-Path $Out "overlay-$($states[$i])-$theme.png"))
    $g.DrawImage($img, $i * $cw, $row * $ch, $cw, $ch)
    $img.Dispose()
  }
}
$g.Dispose()
$sheet.Save((Join-Path $Out "overlay-states.png"), [System.Drawing.Imaging.ImageFormat]::Png)
$sheet.Dispose()
Get-Item (Join-Path $Out "*.png") | Select-Object Name, Length
