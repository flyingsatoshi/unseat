//! Which Unseat reminders go to a phone, what they say, and how long they stay useful.

use crate::engine::Beep;
use crate::settings::{PhoneAlerts, Settings};
use crate::view::format_limit;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReminderKind {
    /// The sitting limit was reached.
    TimeUp,
    /// A repeat reminder while overdue. On Windows these only play a sound; the phone gets
    /// them too (all reminders are mirrored).
    Repeat,
    BreakOver,
    /// Sent from the phone or PC page to check the connection.
    Test,
}

impl ReminderKind {
    /// Apple's `Topic` header. The Web Push spec says a newer push with the same topic
    /// replaces one still held for an offline phone, but in live testing (iOS 18.7, phone
    /// offline ~2 min) Apple delivered all three held pushes on a topic. So a phone that was
    /// offline can get a backlog; each notification carries the PC time it fired.
    pub fn topic(self) -> &'static str {
        match self {
            Self::TimeUp | Self::Repeat => "unseat-sitting",
            Self::BreakOver => "unseat-break",
            Self::Test => "unseat-test",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::TimeUp => "time_up",
            Self::Repeat => "repeat",
            Self::BreakOver => "break_over",
            Self::Test => "test",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub kind: ReminderKind,
    pub title: String,
    pub body: String,
    /// How long Apple should keep trying, and how long the PC keeps retrying. After this the
    /// reminder is stale and counts as missed.
    pub ttl_secs: u64,
}

pub const MIN_TTL_SECS: u64 = 60;
pub const SITTING_TTL_MAX_SECS: u64 = 15 * 60;
pub const BREAK_TTL_SECS: u64 = 10 * 60;
pub const TEST_TTL_SECS: u64 = 5 * 60;

/// The phone version of an engine alert. `None` only for `Beep::None`; sound and Windows
/// notification settings don't filter it (the phone has its own switch).
pub fn reminder_for(beep: Beep, settings: &Settings, sitting: Duration) -> Option<Reminder> {
    let sat = format_limit(Duration::from_secs(sitting.as_secs() / 60 * 60));
    let sitting_ttl = if settings.repeat_reminders {
        settings.repeat_every_secs.clamp(MIN_TTL_SECS, SITTING_TTL_MAX_SECS)
    } else {
        SITTING_TTL_MAX_SECS
    };
    let (kind, title, body, ttl_secs) = match beep {
        Beep::None => return None,
        Beep::LimitReached => (
            ReminderKind::TimeUp,
            "Time to stand up".to_string(),
            format!("You've been sitting for {sat}. Take a break or extend on your PC."),
            sitting_ttl,
        ),
        Beep::Progressive => (
            ReminderKind::Repeat,
            "Still sitting".to_string(),
            format!(
                "{sat} in your seat. A {} minute break resets the clock.",
                settings.break_duration_secs / 60
            ),
            sitting_ttl,
        ),
        Beep::BreakOver => (
            ReminderKind::BreakOver,
            "Break's over".to_string(),
            format!(
                "A new {} session has started.",
                format_limit(Duration::from_secs(settings.sitting_limit_secs))
            ),
            BREAK_TTL_SECS,
        ),
    };
    Some(Reminder { kind, title, body, ttl_secs })
}

pub fn test_reminder() -> Reminder {
    Reminder {
        kind: ReminderKind::Test,
        title: "Unseat test".to_string(),
        body: "Reminders from your PC will appear like this.".to_string(),
        ttl_secs: TEST_TTL_SECS,
    }
}

/// No input for this long (or the inactivity time, if shorter) counts as away from the PC.
/// Capped so idle-only still works with "Pause when idle", where the timer stops (and no
/// reminders fire) once the full inactivity time has passed.
pub const AWAY_AFTER_MAX_SECS: u64 = 120;

/// Whether the PC user is away: idle for a while, locked or asleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presence {
    pub idle_for: Duration,
    pub locked: bool,
}

/// Whether a reminder should go to the phone right now.
pub fn phone_should_alert(settings: &Settings, presence: Presence) -> bool {
    if !settings.phone_enabled {
        return false;
    }
    match settings.phone_alerts {
        PhoneAlerts::All => true,
        PhoneAlerts::IdleOnly => {
            presence.locked || presence.idle_for >= Duration::from_secs(settings.idle_after_secs.min(AWAY_AFTER_MAX_SECS))
        }
    }
}

/// The push body: a Declarative Web Push message. iOS 18.4+ shows it itself; older iOS hands
/// the same JSON to the app's service worker, which shows it. `mutable` asks iOS to let the
/// service worker see it too (so it can confirm delivery to the PC), but iOS 18.7 ignored it in
/// live testing: declarative pushes displayed and opened the app, yet never woke the worker.
/// The event number rides in the `navigate` fragment. `fired_at` (the PC's local time, e.g.
/// "14:05") ends the body, because Apple may deliver a held reminder late: in testing, a push
/// with a 60 s lifetime arrived about 25 s after it expired.
pub fn message(reminder: &Reminder, navigate: &str, event: u64, fired_at: &str) -> Vec<u8> {
    let body = if fired_at.is_empty() { reminder.body.clone() } else { format!("{} Sent {fired_at}.", reminder.body) };
    serde_json::json!({
        "web_push": 8030,
        "mutable": true,
        "notification": {
            "title": reminder.title,
            "body": body,
            "navigate": format!("{navigate}#e={event}"),
            "tag": reminder.kind.topic(),
            "silent": false,
        }
    })
    .to_string()
    .into_bytes()
}
