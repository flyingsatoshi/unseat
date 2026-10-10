//! "Launch with Windows" through the per-user Run key.
//!
//! Task Manager's Startup tab can disable the entry without removing it (it records that
//! under `Explorer\StartupApproved\Run`), so the real state is "Run value present and not
//! disabled there". Turning the setting on clears a Task Manager disable, because the user
//! has just asked for it explicitly.

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APPROVED_KEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");

fn value_name() -> HSTRING {
    // Test runs use their own value so they never touch the user's real startup entry.
    if crate::win::testhook::enabled() {
        HSTRING::from("UnseatTest")
    } else {
        HSTRING::from("Unseat")
    }
}

/// The command for this executable, or `None` for a development build: an entry under
/// `target\` goes stale as soon as the next build lands, so only a real copy (or a test run,
/// which writes its own value) is ever registered.
fn command_line() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    if unseat::is_dev_build(&exe) && !crate::win::testhook::enabled() {
        return None;
    }
    Some(format!("\"{}\"", exe.display()))
}

unsafe fn open(path: PCWSTR, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    (RegOpenKeyExW(HKEY_CURRENT_USER, path, 0, access, &mut key).0 == ERROR_SUCCESS.0)
        .then_some(key)
}

unsafe fn read_bytes(path: PCWSTR, name: &HSTRING) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
    let key = open(path, KEY_QUERY_VALUE)?;
    let mut ty = REG_VALUE_TYPE::default();
    let mut size = 0u32;
    let mut out = None;
    if RegQueryValueExW(key, name, None, Some(&mut ty), None, Some(&mut size)).is_ok() {
        let mut buf = vec![0u8; size as usize];
        if RegQueryValueExW(
            key,
            name,
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
        .is_ok()
        {
            buf.truncate(size as usize);
            out = Some((ty, buf));
        }
    }
    let _ = RegCloseKey(key);
    out
}

/// The command currently registered to run at sign-in, if any.
pub fn registered_command() -> Option<String> {
    unsafe {
        let (ty, bytes) = read_bytes(RUN_KEY, &value_name())?;
        if ty != REG_SZ {
            return None;
        }
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|&c| c != 0)
            .collect();
        Some(String::from_utf16_lossy(&wide))
    }
}

fn disabled_in_task_manager() -> bool {
    unsafe {
        read_bytes(APPROVED_KEY, &value_name())
            .and_then(|(_, b)| b.first().copied())
            .is_some_and(|flag| flag & 1 == 1)
    }
}

pub fn is_enabled() -> bool {
    registered_command().is_some() && !disabled_in_task_manager()
}

unsafe fn set_command(cmd: &str) {
    let Some(key) = open(RUN_KEY, KEY_SET_VALUE) else {
        return;
    };
    let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
    let _ = RegSetValueExW(key, &value_name(), 0, REG_SZ, Some(bytes));
    let _ = RegCloseKey(key);
}

pub fn apply(enabled: bool) {
    let name = value_name();
    unsafe {
        if enabled {
            let Some(cmd) = command_line() else {
                crate::win::testhook::log("autostart skipped for a development build");
                return;
            };
            set_command(&cmd);
            if let Some(approved) = open(APPROVED_KEY, KEY_SET_VALUE) {
                let _ = RegDeleteValueW(approved, &name);
                let _ = RegCloseKey(approved);
            }
        } else if let Some(key) = open(RUN_KEY, KEY_SET_VALUE) {
            let _ = RegDeleteValueW(key, &name);
            let _ = RegCloseKey(key);
        }
    }
    crate::win::testhook::log(&format!("autostart {enabled} {:?}", registered_command()));
}

/// Keep an existing entry pointing at this executable after it has been moved or updated
/// (including one left pointing at an old development build). A Task Manager disable stays.
pub fn repair() {
    if let (Some(current), Some(wanted)) = (registered_command(), command_line()) {
        if !current.eq_ignore_ascii_case(&wanted) {
            unsafe { set_command(&wanted) };
            crate::win::testhook::log(&format!("autostart repaired {wanted:?}"));
        }
    }
}
