use std::time::Duration;
use unseat::{
    dock_frame, edge_offset, nearest_edge, DockController, DockDraw, DockEdge, DockPhase,
    DockReveal, Settings,
};

const WORK: (i32, i32, i32, i32) = (0, 0, 1920, 1040);
const FULL: (i32, i32) = (176, 60);

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

fn controller() -> DockController {
    DockController::new(Duration::from_secs(3), Duration::from_secs(8), DockReveal::TimeUp, true)
}

#[test]
fn defaults_keep_docking_off_and_match_the_approved_behaviour() {
    let s = Settings::default();
    assert!(!s.dock_enabled);
    assert_eq!(s.dock_edge, None);
    assert!(s.dock_autohide);
    assert_eq!(s.dock_hide_after_secs, 3);
    assert_eq!(s.dock_reveal, DockReveal::TimeUp);
    assert_eq!(s.dock_reveal_secs, 8);
    assert!(s.dock_hide_fullscreen);
}

#[test]
fn dock_settings_round_trip_clamp_and_undock_when_disabled() {
    let mut s = Settings::default();
    s.dock_enabled = true;
    s.dock_edge = Some(DockEdge::Left);
    s.dock_offset = 0.25;
    s.dock_hide_after_secs = 999;
    let back = Settings::from_json(&s.to_json());
    assert_eq!(back.dock_edge, Some(DockEdge::Left));
    assert_eq!(back.dock_offset, 0.25);
    assert_eq!(back.dock_hide_after_secs, 30);
    let mut off = back.clone();
    off.dock_enabled = false;
    off.clamp();
    assert_eq!(off.dock_edge, None);
    let old = Settings::from_json(br#"{"settings_version":2}"#);
    assert!(!old.dock_enabled && old.dock_offset == 0.5);
}

#[test]
fn nearest_edge_picks_the_closest_edge_within_the_threshold() {
    assert_eq!(nearest_edge((1730, 400, 1910, 460), WORK, 24), Some(DockEdge::Right));
    assert_eq!(nearest_edge((5, 400, 181, 460), WORK, 24), Some(DockEdge::Left));
    assert_eq!(nearest_edge((800, 3, 976, 63), WORK, 24), Some(DockEdge::Top));
    assert_eq!(nearest_edge((800, 990, 976, 1035), WORK, 24), Some(DockEdge::Bottom));
    assert_eq!(nearest_edge((800, 400, 976, 460), WORK, 24), None);
}

#[test]
fn offset_is_a_fraction_of_the_travel_along_the_edge() {
    assert_eq!(edge_offset(DockEdge::Right, (1744, 0, 1920, 60), WORK), 0.0);
    assert_eq!(edge_offset(DockEdge::Right, (1744, 980, 1920, 1040), WORK), 1.0);
    assert!((edge_offset(DockEdge::Top, (872, 0, 1048, 60), WORK) - 0.5).abs() < 0.01);
}

#[test]
fn expanded_frames_are_flush_with_each_edge() {
    let right = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 1.0, false, 1.0);
    assert_eq!(right.rect.2, 1920);
    assert_eq!(right.rect.2 - right.rect.0, 176 - 6, "edge-side shadow margin removed");
    assert_eq!(right.draw, DockDraw::Pill { dx: 0.0, dy: 0.0 });
    let left = dock_frame(DockEdge::Left, WORK, FULL, 0.5, 1.0, false, 1.0);
    assert_eq!(left.rect.0, 0);
    assert_eq!(left.draw, DockDraw::Pill { dx: -6.0, dy: 0.0 });
    let top = dock_frame(DockEdge::Top, WORK, FULL, 0.5, 1.0, false, 1.0);
    assert_eq!((top.rect.1, top.rect.3), (0, 54));
    let bottom = dock_frame(DockEdge::Bottom, (0, 0, 1920, 1032), FULL, 0.5, 1.0, false, 1.0);
    assert_eq!(bottom.rect.3, 1032, "sits on the work area, above the taskbar");
}

#[test]
fn sliding_out_shrinks_into_the_edge_then_leaves_a_tab() {
    let half = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 0.5, false, 1.0);
    assert_eq!(half.rect.2, 1920);
    assert_eq!(half.rect.2 - half.rect.0, 82 + 6);
    let tab = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 0.0, false, 1.0);
    assert_eq!(tab.draw, DockDraw::Tab);
    assert_eq!((tab.rect.2 - tab.rect.0, tab.rect.3 - tab.rect.1), (8, 56));
    assert_eq!(tab.rect.2, 1920);
    let center_expanded = (half.rect.1 + half.rect.3) / 2;
    assert!(((tab.rect.1 + tab.rect.3) / 2 - center_expanded).abs() <= 1, "tab stays centred");
    let peek = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 0.0, true, 1.0);
    assert_eq!(peek.draw, DockDraw::Peek);
    assert_eq!(peek.rect.2 - peek.rect.0, 44);
    let top_tab = dock_frame(DockEdge::Top, WORK, FULL, 0.0, 0.0, false, 1.5);
    assert_eq!((top_tab.rect.1, top_tab.rect.3 - top_tab.rect.1), (0, 12), "scales with DPI");
}

#[test]
fn frame_codes_change_with_every_visible_change() {
    let a = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 1.0, false, 1.0).code();
    let b = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 0.6, false, 1.0).code();
    let c = dock_frame(DockEdge::Right, WORK, FULL, 0.5, 0.0, false, 1.0).code();
    let d = dock_frame(DockEdge::Left, WORK, FULL, 0.5, 0.0, false, 1.0).code();
    assert!(a != b && b != c && c != d);
}

#[test]
fn hides_after_the_delay_once_the_pointer_leaves_and_never_touches_the_timer() {
    let mut d = controller();
    d.pointer_enter(ms(0));
    d.pointer_leave(ms(1000));
    d.tick(ms(3900));
    assert_eq!(d.phase(), DockPhase::Expanded);
    assert_eq!(d.next_wake(ms(3900)), Some(ms(100)));
    d.tick(ms(4000));
    assert_eq!(d.phase(), DockPhase::Hidden);
    assert!(d.animating(ms(4100)));
    assert!((d.expansion(ms(4090)) - 0.5).abs() < 0.05);
    assert_eq!(d.expansion(ms(4180)), 0.0);
    assert_eq!(d.next_wake(ms(4200)), None, "no wakeups once hidden");
}

#[test]
fn hovering_the_tab_peeks_and_clicking_expands_until_the_pointer_leaves() {
    let mut d = controller();
    d.tick(ms(3000));
    assert_eq!(d.phase(), DockPhase::Hidden);
    d.pointer_enter(ms(5000));
    assert!(d.peeking(ms(5000)));
    d.click(ms(5100));
    assert_eq!(d.phase(), DockPhase::Expanded);
    assert!(d.expansion(ms(5100 + 180)) > 0.999);
    d.tick(ms(20_000));
    assert_eq!(d.phase(), DockPhase::Expanded, "stays while hovered");
    d.pointer_leave(ms(20_000));
    d.tick(ms(23_000));
    assert_eq!(d.phase(), DockPhase::Hidden);
}

#[test]
fn time_up_reveals_for_the_reveal_time_then_hides_unless_hovered() {
    let mut d = controller();
    d.tick(ms(3000));
    assert!(d.alert(ms(10_000), true));
    assert_eq!(d.phase(), DockPhase::Expanded);
    d.tick(ms(17_900));
    assert_eq!(d.phase(), DockPhase::Expanded);
    d.tick(ms(18_000));
    assert_eq!(d.phase(), DockPhase::Hidden);

    assert!(d.alert(ms(30_000), true));
    d.pointer_enter(ms(31_000));
    d.tick(ms(40_000));
    assert_eq!(d.phase(), DockPhase::Expanded, "hovering keeps it open");
}

#[test]
fn reveal_policy_decides_which_alerts_reveal() {
    let mut time_up = controller();
    time_up.tick(ms(3000));
    assert!(!time_up.alert(ms(4000), false), "reminders stay hidden by default");
    let mut all = DockController::new(Duration::from_secs(3), Duration::from_secs(8), DockReveal::AllAlerts, true);
    all.tick(ms(3000));
    assert!(all.alert(ms(4000), false));
    let mut never = DockController::new(Duration::from_secs(3), Duration::from_secs(8), DockReveal::Never, true);
    never.tick(ms(3000));
    assert!(!never.alert(ms(4000), true));
}

#[test]
fn pinning_keeps_it_open_and_unpinning_resumes_auto_hide() {
    let mut d = controller();
    d.set_autohide(ms(0), false);
    d.tick(ms(60_000));
    assert_eq!(d.phase(), DockPhase::Expanded);
    assert_eq!(d.next_wake(ms(60_000)), None);
    d.set_autohide(ms(60_000), true);
    d.tick(ms(63_000));
    assert_eq!(d.phase(), DockPhase::Hidden);
}

#[test]
fn fullscreen_apps_hide_the_overlay_and_its_tab_and_block_reveals() {
    let mut d = controller();
    d.set_fullscreen(ms(0), true);
    assert_eq!(d.phase(), DockPhase::Hidden);
    assert!(d.suppressed());
    assert!(!d.alert(ms(1000), true));
    d.set_fullscreen(ms(2000), false);
    assert!(!d.suppressed());

    let mut pinned = controller();
    pinned.set_autohide(ms(0), false);
    pinned.set_fullscreen(ms(10), true);
    assert_eq!(pinned.phase(), DockPhase::Hidden);
    pinned.set_fullscreen(ms(5000), false);
    assert_eq!(pinned.phase(), DockPhase::Expanded, "a pinned overlay comes back");
}
