use crate::dock::{DockEdge, DockReveal};
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
pub const SETTINGS_VOLUME_MAX: u8 = 100;
pub const DEFAULT_ALERT_VOLUME: u8 = 75;
pub const HISTORY_DAYS: usize = 14;
/// Version 2 made "continue while inactive" the default. Files written before it stored the
/// old `pause` default without the user ever choosing it, so loading migrates them.
/// Version 3 added iPhone reminders, off until the user turns them on.
pub const SETTINGS_VERSION: u32 = 3;

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
    Pause,
    #[default]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::System => system_dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

/// Which Unseat reminders reach a paired iPhone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PhoneAlerts {
    /// Every reminder the PC gives: time up, repeat reminders (even sound-only ones) and
    /// break over.
    #[default]
    All,
    /// The same reminders, but only while the PC is idle, locked or asleep.
    IdleOnly,
}

impl PhoneAlerts {
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::IdleOnly => "idle_only",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "idle_only" => Self::IdleOnly,
            _ => Self::All,
        }
    }
}

impl serde::Serialize for PhoneAlerts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl<'de> serde::Deserialize<'de> for PhoneAlerts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Ok(Self::from_key(&key))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerCheckpoint {
    pub sitting_elapsed_secs: u64,
    pub snooze_until_secs: u64,
    pub running: bool,
    pub saved_at_unix_secs: u64,
    #[serde(default)]
    pub on_break: bool,
    #[serde(default)]
    pub break_elapsed_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayTotal {
    pub date: String,
    pub sitting_secs: u64,
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
    pub settings_version: u32,
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
    #[serde(default = "default_volume")]
    pub alert_volume: u8,
    #[serde(default = "default_true")]
    pub notify_enabled: bool,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default = "default_true")]
    pub always_on_top: bool,
    #[serde(default = "default_true")]
    pub hotkeys_enabled: bool,
    /// Docking is allowed: dragging the overlay to a screen edge attaches it there.
    #[serde(default)]
    pub dock_enabled: bool,
    /// The edge the overlay is attached to, or none while it floats.
    #[serde(default)]
    pub dock_edge: Option<DockEdge>,
    /// Position along the edge, 0 (start) to 1 (end).
    #[serde(default = "default_dock_offset")]
    pub dock_offset: f32,
    #[serde(default = "default_true")]
    pub dock_autohide: bool,
    #[serde(default = "default_dock_hide")]
    pub dock_hide_after_secs: u64,
    #[serde(default)]
    pub dock_reveal: DockReveal,
    #[serde(default = "default_dock_reveal")]
    pub dock_reveal_secs: u64,
    #[serde(default = "default_true")]
    pub dock_hide_fullscreen: bool,
    #[serde(default = "default_true")]
    pub stats_enabled: bool,
    #[serde(default)]
    pub history: Vec<DayTotal>,
    /// Send reminders to paired iPhones. Off until the user turns it on; while off, Unseat
    /// opens no socket and sends nothing.
    #[serde(default)]
    pub phone_enabled: bool,
    #[serde(default)]
    pub phone_alerts: PhoneAlerts,
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
fn default_dock_offset() -> f32 {
    0.5
}
fn default_dock_hide() -> u64 {
    3
}
fn default_dock_reveal() -> u64 {
    8
}
fn default_volume() -> u8 {
    DEFAULT_ALERT_VOLUME
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_version: SETTINGS_VERSION,
            timer_shape: TimerShape::Capsule,
            widget_size: WidgetSize::Small,
            timer_display_mode: TimerDisplayMode::Countdown,
            sitting_limit_secs: default_sitting_limit(),
            break_duration_secs: default_break(),
            idle_after_secs: default_idle(),
            inactivity_behavior: InactivityBehavior::Continue,
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
            alert_volume: DEFAULT_ALERT_VOLUME,
            notify_enabled: true,
            theme: ThemePreference::System,
            always_on_top: true,
            hotkeys_enabled: true,
            dock_enabled: false,
            dock_edge: None,
            dock_offset: default_dock_offset(),
            dock_autohide: true,
            dock_hide_after_secs: default_dock_hide(),
            dock_reveal: DockReveal::TimeUp,
            dock_reveal_secs: default_dock_reveal(),
            dock_hide_fullscreen: true,
            stats_enabled: true,
            history: Vec::new(),
            phone_enabled: false,
            phone_alerts: PhoneAlerts::All,
        }
    }
}

impl Settings {
    pub fn from_json(bytes: &[u8]) -> Self {
        let mut settings = serde_json::from_slice::<Settings>(bytes).unwrap_or_default();
        settings.migrate();
        settings.clamp();
        settings
    }

    fn migrate(&mut self) {
        if self.settings_version < 2 {
            self.inactivity_behavior = InactivityBehavior::Continue;
        }
        self.settings_version = self.settings_version.max(SETTINGS_VERSION);
    }

    /// User preferences reset to defaults; position, today's total, the running timer,
    /// history and the iPhone connection are kept.
    pub fn with_default_preferences(&self) -> Self {
        Self {
            phone_enabled: self.phone_enabled,
            phone_alerts: self.phone_alerts,
            window_x: self.window_x,
            window_y: self.window_y,
            dock_offset: self.dock_offset,
            today_date: self.today_date.clone(),
            today_sitting_secs: self.today_sitting_secs,
            timer_checkpoint: self.timer_checkpoint,
            history: self.history.clone(),
            ..Self::default()
        }
    }

    /// Record a finished day's sitting total, keeping the most recent `HISTORY_DAYS`.
    pub fn archive_day(&mut self, date: &str, sitting_secs: u64) {
        if !self.stats_enabled || date.is_empty() {
            return;
        }
        match self.history.iter_mut().find(|d| d.date == date) {
            Some(day) => day.sitting_secs = sitting_secs,
            None => self.history.push(DayTotal {
                date: date.to_string(),
                sitting_secs,
            }),
        }
        self.history.sort_by(|a, b| a.date.cmp(&b.date));
        let excess = self.history.len().saturating_sub(HISTORY_DAYS);
        self.history.drain(..excess);
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
        self.alert_volume = self.alert_volume.min(SETTINGS_VOLUME_MAX);
        self.dock_hide_after_secs = self.dock_hide_after_secs.clamp(1, 30);
        self.dock_reveal_secs = self.dock_reveal_secs.clamp(3, 60);
        self.dock_offset = if self.dock_offset.is_finite() {
            self.dock_offset.clamp(0.0, 1.0)
        } else {
            default_dock_offset()
        };
        if !self.dock_enabled {
            self.dock_edge = None;
        }
        if !self.stats_enabled {
            self.history.clear();
        }
        let excess = self.history.len().saturating_sub(HISTORY_DAYS);
        self.history.drain(..excess);
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
