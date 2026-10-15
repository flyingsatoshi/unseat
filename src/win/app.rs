use crate::win::{
    autostart, docking, flyout, handoff, hotkeys, idle, install, paint, phone, prompt, session,
    settings_dlg, sound, testhook, theme, tray, tray_panel, widget,
};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unseat::{
    alert_actions, break_over_note, checkpoint_current, clamp_clock, format_today, next_tick_delay,
    restored_today_sitting, status_line, time_up_prompt, timer_render_key, widget_layout, Beep,
    CivilDate, Command, DockDraw, DockFrame, DockPhase, EngineConfig, Input, PhoneAlerts,
    Settings, SittingEngine, ThemePreference, TimerRenderKey, VisibleState, WidgetSize, WIDGET_H,
    WIDGET_W,
};
use windows::core::w;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW,
    GetWindowLongPtrW, IsIconic, KillTimer, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA,
    HWND_MESSAGE, HWND_NOTOPMOST, HWND_TOPMOST, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_HOTKEY,
    WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_OVERLAPPED,
};

const HIDDEN_CLASS: windows::core::PCWSTR = w!("UnseatHidden");
const TICK_ID: usize = 1;
const SOUND_ID: usize = 2;
/// How often to check whether the settings file still restores the running timer.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// Rewrite it at least this often anyway, so a crash just before midnight loses little of
/// that day's total (an archived day keeps the total last written).
const SAVE_AT_LEAST_EVERY: Duration = Duration::from_secs(15 * 60);
/// How far a restore from the file may be off before it counts as stale.
const RESTORE_TOLERANCE: Duration = Duration::from_secs(3);

pub struct App {
    pub engine: SittingEngine,
    pub settings: Settings,
    pub settings_path: PathBuf,
    pub clock_origin_ms: u64,
    pub hidden: HWND,
    pub widget: HWND,
    pub locked: bool,
    pub hover: bool,
    pub dragging: bool,
    pub drag_dx: i32,
    pub drag_dy: i32,
    /// Windows "apps use dark theme"; the effective theme also honors the user's choice.
    pub system_dark: bool,
    pub tray_added: bool,
    pub d2d: Option<paint::D2d>,
    last_key: Option<TimerRenderKey>,
    /// When the periodic save check last ran, and when the file was last written (monotonic).
    last_save_at: Duration,
    last_write_at: Duration,
    /// What the settings file holds: the settings as last written.
    on_disk: Option<Settings>,
    last_tray_select: Option<Instant>,
    last_clock: Option<Duration>,
    last_tip: String,
    hotkeys_on: bool,
    pub widget_visible: bool,
    pub suspended: bool,
    pub dock: Option<unseat::DockController>,
    pub dock_frame: Option<unseat::DockFrame>,
    pub dock_suppressed: bool,
    /// iPhone reminders; inert until turned on or opened from the tray.
    pub phone: phone::Phone,
    /// The "break or extend?" tabs for the current time-up.
    prompt: Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prompt {
    /// Nothing to ask.
    Idle,
    /// Time is up but the pill is not out on screen (overlay hidden, full screen, being
    /// dragged, or docked away); ask once it is.
    Waiting,
    /// The tabs hang from the pill.
    Shown,
}

pub fn run() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        install::set_aumid();
        let mutex = CreateMutexW(None, true, &testhook::singleton())?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(widget::CLASS, &testhook::title("Unseat")).ok();
            // A newer build replaces the running one; anything else just brings it forward.
            let replaced = existing
                .is_some_and(|w| handoff::should_replace(w) && handoff::take_over(w, mutex));
            if !replaced {
                if let Some(existing) = existing {
                    let _ = ShowWindow(existing, SW_SHOWNOACTIVATE);
                    let _ = SetForegroundWindow(existing);
                }
                let _ = ReleaseMutex(mutex);
                let _ = CloseHandle(mutex);
                return Ok(());
            }
        }

        widget::register()?;
        settings_dlg::register();
        flyout::register();
        tray_panel::register();
        prompt::register();
        register_hidden()?;

        let path = settings_file();
        let data_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut settings = Settings::load_from(&path);
        let today = local_date();
        let saved_date = CivilDate::parse_ymd(&settings.today_date).unwrap_or(today);
        let wall_now = wall_now_secs();
        let saved_today = Duration::from_secs(settings.today_sitting_secs);
        if saved_date != today {
            let date = settings.today_date.clone();
            settings.archive_day(&date, saved_today.as_secs());
        }
        let today_sitting = settings.timer_checkpoint.map_or_else(
            || {
                if saved_date == today {
                    saved_today
                } else {
                    Duration::ZERO
                }
            },
            |checkpoint| {
                restored_today_sitting(
                    saved_today,
                    saved_date,
                    today,
                    settings.inactivity_behavior,
                    checkpoint,
                    wall_now,
                    local_time_since_midnight(),
                )
            },
        );
        settings.today_date = today.to_ymd_string();
        settings.today_sitting_secs = today_sitting.as_secs();
        let config = EngineConfig::from_settings(&settings);
        let engine = settings.timer_checkpoint.map_or_else(
            || SittingEngine::new(config, today, today_sitting),
            |checkpoint| SittingEngine::restore(config, today, today_sitting, checkpoint, wall_now),
        );

        let app = Box::new(App {
            engine,
            settings,
            settings_path: path,
            clock_origin_ms: GetTickCount64(),
            hidden: HWND::default(),
            widget: HWND::default(),
            locked: false,
            hover: false,
            dragging: false,
            drag_dx: 0,
            drag_dy: 0,
            system_dark: theme::apps_use_dark(),
            tray_added: false,
            d2d: paint::D2d::new(),
            last_key: None,
            last_save_at: Duration::ZERO,
            last_write_at: Duration::ZERO,
            on_disk: None,
            last_tray_select: None,
            last_clock: None,
            last_tip: String::new(),
            hotkeys_on: false,
            widget_visible: true,
            suspended: false,
            dock: None,
            dock_frame: None,
            dock_suppressed: false,
            phone: phone::Phone::new(&data_dir),
            prompt: Prompt::Idle,
        });
        let ptr = Box::into_raw(app);

        let hidden = CreateWindowExW(
            Default::default(),
            HIDDEN_CLASS,
            &testhook::title("Unseat"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            GetModuleHandleW(None)?,
            Some(ptr as *const std::ffi::c_void),
        )?;
        (*ptr).hidden = hidden;
        (*ptr).phone.set_hidden(hidden);
        session::register(hidden);
        if SetTimer(hidden, TICK_ID, 1000, None) == 0 {
            let error = windows::core::Error::from_win32();
            let _ = DestroyWindow(hidden);
            drop(Box::from_raw(ptr));
            let _ = ReleaseMutex(mutex);
            let _ = CloseHandle(mutex);
            return Err(error);
        }
        (*ptr).add_tray();
        (*ptr).sync_hotkeys();
        (*ptr).phone.sync(&(*ptr).settings);
        if (*ptr).settings.launch_with_windows {
            autostart::repair();
        }

        let (x, y) = widget::clamp_to_work_area(
            (*ptr).settings.window_x,
            (*ptr).settings.window_y,
            WIDGET_W as i32,
            WIDGET_H as i32,
        );
        (*ptr).widget = widget::create(ptr, x, y)?;
        (*ptr).apply_topmost();
        (*ptr).sync_dock();
        (*ptr).repaint();
        (*ptr).tick();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        (*ptr).persist_today();
        (*ptr).phone.shutdown();
        sound::stop();
        hotkeys::unregister(hidden);
        session::unregister(hidden);
        let _ = KillTimer(hidden, TICK_ID);
        let _ = KillTimer(hidden, SOUND_ID);
        flyout::shutdown();
        tray_panel::shutdown();
        prompt::shutdown();
        settings_dlg::shutdown();
        tray::remove(hidden);
        // No window may keep a pointer to the App about to be freed.
        for hwnd in [(*ptr).widget, hidden] {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(hwnd);
        }
        drop(Box::from_raw(ptr));
        let _ = ReleaseMutex(mutex);
        let _ = CloseHandle(mutex);
        Ok(())
    }
}

impl App {
    pub fn clock_now(&mut self) -> Duration {
        let raw =
            Duration::from_millis(unsafe { GetTickCount64().saturating_sub(self.clock_origin_ms) })
                + testhook::clock_offset();
        let now = clamp_clock(self.last_clock, raw);
        self.last_clock = Some(now);
        now
    }

    fn current_input(&self) -> Input {
        Input {
            last_input_age: testhook::idle_override().unwrap_or_else(idle::last_input_age),
            session_locked: self.locked || self.suspended,
        }
    }

    pub fn is_dark(&self) -> bool {
        self.settings.theme.is_dark(self.system_dark)
    }

    fn handle_beep(&mut self, beep: Beep) {
        if beep == Beep::None {
            return;
        }
        testhook::log(&format!("alert {beep:?}"));
        self.mirror_to_phone(beep);
        let sitting = self.engine.snapshot().sitting_elapsed;
        let actions = alert_actions(beep, &self.settings, sitting);
        if actions.play_sound {
            self.play_alert_sound();
        }
        self.dock_alert(beep == Beep::LimitReached);
        // Beside a visible overlay the tabs ask (time up) and a caption tells (break over),
        // whatever the notification setting; a Windows notification stands in only while the
        // overlay is hidden.
        let overlay_shown = self.widget_visible && !self.dock_suppressed;
        match beep {
            Beep::LimitReached => self.show_time_up_prompt(),
            Beep::BreakOver => {
                if let Some(anchor) = self.prompt_anchor() {
                    let note = break_over_note(&self.settings);
                    prompt::show(self as *mut App, anchor, prompt::Kind::BreakOver, &note, ["", ""]);
                }
            }
            _ => {}
        }
        if !overlay_shown {
            if let Some(notice) = actions.notification {
                tray::notify(self.hidden, notice.title, &notice.body);
            }
        }
    }

    /// The pill on screen, if it is fully out: the prompt hangs from it.
    fn prompt_anchor(&mut self) -> Option<prompt::Anchor> {
        if !self.widget_visible || self.dock_suppressed || self.dragging {
            return None;
        }
        let now = self.clock_now();
        let s = widget::paint_scale(self.widget, self.settings.widget_size);
        let l = widget_layout(s);
        let edge = self.docked_edge();
        let (dx, dy) = match (edge, self.dock_frame) {
            (None, _) => (0.0, 0.0),
            (Some(_), Some(DockFrame { draw: DockDraw::Pill { dx, dy }, .. })) => {
                let out = self.dock.as_ref().is_some_and(|d| {
                    d.phase() == DockPhase::Expanded && !d.animating(now) && d.expansion(now) >= 1.0
                });
                if !out {
                    return None;
                }
                (dx, dy)
            }
            _ => return None,
        };
        let mut rc = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.widget, &mut rc);
        }
        let left = rc.left + (l.pill_x + dx).round() as i32;
        let top = rc.top + (l.pill_y + dy).round() as i32;
        let pill = windows::Win32::Foundation::RECT {
            left,
            top,
            right: left + l.pill_w.round() as i32,
            bottom: top + l.pill_h.round() as i32,
        };
        let (wl, wt, wr, wb) = docking::work_area_near((pill.left + pill.right) / 2, (pill.top + pill.bottom) / 2)?;
        Some(prompt::Anchor {
            overlay: self.widget,
            pill,
            work: windows::Win32::Foundation::RECT { left: wl, top: wt, right: wr, bottom: wb },
            scale: s,
            dpi: widget::scale(self.widget),
            radius: l.radius,
            dock: edge,
            dark: self.is_dark(),
        })
    }

    /// Ask "break or extend?" under the pill, or wait until the pill is out on screen.
    fn show_time_up_prompt(&mut self) {
        self.hold_dock(true);
        let Some(anchor) = self.prompt_anchor() else {
            self.prompt = Prompt::Waiting;
            return;
        };
        let p = time_up_prompt(&self.settings, self.engine.snapshot().sitting_elapsed);
        prompt::show(self as *mut App, anchor, prompt::Kind::TimeUp, &p.caption, [&p.break_label, &p.extend_label]);
        self.prompt = Prompt::Shown;
    }

    /// A docked overlay stays out while its question waits.
    fn hold_dock(&mut self, on: bool) {
        let now = self.clock_now();
        let changed = self.dock.as_mut().is_some_and(|d| {
            let changed = d.held() != on;
            d.hold(now, on);
            changed
        });
        if changed {
            self.schedule_dock();
        }
    }

    /// Keep the time-up tabs in step: they wait while the pill is hidden, come back with it,
    /// follow it, keep the sitting time current and go once the session moves on.
    pub fn sync_prompt(&mut self) {
        let snap = self.engine.snapshot();
        if matches!(snap.state, VisibleState::Sitting | VisibleState::Break) {
            if self.prompt != Prompt::Idle {
                prompt::hide_kind(prompt::Kind::TimeUp);
                self.prompt = Prompt::Idle;
                self.hold_dock(false);
            }
            return;
        }
        match self.prompt {
            Prompt::Idle => {}
            Prompt::Shown => match self.prompt_anchor() {
                None => {
                    prompt::hide_kind(prompt::Kind::TimeUp);
                    self.prompt = Prompt::Waiting;
                }
                Some(anchor) => {
                    let caption = (snap.state == VisibleState::Overdue)
                        .then(|| time_up_prompt(&self.settings, snap.sitting_elapsed).caption);
                    prompt::update(anchor, caption.as_ref());
                }
            },
            Prompt::Waiting => {
                if snap.state == VisibleState::Overdue && self.prompt_anchor().is_some() {
                    self.show_time_up_prompt();
                }
            }
        }
    }

    /// The overdue time on the overlay was clicked: ask again.
    pub fn ask_time_up(&mut self) {
        if self.engine.snapshot().state == VisibleState::Overdue {
            self.show_time_up_prompt();
        }
    }

    /// Archive yesterday's total before the engine rolls `today` over at midnight.
    fn archive_if_new_day(&mut self) {
        let today = local_date().to_ymd_string();
        if self.settings.today_date != today {
            let date = std::mem::replace(&mut self.settings.today_date, today);
            let total = self.engine.snapshot().today_sitting.as_secs();
            self.settings.archive_day(&date, total);
        }
    }

    pub fn sync_engine_clock(&mut self) {
        self.archive_if_new_day();
        let now = self.clock_now();
        let input = self.current_input();
        let result = self.engine.tick(now, local_date(), input);
        self.handle_beep(result.beep);
    }

    pub fn apply_engine_command(&mut self, command: Command) {
        self.archive_if_new_day();
        let now = self.clock_now();
        let input = self.current_input();
        let result = self.engine.apply_at(command, now, local_date(), input);
        testhook::log(&format!("command {command:?}"));
        self.phone.on_command(command);
        match command {
            Command::Reset | Command::Snooze(_) | Command::Extend | Command::StartBreak => {
                self.stop_alert_sound();
                prompt::hide();
                self.prompt = Prompt::Idle;
                self.hold_dock(false);
            }
            Command::TogglePause => self.handle_beep(result.beep),
        }
        self.persist_today();
        self.update_tray();
        self.repaint();
        self.schedule_tick(&self.engine.snapshot());
    }

    pub fn on_suspend(&mut self) {
        self.sync_engine_clock();
        self.suspended = true;
        self.sync_engine_clock();
        self.persist_today();
        testhook::log("power suspend");
    }

    pub fn on_resume(&mut self) {
        self.sync_engine_clock();
        self.suspended = false;
        testhook::log("power resume");
        self.tick();
        self.force_repaint();
    }

    pub fn tick(&mut self) {
        self.archive_if_new_day();
        let now = self.clock_now();
        let input = self.current_input();
        let result = self.engine.tick(now, local_date(), input);
        if testhook::enabled() {
            testhook::log(&format!(
                "tick sitting_ms={}",
                result.snapshot.sitting_elapsed.as_millis()
            ));
        }
        self.handle_beep(result.beep);
        self.sync_prompt();
        self.phone.tick(
            &status_line(result.snapshot),
            &format_today(result.snapshot.today_sitting),
            result.snapshot.state,
            result.snapshot.sitting_elapsed,
        );
        if now.saturating_sub(self.last_save_at) >= SAVE_EVERY {
            self.last_save_at = now;
            if self.disk_stale(now) {
                self.persist_today();
            }
        }
        if !self.tray_added {
            self.add_tray();
        }
        self.update_tray();
        if self.docked_edge().is_some() {
            self.dock_tick();
        } else {
            self.repaint();
        }
        self.schedule_tick(&result.snapshot);
    }

    /// Re-arm the update timer so the next update lands just after the face's next second.
    fn schedule_tick(&self, snapshot: &unseat::Snapshot) {
        let delay = next_tick_delay(snapshot).as_millis() as u32;
        unsafe {
            let _ = SetTimer(self.hidden, TICK_ID, delay, None);
        }
    }

    pub fn add_tray(&mut self) {
        if testhook::hidden() {
            // Background test run: no icon, but don't keep retrying either.
            self.tray_added = true;
            testhook::log("tray skipped (hidden test run)");
            return;
        }
        // After an Explorer restart the old icon may or may not survive; start clean. A first
        // add (or a retry after a failed one) has nothing to remove. Each call goes to Explorer.
        if self.tray_added {
            tray::remove(self.hidden);
        }
        let tip = self.tray_tip();
        self.tray_added = tray::add(self.hidden, &tip);
        self.last_tip = if self.tray_added { tip } else { String::new() };
        testhook::log(&format!("tray added={}", self.tray_added));
    }

    fn tray_tip(&self) -> String {
        let snap = self.engine.snapshot();
        format!("Unseat · {}\n{}", status_line(snap), format_today(snap.today_sitting))
    }

    /// Update the tooltip only when its text changes; it is a cross-process call to Explorer.
    fn update_tray(&mut self) {
        if !self.tray_added || testhook::hidden() {
            return;
        }
        let tip = self.tray_tip();
        if tip != self.last_tip {
            tray::set_tip(self.hidden, &tip);
            self.last_tip = tip;
        }
    }

    pub fn show_flyout(&self) {
        let snap = self.engine.snapshot();
        flyout::show(
            self.hidden,
            &status_line(snap),
            &format_today(snap.today_sitting),
            self.is_dark(),
        );
    }

    pub fn sync_hotkeys(&mut self) {
        if self.settings.hotkeys_enabled && !self.hotkeys_on {
            hotkeys::register(self.hidden);
            self.hotkeys_on = true;
        } else if !self.settings.hotkeys_enabled && self.hotkeys_on {
            hotkeys::unregister(self.hidden);
            self.hotkeys_on = false;
        }
    }

    fn on_hotkey(&mut self, action: hotkeys::Action) {
        testhook::log(&format!("hotkey {action:?}"));
        match action {
            hotkeys::Action::TogglePause => self.apply_engine_command(Command::TogglePause),
            hotkeys::Action::Reset => self.apply_engine_command(Command::Reset),
            hotkeys::Action::Snooze => self.snooze_default(),
            hotkeys::Action::Break => self.toggle_break(),
            hotkeys::Action::ToggleOverlay => self.toggle_widget(),
        }
    }

    pub fn toggle_break(&mut self) {
        if self.engine.on_break() {
            self.apply_engine_command(Command::Reset);
        } else {
            self.apply_engine_command(Command::StartBreak);
        }
    }

    /// One line for `scripts/test_app.ps1`: engine state plus the overlay's window state.
    pub fn log_state(&mut self) {
        let snap = self.engine.snapshot();
        let mut rc = windows::Win32::Foundation::RECT::default();
        let ex = unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.widget, &mut rc);
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                self.widget,
                windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE,
            )
        };
        let topmost = ex as u32 & windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST.0 != 0;
        testhook::log(&format!(
            "state {:?} sitting={} remaining={} break={} today={} running={} visible={} topmost={} rect={},{},{},{} tray={} hotkeys={} dark={} dock={:?} dockphase={:?} dockrect={:?}",
            snap.state,
            snap.sitting_elapsed.as_secs(),
            snap.timer_remaining.as_secs(),
            snap.break_elapsed.as_secs(),
            snap.today_sitting.as_secs(),
            snap.running,
            self.widget_visible,
            topmost,
            rc.left,
            rc.top,
            rc.right,
            rc.bottom,
            self.tray_added,
            self.hotkeys_on,
            self.is_dark(),
            self.docked_edge(),
            self.dock.as_ref().map(|d| d.phase()),
            self.dock_frame.map(|f| f.rect),
        ));
    }

    pub fn persist_today(&mut self) {
        let snap = self.engine.snapshot();
        self.settings.today_date = local_date().to_ymd_string();
        self.settings.today_sitting_secs = snap.today_sitting.as_secs();
        self.settings.timer_checkpoint = Some(self.engine.checkpoint(wall_now_secs()));
        self.save_settings();
    }

    pub fn save_settings(&mut self) {
        if self.settings.save_to(&self.settings_path).is_ok() {
            self.on_disk = Some(self.settings.clone());
            self.last_write_at = self.clock_now();
        }
    }

    /// Whether the settings file no longer brings back what a restart should: a preference,
    /// the date or the history changed, or the timer moved in a way that restoring from the
    /// saved checkpoint would not reproduce. While only time passes it stays current (a
    /// restore adds the downtime itself), so it is not rewritten every minute.
    fn disk_stale(&self, now: Duration) -> bool {
        let Some(disk) = &self.on_disk else { return true };
        let Some(checkpoint) = disk.timer_checkpoint else { return true };
        now.saturating_sub(self.last_write_at) >= SAVE_AT_LEAST_EVERY
            || *disk != self.settings
            || !checkpoint_current(
                &self.engine,
                checkpoint,
                Duration::from_secs(disk.today_sitting_secs),
                wall_now_secs(),
                RESTORE_TOLERANCE,
            )
    }

    /// Show the size, theme and always-on-top choices on the overlay without saving them.
    pub fn preview_appearance(&mut self, size: WidgetSize, theme: ThemePreference, on_top: bool) {
        self.settings.widget_size = size;
        self.settings.theme = theme;
        self.settings.always_on_top = on_top;
        self.apply_topmost();
        self.force_repaint();
    }

    /// Adopt preferences saved from the settings window. Live state that changed while the
    /// window was open (position, today's total, the running timer, history) is kept.
    pub fn apply_settings(&mut self, next: Settings) {
        self.sync_engine_clock();
        if next.launch_with_windows != autostart::is_enabled() {
            autostart::apply(next.launch_with_windows);
        }
        let mut merged = next;
        merged.window_x = self.settings.window_x;
        merged.window_y = self.settings.window_y;
        merged.today_date = self.settings.today_date.clone();
        merged.today_sitting_secs = self.settings.today_sitting_secs;
        merged.timer_checkpoint = self.settings.timer_checkpoint;
        merged.history = std::mem::take(&mut self.settings.history);
        // The settings window doesn't edit the phone choices; the iPhone page may have changed
        // them while it was open.
        merged.phone_enabled = self.settings.phone_enabled;
        merged.phone_alerts = self.settings.phone_alerts;
        merged.clamp();
        self.settings = merged;
        self.engine
            .set_config(EngineConfig::from_settings(&self.settings));
        self.sync_engine_clock();
        self.sync_hotkeys();
        self.apply_topmost();
        self.persist_today();
        self.last_tip.clear();
        self.update_tray();
        self.force_repaint();
        self.sync_dock();
        self.force_repaint();
        testhook::log("settings saved");
    }

    pub fn apply_topmost(&self) {
        let after = if self.settings.always_on_top {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        unsafe {
            let _ = SetWindowPos(
                self.widget,
                after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    pub fn hide_widget(&mut self) {
        self.widget_visible = false;
        self.hover = false;
        self.persist_today();
        unsafe {
            let _ = ShowWindow(self.widget, SW_SHOWMINNOACTIVE);
        }
        self.sync_prompt();
    }

    pub fn show_widget(&mut self) {
        if self.dock_hidden() {
            self.dock_click();
        }
        self.widget_visible = true;
        unsafe {
            let _ = ShowWindow(self.widget, SW_SHOWNOACTIVATE);
            // Raise above everything, then drop back to normal level if not always on top.
            let _ = SetWindowPos(
                self.widget,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        self.apply_topmost();
        self.force_repaint();
        self.sync_prompt();
    }

    pub fn toggle_widget(&mut self) {
        if self.dock_hidden() && self.widget_visible {
            self.dock_click();
            return;
        }
        if self.widget_visible {
            self.hide_widget();
        } else {
            self.show_widget();
        }
    }

    pub fn force_repaint(&mut self) {
        self.last_key = None;
        self.repaint();
    }

    pub fn repaint(&mut self) {
        let snap = self.engine.snapshot();
        let scale = widget::paint_scale(self.widget, self.settings.widget_size);
        let dark = self.is_dark();
        let limit = Duration::from_secs(self.settings.sitting_limit_secs);
        let break_dur = Duration::from_secs(self.settings.break_duration_secs);
        let frame = self.current_dock_frame(scale);
        let full = unseat::widget_pixel_size(self.settings.timer_shape, scale);
        let fill = unseat::progress(snap.state, snap.sitting_elapsed, limit, snap.break_elapsed, break_dur);
        let pinned = !self.settings.dock_autohide;
        let key = timer_render_key(
            snap,
            self.settings.timer_display_mode,
            self.settings.timer_shape,
            self.settings.widget_size,
            self.hover,
            dark,
        )
        .at_scale(scale)
        .with_progress(fill, unseat::widget_layout(scale).pill_w)
        .with_dock(frame.map_or(0, |f| f.code() * 2 + u32::from(pinned)));
        let key = match frame.map(|f| f.draw) {
            Some(DockDraw::Tab) => key.for_tab(snap, false),
            Some(DockDraw::Peek) => key.for_tab(snap, true),
            _ => key,
        };
        let minimized = unsafe { IsIconic(self.widget).as_bool() };
        match frame {
            Some(frame) => self.place_docked(frame),
            None => {
                self.dock_frame = None;
                // A minimized window reports its iconic rectangle, so sizing it would move
                // it every second; `show_widget` repaints (and sizes) it when it is back.
                if self.widget_visible && !minimized {
                    widget::resize_to_shape(
                        self.widget,
                        self.settings.timer_shape,
                        self.settings.widget_size,
                    );
                }
            }
        }
        // Nobody sees an overlay that is hidden or held back over a fullscreen app: skip the
        // once-a-second frames until it is back (showing it forces a repaint). A forced
        // repaint (`last_key` cleared) still draws, so the window never shows a stale frame.
        let on_screen = self.widget_visible && !self.dock_suppressed && !minimized;
        if self.last_key == Some(key) || (!on_screen && self.last_key.is_some()) {
            return;
        }
        self.last_key = Some(key);
        let size = frame.map_or(full, |f| (f.rect.2 - f.rect.0, f.rect.3 - f.rect.1));
        paint::paint(
            self.d2d.as_ref(),
            self.widget,
            &paint::OverlayPaint {
                mode: self.settings.timer_display_mode,
                snap,
                dark,
                hover: self.hover,
                limit,
                break_dur,
                scale,
                size,
                dock: frame.map(|f| (f.edge, f.draw)),
                pinned,
            },
        );
    }

    pub fn snooze_default(&mut self) {
        self.snooze(self.settings.snooze_secs);
    }

    pub fn snooze(&mut self, secs: u64) {
        self.settings.snooze_secs = secs;
        self.settings.clamp();
        self.apply_engine_command(Command::Snooze(Duration::from_secs(
            self.settings.snooze_secs,
        )));
    }

    pub fn preview_sound(&mut self, kind: unseat::AlertSound, duration_secs: u64, volume: u8) {
        let duration_secs = duration_secs.min(10);
        sound::play_alert(kind, volume, duration_secs > 0);
        unsafe {
            let _ = KillTimer(self.hidden, SOUND_ID);
            if duration_secs > 0 {
                let _ = SetTimer(
                    self.hidden,
                    SOUND_ID,
                    duration_secs.saturating_mul(1000) as u32,
                    None,
                );
            }
        }
    }

    fn play_alert_sound(&mut self) {
        self.preview_sound(
            self.settings.alert_sound,
            self.settings.alert_duration_secs,
            self.settings.alert_volume,
        );
    }

    fn stop_alert_sound(&mut self) {
        sound::stop();
        unsafe {
            let _ = KillTimer(self.hidden, SOUND_ID);
        }
    }

    fn command(&mut self, id: u32) {
        match id {
            tray::ID_PAUSE => self.apply_engine_command(Command::TogglePause),
            tray::ID_RESET => self.apply_engine_command(Command::Reset),
            tray::ID_BREAK => self.toggle_break(),
            tray::ID_SNOOZE_5 => self.snooze(5 * 60),
            tray::ID_SNOOZE_10 => self.snooze(10 * 60),
            tray::ID_SNOOZE_15 => self.snooze(15 * 60),
            tray::ID_SNOOZE_30 => self.snooze(30 * 60),
            tray::ID_SNOOZE_CUSTOM => self.snooze_default(),
            tray::ID_EXTEND => self.apply_engine_command(Command::Extend),
            tray::ID_SETTINGS => settings_dlg::open(self as *mut App),
            tray::ID_HIDE => self.toggle_widget(),
            tray::ID_PHONE => self.phone.open_admin(),
            tray::ID_QUIT => unsafe {
                self.persist_today();
                PostQuitMessage(0);
            },
            _ => {}
        }
    }

    /// Mirror an engine reminder to paired iPhones (all reminders, or only while away).
    fn mirror_to_phone(&mut self, beep: Beep) {
        let input = self.current_input();
        let presence = unseat::companion::reminder::Presence {
            idle_for: input.last_input_age,
            locked: input.session_locked,
        };
        let sitting = self.engine.snapshot().sitting_elapsed;
        self.phone.on_beep(beep, &self.settings, sitting, presence);
    }

    /// The iPhone page turned reminders on/off or changed when they are sent.
    pub fn apply_phone_settings(&mut self, enabled: bool, alerts: PhoneAlerts) {
        self.settings.phone_enabled = enabled;
        self.settings.phone_alerts = alerts;
        self.save_settings();
        self.phone.sync(&self.settings);
        testhook::log(&format!("phone settings enabled={enabled} alerts={}", alerts.key()));
    }

    /// The system theme changed (WM_SETTINGCHANGE "ImmersiveColorSet").
    pub fn on_theme_change(&mut self) {
        let dark = theme::apps_use_dark();
        if dark != self.system_dark {
            self.system_dark = dark;
            self.force_repaint();
            settings_dlg::refresh();
        }
    }
}

fn settings_file() -> PathBuf {
    let appdata = std::env::var_os("APPDATA").unwrap_or_else(|| ".".into());
    PathBuf::from(appdata).join("Unseat").join("settings.json")
}

pub fn local_date() -> CivilDate {
    unsafe {
        let st = GetLocalTime();
        CivilDate {
            year: st.wYear as i32,
            month: st.wMonth as u8,
            day: st.wDay as u8,
        }
    }
}

fn wall_now_secs() -> u64 {
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        + testhook::clock_offset())
    .as_secs()
}

fn local_time_since_midnight() -> Duration {
    unsafe {
        let st = GetLocalTime();
        Duration::from_secs(st.wHour as u64 * 3600 + st.wMinute as u64 * 60 + st.wSecond as u64)
    }
}

fn register_hidden() -> windows::core::Result<()> {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(hidden_proc),
            lpszClassName: HIDDEN_CLASS,
            hInstance: GetModuleHandleW(None)?.into(),
            hIcon: crate::win::icon::big(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        Ok(())
    }
}

/// The message-only window owns the update timer, tray icon, hotkeys and session events.
/// It never receives broadcasts; those (theme, Explorer restart, power, shutdown) go to the
/// overlay window in `widget.rs`.
unsafe extern "system" fn hidden_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let cs = lp.0 as *const CREATESTRUCTW;
        if !cs.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
        }
        return LRESULT(0);
    }
    let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if app.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_TIMER => {
            if wp.0 == SOUND_ID {
                sound::stop();
                let _ = KillTimer(hwnd, SOUND_ID);
            } else if wp.0 == crate::win::docking::DOCK_ID {
                (*app).dock_tick();
            } else if wp.0 == TICK_ID {
                (*app).tick();
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            (*app).command(wp.0 as u32 & 0xFFFF);
            LRESULT(0)
        }
        WM_HOTKEY => {
            if let Some(action) = hotkeys::action(wp.0) {
                (*app).on_hotkey(action);
            }
            LRESULT(0)
        }
        tray::CALLBACK => {
            let tmsg = tray::lparam_msg(lp);
            if tray::is_popup_open(tmsg) {
                (*app).show_flyout();
            } else if tray::is_popup_close(tmsg) {
                flyout::hide();
            } else if tray::is_balloon_click(tmsg) {
                (*app).show_widget();
            } else if tray::is_context(tmsg) {
                flyout::hide();
                tray_panel::show(app, hwnd);
            } else if tray::is_select(tmsg) || tray::is_dblclk(tmsg) {
                let now = Instant::now();
                let dbl = tray::is_dblclk(tmsg)
                    || (*app)
                        .last_tray_select
                        .is_some_and(|t| now.saturating_duration_since(t).as_millis() < 400);
                if dbl {
                    settings_dlg::open(app);
                    (*app).last_tray_select = None;
                } else {
                    (*app).show_widget();
                    (*app).last_tray_select = Some(now);
                }
            }
            LRESULT(0)
        }
        phone::MSG_PHONE_SETTINGS => {
            let alerts = if wp.0 & 2 != 0 { PhoneAlerts::IdleOnly } else { PhoneAlerts::All };
            (*app).apply_phone_settings(wp.0 & 1 != 0, alerts);
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            (*app).sync_engine_clock();
            (*app).locked = session::lock_state_for_event((*app).locked, wp.0);
            testhook::log(&format!("session locked={}", (*app).locked));
            (*app).tick();
            (*app).force_repaint();
            LRESULT(0)
        }
        WM_DESTROY => {
            (*app).persist_today();
            (*app).stop_alert_sound();
            tray::remove(hwnd);
            session::unregister(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
