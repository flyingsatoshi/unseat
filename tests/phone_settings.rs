use unseat::{PhoneAlerts, Settings, SETTINGS_VERSION};

#[test]
fn iphone_reminders_are_off_by_default_and_mirror_everything_once_on() {
    let s = Settings::default();
    assert!(!s.phone_enabled);
    assert_eq!(s.phone_alerts, PhoneAlerts::All);
    assert_eq!(SETTINGS_VERSION, 3);
}

#[test]
fn version_two_files_load_with_the_phone_off_and_everything_else_kept() {
    let json = br#"{
        "settings_version": 2,
        "sitting_limit_secs": 1800,
        "inactivity_behavior": "pause",
        "launch_with_windows": true,
        "notify_enabled": false,
        "alert_sound": "bell"
    }"#;
    let s = Settings::from_json(json);
    assert!(!s.phone_enabled);
    assert_eq!(s.phone_alerts, PhoneAlerts::All);
    assert_eq!(s.sitting_limit_secs, 1800);
    assert_eq!(s.inactivity_behavior, unseat::InactivityBehavior::Pause);
    assert!(s.launch_with_windows);
    assert!(!s.notify_enabled);
    assert_eq!(s.alert_sound, unseat::AlertSound::Bell);
    assert_eq!(s.settings_version, 3);
}

#[test]
fn phone_choices_round_trip_with_stable_names() {
    let mut s = Settings::default();
    s.phone_enabled = true;
    s.phone_alerts = PhoneAlerts::IdleOnly;
    let json = String::from_utf8(s.to_json()).unwrap();
    assert!(json.contains(r#""phone_enabled": true"#));
    assert!(json.contains(r#""phone_alerts": "idle_only""#));
    let back = Settings::from_json(json.as_bytes());
    assert!(back.phone_enabled);
    assert_eq!(back.phone_alerts, PhoneAlerts::IdleOnly);
}

#[test]
fn an_unknown_phone_mode_falls_back_to_all_without_losing_other_settings() {
    let s = Settings::from_json(br#"{"phone_enabled": true, "phone_alerts": "sometimes", "sitting_limit_secs": 1500}"#);
    assert!(s.phone_enabled);
    assert_eq!(s.phone_alerts, PhoneAlerts::All);
    assert_eq!(s.sitting_limit_secs, 1500);
}

#[test]
fn reset_to_defaults_keeps_the_iphone_connection() {
    let mut s = Settings::default();
    s.phone_enabled = true;
    s.phone_alerts = PhoneAlerts::IdleOnly;
    s.sitting_limit_secs = 1200;
    let d = s.with_default_preferences();
    assert_eq!(d.sitting_limit_secs, 3600);
    assert!(d.phone_enabled);
    assert_eq!(d.phone_alerts, PhoneAlerts::IdleOnly);
}
