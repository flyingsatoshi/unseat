use crate::win::app::App;
use crate::win::paint::widget_size;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, GetWindowRect, LoadCursorW,
    MoveWindow, PostQuitMessage, RegisterClassW, SetCursor, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WindowFromPoint, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HTCLIENT,
    HWND_TOPMOST, IDC_ARROW, SC_MINIMIZE, SC_RESTORE, SWP_NOACTIVATE, SW_SHOWNOACTIVATE, WM_ACTIVATE,
    WM_CLOSE, WM_CREATE, WM_DESTROY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_RBUTTONUP, WM_SHOWWINDOW, WM_SYSCOMMAND, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;
use unseat::{
    hit_control, layout_scale, Command, Hit, TimerShape, WidgetSize, WIDGET_H,
    WIDGET_W,
};

pub const CLASS: windows::core::PCWSTR = w!("UnseatWidget");

pub fn register() -> windows::core::Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: crate::win::icon::big(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc);
        Ok(())
    }
}

pub fn create(app: *mut App, x: i32, y: i32) -> windows::core::Result<HWND> {
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_APPWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("Unseat"),
            WS_POPUP,
            x,
            y,
            WIDGET_W as i32,
            WIDGET_H as i32,
            None,
            None,
            GetModuleHandleW(None)?,
            Some(app as *const std::ffi::c_void),
        )?;
        crate::win::icon::apply(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Ok(hwnd)
    }
}

pub fn scale(hwnd: HWND) -> f32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    }
}

pub fn paint_scale(hwnd: HWND, size: WidgetSize) -> f32 {
    layout_scale(scale(hwnd), size)
}

pub fn clamp_to_work_area(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    unsafe {
        let pt = POINT { x, y };
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut info).as_bool() {
            let wr = info.rcWork;
            let nx = x.clamp(wr.left, (wr.right - w).max(wr.left));
            let ny = y.clamp(wr.top, (wr.bottom - h).max(wr.top));
            (nx, ny)
        } else {
            (x, y)
        }
    }
}

pub fn resize_to_shape(hwnd: HWND, shape: TimerShape, size: WidgetSize) {
    let s = paint_scale(hwnd, size);
    let (w, h) = widget_size(shape, s);
    unsafe {
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        if rc.right - rc.left == w && rc.bottom - rc.top == h {
            return;
        }
        let (x, y) = clamp_to_work_area(rc.left, rc.top, w, h);
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_MOUSEMOVE => {
            (*app).on_widget_mouse(hwnd, lp, true);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            (*app).on_widget_leave(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            (*app).on_widget_down(hwnd, lp);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            (*app).on_widget_up(hwnd);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            (*app).on_widget_right_click();
            LRESULT(0)
        }
        WM_SHOWWINDOW => {
            (*app).widget_visible = wp.0 != 0;
            if wp.0 == 0 {
                (*app).hover = false;
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if wp.0 as u32 & 0xFFFF != 0 {
                (*app).show_widget();
            }
            LRESULT(0)
        }
        WM_SYSCOMMAND => {
            let cmd = (wp.0 as u32) & 0xFFF0;
            if cmd == SC_RESTORE {
                (*app).show_widget();
                LRESULT(0)
            } else if cmd == SC_MINIMIZE {
                (*app).hide_widget();
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_CLOSE => {
            (*app).persist_today();
            crate::win::tray::remove((*app).hidden);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

impl App {
    pub fn on_widget_mouse(&mut self, hwnd: HWND, lp: LPARAM, track: bool) {
        if track {
            unsafe {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
        }
        let x = (lp.0 as i32) & 0xFFFF;
        let y = ((lp.0 as i32) >> 16) & 0xFFFF;
        let x = x as i16 as i32;
        let y = y as i16 as i32;
        if self.dragging {
            unsafe {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let nx = pt.x - self.drag_dx;
                let ny = pt.y - self.drag_dy;
                let s = paint_scale(hwnd, self.settings.widget_size);
                let (w, h) = widget_size(self.settings.timer_shape, s);
                let (nx, ny) = clamp_to_work_area(nx, ny, w, h);
                let _ = MoveWindow(hwnd, nx, ny, w, h, false);
            }
            return;
        }
        if !self.hover {
            self.hover = true;
            self.repaint();
        }
        let _ = (x, y);
        unsafe {
            let _ = SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
        }
    }

    pub fn on_widget_leave(&mut self, hwnd: HWND) {
        if self.dragging {
            return;
        }
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            if WindowFromPoint(pt) == hwnd {
                return;
            }
        }
        self.hover = false;
        self.repaint();
    }

    pub fn on_widget_down(&mut self, hwnd: HWND, lp: LPARAM) {
        let x = lp.0 as i16 as i32;
        let y = (lp.0 >> 16) as i16 as i32;
        let s = paint_scale(hwnd, self.settings.widget_size);
        if let Some(hit) = hit_control(
            self.settings.timer_shape,
            self.hover,
            self.engine.snapshot().state,
            s,
            x,
            y,
        ) {
            match hit {
                Hit::Pause => self.engine.apply(Command::TogglePause),
                Hit::Reset => self.engine.apply(Command::Reset),
                Hit::Snooze => self.snooze_default(),
                Hit::Close => {
                    self.hide_widget();
                    return;
                }
            }
            self.persist_today();
            self.repaint();
            return;
        }
        unsafe {
            let mut rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rc);
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            self.drag_dx = pt.x - rc.left;
            self.drag_dy = pt.y - rc.top;
            self.dragging = true;
            let _ = SetCapture(hwnd);
        }
    }

    pub fn on_widget_up(&mut self, hwnd: HWND) {
        if self.dragging {
            self.dragging = false;
            unsafe {
                let _ = ReleaseCapture();
                let mut rc = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rc);
                self.settings.window_x = rc.left;
                self.settings.window_y = rc.top;
            }
            self.save_settings();
        }
    }

    pub fn on_widget_right_click(&mut self) {
        crate::win::settings_dlg::open(self as *mut App);
    }
}
