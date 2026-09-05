# Unseat Design

Date: 2026-09-02
Status: Approved for spec review

## Goal

Unseat is a local-only Windows desktop reminder that reduces prolonged sitting. It continuously shows how long the user has been actively using the computer, nags once when a sitting limit is reached, and treats time away from the PC as a break. After a qualifying break it starts a fresh sitting session.

It is not a productivity tracker, timesheet, or focus app.

## Non-goals

- Accounts, cloud, telemetry, analytics, crash reporting
- Databases, history graphs, per-app tracking
- Windows toast notifications
- Multi-user profiles
- Calendar or schedule modes
- Hiding the floating timer (quit is how it goes away)

## Constraints

- Windows 10/11 x64
- Rust + native Win32 only. No .NET, Electron, WebView, or UI toolkit (no Slint/egui/iced)
- Single process, single instance (named mutex)
- Standalone `.exe`, no extra runtime install
- Near-zero idle CPU: 1 Hz logic tick, paint only on change
- Target idle RAM: about 5–15 MB
- Local data only, under `%APPDATA%\Unseat`

## Stack

- Language: Rust (edition 2021+), `windows` crate for Win32
- Widget: chrome-less `WS_POPUP` + `WS_EX_LAYERED` + `WS_EX_TOPMOST` + `WS_EX_TOOLWINDOW`
- Paint: Direct2D + DirectWrite
- Tray: `Shell_NotifyIconW` on a hidden message-only window
- Idle: `GetLastInputInfo` each tick
- Lock: `WTSRegisterSessionNotification` / `WM_WTSSESSION_CHANGE`
- Sound: `PlaySoundW` with a tiny embedded WAV, `SND_ASYNC | SND_NODEFAULT`
- Autostart: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `Unseat`
- DPI: Per-monitor V2
- Theme: follow Windows light/dark via `WM_SETTINGCHANGE`

## Architecture

Two layers in one Cargo package:

1. **`unseat-core` (library)** — no Win32. Sitting engine, settings types, today totals, beep decisions. Tested with a fake clock.
2. **`unseat` (binary)** — Win32 host. Feeds `now`, last-input age, and lock state into the engine. Paints the widget, owns tray/settings/autostart/sound/position.

```
Input (clock, last-input age, session locked, user commands)
        │
        ▼
   SittingEngine  →  ViewModel (state, digits, beep, today)
        │
        ▼
   Win32 host (layered card, tray, settings dialog)
```

The host does not implement sitting rules. If it cannot ask the engine, the rule does not exist.

## State machine

Visible states: `Sitting`, `Overdue`, `Break`, `Paused`.

Internal flags: `running` (not paused), `session_locked`, `last_input_age`, `sitting_elapsed`, `break_elapsed`, `limit_beeped`, `progressive_beeps_fired`.

### Launch

Start in `Sitting` immediately. This is always-on, not a stopwatch the user must remember to start.

### Accumulation

Sitting time increases only while **running and active**: not paused, not session-locked, and `last_input_age < idle_after`.

Today's total increases under the same rule (sitting + overdue only). Paused and break time are excluded.

### Idle and lock

- Idle after `idle_after` (default **60 seconds**) → enter `Break`, sitting frozen, `break_elapsed` starts from 0.
- Session lock → enter `Break` immediately (idle age treated as infinite).
- Return before `break_duration` (default **3 minutes**): resume previous sitting elapsed. Idle/lock time is not added to sitting.
- Stay away ≥ `break_duration`: sitting elapsed = 0, overdue and beep counters clear. Remain in `Break` until the user is active and unlocked, then start `Sitting` at 0:00.

### Pause

Full freeze. No sitting accumulation, no beeps. Idle/lock while paused does **not** auto-reset. Resume continues from the paused value. Reset is the only way to clear a paused timer.

### Limit and overdue

When sitting elapsed reaches `sitting_limit` (default **60 minutes**) while running and active:

- State becomes `Overdue`
- Digits keep climbing
- If `sound_enabled` is on, fire one `LimitReached` beep (this still happens when repeat reminders are off)
- If `sound_enabled` and `repeat_reminders` are on (default **on**), a `Progressive` beep every `repeat_every` (default **10 minutes**) of continued active overdue time
- No Windows toasts
- Pause or a qualifying break stops further beeps

### Reset

Sitting elapsed = 0, overdue and beep counters clear, break cleared. **Running is set to true** so the sitting timer starts again from `0:00`.

### Commands

| Command | Effect |
| --- | --- |
| Pause / Resume | Toggle `running` |
| Reset | Sitting = 0, clear overdue/beeps |
| Start | Same as Resume (there is no third Stopped state) |

Tray label is **Pause** when running, **Resume** when paused.

### Day rollover

Today's total is keyed by local calendar date. On the first tick of a new local date, today's sitting seconds reset to 0. In-progress session elapsed is not reset (the current sit can span midnight; only the "Today" counter rolls).

## User interface

### Floating timer

- No title bar, no system buttons, no taskbar button
- Always on top, freely draggable by grabbing anywhere that is not a hover control
- Remembers last position in settings JSON
- If the saved position is off-screen (monitor unplugged), clamp to the nearest work area
- Light/dark follows Windows
- Hover fade: short, cheap timer (~150–200 ms), then sleep. No 60 fps loop while idle

### Shapes (user-selectable)

Stored as `timer_shape`: `capsule` | `card` | `disc`. Default **`capsule`**.

| Shape | Idle face | Today |
| --- | --- | --- |
| Capsule | State dot + time + short label (`sit` / `break` / `paused` / `over`) | Tray and Settings only |
| Card | State label + large time + Today line | On the widget |
| Disc | Time + short label, progress ring toward sitting limit (or remaining break) | Tray and Settings only |

### Hover controls

Window size is **fixed** (control space always reserved). On pointer enter: **Pause/Resume** and **Reset**. On pointer leave: they disappear. Right-click opens Settings. Quit is tray-only. Dragging still works on the rest of the body.

- Capsule: controls on the right of the island
- Card: top-right
- Disc: small pair under the disc

### Color language

| State | Accent |
| --- | --- |
| Sitting | Sage `#7DCEA0` |
| Break | Blue `#7EB6FF` |
| Paused | Warm gray `#C4B8A8` |
| Overdue | Amber `#E8A36A` plus a one-shot edge glow, **no looping pulse** |

Break digits show **remaining** break time (`M:SS`). Sitting/overdue/paused digits show sitting elapsed (`MM:SS` or `H:MM:SS` after 60 minutes).

### Tray menu

```
UNSEAT
Today · 2h 14m sitting
────────
Pause          (or Resume)
Reset
────────
Settings…
────────
Quit
```

### Settings dialog

Small native Win32 dialog. Fields, and only these fields:

| Group | Field | Default |
| --- | --- | --- |
| Appearance | Timer shape: Capsule / Card / Disc (radio buttons) | Capsule |
| Sitting | Limit (minutes) | 60 |
| Sitting | Break (minutes) | 3 |
| Sitting | Idle after (seconds) | 60 |
| Reminders | Sound | on |
| Reminders | Repeat reminders | on |
| Reminders | Repeat every (minutes) | 10 |
| System | Launch with Windows | **on** |
| — | Today · Xh Xm sitting | read-only |

Window position is not a setting. Theme is not a setting.

Validation: limit 5–240 min, break 1–30 min, idle after 15–600 sec, repeat every 2–60 min. Out-of-range edits clamp on apply. Settings persist on each control change; OK/X close the dialog.

## Persistence

Single file: `%APPDATA%\Unseat\settings.json`

```json
{
  "timer_shape": "capsule",
  "sitting_limit_secs": 3600,
  "break_duration_secs": 180,
  "idle_after_secs": 60,
  "sound_enabled": true,
  "repeat_reminders": true,
  "repeat_every_secs": 600,
  "launch_with_windows": true,
  "window_x": 40,
  "window_y": 40,
  "today_date": "2026-09-02",
  "today_sitting_secs": 8040
}
```

Missing file → defaults. Corrupt JSON → defaults, no crash, next successful change rewrites the file. Settings writes are atomic (write temp, then replace).

Session elapsed is **not** persisted across process exit. Quit, crash, or reboot starts a new sitting session at 0:00. Today total **is** persisted. Closing the app is treated as leaving; Unseat does not try to reconstruct a sit across runs.

## Windows integration

- Autostart default **on**. Settings toggle adds/removes the Run key pointing at the current executable path. If the exe moves, the next launch repairs the key when autostart is on.
- Single instance: a second launch focuses the existing widget and exits.
- Notification sound: one short pleasant WAV compiled into the binary. If `sound_enabled` is false, beeps are skipped.
- No installer required for v1. A folder with `unseat.exe` is enough. Autostart uses the absolute path of that exe.

## Error handling

- Direct2D device-lost: recreate resources, skip that frame
- Tray icon failure: keep the widget usable; retry icon add once on next tick
- PlaySound failure: silent, do not crash
- Registry autostart failure: leave the checkbox reflecting last known-good intent; do not crash
- Lock notification registration failure: idle detection still works via `GetLastInputInfo`

## Testing

Confirmed seams (library only):

1. **SittingEngine** — given clock + input snapshot + commands, assert state, elapsed, today delta, and beep kind
2. **Settings** — parse/serialize, defaults, clamp, corrupt JSON → defaults
3. **Today rollover** — local date change zeros today, not session elapsed

Not automated in v1: Direct2D paint, tray, drag, DPI, autostart registry (manual checklist).

Engine tests use an injected `now` and `Input { last_input_age, session_locked }`. No sleeping in tests.

## File layout

```
unseat/
  Cargo.toml                  # lib + bin
  src/lib.rs                  # re-exports core
  src/engine.rs               # SittingEngine
  src/settings.rs             # Settings types + JSON
  src/main.rs                 # Win32 host entry
  src/win/                    # window, tray, d2d, autostart, sound, idle
  assets/beep.wav
  tests/engine.rs
  tests/settings.rs
  docs/superpowers/specs/
```

## Resource policy

- Timer: 1 second (`SetTimer` or equivalent)
- Paint on: state change, displayed second change, hover enter/leave, theme change, DPI change, drag
- Hover animation: at most ~8 frames then stop
- No background threads except what Win32 requires
- No network
