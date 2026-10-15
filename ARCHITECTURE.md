# Architecture

Unseat is one Cargo package with two layers.

## Library (`src/lib.rs`, `--no-default-features`)

No Win32. Everything that decides behavior lives here and is unit-tested:

| Module | Role |
| --- | --- |
| `engine` | `SittingEngine`: session time, overdue, snooze, extend (one more session), manual pause, inactivity (*away*), explicit breaks, alert events, restore from a checkpoint |
| `settings` | `Settings` JSON under `%APPDATA%\Unseat\settings.json`: defaults, clamping, schema migration, reset to defaults, 14-day history |
| `alerts` / `audio` | Which alert events play a sound or show a notification; PCM16 volume scaling |
| `view` | Overlay layout and hit targets at 96 DPI, face digits, tag text, tray status line, edge snapping |
| `theme` | Signal palettes (light and dark): per-state overlay surface, band, digit and tag colours; contrast-checked in tests |
| `dock` | Edge docking: geometry for every edge (`dock_frame`) and a time-driven `DockController` (hide delay, peek, click, alert reveal, pin, fullscreen) |
| `stats`, `date` | Weekly chart data and civil-date arithmetic |
| `about` | Version, commit and release channel from build metadata (`Cargo.toml`, `build.rs`); version comparison and strict parsing of GitHub's latest-release reply |
| `companion` | iPhone reminders: Web Push encryption and VAPID, the reminder policy (all reminders or idle-only, TTLs), the delivery queue (replace, retry, expiry, recovery), pairing, the device store, a strict HTTP parser and the companion API as a pure function. The phone app and PC page are embedded files |

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

The engine reports `LimitReached`, `Progressive` and `BreakOver` regardless of sound settings. `alert_actions` decides: sound if enabled and volume > 0; a Windows notification for time-up and break-over only, so long overdue stretches never pile up toasts. The host shows that notification only while the overlay is hidden; beside a visible overlay the tabs under it ask (`time_up_prompt`) whatever the notification setting, wait while the overlay is hidden or docked away, and hold an auto-hiding dock out (`DockController::hold`). A delayed update that spans several reminder intervals fires one reminder, not a burst.

## Win32 host (`src/win/`, `--features gui`)

| Piece | Role |
| --- | --- |
| `app` | Owns the engine; feeds time, last-input age and lock/sleep state; persists on every command and at shutdown, and otherwise only when a restart would no longer restore the live timer (checked every 60 s) |
| `widget` | Layered Direct2D overlay (no-activate, optionally topmost). Receives the broadcasts the message-only window cannot: theme change, `TaskbarCreated`, display/DPI changes, power, end-session |
| `paint`, `gfx` | Overlay painter (pill as progress bar, docked tab and peek) with a cached back buffer; shared Direct2D helpers, text-format cache, the embedded Sora font loaded from memory, tabular digits measured to fit. Every window draws through one Direct2D factory on the UI thread (`gfx::shared`), so they share one Direct3D device |
| `docking` | Applies `DockController` to the overlay window: placement, slide frames (~16 ms timer only while sliding), drag-to-dock, fullscreen detection |
| `tray_panel` | Custom tray panel that replaces the native menu; keyboard operable |
| `prompt` | When time is up, two small tabs (*Break*, one more session) and a caption hang from the overdue pill, in a layered window kept just below the overlay; a caption when a break ends. Never takes focus |
| `settings_dlg` | Tabbed settings. One row table drives layout, painting, hit testing and keyboard focus |
| `tray`, `flyout` | Tray icon, status tooltip (updated only when its text changes), notifications, hover card |
| `sound` | Embedded WAVs via `PlaySoundW(SND_MEMORY)` from a volume-scaled copy kept alive while playing |
| `hotkeys` | Optional `Ctrl+Alt+Shift` shortcuts |
| `autostart` | Opt-in `HKCU\...\Run` value; reads Task Manager's StartupApproved state; repairs a stale path |
| `update` | Check for updates: one user-initiated HTTPS GET to GitHub's releases API, result shown in Settings; never installs |
| `phone`, `winhttp` | Companion host: loopback listeners, the push sender thread, DPAPI key storage, read-only Tailscale status. Nothing starts until iPhone reminders are on or the page is opened |
| `testhook` | Inert unless `UNSEAT_TEST_HOOKS=1`: fake idle time, advance the clock, log events, snapshot the overlay, separate titles/singleton/startup value for tests |

## Testing

- `cargo test --no-default-features`: engine, settings/migration, alerts/volume, view, theme, stats.
- `cargo test --all-targets`: adds settings-window layout, focus-order and hit-test tests.
- `scripts/test_app.ps1`: end-to-end against the real executable through `testhook`.
- `scripts/test_phone.ps1`: iPhone reminders end to end, against a local fake push service.
- `scripts/bench.ps1`: size, startup, idle CPU and memory, handles, threads, tick timing, repaint cost, window opening, settings writes, handle leaks and shutdown of one or more builds, side by side. `-Budgets scripts/perf-budgets.json` checks the last build against the limits there. CI fails a build whose exe outgrows `max.SizeBytes`; timing and memory depend on the machine, so CI only reports them.

## Data

Local only unless iPhone reminders are turned on (see `SECURITY.md` and `docs/iphone.md`): then a loopback listener for Tailscale Serve and HTTPS requests to Apple's push service. Paired phones live in `companion.json`, apart from settings. Settings are written to a temp file, flushed, then renamed (`write_atomic`). While only time passes they are not rewritten, because restoring the checkpoint already adds the downtime (`checkpoint_current`); they are still rewritten at least every 15 minutes. Corrupt JSON loads defaults. Unknown enum values fall back to defaults. Numeric fields are clamped on load and save.
