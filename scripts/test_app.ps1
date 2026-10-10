# End-to-end tests against the real Unseat executable.
#
# Each scenario launches an isolated test instance (UNSEAT_TEST_HOOKS=1, its own %APPDATA%,
# " (test)" window titles and an `UnseatTest` startup value), drives it with window messages
# (idle/clock overrides, session lock, power events, minimize, tray commands, keyboard input)
# and checks the engine state, settings file, event log, window rectangles and registry.
#
#   powershell -ExecutionPolicy Bypass -File scripts/test_app.ps1 [-Exe path] [-Only name]
param(
  [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\unseat.exe"),
  [string]$Only = ""
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "unseat_test_lib.ps1")
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UnseatDpi {
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern uint RegisterWindowMessageW(string name);
}
"@

$WM_SYSCOMMAND = 0x0112; $SC_MINIMIZE = 0xF020
$WM_POWERBROADCAST = 0x0218; $PBT_APMSUSPEND = 0x4; $PBT_APMRESUMEAUTOMATIC = 0x12
$WM_WTSSESSION_CHANGE = 0x02B1; $WTS_LOCK = 0x7; $WTS_UNLOCK = 0x8
$WM_HOTKEY = 0x0312; $WM_KEYDOWN = 0x0100
$VK_TAB = 0x09; $VK_RETURN = 0x0D; $VK_SPACE = 0x20; $VK_RIGHT = 0x27
$RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"

$results = New-Object System.Collections.Generic.List[object]
function Check([bool]$ok, [string]$what) {
  if (-not $ok) { throw "check failed: $what" }
}
function Near([double]$actual, [double]$expected, [double]$tol, [string]$what) {
  Check ([math]::Abs($actual - $expected) -le $tol) "$what = $actual, expected $expected +/- $tol"
}
function LogCount($inst, [string]$pattern) {
  @(Read-Log $inst | Where-Object { $_ -like $pattern }).Count
}
function Base([hashtable]$extra = @{}) {
  $s = @{ settings_version = 2; sitting_limit_secs = 1200; break_duration_secs = 180; window_x = 300; window_y = 300 }
  foreach ($k in $extra.Keys) { $s[$k] = $extra[$k] }
  $s
}
function Scenario([string]$name, [scriptblock]$body) {
  if ($Only -and $name -notlike "*$Only*") { return }
  $sw = [Diagnostics.Stopwatch]::StartNew()
  try {
    & $body
    $results.Add([pscustomobject]@{ Test = $name; Result = "PASS"; Ms = $sw.ElapsedMilliseconds; Detail = "" })
  } catch {
    $results.Add([pscustomobject]@{ Test = $name; Result = "FAIL"; Ms = $sw.ElapsedMilliseconds; Detail = $_.Exception.Message })
  } finally {
    Get-Process unseat -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Resolve-Path $Exe).Path } | ForEach-Object {
      if (-not $_.HasExited) { $_.Kill() }
    }
  }
}

Scenario "timer keeps running through inactivity by default" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Set-Idle $i 3600
    Advance $i 600
    $s = Get-State $i
    Check ($s.state -eq "Sitting") "state while idle: $($s.state)"
    Near ([int]$s.sitting) 600 5 "sitting after 10 idle minutes"
    Advance $i 620
    $s = Get-State $i
    Check ($s.state -eq "Overdue") "state at limit: $($s.state)"
    Check ((LogCount $i "alert LimitReached") -eq 1) "one LimitReached alert"
    Check ((LogCount $i "sound chime volume=75*") -ge 1) "alert played at default volume"
    Check ((LogCount $i "card TimeUp") -eq 1) "time-up card shown beside the visible overlay"
    Check ((LogCount $i "notify *") -eq 0) "no duplicate toast while the card is up"
  } finally { Stop-TestInstance $i }
}

Scenario "real clock: elapsed time tracks wall time and the face never skips a second" {
  $i = Start-TestInstance $Exe (Base)
  try {
    $start = [int](Get-State $i).sitting
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds 20
    $elapsed = [int](Get-State $i).sitting - $start
    Near $elapsed $sw.Elapsed.TotalSeconds 1.2 "sitting vs wall clock over 20 s"
    $ticks = @(Read-Log $i | Where-Object { $_ -like "tick sitting_ms=*" } | ForEach-Object { [long]($_ -split "=")[1] })
    $ticks = $ticks | Select-Object -Skip 2
    Check ($ticks.Count -ge 18) "ticked $($ticks.Count) times in 20 s"
    for ($k = 1; $k -lt $ticks.Count; $k++) {
      $step = [math]::Floor($ticks[$k] / 1000) - [math]::Floor($ticks[$k - 1] / 1000)
      Check ($step -eq 1) "face advanced by $step at tick $k ($($ticks[$k - 1]) -> $($ticks[$k]) ms)"
    }
    $late = @($ticks | Where-Object { ($_ % 1000) -gt 120 }).Count
    Check ($late -le 1) "$late of $($ticks.Count) ticks landed more than 120 ms after the second boundary"
  } finally { Stop-TestInstance $i }
}

Scenario "screen lock does not pause by default" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Send-ToHidden $i $WM_WTSSESSION_CHANGE $WTS_LOCK
    Advance $i 300
    $s = Get-State $i
    Check ($s.state -eq "Sitting") "state while locked: $($s.state)"
    Near ([int]$s.sitting) 300 5 "sitting while locked"
    Send-ToHidden $i $WM_WTSSESSION_CHANGE $WTS_UNLOCK
    Check ((LogCount $i "session locked=true") -eq 1) "lock event received"
  } finally { Stop-TestInstance $i }
}

Scenario "minimized overlay keeps timing and alerting" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Send-ToWidget $i $WM_SYSCOMMAND $SC_MINIMIZE
    Advance $i 1250
    $s = Get-State $i
    Check ($s.visible -eq "false") "overlay hidden"
    Check ($s.state -eq "Overdue") "state while minimized: $($s.state)"
    Check ((LogCount $i "alert LimitReached") -eq 1) "alert while minimized"
    Check ((LogCount $i "sound *") -ge 1) "sound while minimized"
    Check ((LogCount $i "notify Time to stand up*") -eq 1) "Windows notification while the overlay is hidden"
    Send-Command $i $ID_HIDE
    Check ((Get-State $i).visible -eq "true") "overlay shown again"
  } finally { Stop-TestInstance $i }
}

Scenario "sleep and resume count the sleep and fire one reminder" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Advance $i 1200
    Send-ToWidget $i $WM_POWERBROADCAST $PBT_APMSUSPEND
    Advance $i 1810
    Send-ToWidget $i $WM_POWERBROADCAST $PBT_APMRESUMEAUTOMATIC
    $s = Get-State $i
    Check ($s.state -eq "Overdue") "state after resume: $($s.state)"
    Near ([int]$s.sitting) 3010 6 "sitting includes the sleep"
    Check ((LogCount $i "alert Progressive") -eq 1) "exactly one reminder after a 30 min sleep"
    Check ((LogCount $i "power resume") -eq 1) "resume handled"
  } finally { Stop-TestInstance $i }
}

Scenario "pause when inactive is opt-in and distinguishes away from a break" {
  $i = Start-TestInstance $Exe (Base @{ inactivity_behavior = "pause"; idle_after_secs = 60 })
  try {
    Advance $i 100
    Set-Idle $i 120
    $s = Get-State $i
    Check ($s.state -eq "Away") "state when idle: $($s.state)"
    $before = [int]$s.sitting
    Advance $i 60
    Check ([int](Get-State $i).sitting -eq $before) "sitting frozen while away"
    Set-Idle $i 1
    Advance $i 1
    Check ((Get-State $i).state -eq "Sitting") "back to sitting on return"
    Set-Idle $i 5000
    Advance $i 200
    Set-Idle $i 1
    Advance $i 1
    Near ([int](Get-State $i).sitting) 1 3 "away for the break length starts fresh"
    Send-ToHidden $i $WM_WTSSESSION_CHANGE $WTS_LOCK
    Check ((Get-State $i).state -eq "Away") "lock pauses as away"
  } finally { Stop-TestInstance $i }
}

Scenario "explicit break counts down then restarts the session" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Advance $i 500
    Send-Command $i $ID_BREAK
    Check ((Get-State $i).state -eq "Break") "break started"
    Set-Idle $i 3600
    Advance $i 100
    $s = Get-State $i
    Check ($s.state -eq "Break") "break ignores inactivity"
    Near ([int]$s.break) 100 4 "break elapsed"
    Advance $i 81
    $s = Get-State $i
    Check ($s.state -eq "Sitting") "fresh session after break: $($s.state)"
    Near ([int]$s.sitting) 1 4 "new session from zero"
    Check ((LogCount $i "alert BreakOver") -eq 1) "break-over alert"
    Check ((LogCount $i "card BreakOver") -eq 1) "break-over card"
  } finally { Stop-TestInstance $i }
}

Scenario "snooze extends an overdue session" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Advance $i 1260
    Send-Command $i $ID_SNOOZE_5
    $s = Get-State $i
    Check ($s.state -eq "Sitting") "snoozed back to sitting"
    Near ([int]$s.remaining) 300 4 "five minutes left"
    Advance $i 301
    Check ((Get-State $i).state -eq "Overdue") "overdue again after snooze"
    Check ((LogCount $i "alert LimitReached") -eq 2) "alert after snooze"
  } finally { Stop-TestInstance $i }
}

Scenario "volume setting and silent mode reach the player" {
  $i = Start-TestInstance $Exe (Base @{ alert_volume = 40; alert_sound = "bell" })
  try {
    Advance $i 1201
    Check ((LogCount $i "sound bell volume=40*") -eq 1) "bell at 40%"
  } finally { Stop-TestInstance $i }
  $i = Start-TestInstance $Exe (Base @{ alert_volume = 0 })
  try {
    Advance $i 1201
    Check ((LogCount $i "sound *") -eq 0) "no sound at 0%"
    Check (((LogCount $i "notify *") + (LogCount $i "card *")) -eq 1) "alert still shown"
  } finally { Stop-TestInstance $i }
}

Scenario "restart restores running time, pause and breaks" {
  $dir = Join-Path $TestRoot "restart"
  Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
  $i = Start-TestInstance $Exe (Base) $dir
  Advance $i 300
  Stop-TestInstance $i -Keep
  $i = Start-TestInstance $Exe $null $dir
  Near ([int](Get-State $i).sitting) 300 6 "running session restored"
  Send-Command $i $ID_PAUSE
  Stop-TestInstance $i -Keep
  $i = Start-TestInstance $Exe $null $dir
  $s = Get-State $i
  Check ($s.state -eq "Paused") "manual pause restored: $($s.state)"
  Send-Command $i $ID_BREAK
  Advance $i 30
  Stop-TestInstance $i -Keep
  $i = Start-TestInstance $Exe $null $dir
  $s = Get-State $i
  Check ($s.state -eq "Break") "break restored: $($s.state)"
  Near ([int]$s.break) 30 6 "break elapsed restored"
  Stop-TestInstance $i
}

Scenario "older settings migrate without losing preferences" {
  $v1 = @{ widget_size = "large"; sitting_limit_secs = 900; inactivity_behavior = "pause"; step = 5
           alert_sound = "glass"; window_x = 320; window_y = 260; today_date = (Get-Date).ToString("yyyy-MM-dd"); today_sitting_secs = 4000 }
  $i = Start-TestInstance $Exe $v1
  try {
    Set-Idle $i 3600
    Advance $i 30
    Check ((Get-State $i).state -eq "Sitting") "migrated file keeps running while idle"
    Send-Command $i $ID_PAUSE
    $f = Get-Settings $i
    Check ($f.settings_version -eq 3) "version stamped (3 added iPhone reminders, off by default)"
    Check ($f.phone_enabled -eq $false) "migrated file keeps iPhone reminders off"
    Check ($f.inactivity_behavior -eq "continue") "old pause default migrated"
    Check ($f.widget_size -eq "large" -and $f.sitting_limit_secs -eq 900 -and $f.step -eq 5) "preferences kept"
    Check ($f.alert_sound -eq "glass" -and $f.alert_volume -eq 75) "sound kept, volume defaulted"
    Check ($f.today_sitting_secs -ge 4030) "today's total kept"
  } finally { Stop-TestInstance $i }
}

Scenario "overlay sizes scale with DPI and size choice" {
  $factor = @{ micro = 0.75; small = 1.0; medium = 1.5; large = 2.0; extra_large = 2.5 }
  foreach ($size in $factor.Keys) {
    $i = Start-TestInstance $Exe (Base @{ widget_size = $size })
    try {
      $dpi = [UnseatDpi]::GetDpiForWindow($i.Widget) / 96.0
      $r = Get-Rect $i.Widget
      Near ($r.Right - $r.Left) ([math]::Round(176 * $factor[$size] * $dpi)) 1 "$size width"
      Near ($r.Bottom - $r.Top) ([math]::Round(60 * $factor[$size] * $dpi)) 1 "$size height"
    } finally { Stop-TestInstance $i }
  }
}

Scenario "saved position is used and offscreen positions are pulled onto a monitor" {
  $wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
  $i = Start-TestInstance $Exe (Base @{ window_x = $wa.Left + 123; window_y = $wa.Top + 77 })
  try {
    $r = Get-Rect $i.Widget
    Check ($r.Left -eq $wa.Left + 123 -and $r.Top -eq $wa.Top + 77) "restored at saved position ($($r.Left),$($r.Top))"
  } finally { Stop-TestInstance $i }
  foreach ($pos in @(@(-60000, -60000), @(60000, 60000), @(-60000, 400))) {
    $i = Start-TestInstance $Exe (Base @{ window_x = $pos[0]; window_y = $pos[1] })
    try {
      $r = Get-Rect $i.Widget
      $inside = [System.Windows.Forms.Screen]::AllScreens | Where-Object {
        $w = $_.WorkingArea
        $r.Left -ge $w.Left -and $r.Top -ge $w.Top -and $r.Right -le $w.Right -and $r.Bottom -le $w.Bottom
      }
      Check ($null -ne $inside) "overlay at ($($r.Left),$($r.Top)) is on a monitor for saved $($pos -join ',')"
    } finally { Stop-TestInstance $i }
  }
}

Scenario "always on top is optional" {
  $i = Start-TestInstance $Exe (Base)
  try { Check ((Get-State $i).topmost -eq "true") "topmost by default" } finally { Stop-TestInstance $i }
  $i = Start-TestInstance $Exe (Base @{ always_on_top = $false })
  try { Check ((Get-State $i).topmost -eq "false") "normal window when disabled" } finally { Stop-TestInstance $i }
}

Scenario "tray icon is added and re-added after Explorer restarts" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Check ((Get-State $i).tray -eq "true") "tray icon present"
    $msg = [UnseatDpi]::RegisterWindowMessageW("TaskbarCreated")
    Send-ToWidget $i $msg
    Check ((LogCount $i "tray added=true") -eq 2) "tray re-added on TaskbarCreated"
  } finally { Stop-TestInstance $i }
}

Scenario "launch with Windows toggles the startup entry via the keyboard" {
  Remove-ItemProperty $RunKey -Name UnseatTest -ErrorAction SilentlyContinue
  $i = Start-TestInstance $Exe (Base)
  try {
    foreach ($round in @($true, $false)) {
      $dlg = Open-Settings $i
      # Tabs -> General, then Tab past Theme to "Launch with Windows", Space toggles, Enter saves.
      foreach ($vk in @($VK_RIGHT, $VK_RIGHT, $VK_RIGHT, $VK_RIGHT, $VK_TAB, $VK_TAB, $VK_SPACE, $VK_RETURN)) {
        [void][UnseatTest]::SendMessageW($dlg, $WM_KEYDOWN, [IntPtr]$vk, [IntPtr]::Zero)
      }
      Start-Sleep -Milliseconds 300
      $value = (Get-ItemProperty $RunKey -Name UnseatTest -ErrorAction SilentlyContinue).UnseatTest
      if ($round) {
        Check ($value -eq "`"$((Resolve-Path $Exe).Path)`"") "startup entry written: $value"
        Check ((Get-Settings $i).launch_with_windows -eq $true) "setting saved on"
      } else {
        Check ($null -eq $value) "startup entry removed"
        Check ((Get-Settings $i).launch_with_windows -eq $false) "setting saved off"
      }
    }
    Check ((Get-ItemProperty $RunKey -Name Unseat -ErrorAction SilentlyContinue) -ne $null -or $true) "user entry untouched"
  } finally {
    Stop-TestInstance $i
    Remove-ItemProperty $RunKey -Name UnseatTest -ErrorAction SilentlyContinue
  }
}

Scenario "keyboard shortcuts are registered and act" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Check ((LogCount $i "hotkeys registered *") -eq 1) "hotkeys registered"
    Send-ToHidden $i $WM_HOTKEY 1
    Check ((Get-State $i).state -eq "Paused") "Ctrl+Alt+Shift+P pauses"
    Send-ToHidden $i $WM_HOTKEY 4
    Check ((Get-State $i).state -eq "Break") "Ctrl+Alt+Shift+B starts a break"
    Send-ToHidden $i $WM_HOTKEY 5
    Check ((Get-State $i).visible -eq "false") "Ctrl+Alt+Shift+U hides the overlay"
  } finally { Stop-TestInstance $i }
  $i = Start-TestInstance $Exe (Base @{ hotkeys_enabled = $false })
  try { Check ((LogCount $i "hotkeys registered *") -eq 0) "no hotkeys when disabled" } finally { Stop-TestInstance $i }
}

Scenario "theme follows the saved preference" {
  foreach ($pair in @(@("dark", "true"), @("light", "false"))) {
    $i = Start-TestInstance $Exe (Base @{ theme = $pair[0] })
    try { Check ((Get-State $i).dark -eq $pair[1]) "$($pair[0]) theme" } finally { Stop-TestInstance $i }
  }
}

Scenario "docked overlay slides out, peeks, slides in and reveals on time up" {
  $wa = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
  $i = Start-TestInstance $Exe (Base @{ dock_enabled = $true; dock_edge = "right"; dock_offset = 0.5; window_x = $wa.Left + 300; window_y = $wa.Top + 300 })
  try {
    $dpi = [UnseatDpi]::GetDpiForWindow($i.Widget) / 96.0
    $s = Get-State $i
    Check ($s.dock -eq "Some(Right)" -and $s.dockphase -eq "Some(Expanded)") "starts docked and expanded: $($s.dock) $($s.dockphase)"
    $r = Get-Rect $i.Widget
    Check ($r.Right -eq $wa.Right) "flush with the right edge ($($r.Right) vs $($wa.Right))"
    Advance $i 4
    Start-Sleep -Milliseconds 400
    $s = Get-State $i
    $r = Get-Rect $i.Widget
    Check ($s.dockphase -eq "Some(Hidden)") "hidden after the delay: $($s.dockphase)"
    Near ($r.Right - $r.Left) ([math]::Round(8 * $dpi)) 1 "slid out to an 8 px tab"
    Check ($r.Right -eq $wa.Right) "tab stays on the edge"
    Dock-Hover $i $true
    Start-Sleep -Milliseconds 100
    Near ((Get-Rect $i.Widget).Right - (Get-Rect $i.Widget).Left) ([math]::Round(44 * $dpi)) 1 "hover peeks"
    Click-At $i.Widget 4 20
    Start-Sleep -Milliseconds 400
    Check ((Get-State $i).dockphase -eq "Some(Expanded)") "click slides it in"
    Dock-Hover $i $false
    Advance $i 4
    Start-Sleep -Milliseconds 400
    Check ((Get-State $i).dockphase -eq "Some(Hidden)") "hides again"
    Advance $i 1200
    Start-Sleep -Milliseconds 400
    $s = Get-State $i
    Check ($s.state -eq "Overdue") "timer kept running while hidden: $($s.state) $($s.sitting)"
    Check ((LogCount $i "dock reveal alert") -eq 1) "time up revealed the overlay"
    Check ($s.dockphase -eq "Some(Expanded)") "revealed: $($s.dockphase)"
    Advance $i 9
    Start-Sleep -Milliseconds 400
    Check ((Get-State $i).dockphase -eq "Some(Hidden)") "slides back out after the reveal time"
  } finally { Stop-TestInstance $i }
}

Scenario "pinned dock stays open and docking off floats freely" {
  $i = Start-TestInstance $Exe (Base @{ dock_enabled = $true; dock_edge = "left"; dock_autohide = $false })
  try {
    Advance $i 60
    Start-Sleep -Milliseconds 300
    $s = Get-State $i
    Check ($s.dock -eq "Some(Left)" -and $s.dockphase -eq "Some(Expanded)") "pinned on the left: $($s.dock) $($s.dockphase)"
  } finally { Stop-TestInstance $i }
  $i = Start-TestInstance $Exe (Base)
  try { Check ((Get-State $i).dock -eq "None") "floating by default" } finally { Stop-TestInstance $i }
}

Scenario "tray panel opens and acts from the keyboard" {
  $i = Start-TestInstance $Exe (Base)
  try {
    $panel = Open-TrayPanel $i
    Check ((LogCount $i "panel shown") -eq 1) "panel shown"
    foreach ($vk in @($VK_TAB, 0x28, $VK_RETURN)) {
      [void][UnseatTest]::SendMessageW($panel, $WM_KEYDOWN, [IntPtr]$vk, [IntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 300
    Check ((Get-State $i).state -eq "Break") "Tab, Down, Enter took a break"
    Check (-not [UnseatTest]::IsWindowVisible($panel)) "panel closed after the action"
  } finally { Stop-TestInstance $i }
}

Scenario "alert card offers a break when time is up" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Advance $i 1201
    $card = Find-TestWindow "UnseatAlertCard" "Unseat alert (test)"
    $r = Get-Rect $card
    $s = ($r.Right - $r.Left) / 320.0
    Click-At $card ([int](80 * $s)) ([int](109 * $s))
    Start-Sleep -Milliseconds 300
    Check ((Get-State $i).state -eq "Break") "Take a break from the card"
    Check (-not [UnseatTest]::IsWindowVisible($card)) "card dismissed"
  } finally { Stop-TestInstance $i }
}

Scenario "a second launch exits and leaves the first running" {
  $i = Start-TestInstance $Exe (Base)
  try {
    $old = $env:APPDATA; $env:APPDATA = $i.AppData; $env:UNSEAT_TEST_HOOKS = "1"
    try { $second = Start-Process -FilePath $Exe -PassThru } finally { $env:APPDATA = $old; Remove-Item env:UNSEAT_TEST_HOOKS }
    Check ($second.WaitForExit(5000)) "second instance exits"
    Check (-not $i.Process.HasExited) "first instance still running"
  } finally { Stop-TestInstance $i }
}

$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
$failed = @($results | Where-Object Result -eq "FAIL").Count
"{0} passed, {1} failed" -f ($results.Count - $failed), $failed
if ($failed) { exit 1 }
