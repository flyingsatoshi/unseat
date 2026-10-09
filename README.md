<p align="center">
  <img src="assets/icon.png" alt="Unseat sitting timer icon" width="128">
</p>

<h1 align="center">Unseat</h1>

<p align="center">An always-on-top <strong>sitting timer</strong> for Windows. A tiny overlay watch that counts desk time and nags you to stand up.</p>

<p align="center">No accounts. No cloud. No telemetry.</p>

<p>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"></a>
  <a href="https://github.com/flyingsatoshi/unseat/actions"><img alt="CI" src="https://github.com/flyingsatoshi/unseat/actions/workflows/ci.yml/badge.svg"></a>
</p>

<p align="center">
  <img src="assets/screenshots/overlay-states.png" alt="Unseat overlay: counting, hover controls, overdue, break and paused, in dark and light themes" width="820">
</p>

<p align="center">
  <img src="assets/screenshots/settings.png" alt="Unseat alert settings with sound, volume, reminders and snooze" width="320">
  <img src="assets/screenshots/settings-light.png" alt="Unseat general settings in the light theme with daily statistics" width="320">
</p>

## Sitting timer for Windows

Unseat is a local desktop **timer** and break reminder. It stays on top of your work like a small watch, tracks how long you have been sitting, and alerts you when it is time to get up.

- Keeps counting through idle time, a locked screen, sleep and a minimized overlay; it is wall-clock based, so delayed updates never lose time
- Optional **Pause when inactive** (off by default): the overlay shows *away* while you are gone, and being away for the break length starts a fresh session
- **Breaks**: start one from the tray or `Ctrl+Alt+Shift+B`; it counts down, then a fresh session starts
- Manual pause, inactivity pause (*away*) and breaks each look different on the overlay
- Reverse countdown or ascending stopwatch, without resetting the session
- Timer, pause and break state survive restarts; only Reset starts over
- One alert at the limit, optional repeats while overdue, and a Windows notification when time is up
- Five bundled sounds with a volume slider and Test button; every sound plays at the same loudness
- Snooze from the overlay (click the overdue time), tray, or `Ctrl+Alt+Shift+S`
- XS–XL overlay sizes, optional always-on-top, drag anywhere (snaps to screen edges), position remembered
- Light, dark or system theme; full keyboard navigation in settings
- Optional 14-day statistics with a weekly chart, kept only on this PC
- Optional launch with Windows

Settings live in `%APPDATA%\Unseat\settings.json`.

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the engine and Win32 host split.

## Install

Download `unseat.exe` from [Releases](https://github.com/flyingsatoshi/unseat/releases), put it in a permanent folder of your choice, and run it there. Unseat is portable: it does not copy or relaunch itself, or create shortcuts automatically. You can pin it to the taskbar or create a shortcut yourself.

Close from the taskbar or tray **Quit**. Click the pinned icon to open it again. Hide the overlay from the close chip or tray; the timer keeps running.

Optional: Settings → General → Launch with Windows. This is off by default; the startup registration changes only when you change that switch and save, and the switch shows the real state (including an entry disabled in Task Manager). When it is on and you move the executable, the next launch updates the startup path.

## Shortcuts

Global shortcuts (Settings → General → Keyboard shortcuts, on by default). Shift is included so they never collide with AltGr typing; a combination another app already owns is left alone.

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+Shift+P` | Pause / resume |
| `Ctrl+Alt+Shift+R` | Reset |
| `Ctrl+Alt+Shift+S` | Snooze (default length) |
| `Ctrl+Alt+Shift+B` | Start / end a break |
| `Ctrl+Alt+Shift+U` | Show / hide the overlay |

In settings: `Tab` / `Shift+Tab` move focus, arrow keys change values and tabs, `Space` toggles, `Ctrl+Tab` or `Ctrl+1…4` switch tabs, `Enter` saves and `Esc` cancels.

## Upgrading from 0.1.x

Settings are kept. Earlier versions paused the timer during inactivity by default; that stored default is migrated to the new keep-running behavior once. Turn on **Pause when inactive** under Breaks to get the old behavior back.

## Alerts and snooze

Settings → Alerts picks **Chime**, **Bell**, **Pulse**, **Glass**, or **Soft**, a volume (75% by default, a little louder than before), and **Test**. **Once** plays the clip; **2s / 4s / 8s** loops it for that long. Repeat reminders fire on the interval you set and never stack: after sleep only one plays. A new sound always stops the previous one.

When the sitting limit is reached, snooze **5 / 10 / 15 / 30** minutes or a custom duration. The last choice is saved. Click the overdue time on the overlay, or use the tray **Snooze** menu.

## Build from source

Windows 10/11 x64, [Rust](https://rustup.rs/), and the MSVC Build Tools C++ workload including the Windows SDK/resource compiler.

From an x64 Native Tools prompt (or after `vcvars64.bat`):

```bat
cargo test --locked --all-targets
cargo build --locked --release --features gui
```

The binary is `target\release\unseat.exe`.

End-to-end tests drive the real executable (idle, lock, sleep/resume, minimize, restart, migration, sizes, monitors, tray, startup entry, shortcuts). They run an isolated test instance beside any Unseat you already have open, with its own settings folder and startup value:

```bat
powershell -ExecutionPolicy Bypass -File scripts\test_app.ps1
powershell -ExecutionPolicy Bypass -File scripts\capture_screens.ps1
powershell -ExecutionPolicy Bypass -File scripts\measure.ps1 -Exes target\release\unseat.exe
```

Library tests do not need the GUI feature. The Win32 host is behind `--features gui`.

GNU builds additionally need MinGW `windres` on PATH (or configured through the resource compiler's `RC` environment variable). GUI builds require resources and fail if they cannot embed the application manifest, icon, and version information.

For release signing and handling antivirus false positives, see [Windows distribution](docs/windows-distribution.md). Build/test success does not establish an antivirus verdict.

To regenerate the icon (`scripts\make_icon.ps1`) or alert WAVs (optional):

```bat
python scripts\gen_sounds.py
```

## Privacy

Unseat never phones home. It does not upload usage, crash reports, or identity. Today’s total and the optional 14-day history are stored only on this PC.

## License

[Apache License 2.0](LICENSE). Contributions are accepted under the same terms. Alert sounds in `assets/sounds/` are [CC0](assets/sounds/LICENSE.txt). See [NOTICE](NOTICE) for third-party crate notes.

**Unseat** is a name of this project. Forks may use the code; they should not present themselves as the official Unseat app.
