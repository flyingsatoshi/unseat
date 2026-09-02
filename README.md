# Unseat

A local-only Windows break reminder. It sits on top of your work, tracks how long you have been at the desk, and nags you to get up.

No accounts. No cloud. No telemetry.

<p>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-Apache%202.0-blue.svg"></a>
  <a href="https://github.com/flyingsatoshi/unseat/actions"><img alt="CI" src="https://github.com/flyingsatoshi/unseat/actions/workflows/ci.yml/badge.svg"></a>
</p>

## What it does

- Always-on-top timer that starts when you launch it
- Idle or lock the session and it treats that as a break
- A qualifying break resets the current session
- Pause freezes the clock; reset starts a fresh session
- One beep at the limit, then a reminder while you stay overdue
- Tray icon, hover flyout, and a settings dialog
- Optional launch with Windows

Settings live in `%APPDATA%\Unseat\settings.json`.

## Install

Download `unseat.exe` from [Releases](https://github.com/flyingsatoshi/unseat/releases) and run it. No installer.

Quit from the tray menu. Right-click the timer for Settings.

## Build from source

Windows 10/11 x64, [Rust](https://rustup.rs/), and MSVC Build Tools.

```bat
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cargo test --no-default-features
cargo build --release --features gui
```

The binary is `target\release\unseat.exe`.

Library tests do not need the GUI feature. The Win32 host is behind `--features gui`.

## Privacy

Unseat never phones home. It does not upload usage, crash reports, or identity. Today’s total is stored only on this PC.

## License

[Apache License 2.0](LICENSE). Contributions are accepted under the same terms.

**Unseat** is a name of this project. Forks may use the code; they should not present themselves as the official Unseat app.

## Related

FlyingSatoshi also builds:

- [upwiz.com](https://upwiz.com) — hosting, free and paid
- [finehost.com](https://finehost.com) — hosting
