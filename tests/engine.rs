use std::time::Duration;
use unseat::{
    clamp_clock, next_tick_delay, restored_today_sitting, Beep, CivilDate, Command, EngineConfig,
    InactivityBehavior, Input, Settings, SittingEngine, TimerCheckpoint, VisibleState,
};

/// "Pause when inactive" turned on. Most legacy tests exercise that opt-in mode.
fn cfg() -> EngineConfig {
    EngineConfig {
        sitting_limit: Duration::from_secs(3600),
        break_duration: Duration::from_secs(180),
        idle_after: Duration::from_secs(60),
        inactivity_behavior: InactivityBehavior::Pause,
        repeat_reminders: true,
        repeat_every: Duration::from_secs(600),
    }
}

#[test]
fn continue_mode_counts_a_delayed_idle_gap_as_running_time() {
    let mut c = cfg();
    c.inactivity_behavior = InactivityBehavior::Continue;
    let mut e = SittingEngine::new(c, day(), Duration::ZERO);

    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_secs(20 * 60), day(), idle());

    assert_eq!(r.snapshot.state, VisibleState::Sitting);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(20 * 60));
    assert_eq!(r.snapshot.break_elapsed, Duration::ZERO);
}

#[test]
fn switching_to_continue_mode_leaves_away_state_without_resetting_progress() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(10), day(), active());
    e.tick(Duration::from_secs(10), day(), idle());
    assert_eq!(e.snapshot().state, VisibleState::Away);

    let mut c = cfg();
    c.inactivity_behavior = InactivityBehavior::Continue;
    e.set_config(c);

    assert_eq!(e.snapshot().state, VisibleState::Sitting);
    assert_eq!(e.snapshot().sitting_elapsed, Duration::from_secs(10));
}

#[test]
fn pause_mode_splits_a_delayed_tick_at_the_idle_threshold() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);

    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(
        Duration::from_secs(100),
        day(),
        Input {
            last_input_age: Duration::from_secs(100),
            session_locked: false,
        },
    );

    assert_eq!(r.snapshot.state, VisibleState::Away);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(60));
    assert_eq!(r.snapshot.break_elapsed, Duration::from_secs(40));
}

#[test]
fn delayed_return_from_a_break_only_counts_time_since_last_input() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);

    e.tick(Duration::ZERO, day(), active());
    e.tick(
        Duration::from_secs(100),
        day(),
        Input {
            last_input_age: Duration::from_secs(100),
            session_locked: false,
        },
    );
    let r = e.tick(
        Duration::from_secs(600),
        day(),
        Input {
            last_input_age: Duration::from_secs(5),
            session_locked: false,
        },
    );

    assert_eq!(r.snapshot.state, VisibleState::Sitting);
    assert_eq!(r.snapshot.sitting_elapsed, Duration::from_secs(5));
    assert_eq!(r.snapshot.today_sitting, Duration::from_secs(65));
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
fn idle_threshold_pauses_as_away_and_does_not_add_idle_to_sitting() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::from_secs(0), day(), active());
    e.tick(Duration::from_secs(50), day(), active());
    let r = e.tick(Duration::from_secs(50), day(), idle());
    assert_eq!(r.snapshot.state, VisibleState::Away);
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
fn away_for_the_break_length_resets_sitting_and_stays_away_until_return() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(50), day(), active());
    e.tick(Duration::from_secs(50), day(), idle());
    let still_away = e.tick(Duration::from_secs(50 + 180), day(), idle());
    assert_eq!(still_away.snapshot.sitting_elapsed, Duration::ZERO);
    assert_eq!(still_away.snapshot.state, VisibleState::Away);
    let back = e.tick(Duration::from_secs(50 + 180), day(), active());
    assert_eq!(back.snapshot.state, VisibleState::Sitting);
    assert_eq!(back.snapshot.sitting_elapsed, Duration::ZERO);
}

#[test]
fn lock_pauses_as_away_immediately() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_secs(5), day(), locked());
    assert_eq!(r.snapshot.state, VisibleState::Away);
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
fn engine_reports_alerts_even_when_sound_is_off_so_notifications_still_work() {
    let mut settings = Settings::default();
    settings.sound_enabled = false;
    let mut e = SittingEngine::new(EngineConfig::from_settings(&settings), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_secs(3600), day(), active());
    assert_eq!(r.beep, Beep::LimitReached);
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
fn delayed_active_tick_accounts_for_the_full_elapsed_gap() {
    let mut e = SittingEngine::new(cfg_30_min(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(15 * 60 + 55), day(), active());
    let last = Duration::from_secs(15 * 60 + 55);
    let now = clamp_clock(Some(last), last + Duration::from_secs(20 * 60));
    let r = e.tick(now, day(), active());
    assert_eq!(
        r.snapshot.sitting_elapsed,
        Duration::from_secs(35 * 60 + 55)
    );
    assert_eq!(r.snapshot.state, VisibleState::Overdue);
}

#[test]
fn delayed_tick_at_the_idle_threshold_counts_the_prior_gap_then_pauses_as_away() {
    let mut e = SittingEngine::new(cfg_30_min(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(15 * 60 + 55), day(), active());
    let last = Duration::from_secs(15 * 60 + 55);
    let now = clamp_clock(Some(last), last + Duration::from_secs(20 * 60));
    let r = e.tick(now, day(), idle());
    assert_eq!(
        r.snapshot.sitting_elapsed,
        Duration::from_secs(35 * 60 + 55)
    );
    assert_eq!(r.beep, Beep::LimitReached);
    assert_eq!(r.snapshot.timer_remaining, Duration::ZERO);
    assert_eq!(r.snapshot.state, VisibleState::Away);
    assert_eq!(r.snapshot.break_elapsed, Duration::ZERO);
}

#[test]
fn pause_mode_excludes_a_full_suspend_gap() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(10), day(), active());
    e.tick(Duration::from_secs(10), day(), locked());
    e.tick(Duration::from_secs(10 * 60), day(), locked());
    let resumed = e.tick(Duration::from_secs(10 * 60), day(), active());

    assert_eq!(resumed.snapshot.state, VisibleState::Sitting);
    assert_eq!(resumed.snapshot.sitting_elapsed, Duration::ZERO);
}

#[test]
fn continue_mode_counts_a_full_suspend_gap() {
    let mut c = cfg();
    c.inactivity_behavior = InactivityBehavior::Continue;
    let mut e = SittingEngine::new(c, day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let resumed = e.tick(Duration::from_secs(10 * 60), day(), locked());

    assert_eq!(resumed.snapshot.state, VisibleState::Sitting);
    assert_eq!(
        resumed.snapshot.sitting_elapsed,
        Duration::from_secs(10 * 60)
    );
}

#[test]
fn completion_clamps_countdown_remaining_to_zero() {
    let mut c = cfg();
    c.sitting_limit = Duration::from_secs(10);
    let mut e = SittingEngine::new(c, day(), Duration::ZERO);

    let start = e.tick(Duration::ZERO, day(), active());
    assert_eq!(start.snapshot.timer_remaining, Duration::from_secs(10));

    let completed = e.tick(Duration::from_secs(15), day(), active());
    assert_eq!(completed.snapshot.state, VisibleState::Overdue);
    assert_eq!(completed.snapshot.timer_remaining, Duration::ZERO);
}

#[test]
fn completed_timer_stays_overdue_through_a_short_absence() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());

    let idle_result = e.tick(Duration::from_secs(3660), day(), idle());
    assert_eq!(idle_result.snapshot.timer_remaining, Duration::ZERO);
    assert_eq!(idle_result.snapshot.state, VisibleState::Away);

    let return_result = e.tick(Duration::from_secs(3661), day(), active());
    assert_eq!(return_result.snapshot.timer_remaining, Duration::ZERO);
    assert_eq!(return_result.snapshot.state, VisibleState::Overdue);
}

#[test]
fn away_for_the_break_length_while_overdue_counts_as_a_break() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    e.tick(Duration::from_secs(3600), day(), idle());
    e.tick(Duration::from_secs(3600 + 180), day(), idle());
    let back = e.tick(
        Duration::from_secs(3600 + 181),
        day(),
        Input {
            last_input_age: Duration::from_secs(1),
            session_locked: false,
        },
    );
    assert_eq!(back.snapshot.state, VisibleState::Sitting);
    assert_eq!(back.snapshot.sitting_elapsed, Duration::from_secs(1));
}

#[test]
fn delayed_idle_update_that_crosses_zero_still_reports_completion() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3590), day(), active());

    let result = e.tick(
        Duration::from_secs(3650),
        day(),
        Input {
            last_input_age: Duration::from_secs(50),
            session_locked: false,
        },
    );

    assert_eq!(result.beep, Beep::LimitReached);
    assert_eq!(result.snapshot.timer_remaining, Duration::ZERO);
    assert_eq!(result.snapshot.state, VisibleState::Overdue);
    assert_eq!(result.snapshot.sitting_elapsed, Duration::from_secs(3650));
}

#[test]
fn continue_mode_restores_from_a_wall_clock_checkpoint() {
    let mut c = cfg();
    c.inactivity_behavior = InactivityBehavior::Continue;
    let mut e = SittingEngine::new(c, day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(120), day(), active());
    let checkpoint = e.checkpoint(1_000);

    let restored = SittingEngine::restore(c, day(), Duration::from_secs(120), checkpoint, 1_600);

    assert_eq!(
        restored.snapshot().sitting_elapsed,
        Duration::from_secs(720)
    );
    assert_eq!(
        restored.snapshot().timer_remaining,
        Duration::from_secs(2_880)
    );
    assert_eq!(restored.snapshot().today_sitting, Duration::from_secs(120));
}

#[test]
fn pause_mode_restores_without_counting_time_while_the_app_was_closed() {
    let checkpoint = TimerCheckpoint {
        sitting_elapsed_secs: 120,
        snooze_until_secs: 0,
        running: true,
        saved_at_unix_secs: 1_000,
        on_break: false,
        break_elapsed_secs: 0,
    };

    let restored =
        SittingEngine::restore(cfg(), day(), Duration::from_secs(120), checkpoint, 1_600);

    assert_eq!(
        restored.snapshot().sitting_elapsed,
        Duration::from_secs(120)
    );
    assert_eq!(
        restored.snapshot().timer_remaining,
        Duration::from_secs(3_480)
    );
}

#[test]
fn relaunch_reconstructs_only_the_current_days_continuing_time() {
    let checkpoint = TimerCheckpoint {
        sitting_elapsed_secs: 120,
        snooze_until_secs: 0,
        running: true,
        saved_at_unix_secs: 1_000,
        on_break: false,
        break_elapsed_secs: 0,
    };
    let next_day = CivilDate {
        year: 2026,
        month: 9,
        day: 3,
    };

    assert_eq!(
        restored_today_sitting(
            Duration::from_secs(120),
            day(),
            day(),
            InactivityBehavior::Continue,
            checkpoint,
            1_600,
            Duration::from_secs(10 * 60),
        ),
        Duration::from_secs(720)
    );
    assert_eq!(
        restored_today_sitting(
            Duration::from_secs(9_999),
            day(),
            next_day,
            InactivityBehavior::Continue,
            checkpoint,
            8_200,
            Duration::from_secs(60 * 60),
        ),
        Duration::from_secs(60 * 60)
    );
}

#[test]
fn timestamped_pause_and_resume_exclude_the_entire_paused_gap() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());

    let paused = e.apply_at(
        Command::TogglePause,
        Duration::from_secs(10),
        day(),
        active(),
    );
    assert_eq!(paused.snapshot.sitting_elapsed, Duration::from_secs(10));
    assert_eq!(paused.snapshot.state, VisibleState::Paused);

    let resumed = e.apply_at(
        Command::TogglePause,
        Duration::from_secs(10 * 60),
        day(),
        active(),
    );
    assert_eq!(resumed.snapshot.sitting_elapsed, Duration::from_secs(10));
    assert_eq!(resumed.snapshot.state, VisibleState::Sitting);

    let next = e.tick(Duration::from_secs(10 * 60 + 1), day(), active());
    assert_eq!(next.snapshot.sitting_elapsed, Duration::from_secs(11));
}

#[test]
fn clock_reconciliation_preserves_delayed_time_and_rejects_regression() {
    let last = Duration::from_secs(15 * 60 + 55);
    assert_eq!(
        clamp_clock(Some(last), last + Duration::from_secs(20 * 60)),
        last + Duration::from_secs(20 * 60)
    );
    assert_eq!(
        clamp_clock(Some(last), last + Duration::from_secs(1)),
        last + Duration::from_secs(1)
    );
    assert_eq!(clamp_clock(Some(last), last - Duration::from_secs(5)), last);
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

fn default_cfg() -> EngineConfig {
    EngineConfig::from_settings(&Settings::default())
}

fn long_idle() -> Input {
    Input {
        last_input_age: Duration::from_secs(45 * 60),
        session_locked: false,
    }
}

#[test]
fn default_settings_keep_counting_through_idle_lock_and_sleep() {
    assert_eq!(
        default_cfg().inactivity_behavior,
        InactivityBehavior::Continue
    );
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let idle_r = e.tick(Duration::from_secs(10 * 60), day(), long_idle());
    assert_eq!(idle_r.snapshot.state, VisibleState::Sitting);
    let locked_r = e.tick(Duration::from_secs(20 * 60), day(), locked());
    assert_eq!(locked_r.snapshot.state, VisibleState::Sitting);
    // A 25 minute suspend arrives as one large monotonic gap.
    let resumed = e.tick(Duration::from_secs(45 * 60), day(), active());
    assert_eq!(resumed.snapshot.state, VisibleState::Sitting);
    assert_eq!(resumed.snapshot.sitting_elapsed, Duration::from_secs(45 * 60));
    assert_eq!(resumed.snapshot.today_sitting, Duration::from_secs(45 * 60));
}

#[test]
fn default_settings_reach_the_limit_and_keep_reminding_while_unattended() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let done = e.tick(Duration::from_secs(3600), day(), long_idle());
    assert_eq!(done.beep, Beep::LimitReached);
    assert_eq!(done.snapshot.state, VisibleState::Overdue);
    let reminder = e.tick(Duration::from_secs(4200), day(), locked());
    assert_eq!(reminder.beep, Beep::Progressive);
    let quiet = e.tick(Duration::from_secs(4201), day(), locked());
    assert_eq!(quiet.beep, Beep::None);
}

#[test]
fn a_delayed_reminder_tick_fires_one_reminder_not_a_burst() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    // Three reminder intervals pass in one message (e.g. after resume from sleep).
    let r = e.tick(Duration::from_secs(3600 + 3 * 600 + 5), day(), active());
    assert_eq!(r.beep, Beep::Progressive);
    let next = e.tick(Duration::from_secs(3600 + 3 * 600 + 6), day(), active());
    assert_eq!(next.beep, Beep::None);
}

#[test]
fn manual_pause_inactivity_pause_and_break_are_distinct_states() {
    let mut paused = SittingEngine::new(cfg(), day(), Duration::ZERO);
    paused.apply(Command::TogglePause);
    assert_eq!(paused.snapshot().state, VisibleState::Paused);

    let mut away = SittingEngine::new(cfg(), day(), Duration::ZERO);
    away.tick(Duration::ZERO, day(), active());
    away.tick(Duration::from_secs(1), day(), locked());
    assert_eq!(away.snapshot().state, VisibleState::Away);

    let mut on_break = SittingEngine::new(cfg(), day(), Duration::ZERO);
    on_break.apply(Command::StartBreak);
    assert_eq!(on_break.snapshot().state, VisibleState::Break);
}

#[test]
fn break_counts_down_then_starts_a_fresh_session() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::from_secs(100));
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3700), day(), active());
    e.apply(Command::StartBreak);
    let mid = e.tick(Duration::from_secs(3700 + 60), day(), active());
    assert_eq!(mid.snapshot.state, VisibleState::Break);
    assert_eq!(mid.snapshot.break_remaining, Duration::from_secs(120));
    assert_eq!(mid.snapshot.today_sitting, Duration::from_secs(3800));
    assert_eq!(mid.beep, Beep::None);

    let over = e.tick(Duration::from_secs(3700 + 185), day(), active());
    assert_eq!(over.beep, Beep::BreakOver);
    assert_eq!(over.snapshot.state, VisibleState::Sitting);
    assert_eq!(over.snapshot.sitting_elapsed, Duration::from_secs(5));
    assert_eq!(over.snapshot.today_sitting, Duration::from_secs(3805));
}

#[test]
fn break_ignores_inactivity_even_when_pause_when_inactive_is_on() {
    let mut e = SittingEngine::new(cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.apply(Command::StartBreak);
    let r = e.tick(Duration::from_secs(90), day(), locked());
    assert_eq!(r.snapshot.state, VisibleState::Break);
    assert_eq!(r.snapshot.break_elapsed, Duration::from_secs(90));
}

#[test]
fn pausing_a_break_freezes_its_countdown() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.apply(Command::StartBreak);
    e.apply_at(Command::TogglePause, Duration::from_secs(30), day(), active());
    e.apply_at(Command::TogglePause, Duration::from_secs(300), day(), active());
    let r = e.tick(Duration::from_secs(301), day(), active());
    assert_eq!(r.snapshot.state, VisibleState::Break);
    assert_eq!(r.snapshot.break_elapsed, Duration::from_secs(31));
}

#[test]
fn reset_ends_a_break_early_and_snooze_is_unavailable_during_one() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(3600), day(), active());
    e.apply(Command::StartBreak);
    assert!(!e.can_snooze());
    e.apply(Command::Snooze(Duration::from_secs(600)));
    assert_eq!(e.snapshot().state, VisibleState::Break);
    e.apply(Command::Reset);
    assert_eq!(e.snapshot().state, VisibleState::Sitting);
    assert_eq!(e.snapshot().sitting_elapsed, Duration::ZERO);
}

#[test]
fn starting_a_break_while_paused_runs_the_break() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.apply(Command::TogglePause);
    e.apply(Command::StartBreak);
    assert_eq!(e.snapshot().state, VisibleState::Break);
    assert!(e.snapshot().running);
}

#[test]
fn a_break_survives_restart_and_counts_the_time_the_app_was_closed() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.apply(Command::StartBreak);
    e.tick(Duration::from_secs(30), day(), active());
    let cp = e.checkpoint(5_000);
    assert!(cp.on_break);
    assert_eq!(cp.break_elapsed_secs, 30);

    let resumed = SittingEngine::restore(default_cfg(), day(), Duration::ZERO, cp, 5_060);
    assert_eq!(resumed.snapshot().state, VisibleState::Break);
    assert_eq!(resumed.snapshot().break_elapsed, Duration::from_secs(90));

    let finished = SittingEngine::restore(default_cfg(), day(), Duration::ZERO, cp, 5_600);
    assert_eq!(finished.snapshot().state, VisibleState::Sitting);
    assert_eq!(finished.snapshot().sitting_elapsed, Duration::ZERO);
    assert_eq!(
        restored_today_sitting(
            Duration::from_secs(10),
            day(),
            day(),
            InactivityBehavior::Continue,
            cp,
            5_600,
            Duration::from_secs(3600),
        ),
        Duration::from_secs(10)
    );
}

#[test]
fn restart_restores_a_manual_pause_without_counting_closed_time() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    e.tick(Duration::from_secs(300), day(), active());
    e.apply(Command::TogglePause);
    let cp = e.checkpoint(1_000);
    let restored = SittingEngine::restore(default_cfg(), day(), Duration::ZERO, cp, 9_000);
    assert_eq!(restored.snapshot().state, VisibleState::Paused);
    assert_eq!(restored.snapshot().sitting_elapsed, Duration::from_secs(300));
}

#[test]
fn restart_into_overdue_does_not_replay_missed_alerts() {
    let cp = TimerCheckpoint {
        sitting_elapsed_secs: 3000,
        snooze_until_secs: 0,
        running: true,
        saved_at_unix_secs: 1_000,
        on_break: false,
        break_elapsed_secs: 0,
    };
    let mut e = SittingEngine::restore(default_cfg(), day(), Duration::ZERO, cp, 1_000 + 2_000);
    assert_eq!(e.snapshot().state, VisibleState::Overdue);
    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_secs(1), day(), active());
    assert_eq!(r.beep, Beep::None);
}

#[test]
fn older_checkpoints_without_break_fields_still_load() {
    let json = br#"{"timer_checkpoint":{"sitting_elapsed_secs":5,"snooze_until_secs":0,"running":true,"saved_at_unix_secs":9}}"#;
    let cp = Settings::from_json(json).timer_checkpoint.unwrap();
    assert!(!cp.on_break);
    assert_eq!(cp.break_elapsed_secs, 0);
}

#[test]
fn backwards_clock_jump_never_subtracts_time() {
    let cp = TimerCheckpoint {
        sitting_elapsed_secs: 100,
        snooze_until_secs: 0,
        running: true,
        saved_at_unix_secs: 10_000,
        on_break: false,
        break_elapsed_secs: 0,
    };
    let e = SittingEngine::restore(default_cfg(), day(), Duration::ZERO, cp, 4_000);
    assert_eq!(e.snapshot().sitting_elapsed, Duration::from_secs(100));
}

#[test]
fn next_tick_lands_just_after_the_next_second_boundary() {
    let mut e = SittingEngine::new(default_cfg(), day(), Duration::ZERO);
    e.tick(Duration::ZERO, day(), active());
    let r = e.tick(Duration::from_millis(12_340), day(), active());
    assert_eq!(next_tick_delay(&r.snapshot), Duration::from_millis(680));

    let on_boundary = e.tick(Duration::from_millis(13_000), day(), active());
    assert_eq!(
        next_tick_delay(&on_boundary.snapshot),
        Duration::from_millis(1020)
    );

    e.apply(Command::TogglePause);
    assert_eq!(next_tick_delay(&e.snapshot()), Duration::from_secs(1));
}
