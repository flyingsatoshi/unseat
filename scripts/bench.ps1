# Reproducible benchmark for one or more Unseat builds: executable size, startup, idle CPU,
# memory, handles, GDI/USER objects, threads, timer accuracy, overlay repaint cost, window
# opening, settings writes, handle leaks and shutdown.
#
# Every run is an isolated test copy (UNSEAT_TEST_HOOKS=1, its own %APPDATA% and run id), so it
# sits beside your own Unseat and never touches it, its settings or its startup entry. Builds
# are interleaved and alternate order each round, so drift on the machine hits all of them.
# Test copies log every tick to test-events.log; that cost is identical for every build.
#
#   powershell -ExecutionPolicy Bypass -File scripts\bench.ps1 -Exes base.exe,new.exe
#       [-Rounds 5] [-IdleSeconds 60] [-Cycles 10] [-Phone] [-Extra '"widget_size":"medium"']
#       [-Out bench.json] [-Budgets scripts\perf-budgets.json [-Strict]]
#
# -Phone turns iPhone reminders on in the test copy (listener and sender threads on test ports);
# -Extra adds settings. The idle window starts after the first settings write (about a minute
# in), so each run takes roughly IdleSeconds + 100 s. Reports medians across rounds, p95 for
# per-event latencies, and the change from the first build. Results depend on the hardware:
# compare builds measured in the same run, not numbers from different machines.
param(
  [string[]]$Exes,
  [int]$Rounds = 5,
  [int]$IdleSeconds = 60,
  [int]$Cycles = 10,
  [switch]$Phone,
  # More settings for the test copy, as JSON members: '"dock_enabled":true,"widget_size":"medium"'
  [string]$Extra = "",
  [string]$Out = "",
  # Compare the last build's medians with the limits in this file (scripts\perf-budgets.json).
  [string]$Budgets = "",
  # With -Budgets: exit 1 if a limit is exceeded. Without it the check only reports.
  [switch]$Strict
)
$ErrorActionPreference = "Stop"
$Exes = @($Exes | ForEach-Object { $_ -split "," } | Where-Object { $_ } | ForEach-Object { (Resolve-Path $_).Path })
if (-not $Exes.Count) { throw "Pass -Exes a.exe[,b.exe]" }

Add-Type -TypeDefinition @"
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

public static class UnseatBench {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint m, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
  [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr process, uint flags);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  [DllImport("kernel32.dll")] public static extern bool QueryProcessCycleTime(IntPtr process, out ulong cycles);
  [DllImport("kernel32.dll")] public static extern bool GetProcessHandleCount(IntPtr process, out uint count);
  [DllImport("kernel32.dll")] public static extern bool GetProcessIoCounters(IntPtr process, out IO_COUNTERS c);
  [DllImport("psapi.dll")] public static extern bool GetProcessMemoryInfo(IntPtr process, out PMC c, uint size);

  [StructLayout(LayoutKind.Sequential)] public struct IO_COUNTERS {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount, ReadTransferCount, WriteTransferCount, OtherTransferCount;
  }
  [StructLayout(LayoutKind.Sequential)] public struct PMC {
    public uint cb, PageFaultCount;
    public UIntPtr PeakWorkingSetSize, WorkingSetSize, QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage, QuotaPeakNonPagedPoolUsage,
      QuotaNonPagedPoolUsage, PagefileUsage, PeakPagefileUsage, PrivateUsage;
  }

  public static PMC Memory(IntPtr process) {
    PMC c; GetProcessMemoryInfo(process, out c, (uint)Marshal.SizeOf(typeof(PMC))); return c;
  }
  public static ulong Cycles(IntPtr process) { ulong c; QueryProcessCycleTime(process, out c); return c; }
  public static uint Handles(IntPtr process) { uint c; GetProcessHandleCount(process, out c); return c; }
  public static IO_COUNTERS Io(IntPtr process) { IO_COUNTERS c; GetProcessIoCounters(process, out c); return c; }

  // Launch a test copy and time it: [0] = ms until its overlay window is visible, [1] = ms until
  // its message loop answers (first paint done, ready for input). -1 if it never got there.
  public static double[] Launch(string exe, string appdata, string id, out Process proc) {
    var psi = new ProcessStartInfo(exe);
    psi.UseShellExecute = false;
    psi.EnvironmentVariables["APPDATA"] = appdata;
    psi.EnvironmentVariables["UNSEAT_TEST_HOOKS"] = "1";
    psi.EnvironmentVariables["UNSEAT_TEST_ID"] = id;
    psi.EnvironmentVariables.Remove("UNSEAT_TEST_HIDDEN");
    string title = "Unseat (test " + id + ")";
    var sw = Stopwatch.StartNew();
    proc = Process.Start(psi);
    double visible = -1, ready = -1;
    IntPtr widget = IntPtr.Zero;
    while (sw.ElapsedMilliseconds < 15000) {
      widget = FindWindowW("UnseatWidget", title);
      if (widget != IntPtr.Zero && IsWindowVisible(widget)) { visible = sw.Elapsed.TotalMilliseconds; break; }
      Thread.SpinWait(2000);
    }
    if (widget != IntPtr.Zero) {
      IntPtr r;
      if (SendMessageTimeoutW(widget, 0, IntPtr.Zero, IntPtr.Zero, 0, 15000, out r) != IntPtr.Zero) ready = sw.Elapsed.TotalMilliseconds;
    }
    return new double[] { visible, ready };
  }

  // Milliseconds for one synchronous message round trip.
  public static double Send(IntPtr h, uint msg, long wp, long lp) {
    var sw = Stopwatch.StartNew();
    SendMessageW(h, msg, new IntPtr(wp), new IntPtr(lp));
    return sw.Elapsed.TotalMilliseconds;
  }

  // Open a window with a message to the hidden window and wait until it is visible and painted.
  public static double OpenAndPaint(IntPtr hidden, uint msg, long wp, long lp, string cls, string title) {
    var sw = Stopwatch.StartNew();
    PostMessageW(hidden, msg, new IntPtr(wp), new IntPtr(lp));
    IntPtr h = IntPtr.Zero;
    while (sw.ElapsedMilliseconds < 5000) {
      h = FindWindowW(cls, title);
      if (h != IntPtr.Zero && IsWindowVisible(h)) break;
      Thread.SpinWait(500);
    }
    if (h == IntPtr.Zero) return -1;
    UpdateWindow(h);
    IntPtr r;
    SendMessageTimeoutW(h, 0, IntPtr.Zero, IntPtr.Zero, 0, 5000, out r);
    return sw.Elapsed.TotalMilliseconds;
  }
}
"@
[void][UnseatBench]::SetProcessDpiAwarenessContext([IntPtr](-4))

$WM_APP = 0x8000; $WM_COMMAND = 0x0111; $WM_CLOSE = 0x0010; $WM_KEYDOWN = 0x0100; $WM_MOUSEMOVE = 0x0200
$WM_MOUSELEAVE = 0x02A3
$MSG_ADVANCE = $WM_APP + 0x41; $TRAY_CALLBACK = $WM_APP + 1
$ID_RESET = 11; $ID_SETTINGS = 12; $ID_QUIT = 13
$NIN_POPUPOPEN = 0x406; $NIN_POPUPCLOSE = 0x407; $WM_CONTEXTMENU = 0x7B; $VK_ESCAPE = 0x1B
$root = Join-Path (Split-Path -Parent $PSScriptRoot) "target\test-appdata\bench"
New-Item -ItemType Directory -Force $root | Out-Null

function Pct([double[]]$v, [double]$p) {
  $s = @($v | Where-Object { $_ -ge 0 } | Sort-Object)
  if (-not $s.Count) { return $null }
  $i = [math]::Min($s.Count - 1, [math]::Max(0, [math]::Ceiling($p / 100 * $s.Count) - 1))
  $s[$i]
}
function Median([double[]]$v) {
  $s = @($v | Where-Object { $null -ne $_ -and $_ -ge 0 } | Sort-Object)
  if (-not $s.Count) { return $null }
  if ($s.Count % 2) { $s[($s.Count - 1) / 2] } else { ($s[$s.Count / 2 - 1] + $s[$s.Count / 2]) / 2 }
}
function Snapshot($p) {
  $p.Refresh()
  $m = [UnseatBench]::Memory($p.Handle)
  [pscustomobject]@{
    PrivateMB = [math]::Round($m.PrivateUsage.ToUInt64() / 1MB, 2)
    WorkingSetMB = [math]::Round($m.WorkingSetSize.ToUInt64() / 1MB, 2)
    PeakWorkingSetMB = [math]::Round($m.PeakWorkingSetSize.ToUInt64() / 1MB, 2)
    Handles = [UnseatBench]::Handles($p.Handle)
    Gdi = [UnseatBench]::GetGuiResources($p.Handle, 0)
    User = [UnseatBench]::GetGuiResources($p.Handle, 1)
    Threads = $p.Threads.Count
  }
}
function Assert-NoOtherTestCopy {
  $others = @(Get-Process unseat -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -like "Unseat (test*" })
  if ($others.Count) { throw "Another Unseat test copy is running (pid $($others[0].Id)); stop it first." }
}
function Find-Hidden([string]$title) {
  [UnseatBench]::FindWindowExW([IntPtr](-3), [IntPtr]::Zero, "UnseatHidden", $title)
}
function LogLines([string]$dir) {
  $p = Join-Path $dir "test-events.log"
  if (Test-Path $p) { @(Get-Content $p) } else { @() }
}

function Measure-Build([string]$exe, [int]$round) {
  $id = "b" + [guid]::NewGuid().ToString("N").Substring(0, 8)
  $appdata = Join-Path $root $id
  $dir = Join-Path $appdata "Unseat"
  New-Item -ItemType Directory -Force $dir | Out-Null
  # Silent alerts (the time-up step below would chime otherwise); everything else is default.
  $phoneOn = if ($Phone) { "true" } else { "false" }
  $more = if ($Extra) { ",$Extra" } else { "" }
  "{`"settings_version`":3,`"sound_enabled`":false,`"phone_enabled`":$phoneOn$more}" | Set-Content -Encoding Ascii (Join-Path $dir "settings.json")
  # A copy Windows has never run: the first launch pays for the image load and any on-access scan.
  $fresh = Join-Path $appdata "unseat.exe"
  Copy-Item -LiteralPath $exe -Destination $fresh
  $title = "Unseat (test $id)"
  $row = [ordered]@{ Build = $exe; Round = $round }
  $proc = $null
  try {
    $t = [UnseatBench]::Launch($fresh, $appdata, $id, [ref]$proc)
    $row.FirstVisibleMs = [math]::Round($t[0], 1); $row.FirstReadyMs = [math]::Round($t[1], 1)
    if ($t[1] -lt 0) { throw "test copy never became ready" }
    $widget = [UnseatBench]::FindWindowW("UnseatWidget", $title)
    $hidden = Find-Hidden $title
    # Every build writes its settings about a minute after start; measure idle after that, so
    # the write count shows the steady cadence rather than that first write.
    $settingsWrites = 0
    $fsw = New-Object IO.FileSystemWatcher $dir, "settings.json.tmp"
    $fsw.IncludeSubdirectories = $false
    $fsw.EnableRaisingEvents = $true
    [void]$fsw.WaitForChanged([IO.WatcherChangeTypes]::Created, 75000)
    Start-Sleep -Seconds 2

    # Idle: CPU, memory drift, settings writes and the tick schedule.
    $logBefore = (LogLines $dir).Count
    $before = Snapshot $proc
    $c0 = [UnseatBench]::Cycles($proc.Handle); $cpu0 = $proc.TotalProcessorTime; $io0 = [UnseatBench]::Io($proc.Handle)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $deadline = $IdleSeconds * 1000
    while ($sw.ElapsedMilliseconds -lt $deadline) {
      $e = $fsw.WaitForChanged([IO.WatcherChangeTypes]::Created, [int][math]::Max(1, $deadline - $sw.ElapsedMilliseconds))
      if (-not $e.TimedOut) { $settingsWrites++ }
    }
    $wall = $sw.Elapsed.TotalSeconds
    $proc.Refresh()
    $c1 = [UnseatBench]::Cycles($proc.Handle); $cpu1 = $proc.TotalProcessorTime; $io1 = [UnseatBench]::Io($proc.Handle)
    $fsw.Dispose()
    $after = Snapshot $proc
    $row.IdleMcyclesPerMin = [math]::Round(($c1 - $c0) / 1e6 * 60 / $wall, 2)
    $row.IdleCpuMsPerMin = [math]::Round(($cpu1 - $cpu0).TotalMilliseconds * 60 / $wall, 1)
    $row.IdleWritesPerHour = [math]::Round(($io1.WriteOperationCount - $io0.WriteOperationCount) * 3600 / $wall)
    $row.IdleSettingsSavesPerHour = [math]::Round($settingsWrites * 3600 / $wall, 1)
    $row.IdlePrivateMB = $after.PrivateMB; $row.IdleWorkingSetMB = $after.WorkingSetMB
    $row.IdlePrivateDriftKB = [math]::Round(($after.PrivateMB - $before.PrivateMB) * 1024)
    $row.IdleHandles = $after.Handles; $row.IdleGdi = $after.Gdi; $row.IdleUser = $after.User; $row.IdleThreads = $after.Threads

    $ticks = @(LogLines $dir | Select-Object -Skip $logBefore | Where-Object { $_ -like "tick sitting_ms=*" } | ForEach-Object { [long]($_ -split "=")[1] })
    $late = @($ticks | ForEach-Object { $_ % 1000 })
    $bad = 0
    for ($k = 1; $k -lt $ticks.Count; $k++) {
      if ([math]::Floor($ticks[$k] / 1000) - [math]::Floor($ticks[$k - 1] / 1000) -ne 1) { $bad++ }
    }
    $row.TicksPerMin = [math]::Round($ticks.Count * 60 / $wall, 1)
    $row.TickLateMedianMs = Median $late
    $row.TickLateP95Ms = Pct $late 95
    $row.TickSkippedOrRepeated = $bad

    # Overlay repaint: hover on and off redraws the pill each time.
    $hover = New-Object System.Collections.Generic.List[double]
    $c0 = [UnseatBench]::Cycles($proc.Handle)
    for ($k = 0; $k -lt 100; $k++) {
      $hover.Add([UnseatBench]::Send($widget, $WM_MOUSEMOVE, 0, 0x00140014))
      $hover.Add([UnseatBench]::Send($widget, $WM_MOUSELEAVE, 0, 0))
    }
    $c1 = [UnseatBench]::Cycles($proc.Handle)
    $row.RepaintMedianMs = [math]::Round((Median $hover.ToArray()), 3)
    $row.RepaintP95Ms = [math]::Round((Pct $hover.ToArray() 95), 3)
    $row.RepaintKcycles = [math]::Round(($c1 - $c0) / 1e3 / 200)

    # One full update (engine, tray, phone, repaint) as the timer runs it every second.
    $tick = New-Object System.Collections.Generic.List[double]
    $c0 = [UnseatBench]::Cycles($proc.Handle)
    for ($k = 0; $k -lt 200; $k++) { $tick.Add([UnseatBench]::Send($widget, $MSG_ADVANCE, 0, 0)) }
    $c1 = [UnseatBench]::Cycles($proc.Handle)
    $row.TickMedianMs = [math]::Round((Median $tick.ToArray()), 3)
    $row.TickKcycles = [math]::Round(($c1 - $c0) / 1e3 / 200)

    # Windows: settings, tray panel, hover card and the time-up tabs. First open, then repeats.
    $settings = New-Object System.Collections.Generic.List[double]
    $panel = New-Object System.Collections.Generic.List[double]
    $flyout = New-Object System.Collections.Generic.List[double]
    $peak = 0.0
    $first = $null
    for ($k = 0; $k -lt $Cycles; $k++) {
      $settings.Add([UnseatBench]::OpenAndPaint($hidden, $WM_COMMAND, $ID_SETTINGS, 0, "UnseatSettings", "Unseat Settings (test $id)"))
      $peak = [math]::Max($peak, (Snapshot $proc).PrivateMB)
      [void][UnseatBench]::SendMessageW([UnseatBench]::FindWindowW("UnseatSettings", "Unseat Settings (test $id)"), $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
      $panel.Add([UnseatBench]::OpenAndPaint($hidden, $TRAY_CALLBACK, 0, $WM_CONTEXTMENU, "UnseatTrayPanel", "Unseat panel (test $id)"))
      $peak = [math]::Max($peak, (Snapshot $proc).PrivateMB)
      [void][UnseatBench]::SendMessageW([UnseatBench]::FindWindowW("UnseatTrayPanel", "Unseat panel (test $id)"), $WM_KEYDOWN, [IntPtr]$VK_ESCAPE, [IntPtr]::Zero)
      $flyout.Add([UnseatBench]::OpenAndPaint($hidden, $TRAY_CALLBACK, 0, $NIN_POPUPOPEN, "UnseatFlyout", "Unseat status (test $id)"))
      [void][UnseatBench]::SendMessageW($hidden, $TRAY_CALLBACK, [IntPtr]::Zero, [IntPtr]$NIN_POPUPCLOSE)
      # Time up: the tabs slide out under the pill; Reset puts them away.
      [void][UnseatBench]::SendMessageW($widget, $MSG_ADVANCE, [IntPtr]3700, [IntPtr]::Zero)
      Start-Sleep -Milliseconds 300
      $peak = [math]::Max($peak, (Snapshot $proc).PrivateMB)
      [void][UnseatBench]::SendMessageW($hidden, $WM_COMMAND, [IntPtr]$ID_RESET, [IntPtr]::Zero)
      Start-Sleep -Milliseconds 100
      if ($k -eq 0) { $first = Snapshot $proc }
    }
    Start-Sleep -Milliseconds 500
    $last = Snapshot $proc
    $row.SettingsFirstOpenMs = [math]::Round($settings[0], 1)
    $row.SettingsOpenMedianMs = [math]::Round((Median $settings.ToArray()), 1)
    $row.PanelFirstOpenMs = [math]::Round($panel[0], 1)
    $row.PanelOpenMedianMs = [math]::Round((Median $panel.ToArray()), 1)
    $row.FlyoutFirstOpenMs = [math]::Round($flyout[0], 1)
    $m = [UnseatBench]::Memory($proc.Handle)
    $row.PeakPrivateMB = [math]::Round([math]::Max($peak, $m.PeakPagefileUsage.ToUInt64() / 1MB), 2)
    $row.RetainedPrivateMB = $last.PrivateMB
    $row.PeakWorkingSetMB = $last.PeakWorkingSetMB
    $row.ThreadsAfterUse = $last.Threads
    $row.HandleGrowthPerCycle = if ($Cycles -gt 1) { [math]::Round(($last.Handles - $first.Handles) / ($Cycles - 1), 2) } else { 0 }
    $row.GdiGrowthPerCycle = if ($Cycles -gt 1) { [math]::Round(($last.Gdi - $first.Gdi) / ($Cycles - 1), 2) } else { 0 }
    $row.UserGrowthPerCycle = if ($Cycles -gt 1) { [math]::Round(($last.User - $first.User) / ($Cycles - 1), 2) } else { 0 }

    # Quit (saves first), then a restart with the settings and history now on disk.
    $sw = [Diagnostics.Stopwatch]::StartNew()
    [void][UnseatBench]::PostMessageW($hidden, $WM_COMMAND, [IntPtr]$ID_QUIT, [IntPtr]::Zero)
    if (-not $proc.WaitForExit(10000)) { throw "test copy did not quit" }
    $row.ShutdownMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
    $t = [UnseatBench]::Launch($fresh, $appdata, $id, [ref]$proc)
    $row.RestartReadyMs = [math]::Round($t[1], 1)
    Start-Sleep -Milliseconds 500
    [void][UnseatBench]::PostMessageW((Find-Hidden $title), $WM_COMMAND, [IntPtr]$ID_QUIT, [IntPtr]::Zero)
    [void]$proc.WaitForExit(10000)
  } finally {
    if ($proc -and -not $proc.HasExited) { $proc.Kill(); [void]$proc.WaitForExit(3000) }
    Remove-Item -Recurse -Force $appdata -ErrorAction SilentlyContinue
  }
  [pscustomobject]$row
}

Assert-NoOtherTestCopy
$machine = [ordered]@{
  Cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
  LogicalCpus = [Environment]::ProcessorCount
  RamGB = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB)
  Gpu = @(Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join "; "
  Os = (Get-CimInstance Win32_OperatingSystem).Caption + " " + [Environment]::OSVersion.Version
  PowerPlan = ((powercfg /getactivescheme) -replace '.*\((.*)\).*', '$1')
  StartedAt = (Get-Date).ToString("s")
  Phone = [bool]$Phone
  Extra = $Extra
  IdleSeconds = $IdleSeconds
  Rounds = $Rounds
}
$rows = New-Object System.Collections.Generic.List[object]
for ($round = 1; $round -le $Rounds; $round++) {
  # A copy: @() of an array is the same array, and reversing it would reorder $Exes too.
  $order = [string[]]$Exes.Clone()
  if ($round % 2 -eq 0) { [array]::Reverse($order) }
  foreach ($exe in $order) {
    Write-Host "round $round/$Rounds  $exe"
    $rows.Add((Measure-Build $exe $round))
  }
}

$metrics = @($rows[0].PSObject.Properties.Name | Where-Object { $_ -notin "Build", "Round" })
$summary = foreach ($exe in $Exes) {
  $mine = @($rows | Where-Object { $_.Build -eq $exe })
  $s = [ordered]@{ Build = $exe; SizeBytes = (Get-Item $exe).Length }
  foreach ($m in $metrics) {
    $vals = @($mine | ForEach-Object { $_.$m } | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ })
    $s[$m] = Median $vals
    if ($m -match "Ms$|Mcycles|CpuMs") { $s[$m + "_p95"] = Pct $vals 95 }
  }
  [pscustomobject]$s
}
$table = foreach ($m in @("SizeBytes") + $metrics) {
  $r = [ordered]@{ Metric = $m }
  foreach ($s in $summary) { $r[(Split-Path -Leaf (Split-Path -Parent $s.Build)) + "\" + (Split-Path -Leaf $s.Build)] = $s.$m }
  if ($summary.Count -eq 2 -and $null -ne $summary[0].$m -and [double]$summary[0].$m -ne 0) {
    $r["Change"] = "{0:+0.0;-0.0;0.0}%" -f (100 * ([double]$summary[1].$m - [double]$summary[0].$m) / [double]$summary[0].$m)
  }
  [pscustomobject]$r
}
$table | Format-Table -AutoSize | Out-String -Width 250
$verdicts = @()
if ($Budgets) {
  $limits = (Get-Content -Raw $Budgets | ConvertFrom-Json).max
  $last = @($summary)[-1]
  $verdicts = foreach ($p in $limits.PSObject.Properties) {
    $v = $last.($p.Name)
    [pscustomobject]@{
      Metric = $p.Name
      Median = $v
      Max = $p.Value
      Verdict = if ($null -eq $v) { "not measured" } elseif ([double]$v -le [double]$p.Value) { "ok" } else { "OVER" }
    }
  }
  "Budgets for $(Split-Path -Leaf $last.Build) ($Budgets):"
  $verdicts | Format-Table -AutoSize | Out-String -Width 160
}
if ($Out) {
  [ordered]@{ Machine = $machine; Summary = $summary; Budgets = $verdicts; Rows = $rows } | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $Out
  Write-Host "Wrote $Out"
}
if ($Strict -and @($verdicts | Where-Object { $_.Verdict -eq "OVER" }).Count) { exit 1 }
