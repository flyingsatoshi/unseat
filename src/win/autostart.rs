use windows::Win32::Foundation::{ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: windows::core::PCWSTR = w!("Unseat");

pub fn apply(enabled: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            0,
            KEY_SET_VALUE,
            &mut key,
        )
        .0 != ERROR_SUCCESS.0
        {
            return;
        }
        if enabled {
            if let Ok(exe) = std::env::current_exe() {
                let quoted = format!("\"{}\"", exe.display());
                let mut wide: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = (wide.len() * 2) as u32;
                let _ = RegSetValueExW(
                    key,
                    VALUE,
                    0,
                    REG_SZ,
                    Some(std::slice::from_raw_parts(
                        wide.as_mut_ptr() as *const u8,
                        bytes as usize,
                    )),
                );
            }
        } else {
            let _ = RegDeleteValueW(key, VALUE);
        }
        let _ = RegCloseKey(key);
    }
}
