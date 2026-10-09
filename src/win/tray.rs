use crate::win::app::App;
use crate::win::{flyout, icon};
use unseat::{format_today, status_line};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_LARGE_ICON, NIIF_NOSOUND,
    NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK,
    NIN_POPUPCLOSE, NIN_POPUPOPEN, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenu,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_APP,
    WM_CONTEXTMENU,
};
use windows::core::{w, PCWSTR};

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

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Menu caption with the global shortcut shown right-aligned when shortcuts are on.
fn item(text: &str, key: char, shortcuts: bool) -> Vec<u16> {
    if shortcuts {
        wide(&format!("{text}	Ctrl+Alt+Shift+{key}"))
    } else {
        wide(text)
    }
}

pub fn show_menu(app: &App, hwnd: HWND) {
    flyout::hide();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let snap = app.engine.snapshot();
        let keys = app.settings.hotkeys_enabled;
        let header = wide(&format!("Unseat · {}", status_line(snap)));
        let today = wide(&format_today(snap.today_sitting));
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(header.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(today.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let pause = item(if snap.running { "Pause" } else { "Resume" }, 'P', keys);
        let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE as usize, PCWSTR(pause.as_ptr()));
        let reset = item("Reset", 'R', keys);
        let _ = AppendMenuW(menu, MF_STRING, ID_RESET as usize, PCWSTR(reset.as_ptr()));
        let brk = item(
            if app.engine.on_break() {
                "End break"
            } else {
                "Start break"
            },
            'B',
            keys,
        );
        let _ = AppendMenuW(menu, MF_STRING, ID_BREAK as usize, PCWSTR(brk.as_ptr()));
        let mut custom = Vec::new();
        if let Ok(snooze) = CreatePopupMenu() {
            let enable = if app.engine.can_snooze() {
                MF_STRING
            } else {
                MF_STRING | MF_GRAYED
            };
            let _ = AppendMenuW(snooze, enable, ID_SNOOZE_5 as usize, w!("5 minutes"));
            let _ = AppendMenuW(snooze, enable, ID_SNOOZE_10 as usize, w!("10 minutes"));
            let _ = AppendMenuW(snooze, enable, ID_SNOOZE_15 as usize, w!("15 minutes"));
            let _ = AppendMenuW(snooze, enable, ID_SNOOZE_30 as usize, w!("30 minutes"));
            custom = item(
                &format!("Default ({} min)", app.settings.snooze_secs / 60),
                'S',
                keys,
            );
            let _ = AppendMenuW(
                snooze,
                enable,
                ID_SNOOZE_CUSTOM as usize,
                PCWSTR(custom.as_ptr()),
            );
            let popup = if app.engine.can_snooze() {
                MF_POPUP
            } else {
                MF_POPUP | MF_GRAYED
            };
            let _ = AppendMenuW(menu, popup, snooze.0 as usize, w!("Snooze"));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS as usize, w!("Settings…"));
        let hide = item(
            if app.widget_visible {
                "Hide overlay"
            } else {
                "Show overlay"
            },
            'U',
            keys,
        );
        let _ = AppendMenuW(menu, MF_STRING, ID_HIDE as usize, PCWSTR(hide.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT as usize, w!("Quit"));
        let mut pt = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );
        drop(custom);
        let _ = DestroyMenu(menu);
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
    copy_wide(&mut data.szTip, text);
}

/// Show a Windows notification from the tray icon. Unseat plays its own alert sound, so the
/// notification is silent; Focus Assist / Do Not Disturb still applies.
pub fn notify(hwnd: HWND, title: &str, body: &str) {
    crate::win::testhook::log(&format!("notify {title} | {body}"));
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
