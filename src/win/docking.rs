//! Edge docking for the overlay: placement, slide timing and drag-to-dock.
//!
//! All timing decisions live in `unseat::DockController`; this file turns them into window
//! positions. Frames are scheduled on a separate timer only while a slide runs or a hide is
//! pending, so a docked overlay costs nothing extra at rest. The engine is never touched.

use crate::win::app::App;
use crate::win::widget;
use std::time::Duration;
use unseat::{
    dock_frame, edge_offset, nearest_edge, widget_pixel_size, DockController, DockEdge,
    DockFrame, DockPhase, Rect, DOCK_SNAP,
};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect, KillTimer, SetTimer,
    SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
};

pub const DOCK_ID: usize = 3;

fn monitor_rects(mon: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<(Rect, Rect)> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(mon, &mut info).as_bool() }.then(|| {
        let w = info.rcWork;
        let m = info.rcMonitor;
        ((w.left, w.top, w.right, w.bottom), (m.left, m.top, m.right, m.bottom))
    })
}

/// Work area of the monitor nearest a screen point.
pub fn work_area_near(x: i32, y: i32) -> Option<Rect> {
    unsafe { monitor_rects(MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST)) }
        .map(|(work, _)| work)
}

fn window_rect(hwnd: HWND) -> Rect {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rc);
    }
    (rc.left, rc.top, rc.right, rc.bottom)
}

/// A fullscreen app (game, video, presentation) owns the overlay's monitor.
fn fullscreen_on(hwnd: HWND) -> bool {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                return true;
            }
            if state != QUNS_BUSY {
                return false;
            }
        }
        let fg = GetForegroundWindow();
        if fg.0.is_null() || fg == GetDesktopWindow() || fg == GetShellWindow() || fg == hwnd {
            return false;
        }
        let Some((_, monitor)) = monitor_rects(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))
        else {
            return false;
        };
        let r = window_rect(fg);
        r.0 <= monitor.0 && r.1 <= monitor.1 && r.2 >= monitor.2 && r.3 >= monitor.3
    }
}

impl App {
    fn dock_now(&mut self) -> Duration {
        self.clock_now()
    }

    pub fn docked_edge(&self) -> Option<DockEdge> {
        if self.dragging || !self.settings.dock_enabled {
            None
        } else {
            self.settings.dock_edge
        }
    }

    fn new_controller(&self) -> DockController {
        DockController::new(
            Duration::from_secs(self.settings.dock_hide_after_secs),
            Duration::from_secs(self.settings.dock_reveal_secs),
            self.settings.dock_reveal,
            self.settings.dock_autohide,
        )
    }

    /// Bring docking in line with the settings: attach to the nearest edge when docking was
    /// just turned on, detach when it was turned off, and pick up timing changes.
    pub fn sync_dock(&mut self) {
        if !self.settings.dock_enabled {
            self.dock = None;
            self.dock_frame = None;
            self.show_dock_window(true);
            self.schedule_dock();
            return;
        }
        if self.settings.dock_edge.is_none() {
            let win = window_rect(self.widget);
            if let Some(work) = work_area_near((win.0 + win.2) / 2, (win.1 + win.3) / 2) {
                let gaps = [
                    (DockEdge::Top, win.1 - work.1),
                    (DockEdge::Bottom, work.3 - win.3),
                    (DockEdge::Left, win.0 - work.0),
                    (DockEdge::Right, work.2 - win.2),
                ];
                let edge = gaps.iter().min_by_key(|(_, d)| d.abs()).map(|(e, _)| *e);
                self.settings.dock_edge = edge;
                if let Some(edge) = edge {
                    self.settings.dock_offset = edge_offset(edge, win, work);
                }
            }
        }
        let now = self.dock_now();
        match self.dock.as_mut() {
            Some(d) => {
                d.hide_after = Duration::from_secs(self.settings.dock_hide_after_secs);
                d.reveal_for = Duration::from_secs(self.settings.dock_reveal_secs);
                d.reveal = self.settings.dock_reveal;
                d.set_autohide(now, self.settings.dock_autohide);
            }
            None => {
                let mut d = self.new_controller();
                d.pointer_leave(now);
                self.dock = Some(d);
            }
        }
        self.dock_frame = None;
        self.schedule_dock();
    }

    /// Where the docked overlay is and what it shows right now.
    pub fn current_dock_frame(&mut self, scale: f32) -> Option<DockFrame> {
        let edge = self.docked_edge()?;
        let now = self.dock_now();
        let d = self.dock.as_ref()?;
        let (expansion, peek) = (d.expansion(now), d.peeking(now));
        let center = self.dock_anchor();
        let work = work_area_near(center.0, center.1)?;
        let full = widget_pixel_size(self.settings.timer_shape, scale);
        Some(dock_frame(edge, work, full, self.settings.dock_offset, expansion, peek, scale))
    }

    /// A point on the monitor the overlay is docked to: the saved expanded position.
    fn dock_anchor(&self) -> (i32, i32) {
        (self.settings.window_x + 40, self.settings.window_y + 20)
    }

    /// Move the overlay window to `frame` (or hide it over a fullscreen app).
    pub fn place_docked(&mut self, frame: DockFrame) {
        let suppressed = self.dock.as_ref().is_some_and(|d| d.suppressed());
        self.show_dock_window(!suppressed);
        if self.dock_frame.map(|f| f.rect) != Some(frame.rect) {
            let (l, t, r, b) = frame.rect;
            unsafe {
                let _ = SetWindowPos(
                    self.widget,
                    HWND::default(),
                    l,
                    t,
                    r - l,
                    b - t,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
        self.dock_frame = Some(frame);
    }

    fn show_dock_window(&mut self, show: bool) {
        if show == !self.dock_suppressed {
            return;
        }
        self.dock_suppressed = !show;
        if !self.widget_visible {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.widget, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        // SW_HIDE reports the window as not visible; keep the user's intent.
        self.widget_visible = true;
        self.sync_prompt();
    }

    /// Re-arm the frame timer for the next slide frame or pending hide.
    pub fn schedule_dock(&mut self) {
        let now = self.dock_now();
        let wake = self.dock.as_ref().and_then(|d| d.next_wake(now));
        unsafe {
            match wake {
                Some(delay) if self.docked_edge().is_some() => {
                    let _ = SetTimer(self.hidden, DOCK_ID, delay.as_millis().max(1) as u32, None);
                }
                _ => {
                    let _ = KillTimer(self.hidden, DOCK_ID);
                }
            }
        }
    }

    /// Advance docking: run pending hides, follow fullscreen apps, redraw a slide frame.
    pub fn dock_tick(&mut self) {
        if self.docked_edge().is_none() {
            self.schedule_dock();
            return;
        }
        let now = self.dock_now();
        let fullscreen = self.settings.dock_hide_fullscreen && fullscreen_on(self.widget);
        if let Some(d) = self.dock.as_mut() {
            d.set_fullscreen(now, fullscreen);
            d.tick(now);
        }
        self.repaint();
        self.sync_prompt();
        self.schedule_dock();
    }

    pub fn dock_pointer(&mut self, inside: bool) {
        let now = self.dock_now();
        if let Some(d) = self.dock.as_mut() {
            if inside {
                d.pointer_enter(now);
            } else {
                d.pointer_leave(now);
            }
        }
        self.dock_tick();
    }

    /// A click while docked: on the tab it slides the overlay in. Returns true if consumed.
    pub fn dock_click(&mut self) -> bool {
        if self.docked_edge().is_none() {
            return false;
        }
        let now = self.dock_now();
        let Some(d) = self.dock.as_mut() else {
            return false;
        };
        if d.phase() == DockPhase::Expanded && d.expansion(now) > 0.99 {
            return false;
        }
        d.click(now);
        crate::win::testhook::log("dock reveal click");
        self.dock_tick();
        true
    }

    /// An alert fired; slide in if the reveal setting asks for it.
    pub fn dock_alert(&mut self, time_up: bool) {
        if self.docked_edge().is_none() {
            return;
        }
        let now = self.dock_now();
        let revealed = self.dock.as_mut().is_some_and(|d| d.alert(now, time_up));
        if revealed {
            crate::win::testhook::log("dock reveal alert");
        }
        self.dock_tick();
    }

    /// The pin chip on a docked overlay toggles auto-hide.
    pub fn toggle_pin(&mut self) {
        self.settings.dock_autohide = !self.settings.dock_autohide;
        let now = self.dock_now();
        if let Some(d) = self.dock.as_mut() {
            d.set_autohide(now, self.settings.dock_autohide);
        }
        crate::win::testhook::log(&format!("dock pinned={}", !self.settings.dock_autohide));
        self.save_settings();
        self.force_repaint();
        self.schedule_dock();
    }

    pub fn dock_hidden(&self) -> bool {
        self.docked_edge().is_some()
            && self.dock.as_ref().is_some_and(|d| d.phase() == DockPhase::Hidden)
    }

    /// Finish a drag: dock to an edge the overlay was released near, or float.
    pub fn end_drag_dock(&mut self, hwnd: HWND) {
        let win = window_rect(hwnd);
        let center = ((win.0 + win.2) / 2, (win.1 + win.3) / 2);
        self.settings.window_x = win.0;
        self.settings.window_y = win.1;
        if !self.settings.dock_enabled {
            return;
        }
        let threshold = (DOCK_SNAP * widget::scale(hwnd)).round() as i32;
        let edge = work_area_near(center.0, center.1)
            .and_then(|work| nearest_edge(win, work, threshold).map(|e| (e, work)));
        match edge {
            Some((edge, work)) => {
                self.settings.dock_edge = Some(edge);
                self.settings.dock_offset = edge_offset(edge, win, work);
                let now = self.dock_now();
                let mut d = self.new_controller();
                d.pointer_enter(now);
                self.dock = Some(d);
                crate::win::testhook::log(&format!("docked {edge:?}"));
            }
            None => {
                self.settings.dock_edge = None;
                self.dock = None;
                crate::win::testhook::log("undocked");
            }
        }
        self.dock_frame = None;
        self.force_repaint();
        self.schedule_dock();
    }
}
