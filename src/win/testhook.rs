//! Opt-in seams for driving the real application from `scripts/test_app.ps1`.
//!
//! Everything here is inert unless the process starts with `UNSEAT_TEST_HOOKS=1`. With it
//! set, the overlay window accepts a few private messages to fake input idle time and
//! advance the clock, alert/tray/notification activity is appended to
//! `%APPDATA%\Unseat\test-events.log`, and autostart writes a separate `UnseatTest` value so a
//! test run can never touch the user's real startup entry, and windows get " (test)" titles.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// wParam = idle seconds to report, or `u32::MAX` to use real input again.
pub const MSG_SET_IDLE: u32 = WM_APP + 0x40;
/// wParam = seconds to jump the monotonic and wall clocks forward, then update.
pub const MSG_ADVANCE: u32 = WM_APP + 0x41;
/// Log one `state ...` line describing the engine and overlay.
pub const MSG_DUMP: u32 = WM_APP + 0x42;
/// Write the overlay's premultiplied BGRA pixels to `%APPDATA%\Unseat\overlay.bgra`
/// (8-byte header: width and height as little-endian u32).
pub const MSG_SNAPSHOT: u32 = WM_APP + 0x43;
/// wParam 1/0: the pointer rests on / leaves a docked overlay.
pub const MSG_DOCK_HOVER: u32 = WM_APP + 0x44;

const NO_OVERRIDE: u64 = u64::MAX;
static IDLE_OVERRIDE_SECS: AtomicU64 = AtomicU64::new(NO_OVERRIDE);
static CLOCK_OFFSET_SECS: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UNSEAT_TEST_HOOKS").is_some_and(|v| v == "1"))
}

/// Window title for `base`. Test runs use distinct titles (and a distinct singleton) so a
/// test instance can run beside the user's own Unseat and scripts can tell them apart.
pub fn title(base: &str) -> windows::core::HSTRING {
    if enabled() {
        windows::core::HSTRING::from(format!("{base} (test)"))
    } else {
        windows::core::HSTRING::from(base)
    }
}

pub fn singleton() -> windows::core::HSTRING {
    title("Local\\UnseatSingleton")
}

pub fn set_idle(secs: u32) {
    let value = if secs == u32::MAX {
        NO_OVERRIDE
    } else {
        u64::from(secs)
    };
    IDLE_OVERRIDE_SECS.store(value, Ordering::Relaxed);
}

pub fn idle_override() -> Option<Duration> {
    if !enabled() {
        return None;
    }
    match IDLE_OVERRIDE_SECS.load(Ordering::Relaxed) {
        NO_OVERRIDE => None,
        secs => Some(Duration::from_secs(secs)),
    }
}

pub fn advance(secs: u32) {
    CLOCK_OFFSET_SECS.fetch_add(u64::from(secs), Ordering::Relaxed);
}

/// Extra time added to both clocks; always zero outside test runs.
pub fn clock_offset() -> Duration {
    if enabled() {
        Duration::from_secs(CLOCK_OFFSET_SECS.load(Ordering::Relaxed))
    } else {
        Duration::ZERO
    }
}

pub fn data_path(name: &str) -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(std::path::PathBuf::from(appdata).join("Unseat").join(name))
}

pub fn log(line: &str) {
    if !enabled() {
        return;
    }
    let Some(path) = data_path("test-events.log") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}
