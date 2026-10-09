use crate::date::CivilDate;
use crate::settings::{InactivityBehavior, Settings, TimerCheckpoint};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub sitting_limit: Duration,
    pub break_duration: Duration,
    pub idle_after: Duration,
    pub inactivity_behavior: InactivityBehavior,
    pub repeat_reminders: bool,
    pub repeat_every: Duration,
}

impl EngineConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            sitting_limit: Duration::from_secs(settings.sitting_limit_secs),
            break_duration: Duration::from_secs(settings.break_duration_secs),
            idle_after: Duration::from_secs(settings.idle_after_secs),
            inactivity_behavior: settings.inactivity_behavior,
            repeat_reminders: settings.repeat_reminders,
            repeat_every: Duration::from_secs(settings.repeat_every_secs),
        }
    }

    fn pauses_when_inactive(&self) -> bool {
        self.inactivity_behavior == InactivityBehavior::Pause
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub last_input_age: Duration,
    pub session_locked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    TogglePause,
    Reset,
    Snooze(Duration),
    /// Start an explicit break. It counts down `break_duration` of wall-clock time and then
    /// starts a fresh session. Reset ends a break early.
    StartBreak,
}

/// What the overlay shows. Each non-running reason has its own state so the user can tell
/// a manual pause, an inactivity pause, and a break apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibleState {
    Sitting,
    Overdue,
    /// Paused by the user.
    Paused,
    /// Paused automatically because "Pause when inactive" is on and the user is away.
    Away,
    /// An explicit break started by the user.
    Break,
}

/// Alert events. The engine reports them regardless of sound settings; the host decides
/// whether to play a sound or show a notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beep {
    None,
    LimitReached,
    Progressive,
    BreakOver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub state: VisibleState,
    pub sitting_elapsed: Duration,
    pub timer_remaining: Duration,
    /// Time spent in the current break or away period.
    pub break_elapsed: Duration,
    pub break_remaining: Duration,
    pub today_sitting: Duration,
    pub running: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickResult {
    pub beep: Beep,
    pub snapshot: Snapshot,
}

pub struct SittingEngine {
    config: EngineConfig,
    today: CivilDate,
    today_sitting: Duration,
    sitting_elapsed: Duration,
    break_elapsed: Duration,
    running: bool,
    away: bool,
    on_break: bool,
    last_now: Option<Duration>,
    limit_beeped: bool,
    progressive_beeps_fired: u32,
    snooze_until: Duration,
}

impl SittingEngine {
    pub fn new(config: EngineConfig, today: CivilDate, today_sitting: Duration) -> Self {
        Self {
            config,
            today,
            today_sitting,
            sitting_elapsed: Duration::ZERO,
            break_elapsed: Duration::ZERO,
            running: true,
            away: false,
            on_break: false,
            last_now: None,
            limit_beeped: false,
            progressive_beeps_fired: 0,
            snooze_until: Duration::ZERO,
        }
    }

    pub fn set_config(&mut self, config: EngineConfig) {
        self.config = config;
        if !config.pauses_when_inactive() && self.away {
            self.away = false;
            self.break_elapsed = Duration::ZERO;
        }
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::TogglePause => self.running = !self.running,
            Command::Reset => {
                self.start_fresh_session();
                self.running = true;
            }
            Command::Snooze(extra) => {
                if self.can_snooze() {
                    self.snooze_until = self.sitting_elapsed.saturating_add(extra);
                    self.limit_beeped = false;
                    self.progressive_beeps_fired = 0;
                }
            }
            Command::StartBreak => {
                self.away = false;
                self.on_break = true;
                self.break_elapsed = Duration::ZERO;
                self.running = true;
            }
        }
    }

    pub fn restore(
        config: EngineConfig,
        today: CivilDate,
        today_sitting: Duration,
        checkpoint: TimerCheckpoint,
        wall_now_secs: u64,
    ) -> Self {
        let mut engine = Self::new(config, today, today_sitting);
        engine.sitting_elapsed = Duration::from_secs(checkpoint.sitting_elapsed_secs);
        engine.snooze_until = Duration::from_secs(checkpoint.snooze_until_secs);
        engine.running = checkpoint.running;
        let downtime =
            Duration::from_secs(wall_now_secs.saturating_sub(checkpoint.saved_at_unix_secs));

        if checkpoint.on_break {
            // A break is wall-clock time: it keeps counting while the app is closed.
            engine.on_break = true;
            engine.break_elapsed = Duration::from_secs(checkpoint.break_elapsed_secs);
            if engine.running {
                engine.break_elapsed = engine.break_elapsed.saturating_add(downtime);
            }
            if engine.break_elapsed >= config.break_duration {
                engine.start_fresh_session();
            }
            return engine;
        }

        if engine.running && !config.pauses_when_inactive() {
            engine.sitting_elapsed = engine.sitting_elapsed.saturating_add(downtime);
        }

        let limit = engine.effective_limit();
        engine.limit_beeped = engine.sitting_elapsed >= limit;
        if engine.limit_beeped && engine.config.repeat_reminders {
            let over = engine.sitting_elapsed.saturating_sub(limit).as_secs();
            let every = engine.config.repeat_every.as_secs().max(1);
            engine.progressive_beeps_fired = (over / every) as u32;
        }
        engine
    }

    pub fn checkpoint(&self, wall_now_secs: u64) -> TimerCheckpoint {
        TimerCheckpoint {
            sitting_elapsed_secs: self.sitting_elapsed.as_secs(),
            snooze_until_secs: self.snooze_until.as_secs(),
            running: self.running,
            saved_at_unix_secs: wall_now_secs,
            on_break: self.on_break,
            break_elapsed_secs: if self.on_break {
                self.break_elapsed.as_secs()
            } else {
                0
            },
        }
    }

    pub fn apply_at(
        &mut self,
        cmd: Command,
        now: Duration,
        date: CivilDate,
        input: Input,
    ) -> TickResult {
        let result = self.tick(now, date, input);
        self.apply(cmd);
        TickResult {
            beep: result.beep,
            snapshot: self.snapshot(),
        }
    }

    pub fn can_snooze(&self) -> bool {
        !self.on_break && self.sitting_elapsed >= self.config.sitting_limit
    }

    pub fn on_break(&self) -> bool {
        self.on_break
    }

    fn effective_limit(&self) -> Duration {
        self.config.sitting_limit.max(self.snooze_until)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.visible_state(),
            sitting_elapsed: self.sitting_elapsed,
            timer_remaining: self.effective_limit().saturating_sub(self.sitting_elapsed),
            break_elapsed: self.break_elapsed,
            break_remaining: self
                .config
                .break_duration
                .saturating_sub(self.break_elapsed),
            today_sitting: self.today_sitting,
            running: self.running,
        }
    }

    pub fn tick(&mut self, now: Duration, date: CivilDate, input: Input) -> TickResult {
        if date != self.today {
            self.today = date;
            self.today_sitting = Duration::ZERO;
        }

        let dt = match self.last_now {
            Some(prev) if now >= prev => now - prev,
            _ => Duration::ZERO,
        };
        self.last_now = Some(now);

        if !self.running {
            return self.result(Beep::None);
        }

        if self.on_break {
            self.break_elapsed += dt;
            if self.break_elapsed < self.config.break_duration {
                return self.result(Beep::None);
            }
            // Time past the end of the break belongs to the new session.
            let overflow = self.break_elapsed - self.config.break_duration;
            self.start_fresh_session();
            self.add_sitting(overflow);
            return self.result(Beep::BreakOver);
        }

        let inactive = input.session_locked || input.last_input_age >= self.config.idle_after;
        let mut beep = Beep::None;

        if self.config.pauses_when_inactive() && inactive {
            let mut away_dt = dt;
            if !self.away {
                // Only the part of the gap after the idle threshold was spent away.
                let already_away = if input.session_locked {
                    Duration::ZERO
                } else {
                    input.last_input_age.saturating_sub(self.config.idle_after)
                };
                away_dt = already_away.min(dt);
                beep = self.add_sitting(dt - away_dt);
                self.away = true;
                self.break_elapsed = Duration::ZERO;
            }
            self.break_elapsed += away_dt;
            if self.break_elapsed >= self.config.break_duration {
                self.clear_session_progress();
            }
        } else {
            let mut active_dt = dt;
            if self.away {
                // Returning: only the time since the last input was active.
                active_dt = input.last_input_age.min(dt);
                self.break_elapsed += dt - active_dt;
                if self.break_elapsed >= self.config.break_duration {
                    self.clear_session_progress();
                }
                self.away = false;
                self.break_elapsed = Duration::ZERO;
            }
            let session_beep = self.add_sitting(active_dt);
            if session_beep != Beep::None {
                beep = session_beep;
            }
        }

        self.result(beep)
    }

    fn result(&self, beep: Beep) -> TickResult {
        TickResult {
            beep,
            snapshot: self.snapshot(),
        }
    }

    fn add_sitting(&mut self, d: Duration) -> Beep {
        if d.is_zero() {
            return Beep::None;
        }
        let previous = self.sitting_elapsed;
        self.sitting_elapsed += d;
        self.today_sitting += d;
        self.beep_for(previous, self.sitting_elapsed)
    }

    /// Zero the session counters but keep the current away state.
    fn clear_session_progress(&mut self) {
        self.sitting_elapsed = Duration::ZERO;
        self.limit_beeped = false;
        self.progressive_beeps_fired = 0;
        self.snooze_until = Duration::ZERO;
    }

    fn start_fresh_session(&mut self) {
        self.clear_session_progress();
        self.break_elapsed = Duration::ZERO;
        self.away = false;
        self.on_break = false;
    }

    fn visible_state(&self) -> VisibleState {
        if !self.running {
            VisibleState::Paused
        } else if self.on_break {
            VisibleState::Break
        } else if self.away {
            VisibleState::Away
        } else if self.sitting_elapsed >= self.effective_limit() {
            VisibleState::Overdue
        } else {
            VisibleState::Sitting
        }
    }

    fn beep_for(&mut self, previous: Duration, sitting: Duration) -> Beep {
        let limit = self.effective_limit();
        if previous < limit && sitting >= limit && !self.limit_beeped {
            self.limit_beeped = true;
            return Beep::LimitReached;
        }
        if !self.config.repeat_reminders || sitting < limit {
            return Beep::None;
        }
        let over = sitting.saturating_sub(limit).as_secs();
        let every = self.config.repeat_every.as_secs().max(1);
        let count = (over / every) as u32;
        if count > self.progressive_beeps_fired {
            self.progressive_beeps_fired = count;
            Beep::Progressive
        } else {
            Beep::None
        }
    }
}

/// Reconcile a monotonic clock without discarding time when update messages are delayed.
pub fn clamp_clock(last: Option<Duration>, now: Duration) -> Duration {
    match last {
        Some(prev) => now.max(prev),
        None => now,
    }
}

/// How long the host should wait before the next update so the face changes right on the
/// second boundary instead of drifting against a free-running 1 Hz timer.
pub fn next_tick_delay(snapshot: &Snapshot) -> Duration {
    const MARGIN_MS: u64 = 20;
    let elapsed = match snapshot.state {
        VisibleState::Sitting | VisibleState::Overdue => snapshot.sitting_elapsed,
        VisibleState::Break => snapshot.break_elapsed,
        VisibleState::Paused | VisibleState::Away => return Duration::from_secs(1),
    };
    let to_boundary = 1000 - u64::from(elapsed.subsec_millis());
    Duration::from_millis((to_boundary + MARGIN_MS).clamp(50, 1000 + MARGIN_MS))
}

pub fn restored_today_sitting(
    saved_today_sitting: Duration,
    saved_date: CivilDate,
    today: CivilDate,
    inactivity_behavior: InactivityBehavior,
    checkpoint: TimerCheckpoint,
    wall_now_secs: u64,
    since_local_midnight: Duration,
) -> Duration {
    let same_day = saved_date == today;
    let base = if same_day {
        saved_today_sitting
    } else {
        Duration::ZERO
    };
    if !checkpoint.running
        || checkpoint.on_break
        || inactivity_behavior != InactivityBehavior::Continue
    {
        return base;
    }

    let downtime = Duration::from_secs(wall_now_secs.saturating_sub(checkpoint.saved_at_unix_secs));
    let today_downtime = if same_day {
        downtime
    } else {
        downtime.min(since_local_midnight)
    };
    base.saturating_add(today_downtime)
}
