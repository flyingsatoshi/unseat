use std::time::Duration;
use unseat::{
    face_digits, format_elapsed, format_limit, format_today, hit_control, is_dev_build,
    newer_build_takes_over, progress, timer_digit_px, timer_face_digits,
    timer_render_key, widget_layout, widget_pixel_size, Hit, Snapshot, TimerDisplayMode,
    TimerShape, VisibleState, WidgetSize,
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
fn away_keeps_sitting_digits_instead_of_swapping_to_break_remaining() {
    let away = Snapshot {
        state: VisibleState::Away,
        sitting_elapsed: Duration::from_secs(4 * 60 + 5),
        timer_remaining: Duration::from_secs(15 * 60 + 55),
        break_elapsed: Duration::from_secs(20),
        break_remaining: Duration::from_secs(160),
        today_sitting: Duration::ZERO,
        running: true,
        overdue_by: Duration::ZERO,
    };
    assert_eq!(timer_face_digits(TimerDisplayMode::Countdown, away), "15:55");
    assert_eq!(timer_face_digits(TimerDisplayMode::Stopwatch, away), "04:05");
}

#[test]
fn an_explicit_break_shows_its_own_countdown_in_both_modes() {
    let on_break = Snapshot {
        state: VisibleState::Break,
        sitting_elapsed: Duration::from_secs(3700),
        timer_remaining: Duration::ZERO,
        break_elapsed: Duration::from_millis(20_500),
        break_remaining: Duration::from_millis(159_500),
        today_sitting: Duration::ZERO,
        running: true,
        overdue_by: Duration::ZERO,
    };
    assert_eq!(timer_face_digits(TimerDisplayMode::Countdown, on_break), "02:40");
    assert_eq!(timer_face_digits(TimerDisplayMode::Stopwatch, on_break), "02:40");
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
fn timer_display_modes_use_remaining_or_elapsed_time_without_changing_session_state() {
    let snapshot = Snapshot {
        state: VisibleState::Sitting,
        sitting_elapsed: Duration::from_secs(34),
        timer_remaining: Duration::from_secs(4 * 60 + 26),
        break_elapsed: Duration::ZERO,
        break_remaining: Duration::from_secs(180),
        today_sitting: Duration::from_secs(34),
        running: true,
        overdue_by: Duration::ZERO,
    };

    assert_eq!(
        timer_face_digits(TimerDisplayMode::Countdown, snapshot),
        "04:26"
    );
    assert_eq!(
        timer_face_digits(TimerDisplayMode::Stopwatch, snapshot),
        "00:34"
    );
    assert_eq!(snapshot.sitting_elapsed, Duration::from_secs(34));
}

#[test]
fn stopwatch_keeps_ascending_after_the_goal_is_reached() {
    let snapshot = Snapshot {
        state: VisibleState::Overdue,
        sitting_elapsed: Duration::from_secs(5 * 60 + 29),
        timer_remaining: Duration::ZERO,
        break_elapsed: Duration::ZERO,
        break_remaining: Duration::from_secs(180),
        today_sitting: Duration::from_secs(5 * 60 + 29),
        running: true,
        overdue_by: Duration::ZERO,
    };

    assert_eq!(
        timer_face_digits(TimerDisplayMode::Stopwatch, snapshot),
        "05:29"
    );
}

#[test]
fn long_stopwatch_faces_shrink_to_stay_inside_the_digit_lane() {
    assert_eq!(timer_digit_px("05:29", 26.0, 88.0), 26.0);

    let ten_hours = timer_digit_px("10:00:00", 26.0, 88.0);
    assert!(ten_hours < 26.0);
    assert!(ten_hours >= 20.0);

    let multi_day = timer_digit_px("120:00:00", 26.0, 88.0);
    assert!(multi_day < ten_hours);
    assert!(multi_day > 0.0);
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
        overdue_by: Duration::ZERO,
    };
    let same = timer_render_key(
        snapshot,
        TimerDisplayMode::Countdown,
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
        TimerDisplayMode::Countdown,
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
            TimerDisplayMode::Countdown,
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
            TimerDisplayMode::Countdown,
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
            TimerDisplayMode::Countdown,
            TimerShape::Capsule,
            WidgetSize::Small,
            false,
            true,
        )
    );

    assert_ne!(
        completed_key,
        timer_render_key(
            completed_accounting_only,
            TimerDisplayMode::Stopwatch,
            TimerShape::Capsule,
            WidgetSize::Small,
            false,
            true,
        )
    );

    let stopwatch_key = timer_render_key(
        completed_accounting_only,
        TimerDisplayMode::Stopwatch,
        TimerShape::Capsule,
        WidgetSize::Small,
        false,
        true,
    );
    completed_accounting_only.sitting_elapsed += Duration::from_secs(1);
    assert_ne!(
        stopwatch_key,
        timer_render_key(
            completed_accounting_only,
            TimerDisplayMode::Stopwatch,
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
fn only_a_different_newer_build_takes_over() {
    let old = std::path::Path::new(r"D:\Projects\unseat\target\debug\unseat.exe");
    let app = std::path::Path::new(r"D:\Projects\unseat\dist\unseat.exe");
    let earlier = Some(std::time::UNIX_EPOCH + Duration::from_secs(1_000));
    let later = Some(std::time::UNIX_EPOCH + Duration::from_secs(2_000));
    assert!(newer_build_takes_over(old, earlier, app, later));
    assert!(!newer_build_takes_over(app, later, old, earlier));
    assert!(!newer_build_takes_over(app, earlier, app, later));
    assert!(!newer_build_takes_over(
        app,
        earlier,
        std::path::Path::new(r"d:\projects\UNSEAT\dist\unseat.exe"),
        later
    ));
    assert!(!newer_build_takes_over(old, later, app, later));
    assert!(newer_build_takes_over(old, None, app, later));
    assert!(!newer_build_takes_over(old, earlier, app, None));
}

#[test]
fn today_hm_is_compact() {
    assert_eq!(
        unseat::format_today_hm(Duration::from_secs(2 * 3600 + 14 * 60)),
        "2h 14m"
    );
}

fn snap(state: VisibleState) -> Snapshot {
    Snapshot {
        state,
        sitting_elapsed: Duration::from_secs(3600 + 7 * 60 + 30),
        timer_remaining: Duration::ZERO,
        break_elapsed: Duration::ZERO,
        break_remaining: Duration::from_secs(180),
        today_sitting: Duration::ZERO,
        running: true,
        overdue_by: Duration::from_secs(7 * 60 + 30),
    }
}

#[test]
fn render_key_changes_with_overdue_minutes_and_dpi_but_not_every_overdue_second() {
    let key = |s: Snapshot, scale: f32| {
        timer_render_key(s, TimerDisplayMode::Countdown, TimerShape::Capsule, WidgetSize::Small, false, true)
            .at_scale(scale)
    };
    let a = snap(VisibleState::Overdue);
    let mut b = a;
    b.overdue_by += Duration::from_secs(5);
    let mut c = a;
    c.overdue_by += Duration::from_secs(60);
    assert_eq!(key(a, 1.0), key(b, 1.0));
    assert_ne!(key(a, 1.0), key(c, 1.0));
    assert_ne!(key(a, 1.0), key(a, 1.5));
}

#[test]
fn status_line_reads_naturally_in_every_state() {
    let mut s = snap(VisibleState::Sitting);
    s.timer_remaining = Duration::from_secs(12 * 60 + 1);
    assert_eq!(unseat::status_line(s), "13 min left");
    s.timer_remaining = Duration::from_secs(12 * 60);
    assert_eq!(unseat::status_line(s), "12 min left");
    s.timer_remaining = Duration::from_secs(30);
    assert_eq!(unseat::status_line(s), "Under a minute left");
    assert_eq!(unseat::status_line(snap(VisibleState::Overdue)), "Overdue by 7 min");
    let mut just = snap(VisibleState::Overdue);
    just.overdue_by = Duration::from_secs(20);
    assert_eq!(unseat::status_line(just), "Time to stand up");
    assert_eq!(unseat::status_line(snap(VisibleState::Paused)), "Paused");
    assert!(unseat::status_line(snap(VisibleState::Away)).starts_with("Away"));
    assert_eq!(unseat::status_line(snap(VisibleState::Break)), "On break · 3 min left");
}

#[test]
fn dragging_snaps_to_nearby_work_area_edges_only() {
    let work = (0, 0, 1920, 1040);
    assert_eq!(unseat::snap_to_edges(8, 500, 144, 56, work, 12), (0, 500));
    assert_eq!(unseat::snap_to_edges(1770, 990, 144, 56, work, 12), (1776, 984));
    assert_eq!(unseat::snap_to_edges(400, 300, 144, 56, work, 12), (400, 300));
    let second_monitor = (-1280, 200, 0, 1224);
    assert_eq!(unseat::snap_to_edges(-1275, 210, 144, 56, second_monitor, 12), (-1280, 200));
}

fn center_hit(state: VisibleState, hover: bool, at: (f32, f32)) -> Option<Hit> {
    hit_control(TimerShape::Capsule, hover, state, 1.0, at.0.round() as i32, at.1.round() as i32)
}

#[test]
fn overlay_window_is_the_signal_pill_at_every_size() {
    assert_eq!(widget_pixel_size(TimerShape::Capsule, 1.0), (176, 60));
    assert_eq!(widget_pixel_size(TimerShape::Capsule, WidgetSize::Micro.factor()), (132, 45));
    assert_eq!(widget_pixel_size(TimerShape::Capsule, WidgetSize::ExtraLarge.factor()), (440, 150));
    let l = widget_layout(1.0);
    assert_eq!(l.radius, 12.0, "a little sharper than a capsule");
    assert!(l.radius < l.pill_h / 2.0);
}

#[test]
fn shadow_margin_is_even_on_every_side() {
    let l = widget_layout(1.0);
    let (w, h) = widget_pixel_size(TimerShape::Capsule, 1.0);
    assert!((w as f32 - l.pill_x - l.pill_w - l.pill_x).abs() < 0.01);
    assert!((h as f32 - l.pill_y - l.pill_h - l.pill_y).abs() < 0.01);
}

#[test]
fn mm_ss_fits_the_digit_lane_at_full_size() {
    let l = widget_layout(1.0);
    let lane = l.digits_r - l.digits_l;
    assert_eq!(timer_digit_px("12:52", l.digit_px, lane), l.digit_px, "lane {lane}");
    assert!(l.digits_r <= l.tag_l, "digits run into the tag");
    assert!(l.tag_r <= l.pill_x + l.pill_w - 8.0);
}

#[test]
fn hover_buttons_sit_inside_the_pill_evenly_without_touching() {
    let l = widget_layout(1.0);
    let xs = [l.pause.0, l.brk.0, l.reset.0];
    assert!((xs[1] - xs[0] - (xs[2] - xs[1])).abs() < 0.01);
    assert!(xs[1] - xs[0] > 2.0 * l.control_r, "buttons overlap");
    assert!(l.pause.0 - l.control_r > l.digits_hover_r);
    assert!(l.reset.0 + l.control_r <= l.pill_x + l.pill_w);
    for (_, y) in [l.pause, l.brk, l.reset] {
        assert!(y - l.control_r >= l.pill_y && y + l.control_r <= l.pill_y + l.pill_h);
    }
    let gap = (l.close.0 - l.reset.0).hypot(l.close.1 - l.reset.1);
    assert!(gap > l.close_r + l.control_r, "close chip overlaps reset");
}

#[test]
fn controls_only_exist_while_hovered() {
    let l = widget_layout(1.0);
    for (at, hit) in [(l.pause, Hit::Pause), (l.brk, Hit::Break), (l.reset, Hit::Reset), (l.close, Hit::Close)] {
        assert_eq!(center_hit(VisibleState::Sitting, false, at), None);
        assert_eq!(center_hit(VisibleState::Sitting, true, at), Some(hit));
    }
}

#[test]
fn clicking_a_paused_pill_anywhere_resumes() {
    let l = widget_layout(1.0);
    let middle = (l.pill_x + l.pill_w / 2.0, l.pill_y + l.pill_h / 2.0);
    assert_eq!(center_hit(VisibleState::Paused, false, middle), Some(Hit::Pause));
    assert_eq!(center_hit(VisibleState::Paused, false, (2.0, 2.0)), None, "outside the pill");
}

#[test]
fn overdue_digits_snooze_but_hover_buttons_still_win() {
    let l = widget_layout(1.0);
    let digits = (l.digits_l + 10.0, l.pill_y + l.pill_h / 2.0);
    assert_eq!(center_hit(VisibleState::Overdue, false, digits), Some(Hit::Snooze));
    assert_eq!(center_hit(VisibleState::Sitting, false, digits), None);
    assert_eq!(center_hit(VisibleState::Overdue, true, l.reset), Some(Hit::Reset));
}

#[test]
fn tag_shows_goal_overdue_minutes_or_state() {
    let limit = Duration::from_secs(20 * 60);
    let tag = |state, mode| unseat::tag_text(snap(state), mode, limit);
    assert_eq!(tag(VisibleState::Sitting, TimerDisplayMode::Countdown), "20m");
    assert_eq!(tag(VisibleState::Sitting, TimerDisplayMode::Stopwatch), "/ 20m");
    assert_eq!(tag(VisibleState::Overdue, TimerDisplayMode::Countdown), "+7m");
    assert_eq!(tag(VisibleState::Break, TimerDisplayMode::Countdown), "break");
    assert_eq!(tag(VisibleState::Away, TimerDisplayMode::Countdown), "away");
    assert_eq!(tag(VisibleState::Paused, TimerDisplayMode::Countdown), "paused");
    assert_eq!(unseat::tag_text(snap(VisibleState::Sitting), TimerDisplayMode::Countdown, Duration::from_secs(7200)), "2h");
}

#[test]
fn render_key_follows_the_progress_band_by_the_pixel() {
    let key = |p: f32| {
        timer_render_key(snap(VisibleState::Sitting), TimerDisplayMode::Countdown, TimerShape::Capsule, WidgetSize::Small, false, true)
            .with_progress(p, 164.0)
    };
    assert_eq!(key(0.5), key(0.501));
    assert_ne!(key(0.5), key(0.51));
}
