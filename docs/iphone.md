# iPhone reminders

Get Unseat's reminders on your iPhone: time up, repeat reminders (including the ones that only
play a sound on Windows) and break over. No App Store, no Unseat account, no cloud server of ours.

## What you need

- iOS 16.4 or later (tested on iOS 18.7).
- [Tailscale](https://tailscale.com) on the PC and the iPhone, signed in to the same account.
- In the Tailscale admin console: **DNS → HTTPS Certificates** on. Your PC's Tailscale name then
  appears in public certificate logs; rename the machine first if the name is personal.

## Set up (once)

1. Unseat tray menu → **iPhone reminders**. A page opens in your browser (this PC only).
2. Run the command it shows, for example `tailscale serve --bg 8790`. It shares Unseat with your
   tailnet only. Never use `tailscale funnel` for Unseat.
3. Turn on **Send reminders to paired phones**.
4. **New pairing code**, then scan the QR code with the iPhone camera.
5. In Safari: **Share → Add to Home Screen**, then open **Unseat** from the Home Screen.
6. Type the code, approve the phone on the PC page, then tap **Turn on notifications** and allow.

## Choices

- **Always** (default): every reminder goes to the phone.
- **Only while I'm away from the PC**: only when the PC has been idle past your inactivity time,
  or is locked or asleep.

## How it behaves

- The timer runs on the PC. Reminders reach the phone only while the PC is on, Unseat is running
  and the PC is online.
- If the phone is offline, Apple holds the latest reminder until it goes stale (at most until
  the next repeat, 15 minutes at most; break-over 10 minutes). A newer reminder replaces an older one.
- If the PC can't reach Apple, it retries (2 s, doubling up to 60 s) until the reminder goes stale,
  then counts it as missed. The phone app shows delivered and missed counts.
- Pausing, resetting, snoozing or taking a break on the PC cancels reminders that haven't gone out.
- If Apple drops the phone's subscription, the app says so: open it and tap **Turn on notifications**.
- Remove a phone on the PC page to cut it off at once.

## Privacy

Reminder text is encrypted for your phone; Apple sees only when a push is sent and to which device.
Unseat listens on `127.0.0.1` only and refuses requests that didn't come through your tailnet.
Details: [SECURITY.md](../SECURITY.md).
