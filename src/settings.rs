use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub const SETTINGS_LIMIT_MIN_SECS: u64 = 5 * 60;
pub const SETTINGS_LIMIT_MAX_SECS: u64 = 240 * 60;
pub const SETTINGS_BREAK_MIN_SECS: u64 = 60;
pub const SETTINGS_BREAK_MAX_SECS: u64 = 30 * 60;
pub const SETTINGS_IDLE_MIN_SECS: u64 = 15;
pub const SETTINGS_IDLE_MAX_SECS: u64 = 600;
pub const SETTINGS_REPEAT_MIN_SECS: u64 = 2 * 60;
pub const SETTINGS_REPEAT_MAX_SECS: u64 = 60 * 60;
pub const SETTINGS_STEP_MIN: u64 = 1;
pub const SETTINGS_STEP_MAX: u64 = 30;
pub const SETTINGS_SNOOZE_MIN_SECS: u64 = 60;
pub const SETTINGS_SNOOZE_MAX_SECS: u64 = 60 * 60;
pub const SETTINGS_ALERT_DURATION_MAX_SECS: u64 = 10;
pub const SNOOZE_PRESETS_SECS: [u64; 4] = [5 * 60, 10 * 60, 15 * 60, 30 * 60];
pub const ALERT_DURATION_PRESETS_SECS: [u64; 4] = [0, 2, 4, 8];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerShape {
    #[default]
    Capsule,
    Card,
    Disc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerDisplayMode {
    #[default]
    Countdown,
    Stopwatch,
}

impl TimerDisplayMode {
    pub const ALL: [Self; 2] = [Self::Countdown, Self::Stopwatch];

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::Countdown => "Countdown",
            Self::Stopwatch => "Stopwatch",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InactivityBehavior {
    #[default]
    Pause,
    Continue,
}

impl InactivityBehavior {
    pub const ALL: [Self; 2] = [Self::Pause, Self::Continue];

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::Pause => "Pause",
            Self::Continue => "Continue",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerCheckpoint {
    pub sitting_elapsed_secs: u64,
    pub snooze_until_secs: u64,
    pub running: bool,
    pub saved_at_unix_secs: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetSize {
    Micro,
    #[default]
    Small,
    Medium,
    Large,
    ExtraLarge,
}

impl WidgetSize {
    pub const ALL: [Self; 5] = [
        Self::Micro,
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::ExtraLarge,
    ];

    pub fn factor(self) -> f32 {
        match self {
            Self::Micro => 0.75,
            Self::Small => 1.0,
            Self::Medium => 1.5,
            Self::Large => 2.0,
            Self::ExtraLarge => 2.5,
        }
    }

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::Micro => "XS",
            Self::Small => "S",
            Self::Medium => "M",
            Self::Large => "L",
            Self::ExtraLarge => "XL",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Micro => "Micro",
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::ExtraLarge => "Xtra Large",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(1)
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Self::Small)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlertSound {
    #[default]
    Chime,
    Bell,
    Pulse,
    Glass,
    Soft,
}

impl AlertSound {
    pub const ALL: [Self; 5] = [
        Self::Chime,
        Self::Bell,
        Self::Pulse,
        Self::Glass,
        Self::Soft,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Chime => "chime",
            Self::Bell => "bell",
            Self::Pulse => "pulse",
            Self::Glass => "glass",
            Self::Soft => "soft",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "bell" => Self::Bell,
            "pulse" => Self::Pulse,
            "glass" => Self::Glass,
            "soft" => Self::Soft,
            _ => Self::Chime,
        }
    }

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::Chime => "Chime",
            Self::Bell => "Bell",
            Self::Pulse => "Pulse",
            Self::Glass => "Glass",
            Self::Soft => "Soft",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Self::Chime)
    }
}

impl serde::Serialize for AlertSound {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl<'de> serde::Deserialize<'de> for AlertSound {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Ok(Self::from_key(&key))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub timer_shape: TimerShape,
    #[serde(default)]
    pub widget_size: WidgetSize,
    #[serde(default)]
    pub timer_display_mode: TimerDisplayMode,
    #[serde(default = "default_sitting_limit")]
    pub sitting_limit_secs: u64,
    #[serde(default = "default_break")]
    pub break_duration_secs: u64,
    #[serde(default = "default_idle")]
    pub idle_after_secs: u64,
    #[serde(default)]
    pub inactivity_behavior: InactivityBehavior,
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    #[serde(default = "default_true")]
    pub repeat_reminders: bool,
    #[serde(default = "default_repeat")]
    pub repeat_every_secs: u64,
    #[serde(default)]
    pub launch_with_windows: bool,
    #[serde(default = "default_window")]
    pub window_x: i32,
    #[serde(default = "default_window")]
    pub window_y: i32,
    #[serde(default)]
    pub today_date: String,
    #[serde(default)]
    pub today_sitting_secs: u64,
    #[serde(default = "default_step")]
    pub step: u64,
    #[serde(default)]
    pub alert_sound: AlertSound,
    #[serde(default)]
    pub alert_duration_secs: u64,
    #[serde(default = "default_snooze")]
    pub snooze_secs: u64,
    #[serde(default)]
    pub timer_checkpoint: Option<TimerCheckpoint>,
}

fn default_sitting_limit() -> u64 {
    3600
}
fn default_break() -> u64 {
    180
}
fn default_idle() -> u64 {
    60
}
fn default_repeat() -> u64 {
    600
}
fn default_true() -> bool {
    true
}
fn default_window() -> i32 {
    40
}
fn default_step() -> u64 {
    1
}
fn default_snooze() -> u64 {
    10 * 60
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            timer_shape: TimerShape::Capsule,
            widget_size: WidgetSize::Small,
            timer_display_mode: TimerDisplayMode::Countdown,
            sitting_limit_secs: default_sitting_limit(),
            break_duration_secs: default_break(),
            idle_after_secs: default_idle(),
            inactivity_behavior: InactivityBehavior::Pause,
            sound_enabled: true,
            repeat_reminders: true,
            repeat_every_secs: default_repeat(),
            launch_with_windows: false,
            window_x: 40,
            window_y: 40,
            today_date: String::new(),
            today_sitting_secs: 0,
            step: default_step(),
            alert_sound: AlertSound::Chime,
            alert_duration_secs: 0,
            snooze_secs: default_snooze(),
            timer_checkpoint: None,
        }
    }
}

impl Settings {
    pub fn from_json(bytes: &[u8]) -> Self {
        let mut settings = serde_json::from_slice::<Settings>(bytes).unwrap_or_default();
        settings.clamp();
        settings
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap_or_else(|_| b"{}".to_vec())
    }

    pub fn clamp(&mut self) {
        self.sitting_limit_secs = self
            .sitting_limit_secs
            .clamp(SETTINGS_LIMIT_MIN_SECS, SETTINGS_LIMIT_MAX_SECS);
        self.break_duration_secs = self
            .break_duration_secs
            .clamp(SETTINGS_BREAK_MIN_SECS, SETTINGS_BREAK_MAX_SECS);
        self.idle_after_secs = self
            .idle_after_secs
            .clamp(SETTINGS_IDLE_MIN_SECS, SETTINGS_IDLE_MAX_SECS);
        self.repeat_every_secs = self
            .repeat_every_secs
            .clamp(SETTINGS_REPEAT_MIN_SECS, SETTINGS_REPEAT_MAX_SECS);
        self.step = self.step.clamp(SETTINGS_STEP_MIN, SETTINGS_STEP_MAX);
        self.alert_duration_secs = self
            .alert_duration_secs
            .min(SETTINGS_ALERT_DURATION_MAX_SECS);
        self.snooze_secs = self
            .snooze_secs
            .clamp(SETTINGS_SNOOZE_MIN_SECS, SETTINGS_SNOOZE_MAX_SECS);
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read(path) {
            Ok(bytes) => Self::from_json(&bytes),
            Err(_) => Self::default(),
        }
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&self.to_json())?;
            file.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }
}
