use crate::engine::Beep;
use crate::settings::Settings;
use crate::view::{format_goal_parts, format_limit};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: &'static str,
    pub body: String,
}

/// What the host should do for one engine alert event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertActions {
    pub play_sound: bool,
    pub notification: Option<Notice>,
}

impl AlertActions {
    const NONE: Self = Self {
        play_sound: false,
        notification: None,
    };
}

/// Sounds follow the sound toggle and volume. Windows notifications follow their toggle and
/// are only for the moments that need attention (time is up, break is over), never for
/// repeat reminders, so a long overdue stretch does not pile up toasts. The host uses them
/// only while the overlay is hidden; beside a visible overlay a card speaks instead.
pub fn alert_actions(beep: Beep, settings: &Settings, sitting: Duration) -> AlertActions {
    if beep == Beep::None {
        return AlertActions::NONE;
    }
    AlertActions {
        play_sound: settings.sound_enabled && settings.alert_volume > 0,
        notification: settings
            .notify_enabled
            .then(|| alert_notice(beep, settings, sitting))
            .flatten(),
    }
}

/// The words for an alert, whatever the notification toggle says: the cards beside the
/// overlay use them too.
pub fn alert_notice(beep: Beep, settings: &Settings, sitting: Duration) -> Option<Notice> {
    match beep {
        Beep::LimitReached => Some(Notice {
            title: "Time to stand up",
            body: format!(
                "You've been sitting for {}. Click to take a break or extend.",
                format_limit(Duration::from_secs(sitting.as_secs() / 60 * 60))
            ),
        }),
        Beep::BreakOver => Some(Notice {
            title: "Break's over",
            body: format!(
                "A new {} session has started.",
                format_limit(Duration::from_secs(settings.sitting_limit_secs))
            ),
        }),
        Beep::Progressive | Beep::None => None,
    }
}

/// A one-line caption under the overlay, with a shorter form for when the full one would
/// be wider than a small pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caption {
    pub full: String,
    pub short: String,
}

/// The quiet question under the overlay when time is up: two small tabs (take a break, or
/// one more full session) and a caption. Labels carry the lengths from Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeUpPrompt {
    pub caption: Caption,
    pub break_label: String,
    pub extend_label: String,
}

pub fn time_up_prompt(settings: &Settings, sitting: Duration) -> TimeUpPrompt {
    let sat = format_limit(Duration::from_secs(sitting.as_secs() / 60 * 60));
    TimeUpPrompt {
        caption: Caption {
            full: format!("{sat} sitting · time to stand"),
            short: format!("{sat} sitting"),
        },
        break_label: format!("Break {}", short_minutes(settings.break_duration_secs)),
        extend_label: format!("+{}", short_minutes(settings.sitting_limit_secs)),
    }
}

/// The caption shown for a few seconds when a break ends.
pub fn break_over_note(settings: &Settings) -> Caption {
    Caption {
        full: format!(
            "Break's over · new {} session",
            format_limit(Duration::from_secs(settings.sitting_limit_secs))
        ),
        short: "Break's over".to_string(),
    }
}

/// "10m", "40m", "90m", "2h": the overlay's short form.
fn short_minutes(secs: u64) -> String {
    let (n, unit) = format_goal_parts(Duration::from_secs(secs / 60 * 60));
    format!("{n}{}", if unit == "hr" { "h" } else { "m" })
}
