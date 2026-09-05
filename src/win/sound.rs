use unseat::AlertSound;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ASYNC, SND_LOOP, SND_MEMORY, SND_NODEFAULT, SND_PURGE,
};
use windows::core::PCWSTR;

static CHIME: &[u8] = include_bytes!("../../assets/sounds/chime.wav");
static BELL: &[u8] = include_bytes!("../../assets/sounds/bell.wav");
static PULSE: &[u8] = include_bytes!("../../assets/sounds/pulse.wav");
static GLASS: &[u8] = include_bytes!("../../assets/sounds/glass.wav");
static SOFT: &[u8] = include_bytes!("../../assets/sounds/soft.wav");

fn bytes(kind: AlertSound) -> &'static [u8] {
    match kind {
        AlertSound::Chime => CHIME,
        AlertSound::Bell => BELL,
        AlertSound::Pulse => PULSE,
        AlertSound::Glass => GLASS,
        AlertSound::Soft => SOFT,
    }
}

pub fn play_alert(kind: AlertSound, duration_secs: u64) {
    play_bytes(bytes(kind), duration_secs > 0);
}

pub fn stop() {
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), HMODULE::default(), SND_PURGE);
    }
}

fn play_bytes(wav: &[u8], r#loop: bool) {
    stop();
    let mut flags = SND_ASYNC | SND_MEMORY | SND_NODEFAULT;
    if r#loop {
        flags |= SND_LOOP;
    }
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(wav.as_ptr() as *const u16),
            HMODULE::default(),
            flags,
        );
    }
}
