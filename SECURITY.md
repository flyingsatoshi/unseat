# Security

Unseat is a local Windows app. By default it does not accept network traffic and does not send data off the machine.

The optional iPhone reminders (off until you turn them on) change that, in a limited way:

- Unseat listens on `127.0.0.1` only. Your phone reaches it through `tailscale serve`, inside your own tailnet. Every phone request must carry the identity header that Tailscale Serve adds; Funnel (public) traffic never has it and is refused. Never publish Unseat with `tailscale funnel`.
- Reminders go to Apple's push service (`*.push.apple.com`) over HTTPS, encrypted end to end for your phone (RFC 8291). Apple sees when a push is sent and to which device, not its text.
- Pairing needs a one-time code (5 minutes, 5 tries) and approval on the PC. Unseat stores only a hash of each phone's secret, and its push signing key encrypted for your Windows account (DPAPI), in `%APPDATA%\Unseat\companion.json`.
- The PC page (tray menu → iPhone reminders) is on `127.0.0.1` too and needs a key that changes every run.

If you find a vulnerability, open a [private security advisory](https://github.com/flyingsatoshi/unseat/security/advisories/new) or email the maintainer via GitHub. Please do not file a public issue for an unfixed vulnerability.

Include:

- Unseat version or commit
- Windows version
- Steps to reproduce
- Impact (for example: unexpected autostart, settings overwrite, local file write)

Do not send settings.json if it is not needed; it only lives on the reporter's PC.

We aim to respond within a week.
