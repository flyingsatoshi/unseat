use crate::date::CivilDate;
use crate::settings::Settings;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub sitting_limit: Duration,
    pub break_duration: Duration,
    pub idle_after: Duration,
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
        }
    }

    pub fn set_config(&mut self, config: EngineConfig) {
        self.config = config;
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
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.visible_state(),
            sitting_elapsed: self.sitting_elapsed,
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

        let away = input.session_locked || input.last_input_age >= self.config.idle_after;
        let mut beep = Beep::None;

        if away {
            if !self.in_break {
                self.in_break = true;
                self.break_elapsed = Duration::ZERO;
            }
            self.break_elapsed += dt;
            if self.break_elapsed >= self.config.break_duration {
                self.sitting_elapsed = Duration::ZERO;
                self.limit_beeped = false;
                self.progressive_beeps_fired = 0;
            }
        } else {
            if self.in_break {
                self.in_break = false;
                self.break_elapsed = Duration::ZERO;
            }
            let previous = self.sitting_elapsed;
            self.sitting_elapsed += dt;
            self.today_sitting += dt;
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
        } else if self.sitting_elapsed >= self.config.sitting_limit {
            VisibleState::Overdue
        } else {
            VisibleState::Sitting
        }
    }

    fn beep_for(&mut self, previous: Duration, sitting: Duration) -> Beep {
        if !self.config.sound_enabled {
            return Beep::None;
        }
        let limit = self.config.sitting_limit;
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
