# Phase 1: iPhone push feasibility test

A throwaway test that answers one question before any companion code goes into Unseat: **can
an iPhone Home Screen web app, reached privately over Tailscale, reliably get reminders from
this PC through Apple Web Push?**

It is not part of Unseat. It doesn't touch Unseat's code, settings, autostart or executable.
It has no npm dependencies: Node's built-in crypto and fetch, plus one vendored QR library
(`vendor/qrcode.cjs`, MIT).

## What runs where

| Piece | Where | Reachable from |
| --- | --- | --- |
| Phone test app + API | `127.0.0.1:8780` | Your tailnet only, via `tailscale serve` (HTTPS `*.ts.net`) |
| PC control page | `127.0.0.1:8781` | This PC only |
| Push delivery | Apple (`*.push.apple.com`) | Internet |
| Keys, device hashes, results | `data/` (gitignored) | This PC only |

Funnel (public internet) is never used. `start.ps1` refuses to run if any Serve/Funnel config
already exists, so it can't change an existing setup.

## One-time setup (you)

1. **Tailscale admin console → DNS → enable HTTPS Certificates.** This is off for this tailnet today.
   The PC's machine name will then appear in public Certificate Transparency logs (approved in D2).
2. **Install Tailscale on the iPhone** and sign in with the same account. It isn't in the tailnet yet.
3. Node.js 20+ on this PC (24.20 is installed).

## Run

```powershell
powershell -ExecutionPolicy Bypass -File spikes\iphone-push\start.ps1
```

Then open the PC control page at <http://127.0.0.1:8781/>. Ctrl+C in the terminal stops the server and
the foreground `tailscale serve` session.

## Test checklist

The control page walks through these. Mark every push **seen / not seen** after checking the phone.

| # | Check | How |
| --- | --- | --- |
| A | Tailscale HTTPS | Phone opens the `https://…ts.net` address with no certificate warning. Control page → "Check Tailscale Serve / Funnel": Funnel off. |
| B | Home Screen install & pairing | Scan QR → Safari → Share → Add to Home Screen → open app → type code → Approve on PC |
| C | Enable notifications | In the app: Enable notifications → Allow. Device shows `web.push.apple.com`. |
| 1 | Locked phone | Lock phone, send 1 |
| 2 | Latency & reliability | 20 pushes, 30 s apart, phone locked |
| 2b | Urgency normal | 5 pushes with `Urgency: normal` |
| 3 | Service-worker path | 3 plain-JSON pushes (the iOS 16.4–18.3 path) |
| 4 | Phone offline, long TTL | Airplane mode on, send 3 (TTL 15 min), wait 2 min, airplane off |
| 5 | Phone offline, short TTL | Airplane mode on, send 1 (TTL 30 s), wait 2 min, airplane off |
| 6 | Tailscale off on phone | VPN off, internet on: pushes should still arrive; receipts queue and flush later |
| 7 | Wi-Fi → cellular | Switch networks during a batch |
| 8 | PC offline / back | Disconnect PC, send (expect error), reconnect, send |
| 9 | Tap opens app | Tap a notification |
| 10 | After restart | Restart the phone, send without opening the app |
| D | Revoke | Revoke on PC, open app: "This phone was removed" |

Export the results with **Export results JSON** (also saved to `data/results.json`).

### How delay is measured

- **PC clock:** push sent → the phone's service worker reports back to the PC. It uses one clock, so
  it's reliable, but it's an upper bound: it includes waking the service worker and the reply
  over the tailnet.
- **Phone clock:** the time the phone received it minus the time sent. Only as good as the two clocks' agreement.
- **Receipts ≠ display.** A receipt means the service worker ran. Your seen / not seen marks are the
  evidence that a notification was actually shown.

## Proposed pass criteria (for you to accept or change)

- A, B, C work without workarounds.
- 1: shown on the lock screen.
- 2: at least 19 of 20 seen; no lost subscription; PC-clock p95 ≤ 10 s.
- 4: held pushes arrive after reconnecting. 5: expired push does not arrive.
- 6: pushes arrive with Tailscale off; queued receipts reach the PC after reconnecting.
- 8: the PC reports the failure, and the next push after reconnecting works.
- 9, 10, D behave as described.

## Automated tests

```powershell
cd spikes\iphone-push; node --test test/*.test.mjs
```

Covers: RFC 8291 encryption against the RFC's own example (byte for byte), VAPID JWT rules
Apple documents, pairing code limits, loopback-only listeners, tailnet-only and Host checks,
admin header, request limits, token handling, 410 handling, revocation, and that no secret
is written in plain text to the results.

## Clean up afterwards

1. Ctrl+C the script. `tailscale serve status` should say "No serve config".
2. Delete `spikes/iphone-push/data/`.
3. Remove "Unseat Test" from the Home Screen.
4. Optional: turn HTTPS Certificates off again in the admin console. The certificate log entry is permanent.
