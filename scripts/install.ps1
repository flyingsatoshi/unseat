# Builds the current source and installs it as the Unseat you run day to day.
#
# The app lives at dist\unseat.exe, outside target\, so builds, tests and `cargo clean` never
# touch it. The running Unseat is asked to save and quit (like tray > Quit), the new exe
# replaces the old one, an existing "Launch with Windows" entry is pointed at it, and the new
# build starts. Run this after every change so the next launch is always the new code.
#
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 [-Exe path | -Docker] [-NoLaunch]
#
# Three ways to get the exe; the installed app needs only Windows either way:
#   (default)  build with Rust and the MSVC Build Tools on this PC.
#   -Exe path  install an exe that is already built (a release, a CI artifact); needs no
#              Rust, no build tools and no Docker.
#   -Docker    optional: cross-build in a Rust Linux container (scripts/cross_build.sh), for
#              when local builds are blocked (Smart App Control, os error 4551). The container
#              is capped at -DockerCpus / -DockerMemory because Docker may be shared with other
#              work on this machine.
param(
  [switch]$Docker,
  [string]$Exe = "",
  [switch]$NoLaunch,
  [string]$DockerCpus = "4",
  [string]$DockerMemory = "6g"
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

if ($Exe -and $Docker) { throw "Use either -Exe or -Docker, not both." }
if ($Exe) {
  $built = (Resolve-Path -LiteralPath $Exe).Path
} elseif ($Docker) {
  if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "Docker is not installed. It is optional: run without -Docker to build with Rust, or use -Exe to install a built unseat.exe."
  }
  # Only this staging folder is written by the container, and only it is removed afterwards.
  $staging = Join-Path $root "target\x86_64-pc-windows-gnu"
  $built = Join-Path $staging "release\unseat.exe"
  Remove-Item -LiteralPath $built -ErrorAction SilentlyContinue
  # The container gets the tree without .git, so pass the commit for the version line.
  $sha = (git -C $root rev-parse --short=7 HEAD 2>$null)
  $branch = (git -C $root rev-parse --abbrev-ref HEAD 2>$null)
  $dirty = if (git -C $root status --porcelain --untracked-files=no 2>$null) { "1" } else { "0" }
  # Swap is capped with memory, and downloaded crates are kept in a named volume between builds.
  docker run --rm --cpus $DockerCpus --memory $DockerMemory --memory-swap $DockerMemory `
    -e "UNSEAT_GIT_SHA=$sha" -e "UNSEAT_GIT_BRANCH=$branch" -e "UNSEAT_GIT_DIRTY=$dirty" `
    -v "${root}:/src" -v "unseat-cargo-registry:/usr/local/cargo/registry" `
    $image bash /src/scripts/cross_build.sh
  if ($LASTEXITCODE -ne 0) { throw "Docker build failed with exit code $LASTEXITCODE (see the cross_build messages above)" }
} else {
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust (cargo) is not installed. Install Rust and the MSVC Build Tools, or use -Exe to install a built unseat.exe (-Docker is optional)."
  }
  $built = Join-Path $root "target\release\unseat.exe"
  Push-Location $root
  try {
    cargo build --locked --release --features gui
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE (os error 4551 means Smart App Control blocked it; use -Docker or -Exe)" }
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
# The container build is only a staging copy; keep dist\unseat.exe as the one exe. The rest of
# target\ (local builds, test data) is left alone.
if ($Docker) { Remove-Item -LiteralPath $staging -Recurse -Force }
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
