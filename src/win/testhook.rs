//! Opt-in seams for driving the real application from `scripts/test_app.ps1`.
//!
//! Everything here is inert unless the process starts with `UNSEAT_TEST_HOOKS=1`. With it
//! set, the overlay window accepts a few private messages to fake input idle time and
//! advance the clock, alert/tray/notification activity is appended to
//! `%APPDATA%\Unseat\test-events.log`, and autostart writes a separate `UnseatTest` value so a
//! test run can never touch the user's real startup entry, and windows get " (test)" titles.
//!
//! Isolation from the user's Unseat and from other test runs:
//! - `UNSEAT_TEST_ID=<id>` gives this run its own window titles (" (test <id>)") and its own
//!   single-instance lock, so two test instances never find or attach to each other.
//! - `UNSEAT_TEST_HIDDEN=1` runs in the background: the overlay never shows, and there is no
//!   tray icon, alert card or Windows notification. Long-running harnesses (the live iPhone
//!   test) use it so nothing on screen can be mistaken for the user's Unseat.
//! - Otherwise a visible test overlay carries a "TEST" badge and the tray tooltip says so
//!   (`UNSEAT_TEST_BADGE=0` turns the badge off, for README screenshots).

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
/// Start the iPhone page's listener without opening a browser; logs `phone admin <url>`.
pub const MSG_PHONE_ADMIN: u32 = WM_APP + 0x45;

const NO_OVERRIDE: u64 = u64::MAX;
static IDLE_OVERRIDE_SECS: AtomicU64 = AtomicU64::new(NO_OVERRIDE);
static CLOCK_OFFSET_SECS: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UNSEAT_TEST_HOOKS").is_some_and(|v| v == "1"))
}

/// This run's id from `UNSEAT_TEST_ID`: letters, digits, `-` and `_`, at most 32.
pub fn run_id() -> Option<&'static str> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        if !enabled() {
            return None;
        }
        let raw = std::env::var("UNSEAT_TEST_ID").ok()?;
        let id: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(32).collect();
        (!id.is_empty()).then_some(id)
    })
    .as_deref()
}

/// Background test run: nothing visible on screen (see the module notes).
pub fn hidden() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| enabled() && std::env::var_os("UNSEAT_TEST_HIDDEN").is_some_and(|v| v == "1"))
}

/// A visible test overlay shows a "TEST" badge.
pub fn badge() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| enabled() && !hidden() && !std::env::var_os("UNSEAT_TEST_BADGE").is_some_and(|v| v == "0"))
}

/// Window title for `base`. Test runs use distinct titles (and a distinct singleton) so a
/// test instance can run beside the user's own Unseat and scripts can tell them apart; a run
/// id makes them distinct from other test runs too.
pub fn title(base: &str) -> windows::core::HSTRING {
    match (enabled(), run_id()) {
        (true, Some(id)) => windows::core::HSTRING::from(format!("{base} (test {id})")),
        (true, None) => windows::core::HSTRING::from(format!("{base} (test)")),
        (false, _) => windows::core::HSTRING::from(base),
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
