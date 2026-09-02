use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::System::SystemInformation::GetTickCount;
use std::time::Duration;

pub fn last_input_age() -> Duration {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let age_ms = GetTickCount().wrapping_sub(info.dwTime);
            Duration::from_millis(age_ms as u64)
        } else {
            Duration::ZERO
        }
    }
}
