use crate::date::CivilDate;
use crate::settings::{InactivityBehavior, Settings, TimerCheckpoint};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub sitting_limit: Duration,
    pub break_duration: Duration,
    pub idle_after: Duration,
    pub inactivity_behavior: InactivityBehavior,
    pub sound_enabled: bool,
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
            sound_enabled: settings.sound_enabled,
            repeat_reminders: settings.repeat_reminders,
            repeat_every: Duration::from_secs(settings.repeat_every_secs),
        }
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibleState {
    Sitting,
    Overdue,
    Break,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beep {
    None,
    LimitReached,
    Progressive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub state: VisibleState,
    pub sitting_elapsed: Duration,
    pub timer_remaining: Duration,
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
    in_break: bool,
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
            in_break: false,
            last_now: None,
            limit_beeped: false,
            progressive_beeps_fired: 0,
            snooze_until: Duration::ZERO,
        }
    }

    pub fn set_config(&mut self, config: EngineConfig) {
        self.config = config;
        if config.inactivity_behavior == InactivityBehavior::Continue {
            self.in_break = false;
            self.break_elapsed = Duration::ZERO;
        }
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::TogglePause => self.running = !self.running,
            Command::Reset => {
                self.sitting_elapsed = Duration::ZERO;
                self.break_elapsed = Duration::ZERO;
                self.in_break = false;
                self.running = true;
                self.limit_beeped = false;
                self.progressive_beeps_fired = 0;
                self.snooze_until = Duration::ZERO;
            }
            Command::Snooze(extra) => {
                if self.can_snooze() {
                    self.snooze_until = self.sitting_elapsed.saturating_add(extra);
                    self.limit_beeped = false;
                    self.progressive_beeps_fired = 0;
                }
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

        if engine.running && config.inactivity_behavior == InactivityBehavior::Continue {
            let downtime = wall_now_secs.saturating_sub(checkpoint.saved_at_unix_secs);
            let downtime = Duration::from_secs(downtime);
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
        self.sitting_elapsed >= self.config.sitting_limit
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
            return TickResult {
                beep: Beep::None,
                snapshot: self.snapshot(),
            };
        }

        let inactive = input.session_locked || input.last_input_age >= self.config.idle_after;
        if self.config.inactivity_behavior == InactivityBehavior::Pause
            && inactive
            && self.sitting_elapsed >= self.effective_limit()
        {
            self.in_break = false;
            self.break_elapsed = Duration::ZERO;
            return TickResult {
                beep: Beep::None,
                snapshot: self.snapshot(),
            };
        }

        let away = self.config.inactivity_behavior == InactivityBehavior::Pause && inactive;
        let mut beep = Beep::None;

        if away {
            let mut break_dt = dt;
            if !self.in_break {
                let already_away = if input.session_locked {
                    Duration::ZERO
                } else {
                    input.last_input_age.saturating_sub(self.config.idle_after)
                };
                break_dt = already_away.min(dt);
                let active_dt = dt.saturating_sub(break_dt);
                if !active_dt.is_zero() {
                    let previous = self.sitting_elapsed;
                    self.sitting_elapsed += active_dt;
                    self.today_sitting += active_dt;
                    beep = self.beep_for(previous, self.sitting_elapsed);
                }
                if self.sitting_elapsed >= self.effective_limit() {
                    self.in_break = false;
                    self.break_elapsed = Duration::ZERO;
                    return TickResult {
                        beep,
                        snapshot: self.snapshot(),
                    };
                }
                self.in_break = true;
                self.break_elapsed = Duration::ZERO;
            }
            self.break_elapsed += break_dt;
            if self.break_elapsed >= self.config.break_duration {
                self.sitting_elapsed = Duration::ZERO;
                self.limit_beeped = false;
                self.progressive_beeps_fired = 0;
                self.snooze_until = Duration::ZERO;
            }
        } else {
            let mut active_dt = dt;
            if self.in_break {
                active_dt = input.last_input_age.min(dt);
                self.break_elapsed += dt.saturating_sub(active_dt);
                if self.break_elapsed >= self.config.break_duration {
                    self.sitting_elapsed = Duration::ZERO;
                    self.limit_beeped = false;
                    self.progressive_beeps_fired = 0;
                    self.snooze_until = Duration::ZERO;
                }
                self.in_break = false;
                self.break_elapsed = Duration::ZERO;
            }
            let previous = self.sitting_elapsed;
            self.sitting_elapsed += active_dt;
            self.today_sitting += active_dt;
            beep = self.beep_for(previous, self.sitting_elapsed);
        }

        TickResult {
            beep,
            snapshot: self.snapshot(),
        }
    }

    fn visible_state(&self) -> VisibleState {
        if !self.running {
            VisibleState::Paused
        } else if self.in_break {
            VisibleState::Break
        } else if self.sitting_elapsed >= self.effective_limit() {
            VisibleState::Overdue
        } else {
            VisibleState::Sitting
        }
    }

    fn beep_for(&mut self, previous: Duration, sitting: Duration) -> Beep {
        if !self.config.sound_enabled {
            return Beep::None;
        }
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
    if !checkpoint.running || inactivity_behavior != InactivityBehavior::Continue {
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
