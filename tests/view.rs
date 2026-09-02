use std::time::Duration;
use unseat::{
    format_elapsed, format_limit, format_today, hit_control, progress, widget_pixel_size, Hit, TimerShape,
    VisibleState,
};

#[test]
fn elapsed_uses_h_when_over_an_hour() {
    assert_eq!(format_elapsed(Duration::from_secs(47 * 60 + 12)), "47:12");
    assert_eq!(
        format_elapsed(Duration::from_secs(3600 + 4 * 60 + 8)),
        "1:04:08"
    );
}

#[test]
fn limit_caption_uses_minutes_under_two_hours() {
    assert_eq!(format_limit(Duration::from_secs(3600)), "60 min");
    assert_eq!(format_limit(Duration::from_secs(180)), "3 min");
    assert_eq!(unseat::format_goal_parts(Duration::from_secs(3600)), ("60".into(), "min"));
    assert_eq!(unseat::format_goal_parts(Duration::from_secs(7200)), ("2".into(), "hr"));
}

#[test]
fn today_line_matches_spec() {
    assert_eq!(
        format_today(Duration::from_secs(2 * 3600 + 14 * 60)),
        "Today · 2h 14m"
    );
}

#[test]
fn disc_progress_is_sitting_over_limit() {
    let p = progress(
        VisibleState::Sitting,
        Duration::from_secs(1800),
        Duration::from_secs(3600),
        Duration::ZERO,
        Duration::from_secs(180),
    );
    assert!((p - 0.5).abs() < 0.001);
}

#[test]
fn widget_size_is_the_pill() {
    assert_eq!(widget_pixel_size(TimerShape::Capsule, 1.0), (220, 88));
    assert_eq!(widget_pixel_size(TimerShape::Disc, 1.0), (220, 88));
    assert_eq!(
        widget_pixel_size(TimerShape::Capsule, unseat::WidgetSize::Micro.factor()),
        (165, 66)
    );
    assert_eq!(
        widget_pixel_size(TimerShape::Capsule, unseat::WidgetSize::Large.factor()),
        (440, 176)
    );
}

#[test]
fn running_hides_controls_until_hover() {
    assert_eq!(
        hit_control(TimerShape::Card, false, false, 1.0, 194, 30),
        None
    );
    assert_eq!(
        hit_control(TimerShape::Card, true, false, 1.0, 194, 30),
        Some(Hit::Pause)
    );
    assert_eq!(
        hit_control(TimerShape::Card, true, false, 1.0, 194, 58),
        Some(Hit::Reset)
    );
}

#[test]
fn paused_play_overlay_is_hittable_without_hover() {
    assert_eq!(
        hit_control(TimerShape::Card, false, true, 1.0, 82, 44),
        Some(Hit::Pause)
    );
}

#[test]
fn today_hm_is_compact() {
    assert_eq!(
        unseat::format_today_hm(Duration::from_secs(2 * 3600 + 14 * 60)),
        "2h 14m"
    );
}
