# Architecture

Unseat is one Cargo package with two layers.

## Library (`src/lib.rs`, `--no-default-features`)

No Win32. This is the sitting model:

- `SittingEngine` owns session time, break time, overdue, pause, snooze, and beep decisions
- `Settings` is the JSON file under `%APPDATA%\Unseat\settings.json`
- `view` computes overlay layout and hit targets at 96 DPI, then scaled

The host must not invent sitting rules. If a behavior cannot be asked of the engine, it does not exist.

Clock gaps from sleep or lock are capped with `clamp_clock` (2s per tick) before they reach the engine.

## Win32 host (`src/win/`, `--features gui`)

Feeds `now`, last-input age, and session lock into the engine, then paints and notifies:

| Piece | Role |
| --- | --- |
| Overlay | Layered Direct2D popup, always-on-top, no-activate |
| Hidden window | 1 Hz tick, tray callback, sound stop timer |
| Tray | Pause, reset, snooze, settings, hide overlay, quit |
| Settings | Custom Direct2D dialog; Esc cancels, Enter saves |
| Sound | Embedded PCM WAVs via `PlaySoundW` (`SND_MEMORY`) |
| Autostart | `HKCU\...\Run` value `Unseat` |
| Install | First non-dev launch copies to `%LOCALAPPDATA%\Unseat` |

## Snooze

When sitting time has reached the limit, `Command::Snooze(d)` extends the effective limit by `d`. The overlay returns to sitting until that extra time elapses, then beeps again. Reset and a qualifying break clear snooze.

## Data

Local only. No network. Settings are written to a temp file then renamed. Corrupt JSON loads defaults. Unknown `alert_sound` values fall back to `chime`. Numeric fields are clamped on load and save.
