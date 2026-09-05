use std::time::Duration;
use unseat::{
    Beep, CivilDate, Command, EngineConfig, Input, SittingEngine, VisibleState, clamp_clock,
};

fn cfg() -> EngineConfig {
    EngineConfig {
        sitting_limit: Duration::from_secs(3600),
        break_duration: Duration::from_secs(180),
        idle_after: Duration::from_secs(60),
        sound_enabled: true,
        repeat_reminders: true,
        repeat_every: Duration::from_secs(600),
    }
}

fn active() -> Input {
    Input {
        last_input_age: Duration::from_secs(0),
        session_locked: false,
    }
}

fn idle() -> Input {
    Input {
        last_input_age: Duration::from_secs(60),
        session_locked: false,
    }
}

fn locked() -> Input {
    Input {
        last_input_age: Duration::from_secs(0),
        session_locked: true,
    }
}

fn day() -> CivilDate {
    CivilDate {
        year: 2026,
        month: 9,
        day: 2,
    }
}

#[test]
fn launch_starts_sitting_at_zero() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    let r = e.tick(Duration::from_secs(0), day(), active());
    assert_eq!(r.snapshot.state, VisibleState::Sitting);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::ZERO);
    assert!(r.snapshot.running);
}

#[test]
fn sitting_accumulates_only_while_active_and_running() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::from_secs(0), day(), active());
    let r = e.tick(Duration::from_secs(10), day(), active());
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(10));
    assert_eq!(r.snapshot.today_sitting, Duration::from_secs(10));
}

#[test]
fn pause_freezes_sitting_and_today() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::from_secs(0), day(), active());
    e.tick(Duration::from_secs(10), day(), active());
    e.apply(Command::TogglePause);
    let r = e.tick(Duration::from_secs(40), day(), active());
    assert_eq!(r.snapshot.state, VisibleState::Paused);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(10));
    assert_eq!(r.snapshot.today_sitting, Duration::from_secs(10));
}

#[test]
fn reset_zeros_sitting_keeps_run_mode() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::from_secs(0), day(), active());
    e.tick(Duration::from_secs(10), day(), active());
    e.apply(Command::Reset);
    assert_eq!(e.snapshot().sitting_elapsed, Duration::ZERO);
    assert!(e.snapshot().running);
}

#[test]
fn reset_while_paused_starts_again_from_zero() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::from_secs(0), day(), active());
    e.tick(Duration::from_secs(10), day(), active());
    e.apply(Command::TogglePause);
    e.apply(Command::Reset);
    let r = e.tick(Duration::from_secs(15), day(), active());
    assert!(r.snapshot.running);
    assert_eq!(r.snapshot.state, VisibleState::Sitting);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(5));
}

#[test]
fn idle_threshold_starts_break_and_does_not_add_idle_to_sitting() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::from_secs(0), day(), active());
    e.tick(Duration::from_secs(50), day(), active());
    let r = e.tick(Duration::from_secs(50), day(), idle());
    assert_eq!(r.snapshot.state, VisibleState::Break);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(50));
}

#[test]
fn short_break_resumes_previous_sitting() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(50), day(), active());
    e.tick(Duration::from_secs(50), day(), idle());
    e.tick(Duration::from_secs(110), day(), idle());
    let r = e.tick(Duration::from_secs(110), day(), active());
    assert_eq!(r.snapshot.state, VisibleState::Sitting);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(50));
}

#[test]
fn qualifying_break_resets_sitting_and_stays_in_break_until_return() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(50), day(), active());
    e.tick(Duration::from_secs(50), day(), idle());
    let still_away = e.tick(Duration::from_secs(50 + 180), day(), idle());
    assert_eq!(still_away.snapshot.sitting_elapsed, Duration::ZERO);
    assert_eq!(still_away.snapshot.state, VisibleState::Break);
    let back = e.tick(Duration::from_secs(50 + 180), day(), active());
    assert_eq!(back.snapshot.state, VisibleState::Sitting);
    assert_eq!(back.snapshot.sitting_elapsed, Duration::ZERO);
}

#[test]
fn lock_enters_break_immediately() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_secs(5), day(), locked());
    assert_eq!(r.snapshot.state, VisibleState::Break);
}

#[test]
fn pause_ignores_idle_auto_reset() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(50), day(), active());
    e.apply(Command::TogglePause);
    e.tick(Duration::from_secs(50), day(), idle());
    e.tick(Duration::from_secs(50 + 180), day(), idle());
    e.tick(Duration::from_secs(50 + 180), day(), active());
    assert_eq!(e.snapshot().sitting_elapsed, Duration::from_secs(50));
    assert_eq!(e.snapshot().state, VisibleState::Paused);
}

#[test]
fn new_local_date_zeros_today_not_session() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::from_secs(100));
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(20), day(), active());
    let next = CivilDate {
        year: 2026,
        month: 9,
        day: 3,
    };
    let r = e.tick(Duration::from_secs(21), next, active());
    assert_eq!(r.snapshot.today_sitting, Duration::from_secs(1));
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(21));
}

#[test]
fn limit_reached_beeps_once_and_becomes_overdue() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3599), day(), active());
    let cross = e.tick(Duration::from_secs(3600), day(), active());
    assert_eq!(cross.snapshot.state, VisibleState::Overdue);
    assert_eq!(cross.beep, Beep::LimitReached);
    let later = e.tick(Duration::from_secs(3601), day(), active());
    assert_eq!(later.beep, Beep::None);
}

#[test]
fn progressive_beep_every_ten_minutes_while_overdue() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    let p = e.tick(Duration::from_secs(4200), day(), active());
    assert_eq!(p.beep, Beep::Progressive);
}

#[test]
fn no_beep_when_sound_disabled() {
    let mut c = cfg();
    c.sound_enabled = false;
    let mut e = SittingEngine::new(c, day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_secs(3600), day(), active());
    assert_eq!(r.beep, Beep::None);
    assert_eq!(r.snapshot.state, VisibleState::Overdue);
}

#[test]
fn no_progressive_when_repeat_disabled() {
    let mut c = cfg();
    c.repeat_reminders = false;
    let mut e = SittingEngine::new(c, day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    let r = e.tick(Duration::from_secs(4200), day(), active());
    assert_eq!(r.beep, Beep::None);
}

fn cfg_30_min() -> EngineConfig {
    let mut c = cfg();
    c.sitting_limit = Duration::from_secs(30 * 60);
    c
}

#[test]
fn stalled_tick_does_not_jump_sitting_on_a_30_min_session() {
    let mut e = SittingEngine::new(cfg_30_min(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(15 * 60 + 55), day(), active());
    let last = Duration::from_secs(15 * 60 + 55);
    let now = clamp_clock(Some(last), last + Duration::from_secs(20 * 60));
    let r = e.tick(now, day(), active());
    assert_eq!(
        r.snapshot.sitting_elapsed,
        Duration::from_secs(15 * 60 + 55 + 2)
    );
}

#[test]
fn stalled_tick_while_idle_does_not_instantly_reset_a_30_min_session() {
    let mut e = SittingEngine::new(cfg_30_min(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(15 * 60 + 55), day(), active());
    let last = Duration::from_secs(15 * 60 + 55);
    let now = clamp_clock(Some(last), last + Duration::from_secs(20 * 60));
    let r = e.tick(now, day(), idle());
    assert_eq!(
        r.snapshot.sitting_elapsed,
        Duration::from_secs(15 * 60 + 55)
    );
    assert_eq!(r.snapshot.state, VisibleState::Break);
    assert!(r.snapshot.break_elapsed <= Duration::from_secs(2));
}

#[test]
fn clamp_clock_caps_a_sleep_gap_and_passes_small_steps() {
    let last = Duration::from_secs(15 * 60 + 55);
    assert_eq!(
        clamp_clock(Some(last), last + Duration::from_secs(20 * 60)),
        last + Duration::from_secs(2)
    );
    assert_eq!(
        clamp_clock(Some(last), last + Duration::from_secs(1)),
        last + Duration::from_secs(1)
    );
    assert_eq!(clamp_clock(None, last), last);
}

#[test]
fn snooze_after_limit_returns_to_sitting_until_extra_time_elapses() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    assert_eq!(e.snapshot().state, VisibleState::Overdue);
    e.apply(Command::Snooze(Duration::from_secs(10 * 60)));
    assert_eq!(e.snapshot().state, VisibleState::Sitting);
    let still = e.tick(Duration::from_secs(3600 + 9 * 60), day(), active());
    assert_eq!(still.snapshot.state, VisibleState::Sitting);
    assert_eq!(still.beep, Beep::None);
    let again = e.tick(Duration::from_secs(3600 + 10 * 60), day(), active());
    assert_eq!(again.snapshot.state, VisibleState::Overdue);
    assert_eq!(again.beep, Beep::LimitReached);
}

#[test]
fn snooze_before_limit_is_a_noop() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(10), day(), active());
    e.apply(Command::Snooze(Duration::from_secs(10 * 60)));
    assert!(!e.can_snooze());
    assert_eq!(e.snapshot().state, VisibleState::Sitting);
    let r = e.tick(Duration::from_secs(3600), day(), active());
    assert_eq!(r.snapshot.state, VisibleState::Overdue);
    assert_eq!(r.beep, Beep::LimitReached);
}

#[test]
fn reset_clears_snooze() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    e.apply(Command::Snooze(Duration::from_secs(30 * 60)));
    e.apply(Command::Reset);
    e.tick(Duration::from_secs(3600), day(), active());
    let r = e.tick(Duration::from_secs(3600 + 1), day(), active());
    assert_eq!(r.snapshot.state, VisibleState::Sitting);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(1));
}

#[test]
fn qualifying_break_clears_snooze() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    e.apply(Command::Snooze(Duration::from_secs(20 * 60)));
    e.tick(Duration::from_secs(3600), day(), idle());
    e.tick(Duration::from_secs(3600 + 180), day(), idle());
    let back = e.tick(Duration::from_secs(3600 + 180), day(), active());
    assert_eq!(back.snapshot.state, VisibleState::Sitting);
    assert_eq!(back.snapshot.sitting_elapsed, Duration::ZERO);
}

#[test]
fn snooze_resets_progressive_beeps() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    e.tick(Duration::from_secs(4200), day(), active());
    e.apply(Command::Snooze(Duration::from_secs(10 * 60)));
    let mid = e.tick(Duration::from_secs(4200 + 9 * 60), day(), active());
    assert_eq!(mid.beep, Beep::None);
    let again = e.tick(Duration::from_secs(4200 + 10 * 60), day(), active());
    assert_eq!(again.beep, Beep::LimitReached);
}
