use windows::Win32::Foundation::HWND;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};

pub const WTS_SESSION_LOCK: usize = 0x7;
pub const WTS_SESSION_UNLOCK: usize = 0x8;

pub fn lock_state_for_event(current: bool, event: usize) -> bool {
    match event {
        WTS_SESSION_LOCK => true,
        WTS_SESSION_UNLOCK => false,
        _ => current,
    }
}

pub fn register(hwnd: HWND) {
    unsafe {
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
    }
}

pub fn unregister(hwnd: HWND) {
    unsafe {
        let _ = WTSUnRegisterSessionNotification(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_event_marks_the_session_locked() {
        assert!(lock_state_for_event(false, WTS_SESSION_LOCK));
    }

    #[test]
    fn unlock_event_marks_the_session_unlocked() {
        assert!(!lock_state_for_event(true, WTS_SESSION_UNLOCK));
    }
}
