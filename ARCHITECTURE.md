# Architecture

Unseat is one Cargo package with two layers.

## Library (`src/lib.rs`, `--no-default-features`)

No Win32. This is the sitting model:

- `SittingEngine` owns session time, break time, overdue, pause, snooze, and beep decisions
- `Settings` is the JSON file under `%APPDATA%\Unseat\settings.json`
- `view` computes overlay layout and hit targets at 96 DPI, then scaled

The host must not invent sitting rules. If a behavior cannot be asked of the engine, it does not exist.

The host supplies monotonic elapsed timestamps. The 1 Hz message is only an update trigger; the
engine reconciles the full timestamp gap, so delayed/coalesced messages cannot create drift.
Idle/return boundaries are split using the last-input timestamp rather than assigning the entire
gap to the state observed at the end.

## Win32 host (`src/win/`, `--features gui`)

Feeds `now`, last-input age, and session lock into the engine, then paints and notifies:

| Piece | Role |
| --- | --- |
| Overlay | Layered Direct2D popup, always-on-top, no-activate |
| Hidden window | 1 Hz update trigger, tray callback, sound stop timer |
| Tray | Pause, reset, snooze, settings, hide overlay, quit |
| Settings | Custom Direct2D dialog; Esc cancels, Enter saves |
| Sound | Embedded PCM WAVs via `PlaySoundW` (`SND_MEMORY`) |
| Autostart | Opt-in `HKCU\...\Run` value `Unseat`; written only after a user changes the startup setting |
| Startup identity | Sets the taskbar AppUserModelID; portable executable runs in place without self-copying or child processes |

## Snooze

When sitting time has reached the limit, `Command::Snooze(d)` extends the effective limit by `d`. The overlay returns to sitting until that extra time elapses, then beeps again. Reset and a qualifying break clear snooze.

## Data

Local only. No network. Settings are written to a temp file then renamed. Corrupt JSON loads defaults. Unknown `alert_sound` values fall back to `chime`. Numeric fields are clamped on load and save.

`inactivity_behavior` selects `pause` (the backward-compatible default) or `continue`. Pause mode
excludes idle, lock, sleep, and closed-app time; Continue mode counts through them. A compact timer
checkpoint is stored with settings so relaunch restores the session. Countdown display is derived
with saturating subtraction, remains at `00:00` when overdue, and uses a dark-maroon completed
surface while session accounting and optional reminders continue.

`timer_display_mode` changes presentation only. `countdown` shows the saturating remaining time;
`stopwatch` shows accumulated sitting time and continues ascending while overdue. Both modes share
the same engine session, completion state, alerts, pause behavior, and persisted checkpoint.
