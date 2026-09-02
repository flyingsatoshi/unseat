use std::fs;
use unseat::{Settings, TimerShape};

fn temp_settings_path() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("unseat-test-{}", std::process::id()));
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
    assert!(s.launch_with_windows);
    assert_eq!(s.window_x, 40);
    assert_eq!(s.window_y, 40);
    assert_eq!(s.today_sitting_secs, 0);
    assert_eq!(s.step, 1);
}

#[test]
fn corrupt_json_yields_defaults() {
    let s = Settings::from_json(b"{{{not json");
    assert_eq!(s, Settings::default());
}

#[test]
fn round_trip_json_preserves_fields() {
    let mut s = Settings::default();
    s.timer_shape = TimerShape::Disc;
    s.widget_size = unseat::WidgetSize::ExtraLarge;
    s.sitting_limit_secs = 2700;
    s.step = 5;
    let parsed = Settings::from_json(&s.to_json());
    assert_eq!(parsed.timer_shape, TimerShape::Disc);
    assert_eq!(parsed.widget_size, unseat::WidgetSize::ExtraLarge);
    assert_eq!(parsed.sitting_limit_secs, 2700);
    assert_eq!(parsed.step, 5);
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
    s.clamp();
    assert_eq!(s.step, 30);
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
fn garbage_file_yields_defaults() {
    let path = temp_settings_path().with_file_name("garbage-settings.json");
    fs::write(&path, "not-json{{{").unwrap();
    assert_eq!(Settings::load_from(&path), Settings::default());
}
