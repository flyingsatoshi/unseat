use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use unseat::{AlertSound, InactivityBehavior, Settings, TimerCheckpoint, TimerShape};

fn temp_settings_path() -> std::path::PathBuf {
    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("unseat-test-{}-{id}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir.join("settings.json")
}

#[test]
fn defaults_match_spec() {
    let s = Settings::default();
    assert_eq!(s.timer_shape, TimerShape::Capsule);
    assert_eq!(s.widget_size, unseat::WidgetSize::Small);
    assert_eq!(s.sitting_limit_secs, 3600);
    assert_eq!(s.break_duration_secs, 180);
    assert_eq!(s.idle_after_secs, 60);
    assert!(s.sound_enabled);
    assert!(s.repeat_reminders);
    assert_eq!(s.repeat_every_secs, 600);
    assert!(!s.launch_with_windows);
    assert_eq!(s.window_x, 40);
    assert_eq!(s.window_y, 40);
    assert_eq!(s.today_sitting_secs, 0);
    assert_eq!(s.step, 1);
    assert_eq!(s.alert_sound, AlertSound::Chime);
    assert_eq!(s.alert_duration_secs, 0);
    assert_eq!(s.snooze_secs, 10 * 60);
    assert_eq!(s.inactivity_behavior, InactivityBehavior::Pause);
}

#[test]
fn corrupt_json_yields_defaults() {
    let s = Settings::from_json(b"{{{not json");
    assert_eq!(s, Settings::default());
}

#[test]
fn missing_startup_preference_does_not_register_autostart() {
    assert!(!Settings::from_json(br#"{}"#).launch_with_windows);
}

#[test]
fn explicit_startup_preferences_are_preserved() {
    assert!(Settings::from_json(br#"{"launch_with_windows":true}"#).launch_with_windows);
    assert!(!Settings::from_json(br#"{"launch_with_windows":false}"#).launch_with_windows);
}

#[test]
fn round_trip_json_preserves_fields() {
    let mut s = Settings::default();
    s.timer_shape = TimerShape::Disc;
    s.widget_size = unseat::WidgetSize::ExtraLarge;
    s.sitting_limit_secs = 2700;
    s.step = 5;
    s.inactivity_behavior = InactivityBehavior::Continue;
    let parsed = Settings::from_json(&s.to_json());
    assert_eq!(parsed.timer_shape, TimerShape::Disc);
    assert_eq!(parsed.widget_size, unseat::WidgetSize::ExtraLarge);
    assert_eq!(parsed.sitting_limit_secs, 2700);
    assert_eq!(parsed.step, 5);
    assert_eq!(parsed.inactivity_behavior, InactivityBehavior::Continue);
}

#[test]
fn older_settings_default_to_pausing_during_inactivity() {
    let s = Settings::from_json(br#"{"sitting_limit_secs":3600}"#);
    assert_eq!(s.inactivity_behavior, InactivityBehavior::Pause);
}

#[test]
fn timer_checkpoint_round_trips_for_relaunch_recovery() {
    let mut s = Settings::default();
    s.timer_checkpoint = Some(TimerCheckpoint {
        sitting_elapsed_secs: 123,
        snooze_until_secs: 900,
        running: false,
        saved_at_unix_secs: 2_000,
    });

    let parsed = Settings::from_json(&s.to_json());
    assert_eq!(parsed.timer_checkpoint, s.timer_checkpoint);
}

#[test]
fn inactivity_preference_exposes_pause_and_continue_choices() {
    assert_eq!(
        InactivityBehavior::ALL,
        [InactivityBehavior::Pause, InactivityBehavior::Continue]
    );
    assert_eq!(InactivityBehavior::Pause.chip_label(), "Pause");
    assert_eq!(InactivityBehavior::Continue.chip_label(), "Continue");
    assert_eq!(InactivityBehavior::from_index(0), InactivityBehavior::Pause);
    assert_eq!(
        InactivityBehavior::from_index(1),
        InactivityBehavior::Continue
    );
}

#[test]
fn clamp_brings_values_into_range() {
    let mut s = Settings::default();
    s.sitting_limit_secs = 1;
    s.break_duration_secs = 99_999;
    s.idle_after_secs = 1;
    s.repeat_every_secs = 1;
    s.step = 0;
    s.clamp();
    assert_eq!(s.sitting_limit_secs, 5 * 60);
    assert_eq!(s.break_duration_secs, 30 * 60);
    assert_eq!(s.idle_after_secs, 15);
    assert_eq!(s.repeat_every_secs, 2 * 60);
    assert_eq!(s.step, 1);
    s.step = 99;
    s.alert_duration_secs = 99;
    s.snooze_secs = 1;
    s.clamp();
    assert_eq!(s.step, 30);
    assert_eq!(s.alert_duration_secs, 10);
    assert_eq!(s.snooze_secs, 60);
}

#[test]
fn missing_step_json_defaults_to_one() {
    let s = Settings::from_json(br#"{"sitting_limit_secs":3600}"#);
    assert_eq!(s.step, 1);
}

#[test]
fn missing_file_yields_defaults() {
    let path = temp_settings_path().with_file_name("missing-settings.json");
    let _ = fs::remove_file(&path);
    assert_eq!(Settings::load_from(&path), Settings::default());
}

#[test]
fn save_then_load_round_trips() {
    let path = temp_settings_path();
    let mut s = Settings::default();
    s.timer_shape = TimerShape::Card;
    s.today_sitting_secs = 8040;
    s.save_to(&path).unwrap();
    let loaded = Settings::load_from(&path);
    assert_eq!(loaded.timer_shape, TimerShape::Card);
    assert_eq!(loaded.today_sitting_secs, 8040);
}

#[test]
fn saving_again_replaces_the_existing_settings_file() {
    let path = temp_settings_path();
    let mut s = Settings::default();
    s.save_to(&path).unwrap();

    s.inactivity_behavior = InactivityBehavior::Continue;
    s.timer_checkpoint = Some(TimerCheckpoint {
        sitting_elapsed_secs: 321,
        snooze_until_secs: 0,
        running: true,
        saved_at_unix_secs: 2_000,
    });
    s.save_to(&path).unwrap();

    let loaded = Settings::load_from(&path);
    assert_eq!(loaded.inactivity_behavior, InactivityBehavior::Continue);
    assert_eq!(loaded.timer_checkpoint, s.timer_checkpoint);
}

#[test]
fn garbage_file_yields_defaults() {
    let path = temp_settings_path().with_file_name("garbage-settings.json");
    fs::write(&path, "not-json{{{").unwrap();
    assert_eq!(Settings::load_from(&path), Settings::default());
}

#[test]
fn missing_alert_fields_keep_defaults() {
    let s = Settings::from_json(br#"{"sitting_limit_secs":1800}"#);
    assert_eq!(s.sitting_limit_secs, 1800);
    assert_eq!(s.alert_sound, AlertSound::Chime);
    assert_eq!(s.alert_duration_secs, 0);
    assert_eq!(s.snooze_secs, 10 * 60);
}

#[test]
fn unknown_alert_sound_falls_back_to_chime() {
    let s = Settings::from_json(
        br#"{"alert_sound":"trombone","alert_duration_secs":4,"snooze_secs":900}"#,
    );
    assert_eq!(s.alert_sound, AlertSound::Chime);
    assert_eq!(s.alert_duration_secs, 4);
    assert_eq!(s.snooze_secs, 900);
}

#[test]
fn round_trip_keeps_alert_and_snooze() {
    let mut s = Settings::default();
    s.alert_sound = AlertSound::Glass;
    s.alert_duration_secs = 8;
    s.snooze_secs = 15 * 60;
    let parsed = Settings::from_json(&s.to_json());
    assert_eq!(parsed.alert_sound, AlertSound::Glass);
    assert_eq!(parsed.alert_duration_secs, 8);
    assert_eq!(parsed.snooze_secs, 15 * 60);
}

#[test]
fn snooze_presets_are_five_ten_fifteen_thirty() {
    assert_eq!(
        unseat::SNOOZE_PRESETS_SECS,
        [5 * 60, 10 * 60, 15 * 60, 30 * 60]
    );
}
