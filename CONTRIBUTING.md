# Contributing

Thanks for helping with Unseat.

1. Open an issue before large changes.
2. Keep it local-only. No telemetry or accounts; the network is used only by features the user turns on (iPhone reminders, Check for updates). See `SECURITY.md`.
3. Engine rules live in `src/engine.rs`. Win32 code does not invent timer behavior.
   Colors come from `src/theme.rs`; settings rows from `sections()` in `src/win/settings_dlg.rs`.
4. Persist new options through `Settings` with `serde(default)` and `clamp()`.
5. Run `cargo test --no-default-features` before a PR, and `scripts/test_app.ps1` for host changes. End-to-end tests start isolated test copies of Unseat beside yours: a visible one carries a pink TEST badge and a "TEST copy" tray tooltip, each has its own id, windows and settings folder, and the scripts stop only the copies they started. They refuse to run while another test copy is up (a background one started with `UNSEAT_TEST_HIDDEN=1` included), since test copies share test ports.
6. GUI work needs Windows 10/11 x64, Rust and the MSVC Build Tools (C++ workload with the Windows SDK): `cargo build --locked --release --features gui`. The exe links the C runtime statically and must import only DLLs that ship with Windows; CI checks this. GNU builds also need MinGW `windres`. The manifest, icon and version resources are required. Builds under `target\` never register to start with Windows, and opening a newer build replaces an older one that is running.
   Run `scripts/install.ps1` to replace your everyday copy in `dist\` with the new build. It keeps `%APPDATA%\Unseat` (settings, statistics, iPhone pairing). `-Exe path` installs a built exe, such as the `unseat-exe` CI artifact, with no Rust needed. `-Docker` is an optional cross-build in a container limited to 4 CPUs and 6 GB, for when local builds are blocked (Smart App Control, `os error 4551`).
7. Alert WAVs are original CC0 tones. Regenerate with `python scripts/gen_sounds.py` rather than adding an audio crate.
8. For changes that could affect speed, memory or size, compare your build with the installed one: `scripts/bench.ps1 -Exes dist\unseat.exe,target\release\unseat.exe -Budgets scripts/perf-budgets.json`. CI fails if the exe outgrows `max.SizeBytes` there; raise a budget only with a measured reason, in the same change. Settings writes need `-IdleSeconds 120` or more to show the save cadence.
9. UI changes: regenerate the README images with `scripts/capture_screens.ps1` (it runs a test copy without the TEST marks). The icon comes from `scripts/make_icon.ps1`. For signing and antivirus questions see [docs/windows-distribution.md](docs/windows-distribution.md).

By submitting a pull request you agree that your contribution is licensed under Apache-2.0, the same license as the rest of this repository. Sound files in `assets/sounds/` stay CC0.
