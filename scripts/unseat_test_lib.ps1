# Helpers for driving a real Unseat test instance (dot-source this file).
# A test instance runs with UNSEAT_TEST_HOOKS=1 and its own %APPDATA%, so it never touches
# the user's settings, startup entry or running Unseat. Each instance also gets its own
# UNSEAT_TEST_ID, so its windows ("Unseat (test <id>)") and single-instance lock are its
# own: helpers only ever find the instance they started. Scripts stop only processes they
# started, and refuse to run while another test instance is up (they share test ports).

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
  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lp);
  [DllImport("user32.dll")]
  public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern int GetClassNameW(IntPtr hWnd, System.Text.StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern int GetWindowTextW(IntPtr hWnd, System.Text.StringBuilder s, int n);
  public struct RECT { public int Left, Top, Right, Bottom; }
  // Every Unseat overlay window (shown or not): "pid<TAB>title" per line.
  public static string Overlays() {
    var sb = new System.Text.StringBuilder();
    EnumWindows((h, l) => {
      var c = new System.Text.StringBuilder(64); GetClassNameW(h, c, 64);
      if (c.ToString() == "UnseatWidget") {
        var t = new System.Text.StringBuilder(128); GetWindowTextW(h, t, 128);
        uint pid; GetWindowThreadProcessId(h, out pid);
        sb.Append(pid).Append('\t').Append(t.ToString()).Append('\n');
      }
      return true;
    }, IntPtr.Zero);
    return sb.ToString();
  }
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
# Processes this script started; the only ones it may ever stop.
$script:TestPids = New-Object System.Collections.Generic.List[int]

# Unseat overlays now open: Pid, Title, Test (a test instance) and Id (its run id).
function Get-Overlays {
  @([UnseatTest]::Overlays() -split "`n" | Where-Object { $_ } | ForEach-Object {
    $pid_, $title = $_ -split "`t", 2
    $m = [regex]::Match($title, '^Unseat \(test(?: (?<id>[^)]+))?\)$')
    [pscustomobject]@{ Pid = [int]$pid_; Title = $title; Test = $m.Success; Id = $m.Groups["id"].Value }
  })
}
# The user's own Unseat processes (never a test instance), to prove a run left them alone.
function Get-RealUnseat { @(Get-Overlays | Where-Object { -not $_.Test } | ForEach-Object { $_.Pid } | Sort-Object -Unique) }
# Test instances share test ports and the UnseatTest startup value: one suite at a time.
function Assert-NoOtherTestInstance {
  $others = @(Get-Overlays | Where-Object { $_.Test -and -not $script:TestPids.Contains($_.Pid) })
  if ($others.Count) {
    throw "Another Unseat test instance is running ($(@($others | ForEach-Object { "pid $($_.Pid) '$($_.Title)'" }) -join ', ')). Stop it before running these tests."
  }
}
function TestTitle($inst, [string]$base) { "$base (test $($inst.Id))" }

function Wait-Until([scriptblock]$cond, [int]$ms = 8000) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $ms) {
    $v = & $cond
    if ($v) { return $v }
    Start-Sleep -Milliseconds 50
  }
  throw "timed out waiting for $cond"
}

# Launch $exe as test instance $id with %APPDATA% = $appdata (and optionally hidden).
function Start-UnseatProcess([string]$exe, [string]$appdata, [string]$id, [switch]$Hidden) {
  $saved = @{}
  foreach ($k in "APPDATA", "UNSEAT_TEST_HOOKS", "UNSEAT_TEST_ID", "UNSEAT_TEST_HIDDEN") { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
  $env:APPDATA = $appdata; $env:UNSEAT_TEST_HOOKS = "1"; $env:UNSEAT_TEST_ID = $id
  if ($Hidden) { $env:UNSEAT_TEST_HIDDEN = "1" } else { Remove-Item env:UNSEAT_TEST_HIDDEN -ErrorAction SilentlyContinue }
  try { $proc = Start-Process -FilePath $exe -PassThru } finally {
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
  }
  $script:TestPids.Add($proc.Id)
  $proc
}

function Start-TestInstance([string]$exe, $settings = $null, [string]$appdata = $null, [switch]$Hidden, [string]$Id = "") {
  Assert-NoOtherTestInstance
  if (-not $appdata) { $appdata = Join-Path $TestRoot ([guid]::NewGuid().ToString("N").Substring(0, 8)) }
  if (-not $Id) { $Id = "t" + [guid]::NewGuid().ToString("N").Substring(0, 8) }
  $dir = Join-Path $appdata "Unseat"
  New-Item -ItemType Directory -Force $dir | Out-Null
  if ($null -ne $settings) {
    ($settings | ConvertTo-Json -Depth 6) | Set-Content -Encoding Ascii (Join-Path $dir "settings.json")
  }
  $proc = Start-UnseatProcess $exe $appdata $Id -Hidden:$Hidden
  $title = "Unseat (test $Id)"
  $widget = Wait-Until { $h = [UnseatTest]::FindWindowW("UnseatWidget", $title); if ($h -ne [IntPtr]::Zero) { $h } }
  $hiddenWnd = Wait-Until { $h = [UnseatTest]::FindWindowExW([IntPtr](-3), [IntPtr]::Zero, "UnseatHidden", $title); if ($h -ne [IntPtr]::Zero) { $h } }
  $owner = 0; [void][UnseatTest]::GetWindowThreadProcessId($widget, [ref]$owner)
  if ($owner -ne $proc.Id) { throw "test window $title belongs to pid $owner, not the instance just started ($($proc.Id))" }
  Start-Sleep -Milliseconds 300
  [pscustomobject]@{ Process = $proc; AppData = $appdata; Dir = $dir; Widget = $widget; Hidden = $hiddenWnd; Exe = $exe; Id = $Id }
}

# Launch another copy of $exe as the same test instance (same id and %APPDATA%), as a user
# opening Unseat twice would.
function Start-SameInstance($inst, [string]$exe = $inst.Exe) { Start-UnseatProcess $exe $inst.AppData $inst.Id }

# Stop every process this script started, and nothing else.
function Stop-AllTestInstances {
  foreach ($p in @($script:TestPids)) {
    $proc = Get-Process -Id $p -ErrorAction SilentlyContinue
    if ($proc -and -not $proc.HasExited) { $proc.Kill(); [void]$proc.WaitForExit(3000) }
  }
  $script:TestPids.Clear()
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
  $t = TestTitle $inst "Unseat Settings"
  $dlg = Wait-Until { $h = [UnseatTest]::FindWindowW("UnseatSettings", $t); if ($h -ne [IntPtr]::Zero) { $h } }
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
  $panel = Find-TestWindow "UnseatTrayPanel" (TestTitle $inst "Unseat panel")
  Start-Sleep -Milliseconds 300
  $panel
}

function Dock-Hover($inst, [bool]$on) {
  [void][UnseatTest]::SendMessageW($inst.Widget, $MSG_DOCK_HOVER, [IntPtr]([int]$on), [IntPtr]::Zero)
}
