//! Tray panel: replaces the native right-click menu with a Signal-styled card showing the
//! countdown and the common actions. Actions post the same WM_COMMAND ids the menu used.
//! It closes on Esc, on an action, or when it loses focus, and repaints once a second only
//! while it is open.

use crate::win::app::App;
use crate::win::gfx::{self, Family, Fonts};
use crate::win::tray;
use unseat::{format_today_hm, palette, progress, timer_face_digits, Palette, Rgba, VisibleState};
use windows::core::{w, Interface};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
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
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_DOWN, VK_ESCAPE,
    VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, KillTimer, LoadCursorW, PostMessageW, RegisterClassW, SetForegroundWindow,
    SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, CREATESTRUCTW, GWLP_USERDATA,
    HWND_TOPMOST, IDC_ARROW, SWP_SHOWWINDOW, SW_HIDE, WA_INACTIVE, WM_ACTIVATE, WM_CHAR,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const CLASS: windows::core::PCWSTR = w!("UnseatTrayPanel");
const WIN_W: f32 = 320.0;
const PAD: f32 = 18.0;
const REFRESH_ID: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Pause,
    Break,
    Snooze,
    Reset,
    Settings,
    Phone,
    Overlay,
    Quit,
}

impl Item {
    const ALL: [Self; 8] = [
        Self::Pause,
        Self::Break,
        Self::Snooze,
        Self::Reset,
        Self::Settings,
        Self::Phone,
        Self::Overlay,
        Self::Quit,
    ];

    fn command(self) -> u32 {
        match self {
            Self::Pause => tray::ID_PAUSE,
            Self::Break => tray::ID_BREAK,
            Self::Snooze => tray::ID_SNOOZE_CUSTOM,
            Self::Reset => tray::ID_RESET,
            Self::Settings => tray::ID_SETTINGS,
            Self::Phone => tray::ID_PHONE,
            Self::Overlay => tray::ID_HIDE,
            Self::Quit => tray::ID_QUIT,
        }
    }
}

/// What the panel shows, captured from the app each repaint.
struct View {
    digits: String,
    caption: String,
    fill: f32,
    state: VisibleState,
    running: bool,
    on_break: bool,
    can_snooze: bool,
    snooze_min: u64,
    today: String,
    overlay_shown: bool,
    hotkeys: bool,
    dark: bool,
    phone_on: bool,
}

fn view(app: &App) -> View {
    let snap = app.engine.snapshot();
    let limit = std::time::Duration::from_secs(app.settings.sitting_limit_secs);
    let break_dur = std::time::Duration::from_secs(app.settings.break_duration_secs);
    let caption = match snap.state {
        VisibleState::Overdue => format!("over a {} min limit", limit.as_secs() / 60),
        VisibleState::Break => "of your break left".to_string(),
        VisibleState::Paused => "paused".to_string(),
        VisibleState::Away => "away · paused".to_string(),
        VisibleState::Sitting => format!("left of {} min", limit.as_secs() / 60),
    };
    View {
        digits: if snap.state == VisibleState::Overdue {
            format!("+{}", unseat::format_elapsed(snap.overdue_by))
        } else {
            timer_face_digits(app.settings.timer_display_mode, snap)
        },
        caption,
        fill: progress(snap.state, snap.sitting_elapsed, limit, snap.break_elapsed, break_dur),
        state: snap.state,
        running: snap.running,
        on_break: app.engine.on_break(),
        can_snooze: app.engine.can_snooze(),
        snooze_min: app.settings.snooze_secs / 60,
        today: format!("Today {}", format_today_hm(snap.today_sitting)),
        overlay_shown: app.widget_visible,
        hotkeys: app.settings.hotkeys_enabled,
        dark: app.is_dark(),
        phone_on: app.settings.phone_enabled,
    }
}

fn label(item: Item, v: &View) -> String {
    match item {
        Item::Pause => if v.running { "Pause" } else { "Resume" }.to_string(),
        Item::Break => if v.on_break { "End break" } else { "Take a break" }.to_string(),
        Item::Snooze => format!("Snooze {}m", v.snooze_min),
        Item::Reset => "Reset".to_string(),
        Item::Settings => "Settings".to_string(),
        Item::Phone => "iPhone reminders".to_string(),
        Item::Overlay => if v.overlay_shown { "Hide overlay" } else { "Show overlay" }.to_string(),
        Item::Quit => "Quit Unseat".to_string(),
    }
}

fn hint(item: Item, v: &View) -> String {
    match item {
        Item::Settings => v.today.clone(),
        Item::Phone => if v.phone_on { "On" } else { "Off" }.to_string(),
        Item::Overlay if v.hotkeys => "Ctrl+Alt+Shift+U".to_string(),
        _ => String::new(),
    }
}

fn enabled(item: Item, v: &View) -> bool {
    item != Item::Snooze || v.can_snooze
}

fn px(v: f32, s: f32) -> f32 {
    v * s
}

struct Lay {
    h: f32,
    bar: D2D_RECT_F,
    items: Vec<(Item, D2D_RECT_F)>,
}

/// Chip widths come from the measured labels so the row wraps cleanly.
unsafe fn layout(fonts: &Fonts, v: &View, s: f32) -> Lay {
    let w = px(WIN_W, s);
    let x0 = px(PAD, s);
    let x1 = w - px(PAD, s);
    let bar_top = px(78.0, s);
    let bar = gfx::rect(x0, bar_top, x1, bar_top + px(10.0, s));
    let mut items = Vec::new();
    let chip_h = px(36.0, s);
    let gap = px(8.0, s);
    let (mut x, mut y) = (x0, bar.bottom + px(16.0, s));
    for item in [Item::Pause, Item::Break, Item::Snooze, Item::Reset] {
        let cw = gfx::measure(fonts, &label(item, v), px(13.0, s), gfx::MEDIUM) + px(28.0, s);
        if x + cw > x1 && x > x0 {
            x = x0;
            y += chip_h + gap;
        }
        items.push((item, gfx::rect(x, y, x + cw, y + chip_h)));
        x += cw + gap;
    }
    y += chip_h + px(14.0, s);
    let row_h = px(36.0, s);
    for item in [Item::Settings, Item::Phone, Item::Overlay, Item::Quit] {
        items.push((item, gfx::rect(x0 - px(6.0, s), y, x1 + px(6.0, s), y + row_h)));
        y += row_h;
    }
    Lay {
        h: y + px(12.0, s),
        bar,
        items,
    }
}

struct State {
    app: *mut App,
    focus: Option<Item>,
    hover: Option<Item>,
    tracking: bool,
    scale: f32,
    gfx: Option<&'static gfx::Shared>,
    rt: Option<ID2D1HwndRenderTarget>,
}

impl State {
    fn fonts(&self) -> Option<&'static Fonts> {
        self.gfx.map(|g| &g.fonts)
    }
}

fn title() -> windows::core::HSTRING {
    crate::win::testhook::title("Unseat panel")
}

pub fn register() {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: CLASS,
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    }
}

pub fn hide() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, &title()) {
            let _ = KillTimer(hwnd, REFRESH_ID);
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

/// Open the panel above the tray icon (or toggle it closed if already open).
pub fn show(app: *mut App, owner: HWND) {
    unsafe {
        let hwnd = match FindWindowW(CLASS, &title()) {
            Ok(h) => h,
            Err(_) => match CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
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
        (*st).scale = dpi_scale(hwnd);
        (*st).focus = None;
        (*st).hover = None;
        let v = view(&*app);
        let dark: i32 = v.dark.into();
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as *const core::ffi::c_void,
            4,
        );
        let h = match (*st).fonts() {
            Some(f) => layout(f, &v, (*st).scale).h,
            None => 300.0 * (*st).scale,
        };
        position(hwnd, owner, (*st).scale, h);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetTimer(hwnd, REFRESH_ID, 1000, None);
        let _ = InvalidateRect(hwnd, None, false);
        crate::win::testhook::log("panel shown");
    }
}

fn dpi_scale(hwnd: HWND) -> f32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 1.0,
        d => d as f32 / 96.0,
    }
}

unsafe fn position(hwnd: HWND, owner: HWND, s: f32, h: f32) {
    let ident = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: owner,
        uID: 1,
        guidItem: Default::default(),
    };
    let anchor = match Shell_NotifyIconGetRect(&ident) {
        Ok(r) => r,
        Err(_) => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            RECT {
                left: pt.x,
                top: pt.y,
                right: pt.x,
                bottom: pt.y,
            }
        }
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let mid = POINT {
        x: (anchor.left + anchor.right) / 2,
        y: (anchor.top + anchor.bottom) / 2,
    };
    let work = if GetMonitorInfoW(MonitorFromPoint(mid, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 800,
        }
    };
    let w = (WIN_W * s).round() as i32;
    let h = h.round() as i32;
    let gap = (10.0 * s).round() as i32;
    let mut x = mid.x - w / 2;
    let mut y = anchor.top - h - gap;
    if y < work.top {
        y = anchor.bottom + gap;
    }
    x = x.clamp(work.left + gap, (work.right - w - gap).max(work.left + gap));
    y = y.clamp(work.top + gap, (work.bottom - h - gap).max(work.top + gap));
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_SHOWWINDOW);
}

unsafe fn activate(hwnd: HWND, st: &mut State, item: Item) {
    if st.app.is_null() {
        return;
    }
    let v = view(&*st.app);
    if !enabled(item, &v) {
        return;
    }
    crate::win::testhook::log(&format!("panel {item:?}"));
    hide();
    let _ = hwnd;
    let _ = PostMessageW(
        (*st.app).hidden,
        WM_COMMAND,
        WPARAM(item.command() as usize),
        LPARAM(0),
    );
}

unsafe fn item_at(st: &State, x: f32, y: f32) -> Option<Item> {
    let fonts = st.fonts()?;
    if st.app.is_null() {
        return None;
    }
    let v = view(&*st.app);
    layout(fonts, &v, st.scale)
        .items
        .into_iter()
        .find(|(_, rc)| gfx::contains(*rc, x, y))
        .map(|(i, _)| i)
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
            focus: None,
            hover: None,
            tracking: false,
            scale: dpi_scale(hwnd),
            gfx: gfx::shared(),
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
        WM_ACTIVATE => {
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                hide();
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = InvalidateRect(hwnd, None, false);
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
            let x = (lp.0 as i16) as f32;
            let y = ((lp.0 >> 16) as i16) as f32;
            let h = item_at(st, x, y);
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
            let x = (lp.0 as i16) as f32;
            let y = ((lp.0 >> 16) as i16) as f32;
            if let Some(item) = item_at(st, x, y) {
                activate(hwnd, st, item);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = VIRTUAL_KEY(wp.0 as u16);
            let order = Item::ALL;
            let pos = st.focus.and_then(|f| order.iter().position(|i| *i == f));
            let n = order.len();
            let back = vk == VK_UP
                || vk == VK_LEFT
                || (vk == VK_TAB && GetKeyState(VK_SHIFT.0 as i32) < 0);
            match vk {
                VK_ESCAPE => hide(),
                VK_TAB | VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT => {
                    let i = match pos {
                        None => if back { n - 1 } else { 0 },
                        Some(p) if back => (p + n - 1) % n,
                        Some(p) => (p + 1) % n,
                    };
                    st.focus = Some(order[i]);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                VK_RETURN | VK_SPACE => {
                    if let Some(item) = st.focus {
                        activate(hwnd, st, item);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CHAR => LRESULT(0),
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

fn bar_color(pal: &Palette, state: VisibleState) -> Rgba {
    match state {
        VisibleState::Overdue => pal.alarm,
        VisibleState::Break => pal.brk.tag,
        VisibleState::Away | VisibleState::Paused => pal.text_disabled,
        VisibleState::Sitting => pal.accent,
    }
}

unsafe fn paint(hwnd: HWND, st: &mut State) {
    if st.app.is_null() {
        return;
    }
    let Some(rt) = ensure_rt(hwnd, st) else {
        return;
    };
    let Some(fonts) = st.fonts() else {
        return;
    };
    let v = view(&*st.app);
    let pal = palette(v.dark);
    let s = st.scale;
    let l = layout(fonts, &v, s);
    let w = px(WIN_W, s);
    let x0 = px(PAD, s);
    rt.BeginDraw();
    rt.Clear(Some(&gfx::color(pal.window)));

    let ink = if v.state == VisibleState::Overdue { pal.alarm } else { pal.text };
    gfx::digits(
        &rt,
        fonts,
        &v.digits,
        gfx::rect(x0, px(14.0, s), w - px(PAD, s), px(66.0, s)),
        px(44.0, s),
        gfx::SEMIBOLD,
        gfx::LEADING,
        ink,
    );
    let digits_w = gfx::measure(fonts, &v.digits, px(44.0, s), gfx::SEMIBOLD);
    gfx::text(
        &rt,
        fonts,
        Family::Ui,
        &v.caption,
        gfx::rect(x0 + digits_w + px(10.0, s), px(38.0, s), w - px(PAD, s), px(62.0, s)),
        px(13.0, s),
        gfx::REGULAR,
        gfx::LEADING,
        pal.text_secondary,
    );
    gfx::fill_round(&rt, l.bar, px(5.0, s), pal.control);
    if v.fill > 0.0 {
        let fill_r = l.bar.left + (l.bar.right - l.bar.left) * v.fill;
        gfx::fill_round(
            &rt,
            gfx::rect(l.bar.left, l.bar.top, fill_r.max(l.bar.left + px(10.0, s)), l.bar.bottom),
            px(5.0, s),
            bar_color(pal, v.state),
        );
    }

    let sep_y = l
        .items
        .iter()
        .find(|(i, _)| *i == Item::Settings)
        .map_or(0.0, |(_, rc)| rc.top - px(7.0, s));
    gfx::fill_round(
        &rt,
        gfx::rect(x0, sep_y, w - px(PAD, s), sep_y + px(1.0, s)),
        0.0,
        pal.separator,
    );
    for (item, rc) in &l.items {
        let on = enabled(*item, &v);
        let hot = on && st.hover == Some(*item);
        let chip = matches!(item, Item::Pause | Item::Break | Item::Snooze | Item::Reset);
        let primary = *item == Item::Break;
        if chip {
            let fill = if primary {
                pal.selected
            } else if hot {
                pal.control_hover
            } else {
                pal.card
            };
            gfx::fill_round(&rt, *rc, px(9.0, s), fill);
            if primary && hot {
                gfx::fill_round(&rt, *rc, px(9.0, s), gfx::fade(pal.on_selected, 0.10));
            }
            let color = if primary {
                pal.on_selected
            } else if on {
                pal.text
            } else {
                pal.text_disabled
            };
            gfx::text(&rt, fonts, Family::Ui, &label(*item, &v), *rc, px(13.0, s), if primary { gfx::SEMIBOLD } else { gfx::MEDIUM }, gfx::CENTER, color);
        } else {
            if hot {
                gfx::fill_round(&rt, *rc, px(8.0, s), pal.card);
            }
            let inner = gfx::rect(rc.left + px(8.0, s), rc.top, rc.right - px(8.0, s), rc.bottom);
            gfx::text(&rt, fonts, Family::Ui, &label(*item, &v), inner, px(13.0, s), gfx::MEDIUM, gfx::LEADING, pal.text);
            gfx::text(&rt, fonts, Family::Ui, &hint(*item, &v), inner, px(12.0, s), gfx::REGULAR, gfx::TRAILING, pal.text_secondary);
        }
        if st.focus == Some(*item) {
            gfx::stroke_round(&rt, gfx::inflate(*rc, px(2.0, s)), px(10.0, s), pal.focus, px(2.0, s));
        }
    }
    gfx::stroke_round(
        &rt,
        gfx::rect(0.5, 0.5, w - 0.5, l.h - 0.5),
        px(8.0, s),
        pal.window_stroke,
        1.0,
    );
    if rt.EndDraw(None, None).is_err() {
        st.rt = None;
    }
}
