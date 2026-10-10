# Starts the Phase 1 push test: the local server, then `tailscale serve` in the foreground so the
# phone can reach it over HTTPS on the tailnet. Ctrl+C stops both; nothing is left running and
# the foreground serve session ends with it.
#
#   powershell -ExecutionPolicy Bypass -File spikes\iphone-push\start.ps1 [-NoServe]
param([switch]$NoServe)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw "Node.js 20+ is required." }
if (-not $NoServe) {
  if (-not (Get-Command tailscale -ErrorAction SilentlyContinue)) { throw "Tailscale is not installed or not on PATH." }
  $funnel = (& tailscale funnel status 2>&1 | Out-String).Trim()
  if ($funnel -notmatch "No serve config") {
    Write-Host "Existing Tailscale Serve/Funnel config found:" -ForegroundColor Yellow
    Write-Host $funnel
    throw "Stop here so this test doesn't change an existing setup. Review it, then run with -NoServe and serve port 8780 yourself."
  }
}

$node = Start-Process node -ArgumentList "`"$here\server.mjs`"" -WorkingDirectory $here -NoNewWindow -PassThru
try {
  Start-Sleep -Seconds 1
  if ($node.HasExited) { throw "The test server stopped right away (is port 8780 or 8781 in use?)." }
  if ($NoServe) {
    Wait-Process -Id $node.Id
  } else {
    Write-Host "Publishing http://127.0.0.1:8780 to your tailnet (not the internet). Ctrl+C to stop." -ForegroundColor Cyan
    & tailscale serve 8780
  }
} finally {
  if (-not $node.HasExited) { Stop-Process -Id $node.Id }
  if (-not $NoServe) { & tailscale serve status }
}
