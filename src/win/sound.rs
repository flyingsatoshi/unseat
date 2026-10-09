use std::sync::Mutex;
use unseat::{scale_wav, AlertSound};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ASYNC, SND_LOOP, SND_MEMORY, SND_NODEFAULT, SND_PURGE,
};

static CHIME: &[u8] = include_bytes!("../../assets/sounds/chime.wav");
static BELL: &[u8] = include_bytes!("../../assets/sounds/bell.wav");
static PULSE: &[u8] = include_bytes!("../../assets/sounds/pulse.wav");
static GLASS: &[u8] = include_bytes!("../../assets/sounds/glass.wav");
static SOFT: &[u8] = include_bytes!("../../assets/sounds/soft.wav");

/// The scaled buffer `PlaySoundW(SND_MEMORY | SND_ASYNC)` is reading. It must stay alive and
/// unchanged until playback is stopped, so it is only replaced after `stop()`.
struct Playing {
    kind: AlertSound,
    volume: u8,
    wav: Vec<u8>,
}

static PLAYING: Mutex<Option<Playing>> = Mutex::new(None);

fn bytes(kind: AlertSound) -> &'static [u8] {
    match kind {
        AlertSound::Chime => CHIME,
        AlertSound::Bell => BELL,
        AlertSound::Pulse => PULSE,
        AlertSound::Glass => GLASS,
        AlertSound::Soft => SOFT,
    }
}

/// Play `kind` at `volume` (0–100). Any sound already playing is stopped first, so alerts
/// never overlap. `looped` repeats until `stop()`.
pub fn play_alert(kind: AlertSound, volume: u8, looped: bool) {
    stop();
    if volume == 0 {
        return;
    }
    let Ok(mut playing) = PLAYING.lock() else {
        return;
    };
    let reuse = matches!(&*playing, Some(p) if p.kind == kind && p.volume == volume);
    if !reuse {
        let wav = scale_wav(bytes(kind), volume).unwrap_or_else(|| bytes(kind).to_vec());
        *playing = Some(Playing { kind, volume, wav });
    }
    let Some(current) = playing.as_ref() else {
        return;
    };
    let mut flags = SND_ASYNC | SND_MEMORY | SND_NODEFAULT;
    if looped {
        flags |= SND_LOOP;
    }
    crate::win::testhook::log(&format!("sound {} volume={volume} loop={looped}", kind.key()));
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(current.wav.as_ptr() as *const u16),
            HMODULE::default(),
            flags,
        );
    }
}

pub fn stop() {
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), HMODULE::default(), SND_PURGE);
    }
}
