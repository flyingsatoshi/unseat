# Builds the current source and installs it as the Unseat you run day to day.
#
# The app lives at dist\unseat.exe, outside target\, so builds, tests and `cargo clean` never
# touch it. The running Unseat is asked to save and quit (like tray > Quit), the new exe
# replaces the old one, an existing "Launch with Windows" entry is pointed at it, and the new
# build starts. Run this after every change so the next launch is always the new code.
#
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 [-Docker | -Exe path] [-NoLaunch]
#
# By default it builds with the local MSVC toolchain. -Docker cross-builds in a Rust Linux
# container instead (scripts/cross_build.sh), for when Smart App Control blocks local builds
# with os error 4551. -Exe installs an exe that is already built.
param(
  [switch]$Docker,
  [string]$Exe = "",
  [switch]$NoLaunch
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$app = Join-Path $root "dist\unseat.exe"
$image = "rust:1.98-bookworm"
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UnseatInstall {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wp, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
}
"@

if ($Exe) {
  $built = (Resolve-Path -LiteralPath $Exe).Path
} elseif ($Docker) {
  $built = Join-Path $root "target\x86_64-pc-windows-gnu\release\unseat.exe"
  Remove-Item -LiteralPath $built -ErrorAction SilentlyContinue
  # The container gets the tree without .git, so pass the commit for the version line.
  $sha = (git -C $root rev-parse --short=7 HEAD 2>$null)
  $branch = (git -C $root rev-parse --abbrev-ref HEAD 2>$null)
  $dirty = if (git -C $root status --porcelain --untracked-files=no 2>$null) { "1" } else { "0" }
  docker run --rm -e "UNSEAT_GIT_SHA=$sha" -e "UNSEAT_GIT_BRANCH=$branch" -e "UNSEAT_GIT_DIRTY=$dirty" -v "${root}:/src" $image bash /src/scripts/cross_build.sh
  if ($LASTEXITCODE -ne 0) { throw "Docker build failed with exit code $LASTEXITCODE" }
} else {
  $built = Join-Path $root "target\release\unseat.exe"
  Push-Location $root
  try {
    cargo build --locked --release --features gui
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE (os error 4551 means Smart App Control; use -Docker)" }
  } finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath $built)) { throw "No build at $built" }

# Close the user's Unseat (never a " (test)" instance) wherever it runs from.
$widget = [UnseatInstall]::FindWindowW("UnseatWidget", "Unseat")
if ($widget -ne [IntPtr]::Zero) {
  $procId = 0
  [void][UnseatInstall]::GetWindowThreadProcessId($widget, [ref]$procId)
  $running = Get-Process -Id $procId
  Write-Host "Closing running Unseat: $($running.Path)"
  [void][UnseatInstall]::PostMessageW($widget, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
  if (-not $running.WaitForExit(10000)) { throw "Unseat (pid $procId) did not quit; close it from the tray and rerun." }
}

New-Item -ItemType Directory -Force (Split-Path -Parent $app) | Out-Null
Copy-Item -LiteralPath $built -Destination $app -Force
# The container build is only a staging copy; keep dist\unseat.exe as the one exe.
if ($Docker) { Remove-Item -LiteralPath (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $built))) -Recurse -Force }
$info = Get-Item -LiteralPath $app
Write-Host "Installed $app ($($info.VersionInfo.FileVersion), built $($info.LastWriteTime))"

$command = "`"$app`""
$current = (Get-ItemProperty -Path $runKey -Name Unseat -ErrorAction SilentlyContinue).Unseat
if ($current -and $current -ne $command) {
  Set-ItemProperty -Path $runKey -Name Unseat -Value $command
  Write-Host "Launch with Windows now runs $command (was $current)"
}

if (-not $NoLaunch) {
  Start-Process -FilePath $app
  Write-Host "Started Unseat"
}
