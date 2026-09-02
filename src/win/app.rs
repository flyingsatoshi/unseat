use crate::win::{autostart, flyout, idle, paint, session, settings_dlg, sound, theme, tray, widget};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unseat::{
    Beep, CivilDate, Command, EngineConfig, Input, Settings, SittingEngine, Snapshot, WidgetSize,
    WIDGET_H, WIDGET_W,
};
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM, ERROR_ALREADY_EXISTS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, GetWindowLongPtrW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
    TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, HWND_MESSAGE, MSG, WM_COMMAND, WM_CREATE,
    WM_DESTROY, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_OVERLAPPED, WM_WTSSESSION_CHANGE,
};
use windows::core::w;

const HIDDEN_CLASS: windows::core::PCWSTR = w!("UnseatHidden");
const TICK_ID: usize = 1;
const SINGLETON: windows::core::PCWSTR = w!("Local\\UnseatSingleton");

pub struct App {
    pub engine: SittingEngine,
    pub settings: Settings,
    pub settings_path: PathBuf,
    pub origin: Instant,
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
    last_key: u64,
    ticks_since_save: u32,
    last_tray_select: Option<Instant>,
}

pub fn run() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mutex = CreateMutexW(None, true, SINGLETON)?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(existing) = FindWindowW(widget::CLASS, None) {
                let _ = SetForegroundWindow(existing);
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
        if settings.launch_with_windows {
            autostart::apply(true);
        }
        let today = local_date();
        if CivilDate::parse_ymd(&settings.today_date).is_some_and(|d| d != today) {
            settings.today_date = today.to_ymd_string();
            settings.today_sitting_secs = 0;
        }
        if settings.today_date.is_empty() {
            settings.today_date = today.to_ymd_string();
        }
        let today_sitting = Duration::from_secs(settings.today_sitting_secs);
        let engine = SittingEngine::new(EngineConfig::from_settings(&settings), today, today_sitting);

        let app = Box::new(App {
            engine,
            settings,
            settings_path: path,
            origin: Instant::now(),
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
            last_key: u64::MAX,
            ticks_since_save: 0,
            last_tray_select: None,
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
        let _ = SetTimer(hidden, TICK_ID, 1000, None);
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
        Ok(())
    }
}

impl App {
    pub fn tick(&mut self) {
        let now = self.origin.elapsed();
        let date = local_date();
        let input = Input {
            last_input_age: idle::last_input_age(),
            session_locked: self.locked,
        };
        let result = self.engine.tick(now, date, input);
        match result.beep {
            Beep::None => {}
            Beep::LimitReached | Beep::Progressive => sound::play(),
        }
        self.ticks_since_save += 1;
        if self.ticks_since_save >= 60 {
            self.persist_today();
            self.ticks_since_save = 0;
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
        self.save_settings();
    }

    pub fn save_settings(&self) {
        let _ = self.settings.save_to(&self.settings_path);
    }

    pub fn force_repaint(&mut self) {
        self.last_key = u64::MAX;
        self.repaint();
    }

    pub fn repaint(&mut self) {
        let snap = self.engine.snapshot();
        let key = paint_key(
            &snap,
            self.settings.timer_shape,
            self.settings.widget_size,
            self.hover,
            self.dark,
        );
        if key == self.last_key {
            return;
        }
        self.last_key = key;
        widget::resize_to_shape(
            self.widget,
            self.settings.timer_shape,
            self.settings.widget_size,
        );
        paint::paint(
            self.d2d.as_ref(),
            self.widget,
            self.settings.timer_shape,
            snap,
            self.dark,
            self.hover,
            Duration::from_secs(self.settings.sitting_limit_secs),
            Duration::from_secs(self.settings.break_duration_secs),
            widget::paint_scale(self.widget, self.settings.widget_size),
        );
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                self.widget,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
            );
        }
    }

    fn command(&mut self, id: u32) {
        match id {
            tray::ID_PAUSE => self.engine.apply(Command::TogglePause),
            tray::ID_RESET => self.engine.apply(Command::Reset),
            tray::ID_SETTINGS => settings_dlg::open(self as *mut App),
            tray::ID_QUIT => unsafe {
                PostQuitMessage(0);
            },
            _ => {}
        }
        self.persist_today();
        self.repaint();
    }
}

fn paint_key(
    snap: &Snapshot,
    shape: unseat::TimerShape,
    size: WidgetSize,
    hover: bool,
    dark: bool,
) -> u64 {
    let mut h = snap.sitting_elapsed.as_secs();
    h ^= snap.break_remaining.as_secs() << 8;
    h ^= (snap.state as u64) << 16;
    h ^= (shape as u64) << 20;
    h ^= (hover as u64) << 24;
    h ^= (dark as u64) << 25;
    h ^= snap.today_sitting.as_secs() << 26;
    h ^= (size.index() as u64) << 40;
    h
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

fn register_hidden() -> windows::core::Result<()> {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(hidden_proc),
            lpszClassName: HIDDEN_CLASS,
            hInstance: GetModuleHandleW(None)?.into(),
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
            (*app).tick();
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
                    || (*app).last_tray_select.is_some_and(|t| now.saturating_duration_since(t).as_millis() < 400);
                if dbl {
                    settings_dlg::open(app);
                    (*app).last_tray_select = None;
                } else {
                    (*app).last_tray_select = Some(now);
                }
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match lp.0 as usize {
                session::WTS_SESSION_LOCK => (*app).locked = true,
                session::WTS_SESSION_UNLOCK => (*app).locked = false,
                _ => {}
            }
            (*app).tick();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            (*app).dark = theme::apps_use_dark();
            (*app).last_key = u64::MAX;
            (*app).repaint();
            LRESULT(0)
        }
        WM_DESTROY => {
            (*app).persist_today();
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
