use std::time::Duration;
use unseat::{
    alert_actions, scale_wav, volume_peak, wav_peak, AlertSound, Beep, Settings, MAX_PEAK,
};

const SOUNDS: [(&str, &[u8]); 5] = [
    ("chime", include_bytes!("../assets/sounds/chime.wav")),
    ("bell", include_bytes!("../assets/sounds/bell.wav")),
    ("pulse", include_bytes!("../assets/sounds/pulse.wav")),
    ("glass", include_bytes!("../assets/sounds/glass.wav")),
    ("soft", include_bytes!("../assets/sounds/soft.wav")),
];

#[test]
fn every_embedded_sound_is_a_scalable_pcm16_wav() {
    assert_eq!(SOUNDS.len(), AlertSound::ALL.len());
    for (name, wav) in SOUNDS {
        let scaled = scale_wav(wav, 75).unwrap_or_else(|| panic!("{name} not PCM16"));
        assert_eq!(scaled.len(), wav.len(), "{name} keeps its header and length");
        assert_eq!(&scaled[..36], &wav[..36], "{name} keeps its format chunk");
    }
}

#[test]
fn default_volume_is_moderately_louder_than_the_original_assets() {
    let default = Settings::default().alert_volume;
    let originals: Vec<f32> = SOUNDS.iter().map(|(_, w)| wav_peak(w).unwrap()).collect();
    let mean_original = originals.iter().sum::<f32>() / originals.len() as f32;
    let target = volume_peak(default);
    let gain_db = 20.0 * (target / mean_original).log10();
    assert!(gain_db > 3.0 && gain_db < 8.0, "default gain {gain_db:.1} dB");
}

#[test]
fn all_sounds_reach_the_same_peak_so_switching_never_jumps_in_loudness() {
    for volume in [10u8, 50, 75, 100] {
        for (name, wav) in SOUNDS {
            let peak = wav_peak(&scale_wav(wav, volume).unwrap()).unwrap();
            assert!(
                (peak - volume_peak(volume)).abs() < 0.002,
                "{name} at {volume}: {peak}"
            );
            assert!(peak <= MAX_PEAK + 0.001, "{name} never clips");
        }
    }
}

#[test]
fn volume_curve_is_monotonic_and_silent_at_zero() {
    assert_eq!(volume_peak(0), 0.0);
    assert_eq!(volume_peak(100), MAX_PEAK);
    assert_eq!(volume_peak(255), MAX_PEAK);
    let mut last = -1.0;
    for v in 0..=100u8 {
        let p = volume_peak(v);
        assert!(p > last || v == 0);
        last = p;
    }
    let silent = scale_wav(SOUNDS[0].1, 0).unwrap();
    assert_eq!(wav_peak(&silent).unwrap(), 0.0);
}

#[test]
fn non_wav_data_is_rejected_rather_than_corrupted() {
    assert!(scale_wav(b"not a wav file at all", 50).is_none());
    assert!(scale_wav(b"RIFF\x04\x00\x00\x00WAVE", 50).is_none());
}

#[test]
fn time_up_plays_sound_and_shows_a_notification() {
    let s = Settings::default();
    let a = alert_actions(Beep::LimitReached, &s, Duration::from_secs(3600));
    assert!(a.play_sound);
    let notice = a.notification.unwrap();
    assert_eq!(notice.title, "Time to stand up");
    assert!(notice.body.contains("60 min"), "{}", notice.body);
}

#[test]
fn repeat_reminders_sound_but_never_stack_notifications() {
    let a = alert_actions(Beep::Progressive, &Settings::default(), Duration::from_secs(4200));
    assert!(a.play_sound);
    assert!(a.notification.is_none());
}

#[test]
fn break_over_announces_the_new_session() {
    let a = alert_actions(Beep::BreakOver, &Settings::default(), Duration::ZERO);
    assert_eq!(a.notification.unwrap().title, "Break's over");
}

#[test]
fn sound_toggle_volume_and_notification_toggle_are_independent() {
    let mut s = Settings::default();
    s.sound_enabled = false;
    let a = alert_actions(Beep::LimitReached, &s, Duration::from_secs(3600));
    assert!(!a.play_sound);
    assert!(a.notification.is_some());

    s.sound_enabled = true;
    s.alert_volume = 0;
    assert!(!alert_actions(Beep::LimitReached, &s, Duration::from_secs(3600)).play_sound);

    s.alert_volume = 50;
    s.notify_enabled = false;
    let a = alert_actions(Beep::LimitReached, &s, Duration::from_secs(3600));
    assert!(a.play_sound);
    assert!(a.notification.is_none());
}

#[test]
fn no_event_means_no_action() {
    let a = alert_actions(Beep::None, &Settings::default(), Duration::ZERO);
    assert!(!a.play_sound && a.notification.is_none());
}
