//! The quiet question under the overlay when time is up: two small tabs, "Break 10m" and
//! "+40m", hang from the pill's lower edge (their tops tucked under it) with a one-line
//! caption below. When a break ends the caption alone says so for a few seconds. It is a
//! layered window kept just below the overlay in z-order, so the pill covers the tucked tops
//! and its shadow falls on them. It slides out from behind the pill, never takes focus, and
//! its tabs post commands like the tray does. Docked at the bottom, or with no room below,
//! it hangs above the pill instead.

use crate::win::app::App;
use crate::win::gfx::{self, Family, Fonts};
use crate::win::paint::Surface;
use crate::win::{testhook, tray};
use std::time::Instant;
use unseat::{palette, Caption, DockEdge};
use windows::core::w;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE;
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetCursorPos, GetWindowLongPtrW,
    IsWindowVisible, KillTimer, LoadCursorW, PostMessageW, RegisterClassW,
    SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    CREATESTRUCTW, GWLP_USERDATA, GWL_EXSTYLE, HTCLIENT, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW,
    IDC_HAND, SPI_GETCLIENTAREAANIMATION, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_COMMAND, WM_CREATE,
    WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const CLASS: windows::core::PCWSTR = w!("UnseatPrompt");
const ANIM_ID: usize = 1;
const DISMISS_ID: usize = 2;
const ANIM_MS: f32 = 220.0;
const NOTE_MS: u32 = 10_000;
const MA_NOACTIVATE: isize = 3;

// Design sizes in logical px at the Medium overlay (the handoff's "Hanging tabs"); see
// `multiplier` for other sizes.
const TUCK: f32 = 10.0;
/// Tab height including the part tucked under the pill.
const TAB_H: f32 = 38.0;
const TAB_PAD: f32 = 12.0;
const TAB_GAP: f32 = 4.0;
/// Right edge of the tabs, in from the pill's right edge.
const TAB_INSET: f32 = 18.0;
const TAB_RADIUS: f32 = 10.0;
const TAB_TEXT: f32 = 12.0;
const CAP_GAP: f32 = 6.0;
const CAP_H: f32 = 22.0;
const CAP_PAD: f32 = 9.0;
const CAP_INSET: f32 = 12.0;
const CAP_TEXT: f32 = 11.0;
/// Room for antialiasing at the outer edges.
const EDGE: f32 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// "Break 10m" / "+40m" tabs and a caption, until answered.
    TimeUp,
    /// The caption alone, for a few seconds.
    BreakOver,
}

/// Where the overdue pill is, and how the prompt should look beside it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Anchor {
    pub overlay: HWND,
    /// The pill on screen, in pixels.
    pub pill: RECT,
    /// Work area of the pill's monitor.
    pub work: RECT,
    /// The overlay's scale (DPI times size) and the DPI scale alone.
    pub scale: f32,
    pub dpi: f32,
    /// Pill corner radius in pixels.
    pub radius: f32,
    pub dock: Option<DockEdge>,
    pub dark: bool,
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    above: bool,
    m: f32,
    /// Break and extend tabs (full height, including the tucked part); empty for a note.
    tabs: [D2D_RECT_F; 2],
    chip: D2D_RECT_F,
    /// The full caption fits under the pill; otherwise the short one is shown.
    full_caption: bool,
}

impl Layout {
    /// The part of a tab that shows below (or above) the pill.
    fn visible(&self, tab: D2D_RECT_F) -> D2D_RECT_F {
        let tuck = TUCK * self.m;
        if self.above {
            gfx::rect(tab.left, tab.top, tab.right, tab.bottom - tuck)
        } else {
            gfx::rect(tab.left, tab.top + tuck, tab.right, tab.bottom)
        }
    }
}

struct State {
    app: *mut App,
    kind: Kind,
    caption: Caption,
    labels: [String; 2],
    anchor: Option<Anchor>,
    layout: Option<Layout>,
    hover: Option<usize>,
    tracking: bool,
    shown_at: Option<Instant>,
    animate: bool,
    gfx: Option<&'static gfx::Shared>,
    surface: Option<Surface>,
}

impl State {
    fn fonts(&self) -> Option<&'static Fonts> {
        self.gfx.map(|g| &g.fonts)
    }
}

fn title() -> windows::core::HSTRING {
    testhook::title("Unseat prompt")
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

unsafe fn find() -> Option<(HWND, &'static mut State)> {
    let hwnd = FindWindowW(CLASS, &title()).ok()?;
    let st = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    (!st.is_null()).then(|| (hwnd, &mut *st))
}

/// What is up right now, if anything.
pub fn showing() -> Option<Kind> {
    unsafe { find().filter(|(h, _)| IsWindowVisible(*h).as_bool()).map(|(_, st)| st.kind) }
}

pub fn hide() {
    unsafe {
        if let Some((hwnd, st)) = find() {
            let _ = KillTimer(hwnd, ANIM_ID);
            let _ = KillTimer(hwnd, DISMISS_ID);
            let _ = ShowWindow(hwnd, SW_HIDE);
            st.hover = None;
            st.shown_at = None;
        }
    }
}

/// Hide only if `kind` is the one up.
pub fn hide_kind(kind: Kind) {
    if showing() == Some(kind) {
        hide();
    }
}

pub fn shutdown() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, &title()) {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// Show `kind` under the pill at `anchor`, sliding out from behind it.
pub fn show(app: *mut App, anchor: Anchor, kind: Kind, caption: &Caption, labels: [&str; 2]) {
    if testhook::hidden() {
        testhook::log(&format!("prompt skipped (hidden test run): {kind:?}"));
        return;
    }
    unsafe {
        let hwnd = match FindWindowW(CLASS, &title()) {
            Ok(h) => h,
            Err(_) => match CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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
                Ok(h) => h,
                Err(_) => return,
            },
        };
        let st = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if st.is_null() {
            return;
        }
        let st = &mut *st;
        let again = IsWindowVisible(hwnd).as_bool() && st.kind == kind;
        st.kind = kind;
        st.caption = caption.clone();
        st.labels = labels.map(str::to_string);
        st.anchor = Some(anchor);
        st.hover = None;
        st.layout = st.fonts().map(|f| layout(f, &anchor, kind, &st.labels, &st.caption));
        if !again {
            st.shown_at = Some(Instant::now());
            st.animate = animations_on();
        }
        render(hwnd, st);
        order_below(hwnd, anchor.overlay);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        if st.animate && !again {
            let _ = SetTimer(hwnd, ANIM_ID, 15, None);
        }
        if kind == Kind::BreakOver {
            let _ = SetTimer(hwnd, DISMISS_ID, NOTE_MS, None);
        } else {
            let _ = KillTimer(hwnd, DISMISS_ID);
        }
        testhook::log(&format!("prompt {kind:?}"));
        if let (Some(l), Kind::TimeUp) = (st.layout, kind) {
            let c = |rc: D2D_RECT_F| {
                let v = l.visible(rc);
                (((v.left + v.right) / 2.0) as i32, ((v.top + v.bottom) / 2.0) as i32)
            };
            let ((bx, by), (ex, ey)) = (c(l.tabs[0]), c(l.tabs[1]));
            testhook::log(&format!(
                "prompt tabs break={bx},{by} extend={ex},{ey} at={},{} size={}x{} above={}",
                l.x, l.y, l.w, l.h, l.above
            ));
        }
    }
}

/// Follow the pill (moved, rescaled, rethemed) and keep the caption current; no animation.
pub fn update(anchor: Anchor, caption: Option<&Caption>) {
    unsafe {
        let Some((hwnd, st)) = find() else { return };
        if !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let moved = st.anchor != Some(anchor);
        let caption = caption.unwrap_or(&st.caption).clone();
        if !moved && st.caption == caption {
            return;
        }
        st.caption = caption;
        st.anchor = Some(anchor);
        st.layout = st.fonts().map(|f| layout(f, &anchor, st.kind, &st.labels, &st.caption));
        render(hwnd, st);
        if moved {
            order_below(hwnd, anchor.overlay);
        }
    }
}

/// The last frame and its screen position, for test screenshots.
pub fn snapshot() -> Option<(i32, i32, i32, i32, Vec<u8>)> {
    unsafe {
        let (hwnd, st) = find()?;
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let (l, s) = (st.layout?, st.surface.as_ref()?);
        Some((l.x, l.y, s.w, s.h, s.pixels()?))
    }
}

/// Just below the overlay in z-order, and topmost exactly when the overlay is.
unsafe fn order_below(hwnd: HWND, overlay: HWND) {
    let topmost = GetWindowLongPtrW(overlay, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    let _ = SetWindowPos(hwnd, if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, flags);
    let _ = SetWindowPos(hwnd, overlay, 0, 0, 0, 0, flags);
}

/// Windows' "Show animations" setting.
fn animations_on() -> bool {
    let mut on = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut BOOL as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on.as_bool()
}

/// Scale for the design sizes: exact at the Medium overlay (12 px labels), never under 11 px
/// or over 16 px of text, and smaller still if the tabs would outgrow a tiny pill.
fn multiplier(scale: f32, dpi: f32) -> f32 {
    (8.0 * scale).clamp(11.0 * dpi, 16.0 * dpi) / TAB_TEXT
}

unsafe fn layout(fonts: &Fonts, a: &Anchor, kind: Kind, labels: &[String; 2], caption: &Caption) -> Layout {
    // Everything stays within the pill's width, clear of its left rounded corner.
    let room = (a.pill.right - a.pill.left) as f32 - a.radius - 2.0;
    let mut m = multiplier(a.scale, a.dpi);
    let mut tab_w = [0.0f32; 2];
    let mut inset = 0.0;
    if kind == Kind::TimeUp {
        for _ in 0..2 {
            for (w, label) in tab_w.iter_mut().zip(labels) {
                *w = (gfx::measure(fonts, label, TAB_TEXT * m, gfx::SEMIBOLD) + 2.0 * TAB_PAD * m).ceil();
            }
            // Keep the tucked tops clear of the pill's rounded corner on the right.
            inset = (TAB_INSET * m).max(a.radius + 2.0);
            let span = inset + tab_w[0] + TAB_GAP * m + tab_w[1];
            if span <= room {
                break;
            }
            m *= (room / span).max(0.6);
        }
    }
    let chip_w = |s: &str| (gfx::measure(fonts, s, CAP_TEXT * m, gfx::REGULAR) + 2.0 * CAP_PAD * m).ceil();
    let full_caption = CAP_INSET * m + chip_w(&caption.full) <= room;
    let cap_w = chip_w(if full_caption { &caption.full } else { &caption.short });
    let tabs_span = if kind == Kind::TimeUp { inset + tab_w[0] + TAB_GAP * m + tab_w[1] } else { 0.0 };
    let w = (tabs_span.max(CAP_INSET * m + cap_w) + EDGE * m).ceil();
    // Distances from the window edge that sits under the pill.
    let cap_from = if kind == Kind::TimeUp { TAB_H * m + CAP_GAP * m } else { TUCK * m + CAP_GAP * m };
    let cap_to = cap_from + CAP_H * m;
    let h = (cap_to + EDGE * m).ceil();
    let tuck = (TUCK * m).round() as i32;
    let (wi, hi) = (w as i32, h as i32);
    let fits_below = a.pill.bottom - tuck + hi <= a.work.bottom;
    let above = match a.dock {
        Some(DockEdge::Bottom) => true,
        Some(DockEdge::Top) => false,
        _ => !fits_below,
    };
    let y = if above { a.pill.top + tuck - hi } else { a.pill.bottom - tuck };
    let span = |from: f32, to: f32| if above { (h - to, h - from) } else { (from, to) };
    let (t0, t1) = span(0.0, TAB_H * m);
    let ext_r = w - inset;
    let ext = gfx::rect(ext_r - tab_w[1], t0, ext_r, t1);
    let brk_r = ext.left - TAB_GAP * m;
    let brk = gfx::rect(brk_r - tab_w[0], t0, brk_r, t1);
    let (c0, c1) = span(cap_from, cap_to);
    let chip_r = w - CAP_INSET * m;
    let empty = gfx::rect(0.0, 0.0, 0.0, 0.0);
    Layout {
        x: a.pill.right - wi,
        y,
        w: wi,
        h: hi,
        above,
        m,
        tabs: if kind == Kind::TimeUp { [brk, ext] } else { [empty, empty] },
        chip: gfx::rect(chip_r - cap_w, c0, chip_r, c1),
        full_caption,
    }
}

/// 0 to 1 over the slide, eased out.
fn progress(st: &State) -> f32 {
    if !st.animate {
        return 1.0;
    }
    let t = st.shown_at.map_or(1.0, |at| (at.elapsed().as_secs_f32() * 1000.0 / ANIM_MS).min(1.0));
    1.0 - (1.0 - t).powi(3)
}

fn shifted(rc: D2D_RECT_F, dy: f32) -> D2D_RECT_F {
    gfx::rect(rc.left, rc.top + dy, rc.right, rc.bottom + dy)
}

unsafe fn render(hwnd: HWND, st: &mut State) {
    let (Some(l), Some(a)) = (st.layout, st.anchor) else { return };
    if st.surface.as_ref().map_or(true, |s| s.w != l.w || s.h != l.h) {
        st.surface = Surface::new(st.gfx.map(|g| &g.factory), l.w, l.h);
    }
    let (Some(surface), Some(fonts)) = (st.surface.as_ref(), st.fonts()) else { return };
    let Some(rt) = surface.target() else { return };
    let (hover, kind) = (st.hover, st.kind);
    let c = palette(a.dark).prompt;
    let e = progress(st);
    // Slide out from behind the pill: start shifted toward it by the whole height.
    let dy = (1.0 - e) * l.h as f32 * if l.above { 1.0 } else { -1.0 };
    let m = l.m;
    rt.BeginDraw();
    rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    rt.Clear(Some(&gfx::color(unseat::Rgba(0, 0, 0, 0))));
    if kind == Kind::TimeUp {
        let r = TAB_RADIUS * m;
        let radii = if l.above { [r, r, 0.0, 0.0] } else { [0.0, 0.0, r, r] };
        for (i, tab) in l.tabs.iter().enumerate() {
            let hot = hover == Some(i);
            let (fill, ink) = if i == 0 {
                (if hot { c.primary_hover } else { c.primary }, c.on_primary)
            } else {
                (if hot { c.secondary_hover } else { c.secondary }, c.on_secondary)
            };
            let rc = shifted(*tab, dy);
            gfx::fill_corners(&rt, rc, radii, c.stroke);
            gfx::fill_corners(&rt, gfx::inflate(rc, -1.0), radii.map(|r| (r - 1.0).max(0.0)), fill);
            gfx::text(&rt, fonts, Family::Ui, &st.labels[i], shifted(l.visible(*tab), dy), TAB_TEXT * m, gfx::SEMIBOLD, gfx::CENTER, ink);
        }
    }
    let chip = shifted(l.chip, dy);
    let cr = (chip.bottom - chip.top) / 2.0;
    gfx::fill_round(&rt, chip, cr, c.caption_bg);
    gfx::stroke_round(&rt, gfx::inflate(chip, -0.5), cr, c.stroke, 1.0);
    let caption = if l.full_caption { &st.caption.full } else { &st.caption.short };
    gfx::text(&rt, fonts, Family::Ui, caption, chip, CAP_TEXT * m, gfx::REGULAR, gfx::CENTER, c.caption);
    if rt.EndDraw(None, None).is_err() {
        st.surface = None;
        return;
    }
    if let Some(surface) = st.surface.as_ref() {
        surface.present(hwnd, Some(POINT { x: l.x, y: l.y }), (255.0 * e).round() as u8);
    }
}

/// Which tab is under the client point, by its visible part.
fn tab_at(st: &State, x: f32, y: f32) -> Option<usize> {
    let l = st.layout?;
    if st.kind != Kind::TimeUp {
        return None;
    }
    (0..2).find(|&i| gfx::contains(l.visible(l.tabs[i]), x, y))
}

unsafe fn press(st: &State, tab: Option<usize>) {
    let cmd = match (st.kind, tab) {
        (Kind::TimeUp, Some(0)) => tray::ID_BREAK,
        (Kind::TimeUp, Some(_)) => tray::ID_EXTEND,
        (Kind::BreakOver, _) => {
            hide();
            return;
        }
        (Kind::TimeUp, None) => return,
    };
    hide();
    testhook::log(&format!("prompt action {}", if cmd == tray::ID_BREAK { "Break" } else { "Extend" }));
    if !st.app.is_null() {
        let _ = PostMessageW((*st.app).hidden, WM_COMMAND, WPARAM(cmd as usize), LPARAM(0));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let cs = lp.0 as *const CREATESTRUCTW;
        let app = if cs.is_null() { std::ptr::null_mut() } else { (*cs).lpCreateParams as *mut App };
        let st = Box::new(State {
            app,
            kind: Kind::TimeUp,
            caption: Caption { full: String::new(), short: String::new() },
            labels: [String::new(), String::new()],
            anchor: None,
            layout: None,
            hover: None,
            tracking: false,
            shown_at: None,
            animate: false,
            gfx: gfx::shared(),
            surface: None,
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
        WM_TIMER if wp.0 == ANIM_ID => {
            render(hwnd, st);
            if progress(st) >= 1.0 {
                let _ = KillTimer(hwnd, ANIM_ID);
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == DISMISS_ID => {
            hide();
            LRESULT(0)
        }
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
            let over = tab_at(st, pt.x as f32, pt.y as f32).is_some() || st.kind == Kind::BreakOver;
            let _ = SetCursor(LoadCursorW(None, if over { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
            LRESULT(1)
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
            let h = tab_at(st, x, y);
            if h != st.hover {
                st.hover = h;
                render(hwnd, st);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            st.tracking = false;
            if st.hover.take().is_some() {
                render(hwnd, st);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = ((lp.0 as i16) as f32, ((lp.0 >> 16) as i16) as f32);
            let tab = tab_at(st, x, y);
            press(st, tab);
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
