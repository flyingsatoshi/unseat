use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetDC, GetMonitorInfoW, InvalidateRect, MonitorFromWindow, ReleaseDC,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DrawIconEx, FindWindowW, GetClientRect, GetSystemMetrics,
    GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, SetWindowPos, ShowWindow, DI_NORMAL,
    GWLP_USERDATA, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{w, Interface};

pub const CLASS: windows::core::PCWSTR = w!("UnseatFlyout");
const WIN_W: i32 = 248;
const WIN_H: i32 = 76;

struct State {
    subtitle: String,
    scale: f32,
    factory: Option<ID2D1Factory>,
    dwrite: Option<IDWriteFactory>,
    rt: Option<ID2D1HwndRenderTarget>,
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

pub fn show(owner: HWND, subtitle: &str) {
    unsafe {
        let hwnd = ensure_window();
        if hwnd.0.is_null() {
            return;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if !ptr.is_null() {
            (*ptr).subtitle = subtitle.to_string();
            (*ptr).scale = dpi_scale(hwnd);
        }
        position(hwnd, owner);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = InvalidateRect(hwnd, None, false);
    }
}

pub fn hide() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, None) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn ensure_window() -> HWND {
    unsafe {
        if let Ok(existing) = FindWindowW(CLASS, None) {
            return existing;
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!(""),
            WS_POPUP,
            0,
            0,
            WIN_W,
            WIN_H,
            None,
            None,
            GetModuleHandleW(None).unwrap_or_default(),
            None,
        );
        let Ok(hwnd) = hwnd else {
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
    unsafe {
        let d = GetDpiForWindow(hwnd);
        if d == 0 {
            1.0
        } else {
            d as f32 / 96.0
        }
    }
}

fn position(hwnd: HWND, owner: HWND) {
    unsafe {
        let scale = dpi_scale(hwnd);
        let fw = (WIN_W as f32 * scale).round() as i32;
        let fh = (WIN_H as f32 * scale).round() as i32;
        let ident = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: owner,
            uID: 1,
            guidItem: Default::default(),
        };
        let icon = Shell_NotifyIconGetRect(&ident).unwrap_or_default();
        let mut work = RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        };
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut info).as_bool() {
            work = info.rcWork;
        }
        let icon_cx = (icon.left + icon.right) / 2;
        let mut x = icon_cx - fw / 2;
        let mut y = icon.top - fh - (8.0 * scale).round() as i32;
        if y < work.top {
            y = icon.bottom + (8.0 * scale).round() as i32;
        }
        x = x.clamp(work.left + 8, (work.right - fw - 8).max(work.left + 8));
        y = y.clamp(work.top + 8, (work.bottom - fh - 8).max(work.top + 8));
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, fw, fh, SWP_NOACTIVATE);
    }
}

fn rgb(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

fn rct(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: l,
        top: t,
        right: r,
        bottom: b,
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == windows::Win32::UI::WindowsAndMessaging::WM_CREATE {
        let state = Box::new(State {
            subtitle: String::new(),
            scale: dpi_scale(hwnd),
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok(),
            dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok(),
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
                paint(hwnd, ptr);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            if !ptr.is_null() {
                let _ = Box::from_raw(ptr);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn paint(hwnd: HWND, st: *mut State) {
    let Some(rt) = ensure_rt(hwnd, st) else {
        return;
    };
    let Some(dwrite) = (*st).dwrite.as_ref() else {
        return;
    };
    let s = (*st).scale;
    let w = WIN_W as f32 * s;
    let h = WIN_H as f32 * s;
    rt.BeginDraw();
    rt.Clear(Some(&rgb(0x1C, 0x1C, 0x1E, 1.0)));

    let pad = 14.0 * s;
    let mark = 32.0 * s;
    let tx = pad + mark + 12.0 * s;
    text(
        &rt,
        dwrite,
        "Unseat",
        rct(tx, 14.0 * s, w - pad, 40.0 * s),
        16.0 * s,
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        rgb(0xF2, 0xF2, 0xF7, 1.0),
    );
    text(
        &rt,
        dwrite,
        &(*st).subtitle,
        rct(tx, 40.0 * s, w - pad, h - 14.0 * s),
        12.0 * s,
        DWRITE_FONT_WEIGHT_MEDIUM,
        rgb(0x8E, 0x8E, 0x93, 1.0),
    );
    let _ = rt.EndDraw(None, None);
    let hdc = GetDC(hwnd);
    let mark_i = mark.round() as i32;
    let pad_i = pad.round() as i32;
    let mark_y = ((h - mark) * 0.5).round() as i32;
    let _ = DrawIconEx(
        hdc,
        pad_i,
        mark_y,
        crate::win::icon::small(),
        mark_i,
        mark_i,
        0,
        None,
        DI_NORMAL,
    );
    ReleaseDC(hwnd, hdc);
}

unsafe fn ensure_rt(hwnd: HWND, st: *mut State) -> Option<ID2D1RenderTarget> {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let w = (rc.right - rc.left).max(1) as u32;
    let h = (rc.bottom - rc.top).max(1) as u32;
    if (*st).rt.is_none() {
        let factory = (*st).factory.as_ref()?;
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
            pixelSize: D2D_SIZE_U { width: w, height: h },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        (*st).rt = factory.CreateHwndRenderTarget(&props, &hwnd_props).ok();
    } else if let Some(rt) = (*st).rt.as_ref() {
        let _ = rt.Resize(&D2D_SIZE_U { width: w, height: h });
    }
    (*st).rt.as_ref().and_then(|rt| rt.cast().ok())
}

unsafe fn text(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    s: &str,
    rc: D2D_RECT_F,
    size: f32,
    weight: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
    color: D2D1_COLOR_F,
) {
    let Ok(format) = dwrite.CreateTextFormat(
        w!("Segoe UI Variable Display"),
        None,
        weight,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        size,
        w!("en-us"),
    ) else {
        return;
    };
    let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        rt.DrawText(
            &wide,
            &format,
            &rc,
            &brush,
            Default::default(),
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
    let _ = format;
}
