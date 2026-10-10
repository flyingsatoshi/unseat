//! A second launch of a newer build replaces the running Unseat instead of just showing it,
//! so opening an updated exe always runs the new code. The old instance is closed the same
//! way as Quit (it saves today first), then the new one takes the singleton.

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

const WAIT_MS: u32 = 10_000;

fn running_exe(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 32_768];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
            .is_ok();
        let _ = CloseHandle(process);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn built(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Whether this process is a different, newer build than the one that owns `existing`.
pub fn should_replace(existing: HWND) -> bool {
    let (Ok(mine), Some(theirs)) = (std::env::current_exe(), running_exe(existing)) else {
        return false;
    };
    let (mine, theirs) = (resolved(&mine), resolved(&theirs));
    let replace = unseat::newer_build_takes_over(&theirs, built(&theirs), &mine, built(&mine));
    crate::win::testhook::log(&format!("handoff from {theirs:?} replace={replace}"));
    replace
}

/// Ask the running instance to save and quit, then wait to own the singleton `mutex`.
pub fn take_over(existing: HWND, mutex: HANDLE) -> bool {
    unsafe {
        if PostMessageW(existing, WM_CLOSE, WPARAM(0), LPARAM(0)).is_err() {
            return false;
        }
        let waited = WaitForSingleObject(mutex, WAIT_MS);
        waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED
    }
}
