# End-to-end tests for iPhone reminders against the real Unseat executable.
#
# Each scenario runs an isolated test instance (see unseat_test_lib.ps1) whose push service is a
# local fake (HttpListener on localhost) instead of Apple, so nothing leaves this PC. The phone
# side is driven over HTTP exactly as Tailscale Serve would deliver it (with the
# Tailscale-User-Login header); the PC page through its loopback listener and one-time key.
#
#   powershell -ExecutionPolicy Bypass -File scripts/test_phone.ps1 [-Exe path] [-Only name]
param(
  [string]$Exe = (Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\unseat.exe"),
  [string]$Only = ""
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "unseat_test_lib.ps1")

$PHONE = 18790; $ADMIN = 18791; $FAKE = 18799
$MSG_PHONE_ADMIN = $WM_APP + 0x45
$LOGIN = "owner@example.test"
# RFC 8291 example receiver keys: a valid P-256 point and auth secret.
$P256DH = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
$AUTH = "BTBZMqHH6r4Tts7J_aSIgg"
$FakeDir = Join-Path $TestRoot "fake-push"

$results = New-Object System.Collections.Generic.List[object]
function Check([bool]$ok, [string]$what) { if (-not $ok) { throw "check failed: $what" } }

# ------------------------------------------------------------------ fake push service

function Start-FakePush {
  New-Item -ItemType Directory -Force $FakeDir | Out-Null
  Remove-Item (Join-Path $FakeDir "*") -ErrorAction SilentlyContinue
  Set-FakeStatus 201
  Start-Job -ArgumentList $FAKE, $FakeDir -ScriptBlock {
    param($port, $dir)
    $l = New-Object System.Net.HttpListener
    $l.Prefixes.Add("http://localhost:$port/")
    $l.Start()
    while ($true) {
      $ctx = $l.GetContext()
      $ms = New-Object System.IO.MemoryStream
      $ctx.Request.InputStream.CopyTo($ms)
      $h = @{}
      foreach ($k in $ctx.Request.Headers.AllKeys) { $h[$k] = $ctx.Request.Headers[$k] }
      $status = [int](Get-Content (Join-Path $dir "status.txt") -Raw)
      $delay = 0; $df = Join-Path $dir "delay.txt"; if (Test-Path $df) { $delay = [int](Get-Content $df -Raw) }
      if ($delay -gt 0) { Start-Sleep -Milliseconds $delay }
      $line = @{ path = $ctx.Request.Url.AbsolutePath; method = $ctx.Request.HttpMethod; headers = $h; length = $ms.Length; status = $status } | ConvertTo-Json -Compress -Depth 4
      # Append with full sharing so the test's reads never make a line go missing.
      $bytes = [Text.Encoding]::UTF8.GetBytes($line + "`n")
      for ($try = 0; $try -lt 250; $try++) {
        try {
          $fs = [IO.File]::Open((Join-Path $dir "pushes.jsonl"), [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
          $fs.Write($bytes, 0, $bytes.Length); $fs.Close(); break
        } catch { Start-Sleep -Milliseconds 20 }
      }
      $ctx.Response.StatusCode = $status
      $ctx.Response.Close()
    }
  }
}
function Set-FakeStatus([int]$s) { Set-Content -Path (Join-Path $FakeDir "status.txt") -Value $s -NoNewline }
function Set-FakeDelay([int]$ms) { Set-Content -Path (Join-Path $FakeDir "delay.txt") -Value $ms -NoNewline }
# The fake service appends while tests read, so read with sharing and treat a busy file as "not yet".
function Get-Pushes {
  $f = Join-Path $FakeDir "pushes.jsonl"
  if (-not (Test-Path $f)) { return @() }
  try {
    $fs = [IO.File]::Open($f, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    $text = (New-Object IO.StreamReader($fs)).ReadToEnd(); $fs.Close()
  } catch { return @() }
  @($text -split "`n" | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
}
function Clear-Pushes {
  $f = Join-Path $FakeDir "pushes.jsonl"
  for ($try = 0; $try -lt 50 -and (Test-Path $f); $try++) {
    try { Remove-Item $f -ErrorAction Stop } catch { Start-Sleep -Milliseconds 20 }
  }
}

# ------------------------------------------------------------------ HTTP helpers

function Http([string]$method, [string]$url, [hashtable]$headers = @{}, $body = $null) {
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = $method
  $req.Timeout = 8000
  foreach ($k in $headers.Keys) { $req.Headers[$k] = $headers[$k] }
  if ($null -ne $body) {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Compress -Depth 6))
    $req.ContentType = "application/json"
    $req.ContentLength = $bytes.Length
    $s = $req.GetRequestStream(); $s.Write($bytes, 0, $bytes.Length); $s.Close()
  }
  try { $res = $req.GetResponse() } catch [System.Net.WebException] {
    $res = $_.Exception.Response
    if ($null -eq $res) { return [pscustomobject]@{ Status = 0; Json = $null } }
  }
  $text = (New-Object IO.StreamReader($res.GetResponseStream())).ReadToEnd()
  $code = [int]$res.StatusCode
  $res.Close()
  $json = $null; try { $json = $text | ConvertFrom-Json } catch {}
  [pscustomobject]@{ Status = $code; Json = $json }
}
function Phone([string]$method, [string]$path, $body = $null, [string]$token = "", [string]$login = $LOGIN) {
  $h = @{}
  if ($login) { $h["Tailscale-User-Login"] = $login }
  if ($token) { $h["Authorization"] = "Bearer $token" }
  Http $method "http://127.0.0.1:$PHONE$path" $h $body
}
function Admin($inst, [string]$method, [string]$path, $body = $null) {
  Http $method "http://127.0.0.1:$ADMIN$path" @{ "X-Unseat-Admin" = $inst.AdminKey } $body
}
# Listening sockets on a port, as objects with LocalAddress. Callers wrap the result in @():
# Windows PowerShell 5.1 gives a single [pscustomobject] no .Count.
function Listening([int]$port) {
  netstat -ano -p tcp | Select-String -Pattern "^\s*TCP\s+(\S+):$port\s+\S+\s+LISTENING" |
    ForEach-Object { [pscustomobject]@{ LocalAddress = $_.Matches[0].Groups[1].Value } }
}

function Base([hashtable]$extra = @{}) {
  $s = @{ settings_version = 3; sitting_limit_secs = 1200; break_duration_secs = 180; repeat_every_secs = 300
          window_x = 300; window_y = 300; sound_enabled = $false; notify_enabled = $false }
  foreach ($k in $extra.Keys) { $s[$k] = $extra[$k] }
  $s
}

function Start-Phone($settings) {
  $old = @($env:UNSEAT_TEST_PUSH_ORIGIN, $env:UNSEAT_TEST_PUBLIC_HOST)
  $env:UNSEAT_TEST_PUSH_ORIGIN = "http://localhost:$FAKE"; $env:UNSEAT_TEST_PUBLIC_HOST = "pc.example.ts.net"
  try { $i = Start-TestInstance $Exe $settings } finally { $env:UNSEAT_TEST_PUSH_ORIGIN = $old[0]; $env:UNSEAT_TEST_PUBLIC_HOST = $old[1] }
  [void][UnseatTest]::SendMessageW($i.Widget, $MSG_PHONE_ADMIN, [IntPtr]::Zero, [IntPtr]::Zero)
  $line = Wait-Until { Read-Log $i | Where-Object { $_ -like "phone admin *" } | Select-Object -Last 1 }
  $i | Add-Member AdminKey ($line -replace '^.*#t=', '')
  $i
}

function Sha256Hex([string]$text) {
  $sha = [Security.Cryptography.SHA256]::Create()
  (($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($text)) | ForEach-Object { $_.ToString("x2") }) -join "")
}
function New-Secret { [Convert]::ToBase64String([Guid]::NewGuid().ToByteArray() + [Guid]::NewGuid().ToByteArray()).TrimEnd("=").Replace("+", "-").Replace("/", "_") }

# Pair a phone the way the app does (it makes its own secret and sends only the hash) and turn
# on its notifications; returns the secret, which is the phone's bearer token.
function Pair($inst, [string]$name = "Test iPhone") {
  $secret = New-Secret
  $code = (Admin $inst POST "/admin/pair/new" @{}).Json.code
  $r = Phone POST "/api/pair/request" @{ code = $code.ToLower(); name = $name; secretHash = (Sha256Hex $secret) }
  Check ($r.Status -eq 200) "pair request accepted ($($r.Status))"
  $req = (Admin $inst GET "/admin/state").Json.requests | Where-Object { $_.reqId -eq $r.Json.reqId }
  Check ($req.login -eq $LOGIN) "PC sees the tailnet login"
  Check ((Phone GET "/api/status" -token $secret).Status -eq 401) "not usable before approval"
  Check ((Admin $inst POST "/admin/pair/decide" @{ reqId = $r.Json.reqId; approve = $true }).Status -eq 200) "approved"
  $p = Phone GET "/api/pair/poll?reqId=$($r.Json.reqId)"
  Check ($p.Json.status -eq "approved" -and -not $p.Json.token) "approved, and no secret crosses the network"
  $sub = @{ subscription = @{ endpoint = "http://localhost:$FAKE/push/$name"; keys = @{ p256dh = $P256DH; auth = $AUTH } } }
  Check ((Phone POST "/api/subscribe" $sub $secret).Status -eq 200) "subscribed"
  $secret
}

function Start-PhoneIn([string]$appdata) {
  $old = @($env:UNSEAT_TEST_PUSH_ORIGIN, $env:UNSEAT_TEST_PUBLIC_HOST)
  $env:UNSEAT_TEST_PUSH_ORIGIN = "http://localhost:$FAKE"; $env:UNSEAT_TEST_PUBLIC_HOST = "pc.example.ts.net"
  try { Start-TestInstance $Exe $null $appdata } finally { $env:UNSEAT_TEST_PUSH_ORIGIN = $old[0]; $env:UNSEAT_TEST_PUBLIC_HOST = $old[1] }
}

$PhoneTestHosts = @{ UNSEAT_TEST_PUSH_ORIGIN = "http://localhost:$FAKE"; UNSEAT_TEST_PUBLIC_HOST = "pc.example.ts.net" }

function Stop-Phone($inst) {
  if ($env:UNSEAT_TEST_VERBOSE) { Read-Log $inst | Where-Object { $_ -notlike "tick *" } | Select-Object -Last 30 | Write-Host }
  Stop-TestInstance $inst
}

function Wait-Pushes([int]$n, [int]$ms = 8000) { @(Wait-Until { $p = @(Get-Pushes); if ($p.Count -ge $n) { ,$p } } $ms) }

function Scenario([string]$name, [scriptblock]$body) {
  if ($Only -and $name -notlike "*$Only*") { return }
  $sw = [Diagnostics.Stopwatch]::StartNew()
  Clear-Pushes; Set-FakeStatus 201; Set-FakeDelay 0
  try {
    & $body
    $results.Add([pscustomobject]@{ Test = $name; Result = "PASS"; Ms = $sw.ElapsedMilliseconds; Detail = "" })
  } catch {
    $results.Add([pscustomobject]@{ Test = $name; Result = "FAIL"; Ms = $sw.ElapsedMilliseconds; Detail = $_.Exception.Message })
  } finally {
    Stop-AllTestInstances
    Start-Sleep -Milliseconds 300
  }
}

Assert-NoOtherTestInstance
$RealBefore = Get-RealUnseat

# ------------------------------------------------------------------ scenarios

$job = Start-FakePush
Wait-Until { try { (Http POST "http://localhost:$FAKE/ping" @{} @{}).Status -eq 201 } catch { $false } } | Out-Null
Clear-Pushes

Scenario "iPhone reminders are off by default: no listener, nothing sent" {
  $i = Start-TestInstance $Exe (Base)
  try {
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "alert LimitReached" } | Out-Null
    Start-Sleep -Milliseconds 500
    Check (@(Listening $PHONE).Count -eq 0) "no phone listener"
    Check (@(Listening $ADMIN).Count -eq 0) "no PC-page listener"
    Check (-not ((Read-Log $i) -match "^phone ")) "no phone activity at all"
    Check (-not (Test-Path (Join-Path $i.Dir "companion.json"))) "no companion file written"
    Check (@(Get-Pushes).Count -eq 0) "nothing sent"
  } finally { Stop-Phone $i }
}

Scenario "phone listener is loopback-only and needs the tailnet identity header" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $l = @(Wait-Until { $x = @(Listening $PHONE); if ($x.Count) { ,$x } })
    Check (@($l | Where-Object { $_.LocalAddress -ne "127.0.0.1" }).Count -eq 0) "bound to 127.0.0.1 only ($(@($l | ForEach-Object { $_.LocalAddress }) -join ','))"
    Check (@(Listening $ADMIN | Where-Object { $_.LocalAddress -ne "127.0.0.1" }).Count -eq 0) "PC page bound to 127.0.0.1 only"
    Check ((Phone GET "/" -login "").Status -eq 403) "no tailnet header -> 403"
    Check ((Phone GET "/").Status -eq 200) "through the tailnet -> app"
    Check ((Phone GET "/admin/state").Status -eq 404) "PC page API not on the phone port"
    Check ((Phone GET "/api/status" -token "guess").Status -eq 401) "unknown device -> 401"
    Check ((Http GET "http://127.0.0.1:$ADMIN/admin/state").Status -eq 403) "PC page API needs the key"
    Check ((Http GET "http://127.0.0.1:$ADMIN/admin/state" @{ "X-Unseat-Admin" = $i.AdminKey; "Tailscale-User-Login" = $LOGIN }).Status -eq 403) "PC page refuses tailnet-forwarded requests"
    $raw = New-Object Net.Sockets.TcpClient("127.0.0.1", $PHONE)
    $st = $raw.GetStream(); $b = [Text.Encoding]::ASCII.GetBytes("POST / HTTP/1.1`r`nHost: 127.0.0.1:$PHONE`r`nTransfer-Encoding: chunked`r`n`r`n")
    $st.Write($b, 0, $b.Length); $buf = New-Object byte[] 64; $n = $st.Read($buf, 0, 64); $raw.Close()
    Check ([Text.Encoding]::ASCII.GetString($buf, 0, $n) -like "HTTP/1.1 501*") "chunked bodies refused"
  } finally { Stop-Phone $i }
}

Scenario "every reminder is mirrored, including sound-only repeats" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $token = Pair $i
    Advance $i 1200
    $p = @(Wait-Pushes 1)
    $h = $p[0].headers
    Check ($p[0].method -eq "POST") "POST"
    Check ($h."Content-Encoding" -eq "aes128gcm") "aes128gcm body"
    Check ($h."Urgency" -eq "high") "urgency high"
    Check ($h."Topic" -eq "unseat-sitting") "sitting topic"
    Check ([int]$h."TTL" -gt 200 -and [int]$h."TTL" -le 300) "TTL = time to the next repeat ($($h.TTL))"
    Check ($h."Authorization" -match '^vapid t=[^.]+\.[^.]+\.[^,]+, k=') "VAPID authorization"
    Check ($p[0].length -gt 100 -and $p[0].length -le 4096) "encrypted body within Apple's limit"
    Advance $i 300
    $p = @(Wait-Pushes 2)
    Check (@((Read-Log $i) -match "phone queued repeat").Count -gt 0) "repeat reminder (sound-only on Windows) mirrored"
    Send-Command $i $ID_BREAK
    Advance $i 180
    $p = @(Wait-Pushes 3)
    Check ($p[2].headers.Topic -eq "unseat-break") "break-over reminder on its own topic"
    $s = Wait-Until { $x = (Phone GET "/api/status" -token $token).Json; if ($x.delivered -eq 3) { $x } }
    Check ($s.missed -eq 0) "phone sees 3 delivered, none missed"
    Check (-not ((Read-Log $i) -match "^notify ")) "Windows toast stays off as configured"
  } finally { Stop-Phone $i }
}

Scenario "idle-only waits until the user is away from the PC" {
  $i = Start-Phone (Base @{ phone_enabled = $true; phone_alerts = "idle_only"; idle_after_secs = 60 })
  try {
    Pair $i | Out-Null
    Set-Idle $i 5
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "phone skip LimitReached" } | Out-Null
    Start-Sleep -Milliseconds 800
    Check (@(Get-Pushes).Count -eq 0) "nothing sent while at the desk"
    Set-Idle $i 600
    Advance $i 300
    $p = @(Wait-Pushes 1)
    Check ($p[0].headers.Topic -eq "unseat-sitting") "repeat reminder sent once away"
  } finally { Stop-Phone $i }
}

Scenario "a subscription Apple drops is flagged and recovers when the phone subscribes again" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $token = Pair $i
    Set-FakeStatus 410
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "outcome=Gone" } | Out-Null
    $s = (Phone GET "/api/status" -token $token).Json
    Check ($s.needsResubscribe -eq $true) "phone told to reconnect"
    Check ((Get-Settings $i) -ne $null) "settings intact"
    $dev = (Admin $i GET "/admin/state").Json.devices[0]
    Check ($dev.needsResubscribe -eq $true -and $dev.lastError) "PC page shows the problem"
    Set-FakeStatus 201
    $sub = @{ subscription = @{ endpoint = "http://localhost:$FAKE/push/renewed"; keys = @{ p256dh = $P256DH; auth = $AUTH } } }
    Check ((Phone POST "/api/subscribe" $sub $token).Status -eq 200) "re-subscribed"
    Clear-Pushes
    Advance $i 300
    $p = @(Wait-Pushes 1)
    Check ($p[0].path -eq "/push/renewed") "next reminder goes to the new subscription"
  } finally { Stop-Phone $i }
}

Scenario "PC offline: retries with backoff, delivers on reconnect, else counts missed" {
  $i = Start-Phone (Base @{ phone_enabled = $true; repeat_reminders = $false })
  try {
    $token = Pair $i
    Set-FakeStatus 503
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "phone push time_up event=1 attempt=1 status=503 outcome=Retry" } | Out-Null
    Set-FakeStatus 201
    Wait-Until { (Read-Log $i) -match "phone push time_up event=1 attempt=2 status=201 outcome=Accepted" } 8000 | Out-Null
    Wait-Until { (Phone GET "/api/status" -token $token).Json.delivered -eq 1 } | Out-Null
    # Stay offline past a reminder's life (a test push lives 5 minutes): it is dropped, not sent late.
    Set-FakeStatus 503
    Check ((Phone POST "/api/test" @{} $token).Status -eq 200) "test queued"
    Wait-Until { (Read-Log $i) -match "phone push test event=2 attempt=1 status=503" } | Out-Null
    Advance $i 400
    Wait-Until { (Phone GET "/api/status" -token $token).Json.missed -eq 1 } 8000 | Out-Null
    Set-FakeStatus 201
    Start-Sleep -Seconds 3
    Check (@(Get-Pushes | Where-Object { $_.status -eq 201 }).Count -eq 1) "the expired reminder was never sent late"
  } finally { Stop-Phone $i }
}

Scenario "acting on the PC cancels a queued reminder (no stale push afterwards)" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    Pair $i | Out-Null
    Set-FakeStatus 503
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "outcome=Retry" } | Out-Null
    Send-Command $i $ID_RESET
    Wait-Until { (Read-Log $i) -match "phone note Cancelled" } | Out-Null
    Set-FakeStatus 201
    Start-Sleep -Seconds 5
    Check (@(Get-Pushes | Where-Object { $_.status -eq 201 }).Count -eq 0) "nothing sent after the reset"
  } finally { Stop-Phone $i }
}

Scenario "test push from the phone, rate-limited" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $token = Pair $i
    Check ((Phone POST "/api/test" @{} $token).Status -eq 200) "test accepted"
    $p = @(Wait-Pushes 1)
    Check ($p[0].headers.Topic -eq "unseat-test") "test topic"
    Check ((Phone POST "/api/test" @{} $token).Status -eq 429) "second test too soon"
    # What the phone's service worker does when the push arrives: confirm it to the PC.
    $event = [int](((Read-Log $i) -match "phone queued test event=" | Select-Object -Last 1) -replace '^.*event=(\d+).*$', '$1')
    Check ((Phone POST "/api/receipt" @{ event = $event } $token).Status -eq 200) "receipt accepted"
    Wait-Until { (Read-Log $i) -match "phone receipt event=$event after_ms=\d+" } | Out-Null
    [void](Phone POST "/api/receipt" @{ event = $event } $token)
    $s = (Phone GET "/api/status" -token $token).Json
    Check ($s.received -eq 1) "counted once ($($s.received))"
    Check ((Admin $i GET "/admin/state").Json.devices[0].received -eq 1) "PC page shows it reached the phone"
  } finally { Stop-Phone $i }
}

Scenario "removing a phone locks it out and stops its reminders" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $token = Pair $i
    $id = (Admin $i GET "/admin/state").Json.devices[0].id
    Check ((Admin $i POST "/admin/revoke" @{ deviceId = $id }).Status -eq 200) "removed"
    Check ((Phone GET "/api/status" -token $token).Status -eq 401) "token rejected at once"
    $store = Get-Content (Join-Path $i.Dir "companion.json") -Raw
    Check ($store -notmatch [regex]::Escape($token)) "device secret never stored"
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "phone no receivers" } | Out-Null
    Check (@(Get-Pushes).Count -eq 0) "nothing sent"
  } finally { Stop-Phone $i }
}

Scenario "turning reminders off from the PC page closes the phone listener and saves the choice" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    Wait-Until { @(Listening $PHONE).Count -gt 0 } | Out-Null
    Check ((Admin $i POST "/admin/settings" @{ enabled = $false; alerts = "idle_only" }).Status -eq 200) "accepted"
    Wait-Until { (Read-Log $i) -match "phone listener off" } | Out-Null
    Wait-Until { @(Listening $PHONE).Count -eq 0 } | Out-Null
    $f = Get-Settings $i
    Check ($f.phone_enabled -eq $false -and $f.phone_alerts -eq "idle_only") "saved to settings.json"
    Check ((Admin $i POST "/admin/settings" @{ enabled = $true; alerts = "all" }).Status -eq 200) "back on"
    Wait-Until { @(Listening $PHONE).Count -gt 0 } | Out-Null
  } finally { Stop-Phone $i }
}

Scenario "the device store survives a restart and the key stays the same" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  $appdata = $i.AppData
  try {
    $token = Pair $i
    $key1 = (Phone GET "/api/config").Json.vapidPublicKey
    Stop-TestInstance $i -Keep
    $old = @($env:UNSEAT_TEST_PUSH_ORIGIN, $env:UNSEAT_TEST_PUBLIC_HOST)
    $env:UNSEAT_TEST_PUSH_ORIGIN = "http://localhost:$FAKE"; $env:UNSEAT_TEST_PUBLIC_HOST = "pc.example.ts.net"
    try { $i = Start-TestInstance $Exe $null $appdata } finally { $env:UNSEAT_TEST_PUSH_ORIGIN = $old[0]; $env:UNSEAT_TEST_PUBLIC_HOST = $old[1] }
    Wait-Until { @(Listening $PHONE).Count -gt 0 } | Out-Null
    Check ((Phone GET "/api/status" -token $token).Status -eq 200) "still paired"
    Check ((Phone GET "/api/config").Json.vapidPublicKey -eq $key1) "same VAPID key (subscriptions stay valid)"
    $store = Get-Content (Join-Path $i.Dir "companion.json") -Raw | ConvertFrom-Json
    Check ($store.vapid_protected.Length -gt 100) "key stored encrypted"
    Advance $i 1200
    Wait-Pushes 1 | Out-Null
  } finally { Stop-Phone $i }
}

Scenario "a natural break (Pause when idle) cancels a queued reminder" {
  $i = Start-Phone (Base @{ phone_enabled = $true; inactivity_behavior = "pause"; idle_after_secs = 60 })
  try {
    Pair $i | Out-Null
    Set-FakeStatus 503
    Set-Idle $i 0
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "phone push time_up event=1 attempt=1 status=503" } | Out-Null
    Set-Idle $i 600
    Advance $i 300
    Wait-Until { (Read-Log $i) -match "phone note Cancelled" } | Out-Null
    Set-FakeStatus 201
    Start-Sleep -Seconds 4
    Check (@(Get-Pushes | Where-Object { $_.status -eq 201 }).Count -eq 0) "the stale reminder was not sent after the break"
  } finally { Stop-Phone $i }
}

Scenario "a late 'gone' for the old subscription goes to the new one instead" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $token = Pair $i "late"
    Set-FakeStatus 410; Set-FakeDelay 2500
    Advance $i 1200
    Wait-Until { @(Get-Content (Join-Path $i.Dir "test-events.log") | Where-Object { $_ -like "phone queued time_up*" }).Count -gt 0 } | Out-Null
    Start-Sleep -Milliseconds 700
    Set-FakeStatus 201; Set-FakeDelay 0
    $sub = @{ subscription = @{ endpoint = "http://localhost:$FAKE/push/renewed"; keys = @{ p256dh = $P256DH; auth = $AUTH } } }
    Check ((Phone POST "/api/subscribe" $sub $token).Status -eq 200) "phone re-subscribed while the send was in flight"
    Wait-Until { (Read-Log $i) -match "old subscription gone; resending" } | Out-Null
    $p = @(Wait-Until { $x = @(Get-Pushes | Where-Object { $_.path -eq "/push/renewed" }); if ($x.Count) { ,$x } } 10000)
    Check ($p[0].status -eq 201) "delivered to the new subscription"
    $s = (Phone GET "/api/status" -token $token).Json
    Check (-not $s.needsResubscribe) "the new subscription was not flagged"
    Check ($s.endpoint -like "*/push/renewed") "status reports the PC's copy of the endpoint"
  } finally { Stop-Phone $i }
}

Scenario "a refused cached VAPID token is replaced once, not reused for an hour" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    $token = Pair $i
    Check ((Phone POST "/api/test" @{} $token).Status -eq 200) "first test"
    Wait-Pushes 1 | Out-Null
    Set-FakeStatus 403
    Advance $i 1200
    Wait-Until { (Read-Log $i) -match "phone push time_up .*outcome=Rejected" } | Out-Null
    Check (@((Read-Log $i) -match "token refused; signing a new one").Count -eq 1) "re-signed once"
    $auths = @(Get-Pushes | Where-Object { $_.status -eq 403 } | ForEach-Object { $_.headers.Authorization })
    Check ($auths.Count -eq 2 -and $auths[0] -ne $auths[1]) "second try used a fresh token"
  } finally { Stop-Phone $i }
}

Scenario "a damaged device store is kept aside, never silently overwritten" {
  $appdata = Join-Path $TestRoot ([guid]::NewGuid().ToString("N").Substring(0, 8))
  $dir = Join-Path $appdata "Unseat"; New-Item -ItemType Directory -Force $dir | Out-Null
  (Base @{ phone_enabled = $true }) | ConvertTo-Json | Set-Content -Encoding Ascii (Join-Path $dir "settings.json")
  Set-Content -Path (Join-Path $dir "companion.json") -Value "{ damaged" -NoNewline
  $i = Start-PhoneIn $appdata
  try {
    Wait-Until { @(Listening $PHONE).Count -gt 0 } | Out-Null
    $aside = @(Get-ChildItem $dir -Filter "companion.json.bad-*")
    Check ($aside.Count -eq 1) "damaged file kept aside"
    Check ((Get-Content $aside[0].FullName -Raw) -eq "{ damaged") "with its contents"
    Check (@((Read-Log $i) -match "phone store unreadable").Count -gt 0) "logged"
  } finally { Stop-Phone $i }
}

Scenario "a locked device store is retried, not replaced with an empty one" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  $appdata = $i.AppData
  $lock = $null
  try {
    $token = Pair $i
    Stop-TestInstance $i -Keep
    $store = Join-Path $i.Dir "companion.json"
    $before = Get-Content $store -Raw
    $lock = [IO.File]::Open($store, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    $i = Start-PhoneIn $appdata
    Wait-Until { (Read-Log $i) -match "phone store busy" } | Out-Null
    Check (@(Listening $PHONE).Count -eq 0) "no listener while the store can't be read"
    $lock.Close(); $lock = $null
    Wait-Until { @(Listening $PHONE).Count -gt 0 } 15000 | Out-Null
    Check ((Phone GET "/api/status" -token $token).Status -eq 200) "still paired after the lock cleared"
    Check ((Get-Content $store -Raw) -eq $before) "store unchanged"
  } finally { if ($lock) { $lock.Close() }; Stop-Phone $i }
}

Scenario "a busy phone port is reported on the PC page and retried" {
  $squat = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, $PHONE)
  $squat.Start()
  try {
    $i = Start-Phone (Base @{ phone_enabled = $true })
    try {
      Wait-Until { (Read-Log $i) -match "phone listener failed" } | Out-Null
      $err = (Admin $i GET "/admin/state").Json.listenerError
      Check ($err -like "*$PHONE*in use*") "PC page explains ($err)"
      $squat.Stop(); $squat = $null
      Wait-Until { (Read-Log $i) -match "phone listener on" } 15000 | Out-Null
      Check ($null -eq (Admin $i GET "/admin/state").Json.listenerError) "warning cleared"
      Check ((Phone GET "/").Status -eq 200) "phone reaches Unseat again"
    } finally { Stop-Phone $i }
  } finally { if ($squat) { $squat.Stop() } }
}

Scenario "turning reminders off and on quickly always leaves the listener running" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  try {
    Wait-Until { @(Listening $PHONE).Count -gt 0 } | Out-Null
    for ($n = 0; $n -lt 5; $n++) {
      [void](Phone GET "/")
      [void](Admin $i POST "/admin/settings" @{ enabled = $false })
      [void](Admin $i POST "/admin/settings" @{ enabled = $true })
    }
    Wait-Until { (Get-Settings $i).phone_enabled -eq $true } | Out-Null
    Wait-Until { (Phone GET "/").Status -eq 200 } 15000 | Out-Null
    Wait-Until { @(Listening $PHONE).Count -eq 1 } | Out-Null
  } finally { Stop-Phone $i }
}

Scenario "a newer build takes over and keeps iPhone reminders working" {
  $i = Start-Phone (Base @{ phone_enabled = $true })
  $newer = Join-Path $TestRoot "unseat-newer.exe"
  try {
    $token = Pair $i
    Copy-Item $Exe $newer -Force
    (Get-Item $newer).LastWriteTime = (Get-Date).AddMinutes(5)
    $old = @($env:UNSEAT_TEST_PUSH_ORIGIN, $env:UNSEAT_TEST_PUBLIC_HOST)
    $env:UNSEAT_TEST_PUSH_ORIGIN = "http://localhost:$FAKE"; $env:UNSEAT_TEST_PUBLIC_HOST = "pc.example.ts.net"
    try { $p2 = Start-SameInstance $i $newer } finally { $env:UNSEAT_TEST_PUSH_ORIGIN = $old[0]; $env:UNSEAT_TEST_PUBLIC_HOST = $old[1] }
    Check ($i.Process.WaitForExit(12000)) "older instance quit"
    $owner = Wait-Until { netstat -ano -p tcp | Select-String -Pattern "^\s*TCP\s+127\.0\.0\.1:$PHONE\s+\S+\s+LISTENING\s+(\d+)" | ForEach-Object { [int]$_.Matches[0].Groups[1].Value } | Select-Object -First 1 } 12000
    Check ($owner -eq $p2.Id) "the newer build owns the phone listener"
    Check ((Phone GET "/api/status" -token $token).Status -eq 200) "phone still paired"
    $widget = Wait-Until { $h = [UnseatTest]::FindWindowW("UnseatWidget", (TestTitle $i "Unseat")); $o = 0; [void][UnseatTest]::GetWindowThreadProcessId($h, [ref]$o); if ($h -ne [IntPtr]::Zero -and $o -eq $p2.Id) { $h } }
    [void][UnseatTest]::SendMessageW($widget, $MSG_ADVANCE, [IntPtr]1200, [IntPtr]::Zero)
    Wait-Pushes 1 | Out-Null
  } finally {
    Stop-AllTestInstances
    Start-Sleep -Milliseconds 300
    Remove-Item $newer -ErrorAction SilentlyContinue
    Stop-Phone $i
  }
}

$RealAfter = Get-RealUnseat
$results.Add([pscustomobject]@{ Test = "the user's own Unseat was left alone"; Result = $(if ((@($RealBefore) -join ",") -eq (@($RealAfter) -join ",")) { "PASS" } else { "FAIL" }); Ms = 0; Detail = "before: $($RealBefore -join ',') after: $($RealAfter -join ',')" })

Stop-Job $job -ErrorAction SilentlyContinue; Remove-Job $job -Force -ErrorAction SilentlyContinue
$results | Format-Table -AutoSize -Wrap | Out-String -Width 220
$failed = @($results | Where-Object { $_.Result -ne "PASS" }).Count
"$($results.Count - $failed) passed, $failed failed"
if ($failed) { exit 1 }
