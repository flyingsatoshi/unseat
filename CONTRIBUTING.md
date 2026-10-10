# Contributing

Thanks for helping with Unseat.

1. Open an issue before large changes.
2. Keep it local-only. No telemetry, accounts, or network calls.
3. Engine rules live in `src/engine.rs`. Win32 code does not invent timer behavior.
   Colors come from `src/theme.rs`; settings rows from `sections()` in `src/win/settings_dlg.rs`.
4. Persist new options through `Settings` with `serde(default)` and `clamp()`.
5. Run `cargo test --no-default-features` before a PR, and `scripts/test_app.ps1` for host changes. End-to-end tests start isolated test copies of Unseat beside yours: a visible one carries a pink TEST badge and a "TEST copy" tray tooltip, each has its own id, windows and settings folder, and the scripts stop only the copies they started. They refuse to run while another test copy is up (a background one started with `UNSEAT_TEST_HIDDEN=1` included), since test copies share test ports.
6. GUI work needs MSVC and `cargo build --release --features gui`. Run `scripts/install.ps1` to replace your everyday copy in `dist\` with the new build.
7. Alert WAVs are original CC0 tones. Regenerate with `python scripts/gen_sounds.py` rather than adding an audio crate.

By submitting a pull request you agree that your contribution is licensed under Apache-2.0, the same license as the rest of this repository. Sound files in `assets/sounds/` stay CC0.
