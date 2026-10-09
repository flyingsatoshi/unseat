use crate::engine::Beep;
use crate::settings::Settings;
use crate::view::format_limit;
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

/// Sounds follow the sound toggle and volume. Notifications are shown only for the moments
/// that need attention (time is up, break is over), never for repeat reminders, so a long
/// overdue stretch does not pile up toasts.
pub fn alert_actions(beep: Beep, settings: &Settings, sitting: Duration) -> AlertActions {
    if beep == Beep::None {
        return AlertActions::NONE;
    }
    let play_sound = settings.sound_enabled && settings.alert_volume > 0;
    let notification = if !settings.notify_enabled {
        None
    } else {
        match beep {
            Beep::LimitReached => Some(Notice {
                title: "Time to stand up",
                body: format!(
                    "You've been sitting for {}. Take a break or snooze from the tray.",
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
    };
    AlertActions {
        play_sound,
        notification,
    }
}
