# Contributing

Thanks for helping with Unseat.

1. Open an issue before large changes.
2. Keep it local-only. No telemetry, accounts, or network calls.
3. Engine rules live in `src/engine.rs`. Win32 code does not invent timer behavior.
4. Persist new options through `Settings` with `serde(default)` and `clamp()`.
5. Run `cargo test --no-default-features` before a PR.
6. GUI work needs MSVC and `cargo build --release --features gui`.
7. Alert WAVs are original CC0 tones. Regenerate with `python scripts/gen_sounds.py` rather than adding an audio crate.

By submitting a pull request you agree that your contribution is licensed under Apache-2.0, the same license as the rest of this repository. Sound files in `assets/sounds/` stay CC0.
