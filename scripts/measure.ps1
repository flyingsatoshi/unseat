# Measures startup time, idle CPU and memory of one or more Unseat builds.
# Each build runs with its own empty %APPDATA% (default settings). Builds that support it run
# with UNSEAT_TEST_HOOKS=1 so they can sit beside a normal Unseat; the hooks add no per-tick work.
#
#   powershell -ExecutionPolicy Bypass -File scripts/measure.ps1 -Exes a.exe,b.exe [-Seconds 60] [-Rounds 2]
param([string[]]$Exes, [int]$Seconds = 60, [int]$Rounds = 2)
$ErrorActionPreference = "Stop"
$Exes = @($Exes | ForEach-Object { $_ -split "," } | Where-Object { $_ })
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UnseatMeasure {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
  [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr process, uint flags);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
}
"@
$root = Join-Path (Split-Path -Parent $PSScriptRoot) "target\test-appdata\measure"
$rows = @()
for ($round = 1; $round -le $Rounds; $round++) {
  foreach ($exe in $Exes) {
    $appdata = Join-Path $root ([guid]::NewGuid().ToString("N").Substring(0, 8))
    New-Item -ItemType Directory -Force (Join-Path $appdata "Unseat") | Out-Null
    $old = $env:APPDATA; $env:APPDATA = $appdata; $env:UNSEAT_TEST_HOOKS = "1"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    try { $p = Start-Process -FilePath $exe -PassThru } finally { $env:APPDATA = $old; Remove-Item env:UNSEAT_TEST_HOOKS }
    $ready = $null
    while ($sw.ElapsedMilliseconds -lt 10000 -and -not $ready) {
      foreach ($title in @("Unseat (test)", "Unseat")) {
        $h = [UnseatMeasure]::FindWindowW("UnseatWidget", $title)
        if ($h -ne [IntPtr]::Zero) {
          $owner = 0; [void][UnseatMeasure]::GetWindowThreadProcessId($h, [ref]$owner)
          if ($owner -eq $p.Id) { $ready = $sw.ElapsedMilliseconds }
        }
      }
      if (-not $ready) { Start-Sleep -Milliseconds 5 }
    }
    Start-Sleep -Seconds 5
    $p.Refresh(); $cpu0 = $p.TotalProcessorTime
    $t0 = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds $Seconds
    $p.Refresh(); $cpu1 = $p.TotalProcessorTime; $wall = $t0.Elapsed.TotalSeconds
    $rows += [pscustomobject]@{
      Build = Split-Path -Leaf $exe
      Round = $round
      StartupMs = $ready
      CpuPercent = [math]::Round(100 * ($cpu1 - $cpu0).TotalSeconds / $wall, 4)
      CpuMsPerMin = [math]::Round(($cpu1 - $cpu0).TotalMilliseconds * 60 / $wall, 1)
      WorkingSetMB = [math]::Round($p.WorkingSet64 / 1MB, 2)
      PrivateMB = [math]::Round($p.PrivateMemorySize64 / 1MB, 2)
      PeakWorkingSetMB = [math]::Round($p.PeakWorkingSet64 / 1MB, 2)
      Handles = $p.HandleCount
      GdiObjects = [UnseatMeasure]::GetGuiResources($p.Handle, 0)
      UserObjects = [UnseatMeasure]::GetGuiResources($p.Handle, 1)
      Threads = $p.Threads.Count
    }
    Stop-Process -Id $p.Id -Force
    Remove-Item -Recurse -Force $appdata -ErrorAction SilentlyContinue
  }
}
$rows | Format-Table -AutoSize | Out-String -Width 220
$rows | Group-Object Build | ForEach-Object {
  [pscustomobject]@{
    Build = $_.Name
    StartupMs = [math]::Round(($_.Group | Measure-Object StartupMs -Average).Average)
    CpuMsPerMin = [math]::Round(($_.Group | Measure-Object CpuMsPerMin -Average).Average, 1)
    WorkingSetMB = [math]::Round(($_.Group | Measure-Object WorkingSetMB -Average).Average, 2)
    PrivateMB = [math]::Round(($_.Group | Measure-Object PrivateMB -Average).Average, 2)
    GdiObjects = [math]::Round(($_.Group | Measure-Object GdiObjects -Average).Average)
    Handles = [math]::Round(($_.Group | Measure-Object Handles -Average).Average)
  }
} | Format-Table -AutoSize | Out-String -Width 220
