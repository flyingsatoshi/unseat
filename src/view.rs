use crate::dock::DockEdge;
use crate::engine::{Snapshot, VisibleState};
use crate::settings::{TimerDisplayMode, TimerShape, WidgetSize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Pause, or resume when paused.
    Pause,
    Break,
    Reset,
    /// The corner chip: hide the overlay, or pin it open when docked.
    Close,
    /// The overdue time: ask "break or extend?" again.
    Prompt,
}

/// Overlay window size at 96 DPI for size S. Other sizes multiply `scale`.
pub const WIDGET_W: f32 = 176.0;
pub const WIDGET_H: f32 = 60.0;
/// Pill corner radius: a little sharper than a full capsule.
pub const PILL_RADIUS: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerRenderKey {
    timer_display_mode: TimerDisplayMode,
    face_secs: u64,
    overdue_mins: u64,
    progress_permille: u32,
    scale_milli: u32,
    dock: u32,
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
        overdue_mins: snapshot.overdue_by.as_secs() / 60,
        progress_permille: 0,
        scale_milli: 0,
        dock: 0,
        break_remaining_secs: countdown_secs(snapshot.break_remaining),
        state: snapshot.state,
        shape,
        size,
        hover,
        dark,
    }
}

impl TimerRenderKey {
    /// Include the paint scale so a DPI change (moving to another monitor) repaints at once.
    pub fn at_scale(mut self, scale: f32) -> Self {
        self.scale_milli = (scale * 1000.0).round() as u32;
        self
    }

    /// Include the progress band at the resolution the pill can show (one step per pixel).
    pub fn with_progress(mut self, progress: f32, pill_px: f32) -> Self {
        let steps = pill_px.max(1.0);
        self.progress_permille =
            ((progress.clamp(0.0, 1.0) * steps).round() / steps * 1000.0) as u32;
        self
    }

    /// Include any docking presentation (edge, phase, animation frame).
    pub fn with_dock(mut self, code: u32) -> Self {
        self.dock = code;
        self
    }

    /// A docked tab shows no clock, only progress (and whole minutes while it peeks), so its
    /// key leaves the seconds out: it repaints when what it shows changes, not every second.
    pub fn for_tab(mut self, snapshot: Snapshot, peek: bool) -> Self {
        self.face_secs = if peek { peek_minutes(snapshot).0 } else { 0 };
        self.overdue_mins = 0;
        self.break_remaining_secs = 0;
        self
    }
}

/// What a peeking docked tab shows: whole minutes over the limit, left of the break, or left
/// of the session, and their unit.
pub fn peek_minutes(snapshot: Snapshot) -> (u64, &'static str) {
    match snapshot.state {
        VisibleState::Overdue => (snapshot.overdue_by.as_secs() / 60, "over"),
        VisibleState::Break => (snapshot.break_remaining.as_secs().div_ceil(60), "break"),
        _ => (snapshot.timer_remaining.as_secs().div_ceil(60), "min"),
    }
}

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
    pub radius: f32,
    /// Digit lane while not hovered.
    pub digits_l: f32,
    pub digits_r: f32,
    /// Digit lane while hover controls are shown.
    pub digits_hover_r: f32,
    pub tag_l: f32,
    pub tag_r: f32,
    pub digit_px: f32,
    pub tag_px: f32,
    /// Hover buttons: pause, break and reset centers, and their radius.
    pub pause: (f32, f32),
    pub brk: (f32, f32),
    pub reset: (f32, f32),
    pub control_r: f32,
    pub close: (f32, f32),
    pub close_r: f32,
}

pub fn widget_layout(scale: f32) -> WidgetLayout {
    let s = scale;
    let pill_x = 6.0 * s;
    let pill_y = 6.0 * s;
    let pill_w = WIDGET_W * s - pill_x * 2.0;
    let pill_h = WIDGET_H * s - pill_y * 2.0;
    let pill_r = pill_x + pill_w;
    let mid_y = pill_y + pill_h * 0.5;
    let control_r = 12.0 * s;
    let step = control_r * 2.0 + 4.0 * s;
    let reset = (pill_r - 8.0 * s - control_r, mid_y);
    let brk = (reset.0 - step, mid_y);
    let pause = (brk.0 - step, mid_y);
    let tag_r = pill_r - 16.0 * s;
    let tag_l = tag_r - 40.0 * s;
    let close_r = 7.0 * s;
    WidgetLayout {
        pill_x,
        pill_y,
        pill_w,
        pill_h,
        radius: PILL_RADIUS * s,
        digits_l: pill_x + 16.0 * s,
        digits_r: tag_l - 4.0 * s,
        digits_hover_r: pause.0 - control_r - 6.0 * s,
        tag_l,
        tag_r,
        digit_px: 28.0 * s,
        tag_px: 12.0 * s,
        pause,
        brk,
        reset,
        control_r,
        close: (WIDGET_W * s - 1.0 * s - close_r, 1.0 * s + close_r),
        close_r,
    }
}

fn in_circle((cx, cy): (f32, f32), r: f32, x: f32, y: f32) -> bool {
    (x - cx).hypot(y - cy) <= r
}

/// Where the corner chip sits: the window's top-right corner, moved to the corner away
/// from the screen edge when docked so it never hangs off the screen.
pub fn chip_center(scale: f32, edge: Option<DockEdge>) -> (f32, f32) {
    let l = widget_layout(scale);
    let (x, y) = l.close;
    // Docked windows drop the shadow margin on the edge side (see `dock_frame`).
    let margin = l.pill_x;
    match edge {
        Some(DockEdge::Right) => (WIDGET_W * scale - x, y),
        Some(DockEdge::Left) => (x - margin, y),
        Some(DockEdge::Top) => (x, WIDGET_H * scale - y - margin),
        Some(DockEdge::Bottom) | None => (x, y),
    }
}

/// What a click at (x, y) in window pixels does. Controls only exist while hovered;
/// clicking a paused pill resumes it, and clicking an overdue time snoozes.
pub fn hit_control(
    _shape: TimerShape,
    hover: bool,
    state: VisibleState,
    scale: f32,
    x: i32,
    y: i32,
) -> Option<Hit> {
    hit_overlay(hover, state, scale, x as f32, y as f32, None)
}

/// `hit_control` for a docked pill drawn shifted by `(dx, dy)` with its chip moved off the
/// screen edge.
pub fn hit_overlay(
    hover: bool,
    state: VisibleState,
    scale: f32,
    x: f32,
    y: f32,
    dock: Option<(DockEdge, f32, f32)>,
) -> Option<Hit> {
    let l = widget_layout(scale);
    if hover && in_circle(chip_center(scale, dock.map(|d| d.0)), l.close_r, x, y) {
        return Some(Hit::Close);
    }
    let (xf, yf) = match dock {
        Some((_, dx, dy)) => (x - dx, y - dy),
        None => (x, y),
    };
    if hover {
        for (center, hit) in [(l.pause, Hit::Pause), (l.brk, Hit::Break), (l.reset, Hit::Reset)] {
            if in_circle(center, l.control_r, xf, yf) {
                return Some(hit);
            }
        }
    }
    let in_pill = xf >= l.pill_x
        && xf <= l.pill_x + l.pill_w
        && yf >= l.pill_y
        && yf <= l.pill_y + l.pill_h;
    if !in_pill {
        return None;
    }
    let lane_r = if hover { l.digits_hover_r } else { l.digits_r };
    match state {
        VisibleState::Paused => Some(Hit::Pause),
        VisibleState::Overdue if xf >= l.digits_l && xf <= lane_r => Some(Hit::Prompt),
        _ => None,
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
    if snapshot.state == VisibleState::Break {
        return countdown_secs(snapshot.break_remaining);
    }
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

pub fn format_goal_parts(d: Duration) -> (String, &'static str) {
    let mins = d.as_secs() / 60;
    if mins >= 120 && mins % 60 == 0 {
        ((mins / 60).to_string(), "hr")
    } else {
        (mins.to_string(), "min")
    }
}

/// The overlay's tag beside the digits: the goal ("20m", or "/ 20m" for a stopwatch),
/// minutes over when overdue, or the state when on a break, away or paused.
pub fn tag_text(snapshot: Snapshot, mode: TimerDisplayMode, limit: Duration) -> String {
    let (n, unit) = format_goal_parts(limit);
    let goal = format!("{n}{}", if unit == "hr" { "h" } else { "m" });
    match snapshot.state {
        VisibleState::Overdue => format!("+{}m", snapshot.overdue_by.as_secs() / 60),
        VisibleState::Break => "break".to_string(),
        VisibleState::Away => "away".to_string(),
        VisibleState::Paused => "paused".to_string(),
        VisibleState::Sitting if mode == TimerDisplayMode::Stopwatch => format!("/ {goal}"),
        VisibleState::Sitting => goal,
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

pub fn progress(
    state: VisibleState,
    sitting: Duration,
    limit: Duration,
    break_elapsed: Duration,
    break_dur: Duration,
) -> f32 {
    match state {
        VisibleState::Break | VisibleState::Away => {
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

fn ceil_minutes(d: Duration) -> u64 {
    d.as_secs().div_ceil(60) + u64::from(d.as_secs() % 60 == 0 && d.subsec_nanos() != 0)
}

/// One-line status for the tray tooltip and flyout. Minute resolution, so the tooltip only
/// changes about once a minute.
pub fn status_line(snapshot: Snapshot) -> String {
    match snapshot.state {
        VisibleState::Sitting => match ceil_minutes(snapshot.timer_remaining) {
            0 | 1 => "Under a minute left".to_string(),
            m => format!("{m} min left"),
        },
        VisibleState::Overdue => match snapshot.overdue_by.as_secs() / 60 {
            0 => "Time to stand up".to_string(),
            m => format!("Overdue by {m} min"),
        },
        VisibleState::Paused => "Paused".to_string(),
        VisibleState::Away => "Away · timer paused".to_string(),
        VisibleState::Break => match ceil_minutes(snapshot.break_remaining) {
            0 | 1 => "On break · under a minute left".to_string(),
            m => format!("On break · {m} min left"),
        },
    }
}

/// Snap a dragged window to the work-area edges when it comes within `threshold` pixels.
pub fn snap_to_edges(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    work: (i32, i32, i32, i32),
    threshold: i32,
) -> (i32, i32) {
    let (left, top, right, bottom) = work;
    let snap = |pos: i32, size: i32, lo: i32, hi: i32| {
        if (pos - lo).abs() <= threshold {
            lo
        } else if (pos + size - hi).abs() <= threshold {
            hi - size
        } else {
            pos
        }
    };
    (snap(x, w, left, right), snap(y, h, top, bottom))
}
