use crate::win::{
    alert_card, tray_panel,
    autostart, flyout, hotkeys, idle, install, paint, phone, session, settings_dlg, sound, testhook,
    theme, tray, widget,
};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unseat::{
    alert_actions, clamp_clock, format_today, next_tick_delay, restored_today_sitting,
    status_line, timer_render_key, Beep, CivilDate, Command, EngineConfig, Input, PhoneAlerts, Settings,
    SittingEngine, ThemePreference, TimerRenderKey, WidgetSize, WIDGET_H, WIDGET_W,
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
    GetWindowLongPtrW, KillTimer, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA,
    HWND_MESSAGE, HWND_NOTOPMOST, HWND_TOPMOST, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_HOTKEY,
    WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_OVERLAPPED,
};

const HIDDEN_CLASS: windows::core::PCWSTR = w!("UnseatHidden");
const TICK_ID: usize = 1;
const SOUND_ID: usize = 2;
const SAVE_EVERY: Duration = Duration::from_secs(60);

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
    last_save_at: Duration,
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
}

pub fn run() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        install::set_aumid();
        let mutex = CreateMutexW(None, true, &testhook::singleton())?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // A second launch just brings the running overlay forward.
            if let Ok(existing) = FindWindowW(widget::CLASS, &testhook::title("Unseat")) {
                let _ = ShowWindow(existing, SW_SHOWNOACTIVATE);
                let _ = SetForegroundWindow(existing);
            }
            let _ = ReleaseMutex(mutex);
            let _ = CloseHandle(mutex);
            return Ok(());
        }

        widget::register()?;
        settings_dlg::register();
        flyout::register();
        tray_panel::register();
        alert_card::register();
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
        alert_card::shutdown();
        settings_dlg::shutdown();
        tray::remove(hidden);
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
        let actions = alert_actions(beep, &self.settings, self.engine.snapshot().sitting_elapsed);
        if actions.play_sound {
            self.play_alert_sound();
        }
        self.dock_alert(beep == Beep::LimitReached);
        if let Some(notice) = actions.notification {
            // While the overlay is on screen the alert card beside it replaces the toast.
            let on_screen = self.widget_visible && !self.dock_suppressed;
            let card = match beep {
                Beep::LimitReached => Some(alert_card::Card::TimeUp),
                Beep::BreakOver => Some(alert_card::Card::BreakOver),
                _ => None,
            };
            match card {
                Some(card) if on_screen => {
                    let body = match card {
                        alert_card::Card::TimeUp => format!(
                            "{} minutes in your seat. A {} minute break resets the clock.",
                            self.settings.sitting_limit_secs / 60,
                            self.settings.break_duration_secs / 60
                        ),
                        alert_card::Card::BreakOver => notice.body.clone(),
                    };
                    alert_card::show(self as *mut App, self.widget, card, notice.title, &body);
                }
                _ => tray::notify(self.hidden, notice.title, &notice.body),
            }
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
            Command::Reset | Command::Snooze(_) | Command::StartBreak => {
                self.stop_alert_sound();
                alert_card::hide();
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
        self.phone.update_view(&status_line(result.snapshot), &format_today(result.snapshot.today_sitting));
        if now.saturating_sub(self.last_save_at) >= SAVE_EVERY {
            self.persist_today();
            self.last_save_at = now;
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
        // After an Explorer restart the old icon may or may not survive; start clean.
        tray::remove(self.hidden);
        self.tray_added = tray::add(self.hidden);
        self.last_tip.clear();
        testhook::log(&format!("tray added={}", self.tray_added));
        self.update_tray();
    }

    /// Update the tooltip only when its text changes; it is a cross-process call to Explorer.
    fn update_tray(&mut self) {
        if !self.tray_added {
            return;
        }
        let snap = self.engine.snapshot();
        let tip = format!("Unseat · {}\n{}", status_line(snap), format_today(snap.today_sitting));
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

    pub fn save_settings(&self) {
        let _ = self.settings.save_to(&self.settings_path);
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
        match frame {
            Some(frame) => self.place_docked(frame),
            None => {
                self.dock_frame = None;
                widget::resize_to_shape(
                    self.widget,
                    self.settings.timer_shape,
                    self.settings.widget_size,
                );
            }
        }
        if self.last_key == Some(key) {
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
