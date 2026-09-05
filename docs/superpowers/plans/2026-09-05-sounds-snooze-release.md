# Sounds, Snooze, and Release Hardening

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle open-source alert sounds with preview, duration, and snooze, then harden Unseat for a public GitHub release without pushing.

**Architecture:** Keep sitting rules in `SittingEngine`. Persist new fields on `Settings`. Win32 host plays bundled WAVs via `PlaySoundW`, stops loops on a timer, and exposes snooze from tray, overlay, and settings.

**Tech Stack:** Rust 2021, Win32 (`windows` 0.58), Direct2D, no extra crates.

## Global Constraints

- Windows 10/11 x64, native Win32 only, no extra UI stacks
- Settings JSON under `%APPDATA%\Unseat`, clamp + recover on corrupt input
- Sound licenses must allow redistribution; attribution files required
- Do not push to GitHub
- Stay lightweight: embed small WAVs, no audio libraries

---

## File map

- `src/settings.rs` — `AlertSound`, duration, snooze bounds
- `src/engine.rs` — `Command::Snooze`, effective limit
- `src/view.rs` — `Hit::Snooze` on overdue digits
- `src/win/sound.rs` — bundled clips, preview, loop/stop
- `src/win/settings_dlg.rs` — selector, preview, duration, snooze
- `src/win/tray.rs` / `app.rs` / `widget.rs` — snooze actions, sound timer, shutdown cleanup
- `assets/sounds/` — WAV + LICENSE
- README, ARCHITECTURE, CONTRIBUTING, NOTICE, `.gitignore`
