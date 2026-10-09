//! Global shortcuts: Ctrl+Alt+Shift with P, R, S, B or U.
//!
//! Ctrl+Alt alone doubles as AltGr on many keyboard layouts, so Shift is added to stay out
//! of the way of typing. A combination another app already owns is skipped, never stolen.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    TogglePause,
    Reset,
    Snooze,
    Break,
    ToggleOverlay,
}

const KEYS: [(i32, u32, Action); 5] = [
    (1, b'P' as u32, Action::TogglePause),
    (2, b'R' as u32, Action::Reset),
    (3, b'S' as u32, Action::Snooze),
    (4, b'B' as u32, Action::Break),
    (5, b'U' as u32, Action::ToggleOverlay),
];

fn modifiers() -> HOT_KEY_MODIFIERS {
    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT
}

/// Register every shortcut; returns how many were free to take.
pub fn register(hwnd: HWND) -> usize {
    let mut ok = 0;
    for (id, vk, _) in KEYS {
        if unsafe { RegisterHotKey(hwnd, id, modifiers(), vk) }.is_ok() {
            ok += 1;
        }
    }
    crate::win::testhook::log(&format!("hotkeys registered {ok}/{}", KEYS.len()));
    ok
}

pub fn unregister(hwnd: HWND) {
    for (id, _, _) in KEYS {
        unsafe {
            let _ = UnregisterHotKey(hwnd, id);
        }
    }
}

pub fn action(id: usize) -> Option<Action> {
    KEYS.iter()
        .find(|(k, _, _)| *k as usize == id)
        .map(|(_, _, a)| *a)
}
