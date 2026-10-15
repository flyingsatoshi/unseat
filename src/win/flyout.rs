use crate::win::gfx::{self, Family};
use unseat::palette;
use windows::core::{w, Interface};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    ID2D1HwndRenderTarget, ID2D1RenderTarget, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetDC, GetMonitorInfoW, InvalidateRect, MonitorFromRect, ReleaseDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DrawIconEx, FindWindowW, GetClientRect,
    GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, SetWindowPos, ShowWindow, DI_NORMAL,
    GWLP_USERDATA, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WM_CREATE,
    WM_DESTROY, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const CLASS: windows::core::PCWSTR = w!("UnseatFlyout");
const WIN_W: i32 = 264;
const WIN_H: i32 = 76;

struct State {
    status: String,
    today: String,
    dark: bool,
    scale: f32,
    gfx: Option<&'static gfx::Shared>,
    rt: Option<ID2D1HwndRenderTarget>,
}

fn title() -> windows::core::HSTRING {
    crate::win::testhook::title("Unseat status")
}

pub fn register() {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: CLASS,
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    }
}

/// Show the hover card above the tray icon. `status` is the timer line, `today` the total.
pub fn show(owner: HWND, status: &str, today: &str, dark: bool) {
    unsafe {
        let hwnd = ensure_window();
        if hwnd.0.is_null() {
            return;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if !ptr.is_null() {
            (*ptr).status = status.to_string();
            (*ptr).today = today.to_string();
            (*ptr).dark = dark;
            (*ptr).scale = dpi_scale(hwnd);
        }
        let dark_attr: i32 = dark.into();
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_attr as *const _ as *const core::ffi::c_void,
            4,
        );
        position(hwnd, owner);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(hwnd, None, false);
    }
}

pub fn hide() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, &title()) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn shutdown() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, &title()) {
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn ensure_window() -> HWND {
    unsafe {
        if let Ok(existing) = FindWindowW(CLASS, &title()) {
            return existing;
        }
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            &title(),
            WS_POPUP,
            0,
            0,
            WIN_W,
            WIN_H,
            None,
            None,
            GetModuleHandleW(None).unwrap_or_default(),
            None,
        ) else {
            return HWND::default();
        };
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
        hwnd
    }
}

fn dpi_scale(hwnd: HWND) -> f32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 1.0,
        d => d as f32 / 96.0,
    }
}

fn position(hwnd: HWND, owner: HWND) {
    unsafe {
        let ident = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: owner,
            uID: 1,
            guidItem: Default::default(),
        };
        let icon = Shell_NotifyIconGetRect(&ident).unwrap_or_default();
        // Use the monitor that holds the tray icon, not wherever the flyout last was.
        let mon = MonitorFromRect(&icon, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(mon, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        };
        let scale = dpi_scale(hwnd);
        let fw = (WIN_W as f32 * scale).round() as i32;
        let fh = (WIN_H as f32 * scale).round() as i32;
        let gap = (8.0 * scale).round() as i32;
        let icon_cx = (icon.left + icon.right) / 2;
        let mut x = icon_cx - fw / 2;
        let mut y = icon.top - fh - gap;
        if y < work.top {
            y = icon.bottom + gap;
        }
        x = x.clamp(work.left + gap, (work.right - fw - gap).max(work.left + gap));
        y = y.clamp(work.top + gap, (work.bottom - fh - gap).max(work.top + gap));
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, fw, fh, SWP_NOACTIVATE);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let state = Box::new(State {
            status: String::new(),
            today: String::new(),
            dark: true,
            scale: dpi_scale(hwnd),
            gfx: gfx::shared(),
            rt: None,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        return LRESULT(0);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            if !ptr.is_null() {
                paint(hwnd, &mut *ptr);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            if !ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn paint(hwnd: HWND, st: &mut State) {
    let Some(rt) = ensure_rt(hwnd, st) else {
        return;
    };
    let Some(fonts) = st.gfx.map(|g| &g.fonts) else {
        return;
    };
    let pal = palette(st.dark);
    let s = st.scale;
    let w = WIN_W as f32 * s;
    let h = WIN_H as f32 * s;
    rt.BeginDraw();
    rt.Clear(Some(&gfx::color(pal.window)));
    let pad = 16.0 * s;
    let mark = 36.0 * s;
    let tx = pad + mark + 12.0 * s;
    gfx::text(
        &rt,
        fonts,
        Family::Ui,
        &st.status,
        gfx::rect(tx, 13.0 * s, w - pad, 39.0 * s),
        15.0 * s,
        gfx::SEMIBOLD,
        gfx::LEADING,
        pal.text,
    );
    gfx::text(
        &rt,
        fonts,
        Family::Ui,
        &st.today,
        gfx::rect(tx, 39.0 * s, w - pad, h - 13.0 * s),
        12.0 * s,
        gfx::REGULAR,
        gfx::LEADING,
        pal.text_secondary,
    );
    if rt.EndDraw(None, None).is_err() {
        st.rt = None;
        return;
    }
    let hdc = GetDC(hwnd);
    let mark_i = mark.round() as i32;
    let _ = DrawIconEx(
        hdc,
        pad.round() as i32,
        ((h - mark) * 0.5).round() as i32,
        crate::win::icon::big(),
        mark_i,
        mark_i,
        0,
        None,
        DI_NORMAL,
    );
    ReleaseDC(hwnd, hdc);
}

unsafe fn ensure_rt(hwnd: HWND, st: &mut State) -> Option<ID2D1RenderTarget> {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let size = D2D_SIZE_U {
        width: (rc.right - rc.left).max(1) as u32,
        height: (rc.bottom - rc.top).max(1) as u32,
    };
    if st.rt.is_none() {
        let factory = &st.gfx?.factory;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: size,
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        st.rt = factory.CreateHwndRenderTarget(&props, &hwnd_props).ok();
    } else if let Some(rt) = st.rt.as_ref() {
        let _ = rt.Resize(&size);
    }
    st.rt.as_ref().and_then(|rt| rt.cast().ok())
}
