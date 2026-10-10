use crate::win::{flyout, icon};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_LARGE_ICON, NIIF_NOSOUND,
    NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK,
    NIN_POPUPCLOSE, NIN_POPUPOPEN, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{WM_APP, WM_CONTEXTMENU};

pub const CALLBACK: u32 = WM_APP + 1;
pub const ID_PAUSE: u32 = 10;
pub const ID_RESET: u32 = 11;
pub const ID_SETTINGS: u32 = 12;
pub const ID_HIDE: u32 = 14;
pub const ID_QUIT: u32 = 13;
pub const ID_SNOOZE_5: u32 = 20;
pub const ID_SNOOZE_10: u32 = 21;
pub const ID_SNOOZE_15: u32 = 22;
pub const ID_SNOOZE_30: u32 = 23;
pub const ID_SNOOZE_CUSTOM: u32 = 24;
pub const ID_BREAK: u32 = 25;
pub const ID_PHONE: u32 = 26;

pub fn add(hwnd: HWND) -> bool {
    if !nid(hwnd, true) {
        return false;
    }
    unsafe {
        let mut data = empty_nid(hwnd);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &mut data);
    }
    true
}

pub fn set_tip(hwnd: HWND, text: &str) {
    unsafe {
        let mut data = empty_nid(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = CALLBACK;
        data.hIcon = icon::small();
        tip(&mut data, text);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &mut data);
    }
}

pub fn remove(hwnd: HWND) {
    flyout::hide();
    unsafe {
        let mut data = empty_nid(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &mut data);
    }
}

fn nid(hwnd: HWND, add: bool) -> bool {
    unsafe {
        let mut data = empty_nid(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = CALLBACK;
        data.hIcon = icon::small();
        tip(&mut data, "Unseat");
        let op = if add { NIM_ADD } else { NIM_MODIFY };
        Shell_NotifyIconW(op, &mut data).as_bool()
    }
}

fn empty_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW::default();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data
}

fn tip(data: &mut NOTIFYICONDATAW, text: &str) {
    if crate::win::testhook::enabled() {
        // A test instance's icon must never pass for the user's Unseat.
        copy_wide(&mut data.szTip, &format!("TEST copy · {text}"));
    } else {
        copy_wide(&mut data.szTip, text);
    }
}

/// Show a Windows notification from the tray icon. Unseat plays its own alert sound, so the
/// notification is silent; Focus Assist / Do Not Disturb still applies.
pub fn notify(hwnd: HWND, title: &str, body: &str) {
    crate::win::testhook::log(&format!("notify {title} | {body}"));
    if crate::win::testhook::hidden() {
        return;
    }
    unsafe {
        let mut data = empty_nid(hwnd);
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON | NIIF_NOSOUND;
        data.hBalloonIcon = icon::big();
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, body);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &mut data);
    }
}

fn copy_wide(dst: &mut [u16], text: &str) {
    let mut n = 0;
    for ch in text.encode_utf16().take(dst.len().saturating_sub(1)) {
        dst[n] = ch;
        n += 1;
    }
    dst[n] = 0;
}

pub fn is_balloon_click(msg: u32) -> bool {
    msg == NIN_BALLOONUSERCLICK
}

pub fn lparam_msg(lp: LPARAM) -> u32 {
    lp.0 as u32 & 0xFFFF
}

pub fn is_popup_open(msg: u32) -> bool {
    msg == NIN_POPUPOPEN
}

pub fn is_popup_close(msg: u32) -> bool {
    msg == NIN_POPUPCLOSE
}

pub fn is_select(msg: u32) -> bool {
    msg == NIN_SELECT
}

pub fn is_context(msg: u32) -> bool {
    msg == WM_CONTEXTMENU
        || msg == windows::Win32::UI::WindowsAndMessaging::WM_RBUTTONUP
}

pub fn is_dblclk(msg: u32) -> bool {
    msg == windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDBLCLK
}
