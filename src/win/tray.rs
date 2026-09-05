use crate::win::app::App;
use crate::win::{flyout, icon};
use unseat::format_today;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NIN_POPUPCLOSE, NIN_POPUPOPEN, NIN_SELECT, NOTIFYICONDATAW,
    NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenu,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_APP,
    WM_CONTEXTMENU,
};
use windows::core::w;

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

pub fn update(hwnd: HWND, today: std::time::Duration, running: bool) {
    let _ = nid_tip(hwnd, today, running);
}

pub fn remove(hwnd: HWND) {
    flyout::hide();
    unsafe {
        let mut data = empty_nid(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &mut data);
    }
}

pub fn show_menu(app: &App, hwnd: HWND) {
    flyout::hide();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let today = format_today(app.engine.snapshot().today_sitting);
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, w!("Unseat"));
        let today_w: Vec<u16> = today.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_GRAYED,
            0,
            windows::core::PCWSTR(today_w.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let pause = if app.engine.snapshot().running {
            w!("Pause")
        } else {
            w!("Resume")
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE as usize, pause);
        let _ = AppendMenuW(menu, MF_STRING, ID_RESET as usize, w!("Reset"));
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
            let custom = format!("Custom ({} min)", app.settings.snooze_secs / 60);
            let custom_w: Vec<u16> = custom.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = AppendMenuW(
                snooze,
                enable,
                ID_SNOOZE_CUSTOM as usize,
                windows::core::PCWSTR(custom_w.as_ptr()),
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
        let hide = if app.widget_visible {
            w!("Hide overlay")
        } else {
            w!("Show overlay")
        };
        let _ = AppendMenuW(menu, MF_STRING, ID_HIDE as usize, hide);
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
        let _ = DestroyMenu(menu);
    }
}

pub fn show_flyout(app: &App, hwnd: HWND) {
    let today = format_today(app.engine.snapshot().today_sitting);
    flyout::show(hwnd, &today);
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

fn nid_tip(hwnd: HWND, today: std::time::Duration, running: bool) -> bool {
    unsafe {
        let mut data = empty_nid(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = CALLBACK;
        data.hIcon = icon::small();
        let state = if running { "active" } else { "paused" };
        let tip_text = format!("Unseat ({state})\n{}", format_today(today));
        tip(&mut data, &tip_text);
        Shell_NotifyIconW(NIM_MODIFY, &mut data).as_bool()
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
    let mut wide: Vec<u16> = text.encode_utf16().take(127).collect();
    wide.push(0);
    for (i, ch) in wide.iter().enumerate() {
        if i < data.szTip.len() {
            data.szTip[i] = *ch;
        }
    }
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
