use std::time::Duration;
use unseat::{
    face_digits, format_elapsed, format_limit, format_today, hit_control, installed_exe,
    is_dev_build, pill_background_rgb, progress, timer_render_key, widget_layout,
    widget_pixel_size, Hit, Snapshot, TimerShape, VisibleState, WidgetSize,
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
    assert_eq!(
        unseat::format_goal_parts(Duration::from_secs(3600)),
        ("60".into(), "min")
    );
    assert_eq!(
        unseat::format_goal_parts(Duration::from_secs(7200)),
        ("2".into(), "hr")
    );
}

#[test]
fn goal_tag_keeps_sitting_limit_on_break() {
    let limit = Duration::from_secs(30 * 60);
    let brk = Duration::from_secs(3 * 60);
    assert_eq!(unseat::tag_goal(VisibleState::Break, limit, brk), limit);
    assert_eq!(unseat::tag_goal(VisibleState::Sitting, limit, brk), limit);
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
    assert_eq!(widget_pixel_size(TimerShape::Capsule, 1.0), (144, 56));
    assert_eq!(widget_pixel_size(TimerShape::Disc, 1.0), (144, 56));
    assert_eq!(
        widget_pixel_size(TimerShape::Capsule, unseat::WidgetSize::Micro.factor()),
        (108, 42)
    );
    assert_eq!(
        widget_pixel_size(TimerShape::Capsule, unseat::WidgetSize::Large.factor()),
        (288, 112)
    );
}

#[test]
fn digit_lane_hugs_the_time() {
    let l = widget_layout(1.0);
    let lane = l.digits_r - l.digits_l;
    assert!(lane >= 84.0, "digit lane {lane}px is too narrow for MM:SS");
    assert!(
        lane <= 92.0,
        "digit lane {lane}px leaves a stale hole beside the time"
    );
    let gap = l.tag_x - l.digits_r;
    assert!(gap <= 8.0 + 0.01, "controls sit {gap}px from the time");
}

#[test]
fn face_sits_close_to_the_bar() {
    let l = widget_layout(1.0);
    assert!(
        l.digits_b - l.digits_t <= l.digit_px + 6.0,
        "digit box {}px is taller than the face",
        l.digits_b - l.digits_t
    );
    assert!(
        l.bar_y - l.digits_b <= 3.0 + 0.01,
        "gap above the bar is {}px",
        l.bar_y - l.digits_b
    );
    assert!(
        l.pill_h <= 46.0,
        "pill height {}px still has vertical slack",
        l.pill_h
    );
}

#[test]
fn window_chrome_is_even() {
    let l = widget_layout(1.0);
    let (w, h) = widget_pixel_size(TimerShape::Capsule, 1.0);
    let right = w as f32 - l.pill_x - l.pill_w;
    let bottom = h as f32 - l.pill_y - l.pill_h;
    assert!(
        (l.pill_x - right).abs() < 0.01,
        "left {} vs right {right}",
        l.pill_x
    );
    assert!(
        (l.pill_y - bottom).abs() < 0.01,
        "top {} vs bottom {bottom}",
        l.pill_y
    );
}

#[test]
fn goal_tag_stays_inside_the_pill() {
    let l = widget_layout(1.0);
    assert!(l.tag_x >= l.pill_x + 4.0);
    assert!(l.tag_x + l.tag_w <= l.pill_x + l.pill_w - 4.0);
    assert!(l.tag_y >= l.pill_y);
    assert!(l.tag_y + l.tag_h <= l.pill_y + l.pill_h);
}

#[test]
fn running_hides_controls_until_hover() {
    let l = widget_layout(1.0);
    let (px, py) = (l.pause.0.round() as i32, l.pause.1.round() as i32);
    let (rx, ry) = (l.reset.0.round() as i32, l.reset.1.round() as i32);
    assert_eq!(
        hit_control(TimerShape::Card, false, VisibleState::Sitting, 1.0, px, py),
        None
    );
    assert_eq!(
        hit_control(TimerShape::Card, true, VisibleState::Sitting, 1.0, px, py),
        Some(Hit::Pause)
    );
    assert_eq!(
        hit_control(TimerShape::Card, true, VisibleState::Sitting, 1.0, rx, ry),
        Some(Hit::Reset)
    );
}

#[test]
fn break_keeps_sitting_digits_instead_of_swapping_to_break_remaining() {
    assert_eq!(face_digits(Duration::from_secs(15 * 60 + 55)), "15:55");
}

#[test]
fn countdown_face_is_stable_across_repaints_and_stops_at_zero() {
    let remaining = Duration::from_secs(2 * 60 + 13);
    assert_eq!(face_digits(remaining), "02:13");
    assert_eq!(face_digits(remaining), "02:13");
    assert_eq!(face_digits(Duration::ZERO), "00:00");
}

#[test]
fn countdown_does_not_show_zero_before_expiry() {
    assert_eq!(face_digits(Duration::from_millis(1)), "00:01");
    assert_eq!(face_digits(Duration::from_millis(1001)), "00:02");
}

#[test]
fn completed_timer_uses_a_subtle_dark_maroon_background() {
    assert_eq!(
        pill_background_rgb(VisibleState::Overdue),
        [0x2A, 0x10, 0x16]
    );
    assert_eq!(
        pill_background_rgb(VisibleState::Sitting),
        [0x10, 0x12, 0x14]
    );
}

#[test]
fn render_key_tracks_countdown_state_without_hash_collisions() {
    let snapshot = Snapshot {
        state: VisibleState::Sitting,
        sitting_elapsed: Duration::from_secs(10),
        timer_remaining: Duration::from_secs(20),
        break_elapsed: Duration::ZERO,
        break_remaining: Duration::from_secs(180),
        today_sitting: Duration::from_secs(10),
        running: true,
    };
    let same = timer_render_key(
        snapshot,
        TimerShape::Capsule,
        WidgetSize::Small,
        false,
        true,
    );
    let mut advanced = snapshot;
    advanced.timer_remaining = Duration::from_secs(19);
    let mut completed_accounting_only = snapshot;
    completed_accounting_only.state = VisibleState::Overdue;
    completed_accounting_only.timer_remaining = Duration::ZERO;
    let completed_key = timer_render_key(
        completed_accounting_only,
        TimerShape::Capsule,
        WidgetSize::Small,
        false,
        true,
    );
    completed_accounting_only.sitting_elapsed += Duration::from_secs(1);
    completed_accounting_only.today_sitting += Duration::from_secs(1);

    assert_eq!(
        same,
        timer_render_key(
            snapshot,
            TimerShape::Capsule,
            WidgetSize::Small,
            false,
            true
        )
    );
    assert_ne!(
        same,
        timer_render_key(
            advanced,
            TimerShape::Capsule,
            WidgetSize::Small,
            false,
            true
        )
    );
    assert_eq!(
        completed_key,
        timer_render_key(
            completed_accounting_only,
            TimerShape::Capsule,
            WidgetSize::Small,
            false,
            true,
        )
    );
}

#[test]
fn downloads_exe_is_not_a_dev_build() {
    assert!(!is_dev_build(std::path::Path::new(
        r"C:\Users\me\Downloads\unseat.exe"
    )));
    assert!(is_dev_build(std::path::Path::new(
        r"D:\Projects\unseat\target\release\unseat.exe"
    )));
}

#[test]
fn installed_exe_lives_under_localappdata_unseat() {
    assert_eq!(
        installed_exe(std::path::Path::new(r"C:\Users\me\AppData\Local")),
        std::path::Path::new(r"C:\Users\me\AppData\Local\Unseat\unseat.exe")
    );
}

#[test]
fn paused_play_overlay_is_hittable_without_hover() {
    let (cx, cy, _r) = unseat::play_center(1.0);
    assert_eq!(
        hit_control(
            TimerShape::Card,
            false,
            VisibleState::Paused,
            1.0,
            cx.round() as i32,
            cy.round() as i32
        ),
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

#[test]
fn close_is_hittable_only_on_hover() {
    let l = widget_layout(1.0);
    let x = l.close.0.round() as i32;
    let y = l.close.1.round() as i32;
    assert_eq!(
        hit_control(TimerShape::Card, false, VisibleState::Sitting, 1.0, x, y),
        None
    );
    assert_eq!(
        hit_control(TimerShape::Card, true, VisibleState::Sitting, 1.0, x, y),
        Some(Hit::Close)
    );
}

#[test]
fn close_does_not_steal_pause_or_reset() {
    let l = widget_layout(1.0);
    let (px, py) = (l.pause.0.round() as i32, l.pause.1.round() as i32);
    let (rx, ry) = (l.reset.0.round() as i32, l.reset.1.round() as i32);
    assert_eq!(
        hit_control(TimerShape::Card, true, VisibleState::Sitting, 1.0, px, py),
        Some(Hit::Pause)
    );
    assert_eq!(
        hit_control(TimerShape::Card, true, VisibleState::Sitting, 1.0, rx, ry),
        Some(Hit::Reset)
    );
}

#[test]
fn close_sits_in_the_window_corner() {
    let l = widget_layout(1.0);
    let (w, _h) = widget_pixel_size(TimerShape::Capsule, 1.0);
    let right = w as f32 - (l.close.0 + l.close_r);
    let top = l.close.1 - l.close_r;
    assert!(
        (right - top).abs() < 0.01,
        "right inset {right} vs top {top}"
    );
    assert!(
        right <= 2.0 + 0.01,
        "close inset {right} should sit in the chrome"
    );
    let dx = l.close.0 - l.pause.0;
    let dy = l.close.1 - l.pause.1;
    let sep = (dx * dx + dy * dy).sqrt() - l.close_r - l.control_r;
    assert!(sep >= 5.5, "close overlaps pause (gap {sep})");
}

#[test]
fn control_chip_padding_is_even() {
    let l = widget_layout(1.0);
    let pad_left = (l.pause.0 - l.control_r) - l.tag_x;
    let pad_right = (l.tag_x + l.tag_w) - (l.pause.0 + l.control_r);
    let pad_top = (l.pause.1 - l.control_r) - l.tag_y;
    let pad_bottom = (l.tag_y + l.tag_h) - (l.reset.1 + l.control_r);
    assert!(
        (pad_left - pad_right).abs() < 0.01,
        "left {pad_left} vs right {pad_right}"
    );
    assert!(
        (pad_top - pad_bottom).abs() < 0.01,
        "top {pad_top} vs bottom {pad_bottom}"
    );
    let inset_l = l.digits_l - l.pill_x;
    let inset_r = (l.pill_x + l.pill_w) - (l.tag_x + l.tag_w);
    assert!(
        (inset_l - inset_r).abs() < 0.01,
        "pill left {inset_l} vs right {inset_r}"
    );
}

#[test]
fn overdue_digits_are_a_snooze_hit() {
    let l = widget_layout(1.0);
    let x = ((l.digits_l + l.digits_r) * 0.5).round() as i32;
    let y = ((l.digits_t + l.digits_b) * 0.5).round() as i32;
    assert_eq!(
        hit_control(TimerShape::Card, false, VisibleState::Sitting, 1.0, x, y),
        None
    );
    assert_eq!(
        hit_control(TimerShape::Card, false, VisibleState::Overdue, 1.0, x, y),
        Some(Hit::Snooze)
    );
}

#[test]
fn overdue_controls_still_beat_snooze() {
    let l = widget_layout(1.0);
    let (px, py) = (l.pause.0.round() as i32, l.pause.1.round() as i32);
    assert_eq!(
        hit_control(TimerShape::Card, true, VisibleState::Overdue, 1.0, px, py),
        Some(Hit::Pause)
    );
}
