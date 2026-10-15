<p align="center">
  <img src="assets/icon.png" alt="Unseat icon" width="112">
</p>

<h1 align="center">Unseat</h1>

<p align="center">A tiny always-on-top sitting timer for Windows.<br>
The pill fills as you sit and turns red when it's time to move.</p>

<p align="center">
  <a href="https://github.com/flyingsatoshi/unseat/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/flyingsatoshi/unseat/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: Apache 2.0" src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"></a>
</p>

<p align="center">
  <img src="assets/screenshots/overlay-states.png" alt="The overlay while sitting, on hover with pause, break and reset, overdue, on a break and paused, in dark and light" width="860">
</p>

<p align="center"><b>Time's up? Take a break, or sit one more session.</b><br>
  <img src="assets/screenshots/time-up-prompt.png" alt="Two small tabs under the red overlay: Break 3m and +20m, with the caption 20 min sitting · time to stand" width="560">
</p>

<p align="center"><b>Docks to a screen edge.</b> Point at it to peek; it slides out when time is up.<br>
  <img src="assets/screenshots/dock-states.png" alt="Docked to the right edge: expanded, tucked into a slim progress tab, and peeking with 13 min" width="640">
</p>

<p align="center"><b>Everything else is one click away in the tray.</b><br>
  <img src="assets/screenshots/tray-panel.png" alt="Tray panel with the overdue time, progress, Pause, Take a break, Snooze, Reset, Settings and iPhone reminders" width="560">
</p>

<p align="center"><b>Simple settings, light or dark.</b><br>
  <img src="assets/screenshots/settings.png" alt="Settings: Timer and Alerts tabs in dark, General tab with the weekly chart in light" width="860">
</p>

## Also

- Keeps counting through idle time, a locked screen and sleep; restarts pick up where they left off.
- Shortcuts: `Ctrl+Alt+Shift` + `P` pause · `B` break · `S` snooze · `R` reset · `U` show/hide.
- Optional iPhone reminders over your own Tailscale network ([setup](docs/iphone.md)).
- Five alert sounds, five sizes, countdown or stopwatch, 14-day stats.
- Local only: no account, cloud or telemetry. One 1.3 MB exe, no installer or runtime.

## Install

Windows 10/11 (x64). Download `unseat.exe` from [Releases](https://github.com/flyingsatoshi/unseat/releases), keep it in a folder of your choice and run it. Settings live in `%APPDATA%\Unseat`.

> 0.2, shown here, is not on Releases yet: take the `unseat-exe` artifact from the latest [CI run on main](https://github.com/flyingsatoshi/unseat/actions/workflows/ci.yml?query=branch%3Amain).

## Build

```bat
cargo build --locked --release --features gui
```

Needs Rust and the MSVC Build Tools. Tests, installing and benchmarks: [CONTRIBUTING.md](CONTRIBUTING.md). Design: [ARCHITECTURE.md](ARCHITECTURE.md).

## License

[Apache 2.0](LICENSE). Sounds [CC0](assets/sounds/LICENSE.txt), [Sora](https://github.com/sora-xor/sora-font) font [OFL 1.1](assets/fonts/OFL.txt). See [NOTICE](NOTICE). Forks may use the code but should not present themselves as the official Unseat.
