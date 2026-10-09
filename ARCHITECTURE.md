# Architecture

Unseat is one Cargo package with two layers.

## Library (`src/lib.rs`, `--no-default-features`)

No Win32. Everything that decides behavior lives here and is unit-tested:

| Module | Role |
| --- | --- |
| `engine` | `SittingEngine`: session time, overdue, snooze, manual pause, inactivity (*away*), explicit breaks, alert events, restore from a checkpoint |
| `settings` | `Settings` JSON under `%APPDATA%\Unseat\settings.json`: defaults, clamping, schema migration, reset to defaults, 14-day history |
| `alerts` / `audio` | Which alert events play a sound or show a notification; PCM16 volume scaling |
| `view` | Overlay layout and hit targets at 96 DPI, face digits, tag text, tray status line, edge snapping |
| `theme` | Light and dark palettes, contrast-checked in tests |
| `stats`, `date` | Weekly chart data and civil-date arithmetic |

The host must not invent timer rules. If a behavior cannot be asked of the engine, it does not exist.

### Time

The host supplies monotonic timestamps (`GetTickCount64`, which keeps counting through sleep). The engine reconciles the whole gap on every update, so delayed or coalesced messages never lose or invent time, and a backwards step is ignored. Updates are scheduled just after the face's next second boundary (`next_tick_delay`), so the display never skips or repeats a second. Restarts use a wall-clock checkpoint; a clock that moved backwards never subtracts time.

### States

| State | Meaning |
| --- | --- |
| Sitting / Overdue | Counting; overdue once the (snoozed) limit is reached. Reminders repeat while overdue. |
| Paused | Paused by the user. Nothing counts. |
| Away | Only with *Pause when inactive* (off by default): idle past the threshold, locked or asleep. Sitting time is frozen; away for the break length starts a fresh session. |
| Break | Started by the user. Counts the break length in wall-clock time, ignores inactivity, then starts a fresh session and reports `BreakOver`. |

`inactivity_behavior` keeps its 0.1 JSON name. Settings version 2 made `continue` the default; files written before it stored `pause` without the user choosing it, so loading migrates them once.

### Alerts

The engine reports `LimitReached`, `Progressive` and `BreakOver` regardless of sound settings. `alert_actions` decides: sound if enabled and volume > 0; a notification for time-up and break-over only, so long overdue stretches never pile up toasts. A delayed update that spans several reminder intervals fires one reminder, not a burst.

## Win32 host (`src/win/`, `--features gui`)

| Piece | Role |
| --- | --- |
| `app` | Owns the engine; feeds time, last-input age and lock/sleep state; persists every 60 s, on every command and at shutdown |
| `widget` | Layered Direct2D overlay (no-activate, optionally topmost). Receives the broadcasts the message-only window cannot: theme change, `TaskbarCreated`, display/DPI changes, power, end-session |
| `paint`, `gfx` | Overlay painter with a cached back buffer and render target; shared Direct2D helpers and text-format cache |
| `settings_dlg` | Tabbed settings. One row table drives layout, painting, hit testing and keyboard focus |
| `tray`, `flyout` | Tray menu, status tooltip (updated only when its text changes), notifications, hover card |
| `sound` | Embedded WAVs via `PlaySoundW(SND_MEMORY)` from a volume-scaled copy kept alive while playing |
| `hotkeys` | Optional `Ctrl+Alt+Shift` shortcuts |
| `autostart` | Opt-in `HKCU\...\Run` value; reads Task Manager's StartupApproved state; repairs a stale path |
| `testhook` | Inert unless `UNSEAT_TEST_HOOKS=1`: fake idle time, advance the clock, log events, snapshot the overlay, separate titles/singleton/startup value for tests |

## Testing

- `cargo test --no-default-features`: engine, settings/migration, alerts/volume, view, theme, stats.
- `cargo test --all-targets`: adds settings-window layout, focus-order and hit-test tests.
- `scripts/test_app.ps1`: end-to-end against the real executable through `testhook`.

## Data

Local only. No network. Settings are written to a temp file then renamed. Corrupt JSON loads defaults. Unknown enum values fall back to defaults. Numeric fields are clamped on load and save.
