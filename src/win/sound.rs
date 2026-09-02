use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

static BEEP: &[u8] = include_bytes!("../../assets/beep.wav");

pub fn play() {
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(BEEP.as_ptr() as *const u16),
            HMODULE::default(),
            SND_ASYNC | SND_MEMORY | SND_NODEFAULT,
        );
    }
}
