# Helpers for driving a real Unseat test instance (dot-source this file).
# A test instance runs with UNSEAT_TEST_HOOKS=1 and its own %APPDATA%, so it never touches
# the user's settings, startup entry or running Unseat.

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UnseatTest {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")]
  public static extern IntPtr SendMessageW(IntPtr hWnd, uint msg, IntPtr wp, IntPtr lp);
  [DllImport("user32.dll")]
  public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wp, IntPtr lp);
  [DllImport("user32.dll")]
  public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")]
  public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")]
  public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][UnseatTest]::SetProcessDpiAwarenessContext([IntPtr](-4))

$WM_APP = 0x8000
$WM_COMMAND = 0x0111
$MSG_SET_IDLE = $WM_APP + 0x40
$MSG_ADVANCE = $WM_APP + 0x41
$MSG_DUMP = $WM_APP + 0x42
$MSG_SNAPSHOT = $WM_APP + 0x43
$MSG_DOCK_HOVER = $WM_APP + 0x44
$TRAY_CALLBACK = $WM_APP + 1
$ID_PAUSE = 10; $ID_RESET = 11; $ID_SETTINGS = 12; $ID_QUIT = 13; $ID_HIDE = 14
$ID_SNOOZE_5 = 20; $ID_SNOOZE_CUSTOM = 24; $ID_BREAK = 25
$TestRoot = Join-Path (Split-Path -Parent $PSScriptRoot) "target\test-appdata"

function Wait-Until([scriptblock]$cond, [int]$ms = 8000) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $ms) {
    $v = & $cond
    if ($v) { return $v }
    Start-Sleep -Milliseconds 50
  }
  throw "timed out waiting for $cond"
}

function Start-TestInstance([string]$exe, $settings = $null, [string]$appdata = $null) {
  if (-not $appdata) { $appdata = Join-Path $TestRoot ([guid]::NewGuid().ToString("N").Substring(0, 8)) }
  $dir = Join-Path $appdata "Unseat"
  New-Item -ItemType Directory -Force $dir | Out-Null
  if ($null -ne $settings) {
    ($settings | ConvertTo-Json -Depth 6) | Set-Content -Encoding Ascii (Join-Path $dir "settings.json")
  }
  $oldAppData = $env:APPDATA; $oldHooks = $env:UNSEAT_TEST_HOOKS
  $env:APPDATA = $appdata; $env:UNSEAT_TEST_HOOKS = "1"
  try { $proc = Start-Process -FilePath $exe -PassThru } finally {
    $env:APPDATA = $oldAppData; $env:UNSEAT_TEST_HOOKS = $oldHooks
  }
  $widget = Wait-Until { $h = [UnseatTest]::FindWindowW("UnseatWidget", "Unseat (test)"); if ($h -ne [IntPtr]::Zero) { $h } }
  $hidden = Wait-Until { $h = [UnseatTest]::FindWindowExW([IntPtr](-3), [IntPtr]::Zero, "UnseatHidden", "Unseat (test)"); if ($h -ne [IntPtr]::Zero) { $h } }
  Start-Sleep -Milliseconds 300
  [pscustomobject]@{ Process = $proc; AppData = $appdata; Dir = $dir; Widget = $widget; Hidden = $hidden; Exe = $exe }
}

function Stop-TestInstance($inst, [switch]$Keep) {
  if (-not $inst.Process.HasExited) {
    [void][UnseatTest]::SendMessageW($inst.Hidden, $WM_COMMAND, [IntPtr]$ID_QUIT, [IntPtr]::Zero)
    if (-not $inst.Process.WaitForExit(5000)) { Stop-Process -Id $inst.Process.Id -Force }
  }
  if (-not $Keep) { Remove-Item -Recurse -Force $inst.AppData -ErrorAction SilentlyContinue }
}

function Advance($inst, [int]$secs) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $MSG_ADVANCE, [IntPtr]$secs, [IntPtr]::Zero)
}

function Set-Idle($inst, [long]$secs) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $MSG_SET_IDLE, [IntPtr]$secs, [IntPtr]::Zero)
}

function Send-Command($inst, [int]$id) {
  [void][UnseatTest]::SendMessageW($inst.Hidden, $WM_COMMAND, [IntPtr]$id, [IntPtr]::Zero)
}

function Send-ToWidget($inst, [uint32]$msg, [long]$wp = 0, [long]$lp = 0) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $msg, [IntPtr]$wp, [IntPtr]$lp)
}

function Send-ToHidden($inst, [uint32]$msg, [long]$wp = 0, [long]$lp = 0) {
  [void][UnseatTest]::SendMessageW($inst.Hidden, $msg, [IntPtr]$wp, [IntPtr]$lp)
}

function Read-Log($inst) {
  $p = Join-Path $inst.Dir "test-events.log"
  if (Test-Path $p) { Get-Content $p } else { @() }
}

function Get-State($inst) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $MSG_DUMP, [IntPtr]::Zero, [IntPtr]::Zero)
  $line = Read-Log $inst | Where-Object { $_ -like "state *" } | Select-Object -Last 1
  $parts = $line -split " "
  $state = @{ state = $parts[1] }
  foreach ($kv in $parts[2..($parts.Count - 1)]) {
    $k, $v = $kv -split "=", 2
    $state[$k] = $v
  }
  $state
}

function Get-Settings($inst) {
  Get-Content -Raw (Join-Path $inst.Dir "settings.json") | ConvertFrom-Json
}

function Get-Rect([IntPtr]$hwnd) {
  $r = New-Object UnseatTest+RECT
  [void][UnseatTest]::GetWindowRect($hwnd, [ref]$r)
  $r
}

function Get-OverlayBitmap($inst, [switch]$Hover) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $MSG_SNAPSHOT, [IntPtr]([int][bool]$Hover), [IntPtr]::Zero)
  $bytes = [IO.File]::ReadAllBytes((Join-Path $inst.Dir "overlay.bgra"))
  $w = [BitConverter]::ToInt32($bytes, 0); $h = [BitConverter]::ToInt32($bytes, 4)
  $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format32bppPArgb)
  $data = $bmp.LockBits((New-Object System.Drawing.Rectangle 0, 0, $w, $h), [System.Drawing.Imaging.ImageLockMode]::WriteOnly, $bmp.PixelFormat)
  for ($y = 0; $y -lt $h; $y++) {
    [Runtime.InteropServices.Marshal]::Copy($bytes, 8 + $y * $w * 4, [IntPtr]($data.Scan0.ToInt64() + $y * $data.Stride), $w * 4)
  }
  $bmp.UnlockBits($data)
  $bmp
}

function Open-Settings($inst) {
  Send-Command $inst $ID_SETTINGS
  $dlg = Wait-Until { $h = [UnseatTest]::FindWindowW("UnseatSettings", "Unseat Settings (test)"); if ($h -ne [IntPtr]::Zero) { $h } }
  [void][UnseatTest]::SetForegroundWindow($dlg)
  Start-Sleep -Milliseconds 400
  $dlg
}

function Click-At([IntPtr]$hwnd, [int]$x, [int]$y) {
  $lp = [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF))
  [void][UnseatTest]::SendMessageW($hwnd, 0x0201, [IntPtr]1, $lp)
  [void][UnseatTest]::SendMessageW($hwnd, 0x0202, [IntPtr]0, $lp)
}

function Click-SettingsTab([IntPtr]$dlg, [int]$index) {
  # Five equal tab chips across 400 - 2 * 16 logical px with 4 px gaps, centred at y = 77.
  $r = Get-Rect $dlg
  $s = ($r.Right - $r.Left) / 400.0
  $seg = (368 - 16) / 5.0
  $x = [int]((16 + ($seg + 4) * $index + $seg / 2) * $s)
  $y = [int](77 * $s)
  Click-At $dlg $x $y
  Start-Sleep -Milliseconds 250
}

function Close-Window([IntPtr]$hwnd) {
  [void][UnseatTest]::PostMessageW($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 300
}

function Find-TestWindow([string]$class, [string]$title, [int]$ms = 4000) {
  Wait-Until { $h = [UnseatTest]::FindWindowW($class, $title); if ($h -ne [IntPtr]::Zero -and [UnseatTest]::IsWindowVisible($h)) { $h } } $ms
}

function Open-TrayPanel($inst) {
  [void][UnseatTest]::SendMessageW($inst.Hidden, $TRAY_CALLBACK, [IntPtr]::Zero, [IntPtr]0x007B)
  $panel = Find-TestWindow "UnseatTrayPanel" "Unseat panel (test)"
  Start-Sleep -Milliseconds 300
  $panel
}

function Dock-Hover($inst, [bool]$on) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $MSG_DOCK_HOVER, [IntPtr]([int]$on), [IntPtr]::Zero)
}
