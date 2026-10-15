use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{
    FindResourceW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, HICON, ICON_BIG, ICON_SMALL,
    LR_DEFAULTCOLOR, RT_GROUP_ICON, RT_ICON, SM_CXICON, SM_CXSMICON, WM_SETICON,
};

/// `1 ICON "assets/unseat.ico"` in unseat.rc: the icon is read from the exe's own resources
/// rather than embedded a second time.
const ICON_GROUP: u16 = 1;

/// A resource of this executable. Resources are part of the mapped image, so the bytes stay
/// valid for the life of the process.
fn resource(kind: PCWSTR, id: u16) -> Option<&'static [u8]> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        // MAKEINTRESOURCE: a small integer id passed in place of a name.
        let found = FindResourceW(module, PCWSTR(usize::from(id) as *const u16), kind);
        if found.is_invalid() {
            return None;
        }
        let size = SizeofResource(module, found) as usize;
        let ptr = LockResource(LoadResource(module, found).ok()?) as *const u8;
        (!ptr.is_null() && size > 0).then(|| std::slice::from_raw_parts(ptr, size))
    }
}

/// The image closest to `cx` pixels wide, from the icon group directory (GRPICONDIR: a 6-byte
/// header, then 14-byte entries whose last two bytes are the RT_ICON id).
fn image_for(cx: i32) -> Option<&'static [u8]> {
    let dir = resource(RT_GROUP_ICON, ICON_GROUP)?;
    let count = usize::from(u16::from_le_bytes([*dir.get(4)?, *dir.get(5)?]));
    let (entries, _) = dir.get(6..)?.as_chunks::<14>();
    let best = entries
        .iter()
        .take(count)
        .min_by_key(|e| {
            let w = if e[0] == 0 { 256 } else { i32::from(e[0]) };
            (w - cx).abs()
        })?;
    resource(RT_ICON, u16::from_le_bytes([best[12], best[13]]))
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
