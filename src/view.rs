use crate::engine::{Snapshot, VisibleState};
use crate::settings::{TimerDisplayMode, TimerShape, WidgetSize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Pause,
    Reset,
    Close,
    Snooze,
}

pub const WIDGET_W: f32 = 144.0;
pub const WIDGET_H: f32 = 56.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerRenderKey {
    timer_display_mode: TimerDisplayMode,
    face_secs: u64,
    break_remaining_secs: u64,
    state: VisibleState,
    shape: TimerShape,
    size: WidgetSize,
    hover: bool,
    dark: bool,
}

pub fn timer_render_key(
    snapshot: Snapshot,
    timer_display_mode: TimerDisplayMode,
    shape: TimerShape,
    size: WidgetSize,
    hover: bool,
    dark: bool,
) -> TimerRenderKey {
    TimerRenderKey {
        timer_display_mode,
        face_secs: timer_face_seconds(timer_display_mode, snapshot),
        break_remaining_secs: countdown_secs(snapshot.break_remaining),
        state: snapshot.state,
        shape,
        size,
        hover,
        dark,
    }
}

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

#[derive(Clone, Copy, Debug)]
pub struct WidgetLayout {
    pub pill_x: f32,
    pub pill_y: f32,
    pub pill_w: f32,
    pub pill_h: f32,
    pub tag_x: f32,
    pub tag_y: f32,
    pub tag_w: f32,
    pub tag_h: f32,
    pub pause: (f32, f32),
    pub reset: (f32, f32),
    pub control_r: f32,
    pub digits_l: f32,
    pub digits_t: f32,
    pub digits_r: f32,
    pub digits_b: f32,
    pub bar_x: f32,
    pub bar_y: f32,
    pub bar_w: f32,
    pub bar_h: f32,
    pub digit_px: f32,
    pub play: (f32, f32, f32),
    pub close: (f32, f32),
    pub close_r: f32,
}

pub fn widget_layout(scale: f32) -> WidgetLayout {
    let s = scale;
    let pill_x = 6.0 * s;
    let pill_y = 6.0 * s;
    let pill_w = WIDGET_W * s - pill_x * 2.0;
    let pill_h = WIDGET_H * s - pill_y * 2.0;
    let pad = 8.0 * s;
    let control_r = 9.0 * s;
    let inner_pad = 2.0 * s;
    let col_gap = 6.0 * s;
    let tag_w = inner_pad * 2.0 + control_r * 2.0;
    let chip_inset = 2.0 * s;
    let tag_h = pill_h - chip_inset * 2.0;
    let tag_x = pill_x + pill_w - pad - tag_w;
    let tag_y = pill_y + chip_inset;
    let pause_x = tag_x + inner_pad + control_r;
    let gap = (tag_h - inner_pad * 2.0 - control_r * 4.0).max(0.0);
    let pause = (pause_x, tag_y + inner_pad + control_r);
    let reset = (pause_x, pause.1 + control_r + gap + control_r);
    let digits_l = pill_x + pad;
    let digits_r = tag_x - col_gap;
    let bar_h = 3.0 * s;
    let bar_y = pill_y + pill_h - 5.0 * s - bar_h;
    let play_x = (digits_l + digits_r) * 0.5;
    let play_y = pill_y + pill_h * 0.5;
    let close_r = 6.0 * s;
    let close_inset = 1.0 * s;
    WidgetLayout {
        pill_x,
        pill_y,
        pill_w,
        pill_h,
        tag_x,
        tag_y,
        tag_w,
        tag_h,
        pause,
        reset,
        control_r,
        digits_l,
        digits_t: pill_y + 3.0 * s,
        digits_r,
        digits_b: bar_y - 2.0 * s,
        bar_x: digits_l,
        bar_y,
        bar_w: (digits_r - digits_l).max(8.0 * s),
        bar_h,
        digit_px: 26.0 * s,
        play: (play_x, play_y, 18.0 * s),
        close: (WIDGET_W * s - close_inset - close_r, close_inset + close_r),
        close_r,
    }
}

pub fn play_center(scale: f32) -> (f32, f32, f32) {
    widget_layout(scale).play
}

/// Hover chips stacked in a right-edge tag: pause on top, reset below.
pub fn control_centers(_shape: TimerShape, scale: f32) -> ((f32, f32), (f32, f32), f32) {
    let l = widget_layout(scale);
    (l.pause, l.reset, l.control_r)
}

pub fn hit_control(
    _shape: TimerShape,
    hover: bool,
    state: VisibleState,
    scale: f32,
    x: i32,
    y: i32,
) -> Option<Hit> {
    let xf = x as f32;
    let yf = y as f32;
    let layout = widget_layout(scale);
    let paused = state == VisibleState::Paused;
    if paused {
        let (cx, cy, r) = layout.play;
        if (xf - cx).hypot(yf - cy) <= r {
            return Some(Hit::Pause);
        }
    }
    if hover {
        let (cx, cy) = layout.close;
        if (xf - cx).hypot(yf - cy) <= layout.close_r {
            return Some(Hit::Close);
        }
        let (px, py) = layout.pause;
        let (rx, ry) = layout.reset;
        let r = layout.control_r;
        if !paused && (xf - px).hypot(yf - py) <= r {
            return Some(Hit::Pause);
        }
        if (xf - rx).hypot(yf - ry) <= r {
            return Some(Hit::Reset);
        }
    }
    if state == VisibleState::Overdue
        && xf >= layout.digits_l
        && xf <= layout.digits_r
        && yf >= layout.digits_t
        && yf <= layout.digits_b
    {
        return Some(Hit::Snooze);
    }
    None
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

pub fn face_digits(timer_remaining: Duration) -> String {
    format_elapsed(Duration::from_secs(countdown_secs(timer_remaining)))
}

pub fn timer_face_digits(timer_display_mode: TimerDisplayMode, snapshot: Snapshot) -> String {
    format_elapsed(Duration::from_secs(timer_face_seconds(
        timer_display_mode,
        snapshot,
    )))
}

/// Keeps expanding hour-based stopwatch values inside the timer's fixed digit lane.
pub fn timer_digit_px(digits: &str, base_px: f32, lane_width: f32) -> f32 {
    let em_width = digits
        .chars()
        .fold(0.0, |width, ch| width + if ch == ':' { 0.28 } else { 0.58 });
    let estimated_width = em_width * base_px;
    if estimated_width <= lane_width || estimated_width <= 0.0 {
        base_px
    } else {
        base_px * lane_width.max(0.0) / estimated_width
    }
}

fn timer_face_seconds(timer_display_mode: TimerDisplayMode, snapshot: Snapshot) -> u64 {
    match timer_display_mode {
        TimerDisplayMode::Countdown => countdown_secs(snapshot.timer_remaining),
        TimerDisplayMode::Stopwatch => snapshot.sitting_elapsed.as_secs(),
    }
}

fn countdown_secs(remaining: Duration) -> u64 {
    remaining
        .as_secs()
        .saturating_add(u64::from(remaining.subsec_nanos() != 0))
}

pub fn pill_background_rgb(state: VisibleState) -> [u8; 3] {
    match state {
        VisibleState::Overdue => [0x2A, 0x10, 0x16],
        _ => [0x10, 0x12, 0x14],
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

pub fn tag_goal(_state: VisibleState, limit: Duration, _break_dur: Duration) -> Duration {
    limit
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
