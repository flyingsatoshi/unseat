use crate::engine::VisibleState;
use crate::settings::{TimerShape, WidgetSize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Pause,
    Reset,
}

pub const WIDGET_W: f32 = 220.0;
pub const WIDGET_H: f32 = 88.0;

/// Logical layout at 96 DPI for Small. Other sizes multiply `scale`.
pub fn widget_pixel_size(_shape: TimerShape, scale: f32) -> (i32, i32) {
    (
        (WIDGET_W * scale).round() as i32,
        (WIDGET_H * scale).round() as i32,
    )
}

pub fn layout_scale(dpi_scale: f32, size: WidgetSize) -> f32 {
    dpi_scale * size.factor()
}

pub fn play_center(scale: f32) -> (f32, f32, f32) {
    let s = scale;
    (82.0 * s, 44.0 * s, 28.0 * s)
}

/// Hover chips stacked in a right-edge tag: pause on top, reset below.
pub fn control_centers(_shape: TimerShape, scale: f32) -> ((f32, f32), (f32, f32), f32) {
    let s = scale;
    ((194.0 * s, 30.0 * s), (194.0 * s, 58.0 * s), 11.0 * s)
}

pub fn hit_control(
    shape: TimerShape,
    hover: bool,
    paused: bool,
    scale: f32,
    x: i32,
    y: i32,
) -> Option<Hit> {
    let xf = x as f32;
    let yf = y as f32;
    if paused {
        let (cx, cy, r) = play_center(scale);
        if (xf - cx).hypot(yf - cy) <= r {
            return Some(Hit::Pause);
        }
    }
    if !hover {
        return None;
    }
    let ((px, py), (rx, ry), r) = control_centers(shape, scale);
    if !paused && (xf - px).hypot(yf - py) <= r {
        Some(Hit::Pause)
    } else if (xf - rx).hypot(yf - ry) <= r {
        Some(Hit::Reset)
    } else {
        None
    }
}

pub fn format_elapsed(d: Duration) -> String {
    let total = d.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub fn format_break_remaining(d: Duration) -> String {
    let total = d.as_secs();
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{minutes}:{seconds:02}")
}

pub fn format_goal_parts(d: Duration) -> (String, &'static str) {
    let mins = d.as_secs() / 60;
    if mins >= 120 && mins % 60 == 0 {
        ((mins / 60).to_string(), "hr")
    } else {
        (mins.to_string(), "min")
    }
}

pub fn format_limit(d: Duration) -> String {
    let (n, unit) = format_goal_parts(d);
    format!("{n} {unit}")
}

pub fn format_today(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    let hours = total_mins / 60;
    let minutes = total_mins % 60;
    format!("Today · {hours}h {minutes}m")
}

pub fn format_today_hm(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    let hours = total_mins / 60;
    let minutes = total_mins % 60;
    format!("{hours}h {minutes}m")
}

pub fn status_word(state: VisibleState) -> &'static str {
    match state {
        VisibleState::Sitting => "ACTIVE",
        VisibleState::Break => "BREAK",
        VisibleState::Paused => "PAUSED",
        VisibleState::Overdue => "OVERDUE",
    }
}

pub fn state_label(state: VisibleState) -> &'static str {
    match state {
        VisibleState::Sitting => "on",
        VisibleState::Break => "break",
        VisibleState::Paused => "paused",
        VisibleState::Overdue => "over",
    }
}

pub fn card_kicker(state: VisibleState) -> &'static str {
    match state {
        VisibleState::Sitting => "Active",
        VisibleState::Break => "Break",
        VisibleState::Paused => "Paused",
        VisibleState::Overdue => "Overdue",
    }
}

pub fn progress(
    state: VisibleState,
    sitting: Duration,
    limit: Duration,
    break_elapsed: Duration,
    break_dur: Duration,
) -> f32 {
    match state {
        VisibleState::Break => {
            let denom = break_dur.as_secs_f32().max(0.001);
            (break_elapsed.as_secs_f32() / denom).clamp(0.0, 1.0)
        }
        VisibleState::Overdue => 1.0,
        _ => {
            let denom = limit.as_secs_f32().max(0.001);
            (sitting.as_secs_f32() / denom).clamp(0.0, 1.0)
        }
    }
}
