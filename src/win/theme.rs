use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
};
use windows::core::w;

pub fn apps_use_dark() -> bool {
    unsafe {
        let mut key = windows::Win32::System::Registry::HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            0,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return true;
        }
        let mut data: u32 = 1;
        let mut size = 4u32;
        let mut ty = REG_DWORD;
        let ok = RegQueryValueExW(
            key,
            w!("AppsUseLightTheme"),
            None,
            Some(&mut ty),
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(key);
        if ok {
            data == 0
        } else {
            true
        }
    }
}
