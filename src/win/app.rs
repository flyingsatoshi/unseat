use crate::win::{flyout, idle, install, paint, session, settings_dlg, sound, theme, tray, widget};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unseat::{
    clamp_clock, restored_today_sitting, timer_render_key, Beep, CivilDate, Command, EngineConfig,
    Input, Settings, SittingEngine, TimerRenderKey, WIDGET_H, WIDGET_W,
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
    SetWindowLongPtrW, ShowWindow, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, HWND_MESSAGE,
    MSG, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETTINGCHANGE, WM_TIMER, WM_WTSSESSION_CHANGE,
    WNDCLASSW, WS_OVERLAPPED,
};

const HIDDEN_CLASS: windows::core::PCWSTR = w!("UnseatHidden");
const TICK_ID: usize = 1;
const SOUND_ID: usize = 2;
const SINGLETON: windows::core::PCWSTR = w!("Local\\UnseatSingleton");

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
    pub dark: bool,
    pub tray_added: bool,
    pub tray_retried: bool,
    pub d2d: Option<paint::D2d>,
    last_key: Option<TimerRenderKey>,
    last_save_at: Duration,
    last_tray_select: Option<Instant>,
    last_clock: Option<Duration>,
    pub widget_visible: bool,
    pub suspended: bool,
}

pub fn run() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        install::set_aumid();
        let mutex = CreateMutexW(None, true, SINGLETON)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(existing) = FindWindowW(widget::CLASS, None) {
                let _ = ShowWindow(
                    existing,
                    windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
                );
                let _ = SetForegroundWindow(existing);
            }
            if let Ok(host) = FindWindowW(HIDDEN_CLASS, None) {
                let _ = SetForegroundWindow(host);
            }
            let _ = ReleaseMutex(mutex);
            return Ok(());
        }

        widget::register()?;
        settings_dlg::register();
        flyout::register();
        register_hidden()?;

        let path = settings_file();
        let mut settings = Settings::load_from(&path);
        let today = local_date();
        let saved_date = CivilDate::parse_ymd(&settings.today_date).unwrap_or(today);
        let wall_now = wall_now_secs();
        let saved_today = Duration::from_secs(settings.today_sitting_secs);
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
            dark: theme::apps_use_dark(),
            tray_added: false,
            tray_retried: false,
            d2d: paint::D2d::new(),
            last_key: None,
            last_save_at: Duration::ZERO,
            last_tray_select: None,
            last_clock: None,
            widget_visible: true,
            suspended: false,
        });
        let ptr = Box::into_raw(app);

        let hidden = CreateWindowExW(
            Default::default(),
            HIDDEN_CLASS,
            w!("Unseat"),
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
        session::register(hidden);
        if SetTimer(hidden, TICK_ID, 1000, None) == 0 {
            let error = windows::core::Error::from_win32();
            let _ = DestroyWindow(hidden);
            let _ = Box::from_raw(ptr);
            let _ = ReleaseMutex(mutex);
            let _ = CloseHandle(mutex);
            return Err(error);
        }
        (*ptr).tray_added = tray::add(hidden);

        let (x, y) = widget::clamp_to_work_area(
            (*ptr).settings.window_x,
            (*ptr).settings.window_y,
            WIDGET_W as i32,
            WIDGET_H as i32,
        );
        (*ptr).widget = widget::create(ptr, x, y)?;
        (*ptr).repaint();
        (*ptr).tick();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        sound::stop();
        session::unregister(hidden);
        let _ = KillTimer(hidden, TICK_ID);
        let _ = KillTimer(hidden, SOUND_ID);
        flyout::shutdown();
        settings_dlg::shutdown();
        tray::remove(hidden);
        let _ = Box::from_raw(ptr);
        let _ = ReleaseMutex(mutex);
        let _ = CloseHandle(mutex);
        Ok(())
    }
}

impl App {
    fn clock_now(&mut self) -> Duration {
        let raw =
            Duration::from_millis(unsafe { GetTickCount64().saturating_sub(self.clock_origin_ms) });
        let now = clamp_clock(self.last_clock, raw);
        self.last_clock = Some(now);
        now
    }

    fn current_input(&self) -> Input {
        Input {
            last_input_age: idle::last_input_age(),
            session_locked: self.locked || self.suspended,
        }
    }

    fn handle_beep(&mut self, beep: Beep) {
        match beep {
            Beep::None => {}
            Beep::LimitReached | Beep::Progressive => self.play_alert_sound(),
        }
    }

    pub fn sync_engine_clock(&mut self) {
        let now = self.clock_now();
        let input = self.current_input();
        let result = self.engine.tick(now, local_date(), input);
        self.handle_beep(result.beep);
    }

    pub fn apply_engine_command(&mut self, command: Command) {
        let now = self.clock_now();
        let input = self.current_input();
        let result = self.engine.apply_at(command, now, local_date(), input);
        match command {
            Command::Reset | Command::Snooze(_) => self.stop_alert_sound(),
            Command::TogglePause => self.handle_beep(result.beep),
        }
    }

    pub fn on_suspend(&mut self) {
        self.sync_engine_clock();
        self.suspended = true;
        self.sync_engine_clock();
        self.persist_today();
    }

    pub fn on_resume(&mut self) {
        self.sync_engine_clock();
        self.suspended = false;
        self.sync_engine_clock();
        self.force_repaint();
    }

    pub fn tick(&mut self) {
        let now = self.clock_now();
        let input = self.current_input();
        let result = self.engine.tick(now, local_date(), input);
        self.handle_beep(result.beep);
        if now.saturating_sub(self.last_save_at) >= Duration::from_secs(60) {
            self.persist_today();
            self.last_save_at = now;
        }
        if !self.tray_added && !self.tray_retried {
            self.tray_added = tray::add(self.hidden);
            self.tray_retried = true;
        }
        tray::update(
            self.hidden,
            result.snapshot.today_sitting,
            result.snapshot.running,
        );
        self.repaint();
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

    pub fn hide_widget(&mut self) {
        self.widget_visible = false;
        self.hover = false;
        self.persist_today();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                self.widget,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE,
            );
        }
    }

    pub fn show_widget(&mut self) {
        self.widget_visible = true;
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                self.widget,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
            );
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                self.widget,
                windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
                0,
                0,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
        }
        self.force_repaint();
    }

    pub fn force_repaint(&mut self) {
        self.last_key = None;
        self.repaint();
    }

    pub fn repaint(&mut self) {
        let snap = self.engine.snapshot();
        let key = timer_render_key(
            snap,
            self.settings.timer_display_mode,
            self.settings.timer_shape,
            self.settings.widget_size,
            self.hover,
            self.dark,
        );
        if self.last_key == Some(key) {
            return;
        }
        self.last_key = Some(key);
        widget::resize_to_shape(
            self.widget,
            self.settings.timer_shape,
            self.settings.widget_size,
        );
        paint::paint(
            self.d2d.as_ref(),
            self.widget,
            self.settings.timer_shape,
            self.settings.timer_display_mode,
            snap,
            self.dark,
            self.hover,
            Duration::from_secs(self.settings.sitting_limit_secs),
            Duration::from_secs(self.settings.break_duration_secs),
            widget::paint_scale(self.widget, self.settings.widget_size),
        );
        if self.widget_visible {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                    self.widget,
                    windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
                );
            }
        }
    }

    pub fn snooze_default(&mut self) {
        self.snooze(self.settings.snooze_secs);
    }

    pub fn snooze(&mut self, secs: u64) {
        self.settings.snooze_secs = secs;
        self.settings.clamp();
        self.stop_alert_sound();
        self.apply_engine_command(Command::Snooze(Duration::from_secs(
            self.settings.snooze_secs,
        )));
    }

    pub fn preview_sound(&mut self, kind: unseat::AlertSound, duration_secs: u64) {
        let duration_secs = duration_secs.min(10);
        sound::play_alert(kind, duration_secs);
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
        self.preview_sound(self.settings.alert_sound, self.settings.alert_duration_secs);
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
            tray::ID_SNOOZE_5 => self.snooze(5 * 60),
            tray::ID_SNOOZE_10 => self.snooze(10 * 60),
            tray::ID_SNOOZE_15 => self.snooze(15 * 60),
            tray::ID_SNOOZE_30 => self.snooze(30 * 60),
            tray::ID_SNOOZE_CUSTOM => self.snooze_default(),
            tray::ID_SETTINGS => settings_dlg::open(self as *mut App),
            tray::ID_HIDE => {
                if self.widget_visible {
                    self.hide_widget();
                } else {
                    self.show_widget();
                }
            }
            tray::ID_QUIT => unsafe {
                PostQuitMessage(0);
            },
            _ => {}
        }
        self.persist_today();
        self.repaint();
    }
}

fn settings_file() -> PathBuf {
    let appdata = std::env::var_os("APPDATA").unwrap_or_else(|| ".".into());
    PathBuf::from(appdata).join("Unseat").join("settings.json")
}

fn local_date() -> CivilDate {
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
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
            } else if wp.0 == TICK_ID {
                (*app).tick();
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            (*app).command(wp.0 as u32 & 0xFFFF);
            LRESULT(0)
        }
        tray::CALLBACK => {
            let tmsg = tray::lparam_msg(lp);
            if tray::is_popup_open(tmsg) {
                tray::show_flyout(&*app, hwnd);
            } else if tray::is_popup_close(tmsg) {
                flyout::hide();
            } else if tray::is_context(tmsg) {
                tray::show_menu(&*app, hwnd);
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
        WM_WTSSESSION_CHANGE => {
            (*app).sync_engine_clock();
            (*app).locked = session::lock_state_for_event((*app).locked, wp.0);
            (*app).sync_engine_clock();
            (*app).force_repaint();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            (*app).dark = theme::apps_use_dark();
            (*app).last_key = None;
            (*app).repaint();
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
