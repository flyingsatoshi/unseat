//! In-app alert card shown beside the overlay when time is up (and briefly when a break
//! ends). It never takes focus; its buttons post the same commands as the tray. When the
//! overlay is hidden the host shows a Windows notification instead.

use crate::win::app::App;
use crate::win::gfx::{self, Family, Fonts};
use crate::win::tray;
use unseat::palette;
use windows::core::{w, Interface};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetClientRect, GetWindowLongPtrW,
    GetWindowRect, KillTimer, LoadCursorW, PostMessageW, RegisterClassW, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CREATESTRUCTW, GWLP_USERDATA, HWND_TOPMOST,
    IDC_HAND, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const CLASS: windows::core::PCWSTR = w!("UnseatAlertCard");
const WIN_W: f32 = 320.0;
const WIN_H: f32 = 146.0;
const DISMISS_ID: usize = 1;
const MA_NOACTIVATE: isize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Card {
    TimeUp,
    BreakOver,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Primary,
    Secondary,
    Close,
}

struct State {
    app: *mut App,
    card: Card,
    title: String,
    body: String,
    snooze_min: u64,
    hover: Option<Button>,
    tracking: bool,
    scale: f32,
    factory: Option<ID2D1Factory>,
    fonts: Option<Fonts>,
    rt: Option<ID2D1HwndRenderTarget>,
}

fn title() -> windows::core::HSTRING {
    crate::win::testhook::title("Unseat alert")
}

fn px(v: f32, s: f32) -> f32 {
    v * s
}

fn buttons(st: &State) -> Vec<(Button, D2D_RECT_F)> {
    let s = st.scale;
    let y = px(WIN_H - 18.0 - 38.0, s);
    let mut out = vec![(
        Button::Close,
        gfx::rect(px(WIN_W - 42.0, s), px(12.0, s), px(WIN_W - 12.0, s), px(42.0, s)),
    )];
    match st.card {
        Card::TimeUp => {
            out.push((Button::Primary, gfx::rect(px(20.0, s), y, px(152.0, s), y + px(38.0, s))));
            out.push((Button::Secondary, gfx::rect(px(160.0, s), y, px(WIN_W - 20.0, s), y + px(38.0, s))));
        }
        Card::BreakOver => {
            out.push((Button::Primary, gfx::rect(px(20.0, s), y, px(120.0, s), y + px(38.0, s))));
        }
    }
    out
}

pub fn register() {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: CLASS,
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    }
}

pub fn hide() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, &title()) {
            let _ = KillTimer(hwnd, DISMISS_ID);
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

/// Show `card` next to the overlay window `anchor`.
pub fn show(app: *mut App, anchor: HWND, card: Card, title_text: &str, body: &str) {
    unsafe {
        let hwnd = match FindWindowW(CLASS, &title()) {
            Ok(h) => h,
            Err(_) => match CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                &title(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                GetModuleHandleW(None).unwrap_or_default(),
                Some(app as *const std::ffi::c_void),
            ) {
                Ok(h) => {
                    let pref = DWMWCP_ROUND;
                    let _ = DwmSetWindowAttribute(
                        h,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        &pref as *const _ as *const core::ffi::c_void,
                        std::mem::size_of_val(&pref) as u32,
                    );
                    h
                }
                Err(_) => return,
            },
        };
        let st = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if st.is_null() {
            return;
        }
        (*st).card = card;
        (*st).title = title_text.to_string();
        (*st).body = body.to_string();
        (*st).snooze_min = (*app).settings.snooze_secs / 60;
        (*st).hover = None;
        (*st).scale = match GetDpiForWindow(anchor) {
            0 => 1.0,
            d => d as f32 / 96.0,
        };
        place(hwnd, anchor, (*st).scale);
        let secs = if card == Card::TimeUp { 60 } else { 10 };
        let _ = SetTimer(hwnd, DISMISS_ID, secs * 1000, None);
        let _ = InvalidateRect(hwnd, None, false);
        crate::win::testhook::log(&format!("card {card:?}"));
    }
}

/// Beside the overlay: below it if there is room, otherwise above, kept on its monitor.
unsafe fn place(hwnd: HWND, anchor: HWND, s: f32) {
    let mut a = RECT::default();
    let _ = GetWindowRect(anchor, &mut a);
    let mid = POINT {
        x: (a.left + a.right) / 2,
        y: (a.top + a.bottom) / 2,
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let work = if GetMonitorInfoW(MonitorFromPoint(mid, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
        info.rcWork
    } else {
        a
    };
    let w = (WIN_W * s).round() as i32;
    let h = (WIN_H * s).round() as i32;
    let gap = (8.0 * s).round() as i32;
    let mut y = a.bottom + gap;
    if y + h > work.bottom {
        y = a.top - gap - h;
    }
    let mut x = a.left;
    if x + w > work.right {
        x = a.right - w;
    }
    x = x.clamp(work.left + gap, (work.right - w - gap).max(work.left + gap));
    y = y.clamp(work.top + gap, (work.bottom - h - gap).max(work.top + gap));
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

unsafe fn press(st: &State, b: Button) {
    hide();
    if st.app.is_null() {
        return;
    }
    let cmd = match (st.card, b) {
        (Card::TimeUp, Button::Primary) => tray::ID_BREAK,
        (Card::TimeUp, Button::Secondary) => tray::ID_SNOOZE_CUSTOM,
        _ => return,
    };
    crate::win::testhook::log(&format!("card action {b:?}"));
    let _ = PostMessageW((*st.app).hidden, WM_COMMAND, WPARAM(cmd as usize), LPARAM(0));
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let cs = lp.0 as *const CREATESTRUCTW;
        let app = if cs.is_null() {
            std::ptr::null_mut()
        } else {
            (*cs).lpCreateParams as *mut App
        };
        let st = Box::new(State {
            app,
            card: Card::TimeUp,
            title: String::new(),
            body: String::new(),
            snooze_min: 10,
            hover: None,
            tracking: false,
            scale: 1.0,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok(),
            fonts: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok().map(Fonts::new),
            rt: None,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(st) as isize);
        return LRESULT(0);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let st = &mut *ptr;
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
        WM_TIMER => {
            hide();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !st.tracking {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                st.tracking = TrackMouseEvent(&mut tme).is_ok();
            }
            let (x, y) = ((lp.0 as i16) as f32, ((lp.0 >> 16) as i16) as f32);
            let h = buttons(st).into_iter().find(|(_, rc)| gfx::contains(*rc, x, y)).map(|(b, _)| b);
            if h != st.hover {
                st.hover = h;
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            st.tracking = false;
            st.hover = None;
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lp.0 as i16) as f32, ((lp.0 >> 16) as i16) as f32);
            if let Some((b, _)) = buttons(st).into_iter().find(|(_, rc)| gfx::contains(*rc, x, y)) {
                press(st, b);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            paint(hwnd, st);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn ensure_rt(hwnd: HWND, st: &mut State) -> Option<ID2D1RenderTarget> {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let size = D2D_SIZE_U {
        width: (rc.right - rc.left).max(1) as u32,
        height: (rc.bottom - rc.top).max(1) as u32,
    };
    if st.rt.is_none() {
        let factory = st.factory.as_ref()?;
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

unsafe fn paint(hwnd: HWND, st: &mut State) {
    let Some(rt) = ensure_rt(hwnd, st) else {
        return;
    };
    let Some(fonts) = st.fonts.as_ref() else {
        return;
    };
    let dark = st.app.is_null() || (*st.app).is_dark();
    let pal = palette(dark);
    let s = st.scale;
    let (bg, ink, soft) = match st.card {
        Card::TimeUp => (pal.alarm, pal.on_alarm, gfx::fade(pal.on_alarm, 0.85)),
        Card::BreakOver => (pal.window, pal.text, pal.text_secondary),
    };
    rt.BeginDraw();
    rt.Clear(Some(&gfx::color(bg)));
    gfx::text(&rt, fonts, Family::Ui, &st.title, gfx::rect(px(20.0, s), px(16.0, s), px(WIN_W - 48.0, s), px(46.0, s)), px(20.0, s), gfx::SEMIBOLD, gfx::LEADING, ink);
    gfx::text_wrapped(&rt, fonts, &st.body, gfx::rect(px(20.0, s), px(48.0, s), px(WIN_W - 20.0, s), px(86.0, s)), px(13.0, s), soft);
    for (b, rc) in buttons(st) {
        let hot = st.hover == Some(b);
        match b {
            Button::Close => {
                if hot {
                    gfx::fill_round(&rt, rc, px(8.0, s), gfx::fade(ink, 0.14));
                }
                let (cx, cy) = ((rc.left + rc.right) / 2.0, (rc.top + rc.bottom) / 2.0);
                let arm = px(5.0, s);
                gfx::line(&rt, (cx - arm, cy - arm), (cx + arm, cy + arm), soft, px(1.5, s));
                gfx::line(&rt, (cx + arm, cy - arm), (cx - arm, cy + arm), soft, px(1.5, s));
            }
            Button::Primary => {
                let (fill, text) = match st.card {
                    Card::TimeUp => (pal.on_alarm, pal.alarm),
                    Card::BreakOver => (pal.selected, pal.on_selected),
                };
                gfx::fill_round(&rt, rc, px(9.0, s), if hot { gfx::fade(fill, 0.9) } else { fill });
                let caption = if st.card == Card::TimeUp { "Take a break" } else { "OK" };
                gfx::text(&rt, fonts, Family::Ui, caption, rc, px(13.5, s), gfx::SEMIBOLD, gfx::CENTER, text);
            }
            Button::Secondary => {
                gfx::fill_round(&rt, rc, px(9.0, s), gfx::fade(ink, if hot { 0.26 } else { 0.18 }));
                let caption = format!("Snooze {} min", st.snooze_min);
                gfx::text(&rt, fonts, Family::Ui, &caption, rc, px(13.5, s), gfx::MEDIUM, gfx::CENTER, ink);
            }
        }
    }
    if rt.EndDraw(None, None).is_err() {
        st.rt = None;
    }
}
