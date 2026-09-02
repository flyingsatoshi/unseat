use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR,
    SM_CXICON, SM_CXSMICON, WM_SETICON, HICON,
};

const ICO: &[u8] = include_bytes!("../../assets/unseat.ico");

fn image_for(cx: i32) -> Option<&'static [u8]> {
    if ICO.len() < 6 {
        return None;
    }
    let count = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
    let mut best_i = 0usize;
    let mut best_d = i32::MAX;
    for i in 0..count {
        let o = 6 + i * 16;
        if o + 16 > ICO.len() {
            break;
        }
        let w = if ICO[o] == 0 { 256 } else { ICO[o] as i32 };
        let d = (w - cx).abs();
        if d < best_d {
            best_d = d;
            best_i = i;
        }
    }
    let o = 6 + best_i * 16;
    let size = u32::from_le_bytes(ICO[o + 8..o + 12].try_into().ok()?) as usize;
    let offset = u32::from_le_bytes(ICO[o + 12..o + 16].try_into().ok()?) as usize;
    ICO.get(offset..offset.saturating_add(size))
}

fn load(cx: i32) -> HICON {
    let cx = cx.max(16);
    let Some(bytes) = image_for(cx) else {
        return HICON::default();
    };
    unsafe {
        CreateIconFromResourceEx(bytes, true, 0x00030000, cx, cx, LR_DEFAULTCOLOR).unwrap_or_default()
    }
}

pub fn small() -> HICON {
    static ICON: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    HICON(*ICON.get_or_init(|| load(unsafe { GetSystemMetrics(SM_CXSMICON) }).0 as isize) as _)
}

pub fn big() -> HICON {
    static ICON: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    HICON(*ICON.get_or_init(|| load(unsafe { GetSystemMetrics(SM_CXICON) }).0 as isize) as _)
}

pub fn apply(hwnd: HWND) {
    unsafe {
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_SMALL as usize),
            LPARAM(small().0 as isize),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(big().0 as isize),
        );
    }
}
