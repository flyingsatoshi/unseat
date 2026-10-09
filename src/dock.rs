//! Edge docking: where a docked overlay sits, and when it slides in and out.
//!
//! The controller is pure and time-driven so it can be tested without windows. The host
//! feeds pointer, click, alert and fullscreen events with a monotonic timestamp, asks
//! `frame()` for the window rectangle and what to draw, and wakes again at `next_wake()`
//! (every ~16 ms only while a slide is running, otherwise only for a pending hide).
//! Docking only moves a window; it never touches the timer.

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockEdge {
    Top,
    Bottom,
    Left,
    #[default]
    Right,
}

impl DockEdge {
    pub const ALL: [Self; 4] = [Self::Top, Self::Bottom, Self::Left, Self::Right];

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }

    fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Which alerts slide a hidden overlay back into view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockReveal {
    #[default]
    TimeUp,
    AllAlerts,
    Never,
}

impl DockReveal {
    pub const ALL: [Self; 3] = [Self::TimeUp, Self::AllAlerts, Self::Never];

    pub fn chip_label(self) -> &'static str {
        match self {
            Self::TimeUp => "Time up",
            Self::AllAlerts => "All alerts",
            Self::Never => "Never",
        }
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}

/// Screen rectangle as (left, top, right, bottom).
pub type Rect = (i32, i32, i32, i32);

/// Logical sizes at 96 DPI; multiplied by the overlay scale.
pub const DOCK_SNAP: f32 = 24.0;
pub const TAB_THICK: f32 = 8.0;
pub const TAB_LEN: f32 = 56.0;
/// Shadow margin around the pill inside the overlay window (matches `widget_layout`).
pub const PILL_MARGIN: f32 = 6.0;

pub const SLIDE_OUT: Duration = Duration::from_millis(180);
pub const SLIDE_IN: Duration = Duration::from_millis(180);
pub const REVEAL_IN: Duration = Duration::from_millis(220);
pub const FRAME: Duration = Duration::from_millis(16);

/// The edge a window rectangle is within `threshold` pixels of, nearest first.
pub fn nearest_edge(win: Rect, work: Rect, threshold: i32) -> Option<DockEdge> {
    let gaps = [
        (DockEdge::Top, (win.1 - work.1).abs()),
        (DockEdge::Bottom, (work.3 - win.3).abs()),
        (DockEdge::Left, (win.0 - work.0).abs()),
        (DockEdge::Right, (work.2 - win.2).abs()),
    ];
    gaps.iter()
        .filter(|(_, d)| *d <= threshold)
        .min_by_key(|(_, d)| *d)
        .map(|(e, _)| *e)
}

/// Position along the edge as a fraction of the available travel (0 = start, 1 = end), so
/// a docked overlay keeps its relative place when the resolution changes.
pub fn edge_offset(edge: DockEdge, win: Rect, work: Rect) -> f32 {
    let (pos, len, lo, hi) = if edge.vertical() {
        (win.1, win.3 - win.1, work.1, work.3)
    } else {
        (win.0, win.2 - win.0, work.0, work.2)
    };
    let travel = (hi - lo - len).max(1) as f32;
    ((pos - lo) as f32 / travel).clamp(0.0, 1.0)
}

/// What the overlay window shows while docked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockDraw {
    /// The pill, translated by (dx, dy) inside the window, its edge-side corners square.
    Pill { dx: f32, dy: f32 },
    /// A thin tab flush with the edge.
    Tab,
    /// A wider tab showing minutes left, while the pointer rests on the tab.
    Peek,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockFrame {
    pub rect: Rect,
    pub draw: DockDraw,
    pub edge: DockEdge,
}

impl DockFrame {
    /// Compact key for repaint decisions.
    pub fn code(&self) -> u32 {
        let edge = self.edge as u32;
        let (kind, extent) = match self.draw {
            DockDraw::Pill { .. } => (1, (self.rect.2 - self.rect.0).max(self.rect.3 - self.rect.1) as u32),
            DockDraw::Tab => (2, 0),
            DockDraw::Peek => (3, 0),
        };
        1 + edge + 4 * kind + 16 * extent
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Window rectangle and drawing for `expansion` (1 = fully out, 0 = hidden behind the edge)
/// of an overlay whose undocked window is `full` pixels (w, h) at `scale`.
pub fn dock_frame(
    edge: DockEdge,
    work: Rect,
    full: (i32, i32),
    offset: f32,
    expansion: f32,
    peek: bool,
    scale: f32,
) -> DockFrame {
    let (fw, fh) = (full.0 as f32, full.1 as f32);
    let margin = PILL_MARGIN * scale;
    let along_len = if edge.vertical() { fh } else { fw };
    let (lo, hi) = if edge.vertical() {
        (work.1 as f32, work.3 as f32)
    } else {
        (work.0 as f32, work.2 as f32)
    };
    let start = lo + (hi - lo - along_len).max(0.0) * offset.clamp(0.0, 1.0);
    let center = start + along_len / 2.0;

    let place = |thick: f32, len: f32, at_start: f32| -> Rect {
        let a0 = at_start.round() as i32;
        let a1 = (at_start + len).round() as i32;
        let t = thick.round() as i32;
        match edge {
            DockEdge::Left => (work.0, a0, work.0 + t, a1),
            DockEdge::Right => (work.2 - t, a0, work.2, a1),
            DockEdge::Top => (a0, work.1, a1, work.1 + t),
            DockEdge::Bottom => (a0, work.3 - t, a1, work.3),
        }
    };

    let tab_thick = (TAB_THICK * scale).max(4.0);
    if peek && expansion <= 0.0 {
        let (thick, len) = if edge.vertical() {
            (44.0 * scale, fh - 2.0 * margin)
        } else {
            (30.0 * scale, 96.0 * scale)
        };
        return DockFrame { rect: place(thick, len, center - len / 2.0), draw: DockDraw::Peek, edge };
    }
    // Perpendicular size of the pill and how much of it is out.
    let pill_thick = if edge.vertical() { fw - 2.0 * margin } else { fh - 2.0 * margin };
    let visible = pill_thick * expansion.clamp(0.0, 1.0);
    if visible <= tab_thick {
        let len = TAB_LEN * scale;
        return DockFrame { rect: place(tab_thick, len, center - len / 2.0), draw: DockDraw::Tab, edge };
    }
    let thick = visible + margin;
    let hidden = pill_thick - visible;
    let (dx, dy) = match edge {
        DockEdge::Right => (0.0, 0.0),
        DockEdge::Bottom => (0.0, 0.0),
        DockEdge::Left => (-margin - hidden, 0.0),
        DockEdge::Top => (0.0, -margin - hidden),
    };
    DockFrame { rect: place(thick, along_len, start), draw: DockDraw::Pill { dx, dy }, edge }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPhase {
    Expanded,
    Hidden,
}

/// Docked visibility over time.
#[derive(Clone, Debug)]
pub struct DockController {
    pub hide_after: Duration,
    pub reveal_for: Duration,
    pub reveal: DockReveal,
    pub autohide: bool,
    phase: DockPhase,
    from: f32,
    to: f32,
    anim_start: Duration,
    anim_len: Duration,
    hover: bool,
    left_at: Option<Duration>,
    reveal_until: Option<Duration>,
    fullscreen: bool,
}

impl DockController {
    pub fn new(hide_after: Duration, reveal_for: Duration, reveal: DockReveal, autohide: bool) -> Self {
        Self {
            hide_after,
            reveal_for,
            reveal,
            autohide,
            phase: DockPhase::Expanded,
            from: 1.0,
            to: 1.0,
            anim_start: Duration::ZERO,
            anim_len: Duration::ZERO,
            hover: false,
            left_at: Some(Duration::ZERO),
            reveal_until: None,
            fullscreen: false,
        }
    }

    pub fn phase(&self) -> DockPhase {
        self.phase
    }

    /// Expansion at `now`, eased; 1 = fully out.
    pub fn expansion(&self, now: Duration) -> f32 {
        if self.anim_len.is_zero() {
            return self.to;
        }
        let t = (now.saturating_sub(self.anim_start).as_secs_f32() / self.anim_len.as_secs_f32()).clamp(0.0, 1.0);
        let eased = if self.to > self.from {
            1.0 - (1.0 - t).powi(3)
        } else {
            t * t * (3.0 - 2.0 * t)
        };
        lerp(self.from, self.to, eased)
    }

    pub fn animating(&self, now: Duration) -> bool {
        !self.anim_len.is_zero() && now < self.anim_start + self.anim_len
    }

    /// Pointer resting on a hidden tab shows the peek tab.
    pub fn peeking(&self, now: Duration) -> bool {
        self.phase == DockPhase::Hidden && self.hover && !self.animating(now)
    }

    /// Hidden entirely (no tab) while a fullscreen app owns the monitor.
    pub fn suppressed(&self) -> bool {
        self.fullscreen && self.phase == DockPhase::Hidden && !self.hover
    }

    fn animate(&mut self, now: Duration, to: f32, len: Duration) {
        self.from = self.expansion(now);
        self.to = to;
        self.anim_start = now;
        self.anim_len = len;
    }

    fn expand(&mut self, now: Duration, len: Duration) {
        if self.phase != DockPhase::Expanded || self.to < 1.0 {
            self.phase = DockPhase::Expanded;
            self.animate(now, 1.0, len);
        }
    }

    fn hide(&mut self, now: Duration) {
        self.phase = DockPhase::Hidden;
        self.animate(now, 0.0, SLIDE_OUT);
        self.left_at = None;
        self.reveal_until = None;
    }

    pub fn pointer_enter(&mut self, _now: Duration) {
        self.hover = true;
        self.left_at = None;
    }

    pub fn pointer_leave(&mut self, now: Duration) {
        self.hover = false;
        self.left_at = Some(now);
    }

    /// A click on the tab (or a shortcut) slides the overlay in.
    pub fn click(&mut self, now: Duration) {
        self.expand(now, SLIDE_IN);
        if !self.hover {
            self.left_at = Some(now);
        }
    }

    pub fn set_autohide(&mut self, now: Duration, autohide: bool) {
        self.autohide = autohide;
        if !autohide {
            self.expand(now, SLIDE_IN);
        } else if !self.hover {
            self.left_at = Some(now);
        }
    }

    /// An alert fired. `time_up` distinguishes the limit being reached from reminders and
    /// break-over. Returns whether the overlay is being revealed.
    pub fn alert(&mut self, now: Duration, time_up: bool) -> bool {
        let wanted = match self.reveal {
            DockReveal::Never => false,
            DockReveal::TimeUp => time_up,
            DockReveal::AllAlerts => true,
        };
        if !wanted || self.fullscreen {
            return false;
        }
        let was_hidden = self.phase == DockPhase::Hidden;
        self.expand(now, REVEAL_IN);
        self.reveal_until = Some(now + self.reveal_for);
        was_hidden
    }

    pub fn set_fullscreen(&mut self, now: Duration, on: bool) {
        if on && !self.fullscreen && self.phase == DockPhase::Expanded && !self.hover {
            self.hide(now);
        }
        if !on && self.fullscreen && !self.autohide {
            self.expand(now, SLIDE_IN);
        }
        self.fullscreen = on;
    }

    /// Advance timers; call at `next_wake` or on any event.
    pub fn tick(&mut self, now: Duration) {
        if self.phase != DockPhase::Expanded || self.hover {
            return;
        }
        if !self.autohide && !self.fullscreen {
            return;
        }
        if let Some(until) = self.reveal_until {
            if now < until {
                return;
            }
            self.hide(now);
            return;
        }
        if let Some(left) = self.left_at {
            if now >= left + self.hide_after {
                self.hide(now);
            }
        }
    }

    /// When the host should call `tick` next, if at all.
    pub fn next_wake(&self, now: Duration) -> Option<Duration> {
        if self.animating(now) {
            return Some(FRAME);
        }
        if self.phase != DockPhase::Expanded || self.hover || (!self.autohide && !self.fullscreen) {
            return None;
        }
        let due = match (self.reveal_until, self.left_at) {
            (Some(until), _) => until,
            (None, Some(left)) => left + self.hide_after,
            (None, None) => return None,
        };
        Some(due.saturating_sub(now).max(FRAME))
    }
}
