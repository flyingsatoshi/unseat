use crate::win::app::App;
use unseat::{
    format_today, AlertSound, WidgetSize, ALERT_DURATION_PRESETS_SECS, SNOOZE_PRESETS_SECS,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, ScreenToClient, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetClientRect, GetWindowLongPtrW,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
    HTCAPTION, HTCLIENT, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SW_SHOW, WM_CLOSE,
    WM_CREATE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_PAINT, WNDCLASSW, WS_POPUP, WS_VISIBLE, WS_EX_TOPMOST,
};
use windows::core::{w, Interface};

pub const CLASS: windows::core::PCWSTR = w!("UnseatSettings");

const WIN_W: i32 = 300;
const WIN_H: i32 = 800;
const BTN_RADIUS: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Limit,
    Step,
    Break,
    Idle,
    Every,
    Snooze,
}

impl Field {
    fn unit(self) -> &'static str {
        match self {
            Self::Idle => "sec",
            Self::Step => "",
            _ => "min",
        }
    }

    fn min(self) -> u64 {
        match self {
            Self::Limit => 5,
            Self::Step => 1,
            Self::Break => 1,
            Self::Idle => 15,
            Self::Every => 2,
            Self::Snooze => 1,
        }
    }

    fn max(self) -> u64 {
        match self {
            Self::Limit => 240,
            Self::Step => 30,
            Self::Break => 30,
            Self::Idle => 600,
            Self::Every => 60,
            Self::Snooze => 60,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    None,
    Close,
    Size(usize),
    Sound,
    Preview,
    SoundKind(usize),
    Duration(usize),
    SnoozePreset(usize),
    Repeat,
    Auto,
    Cancel,
    Save,
    Minus(Field),
    Plus(Field),
}

struct Dialog {
    app: *mut App,
    size: WidgetSize,
    revert_size: WidgetSize,
    sound: bool,
    repeat: bool,
    auto: bool,
    sitting_min: u64,
    break_min: u64,
    idle_sec: u64,
    every_min: u64,
    step: u64,
    sound_kind: AlertSound,
    alert_duration: u64,
    snooze_min: u64,
    saved: bool,
    hover: Hit,
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

pub fn open(app: *mut App) {
    unsafe {
        if let Ok(existing) = FindWindowW(CLASS, None) {
            let _ = SetForegroundWindow(existing);
            return;
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            CLASS,
            w!(""),
            WS_POPUP | WS_VISIBLE,
            120,
            80,
            WIN_W,
            WIN_H,
            None,
            None,
            GetModuleHandleW(None).unwrap_or_default(),
            Some(app as *const std::ffi::c_void),
        );
        if let Ok(hwnd) = hwnd {
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&pref) as u32,
            );
            let scale = dpi_scale(hwnd);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                (WIN_W as f32 * scale).round() as i32,
                (WIN_H as f32 * scale).round() as i32,
                SWP_NOMOVE | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(hwnd, SW_SHOW);
            crate::win::icon::apply(hwnd);
            let _ = SetForegroundWindow(hwnd);
        }
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

fn rct(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

fn contains(rc: D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

fn px(v: f32, scale: f32) -> f32 {
    v * scale
}

unsafe fn dpi_scale(hwnd: HWND) -> f32 {
    let d = GetDpiForWindow(hwnd);
    if d == 0 {
        1.0
    } else {
        d as f32 / 96.0
    }
}

struct Step {
    minus: D2D_RECT_F,
    well: D2D_RECT_F,
    plus: D2D_RECT_F,
}

struct Lay {
    w: f32,
    h: f32,
    scale: f32,
    close: D2D_RECT_F,
        title: D2D_RECT_F,
        timer_head: D2D_RECT_F,
    timer_card: D2D_RECT_F,
    sitting_row: D2D_RECT_F,
    step_row: D2D_RECT_F,
    overlay_label: D2D_RECT_F,
    sizes: [D2D_RECT_F; 5],
    breaks_head: D2D_RECT_F,
    breaks_card: D2D_RECT_F,
    break_row: D2D_RECT_F,
    idle_row: D2D_RECT_F,
    alerts_head: D2D_RECT_F,
    alerts_card: D2D_RECT_F,
    sound: D2D_RECT_F,
    preview: D2D_RECT_F,
    sounds: [D2D_RECT_F; 5],
    duration_label: D2D_RECT_F,
    durations: [D2D_RECT_F; 4],
    repeat: D2D_RECT_F,
    every_row: D2D_RECT_F,
    snooze_head: D2D_RECT_F,
    snooze_card: D2D_RECT_F,
    snoozes: [D2D_RECT_F; 4],
    snooze_row: D2D_RECT_F,
    system_head: D2D_RECT_F,
    auto: D2D_RECT_F,
    today: D2D_RECT_F,
    cancel: D2D_RECT_F,
    save: D2D_RECT_F,
    step_limit: Step,
    step_incr: Step,
    step_break: Step,
    step_idle: Step,
    step_every: Step,
    step_snooze: Step,
}

impl Lay {
    fn step(&self, field: Field) -> &Step {
        match field {
            Field::Limit => &self.step_limit,
            Field::Step => &self.step_incr,
            Field::Break => &self.step_break,
            Field::Idle => &self.step_idle,
            Field::Every => &self.step_every,
            Field::Snooze => &self.step_snooze,
        }
    }

    fn row_label(&self, row: D2D_RECT_F, right: f32) -> D2D_RECT_F {
        rct(
            row.left + px(12.0, self.scale),
            row.top,
            right,
            row.bottom,
        )
    }
}

fn stepper(row: D2D_RECT_F, scale: f32) -> Step {
    let h = px(26.0, scale);
    let btn = px(22.0, scale);
    let well_w = px(62.0, scale);
    let gap = px(3.0, scale);
    let y = (row.top + row.bottom - h) * 0.5;
    let right = row.right - px(10.0, scale);
    let plus_l = right - btn;
    let well_r = plus_l - gap;
    let well_l = well_r - well_w;
    let minus_r = well_l - gap;
    let minus_l = minus_r - btn;
    Step {
        minus: rct(minus_l, y, minus_r, y + h),
        well: rct(well_l, y, well_r, y + h),
        plus: rct(plus_l, y, right, y + h),
    }
}

fn layout(scale: f32) -> Lay {
    let w = px(WIN_W as f32, scale);
    let h = px(WIN_H as f32, scale);
    let x = px(12.0, scale);
    let card_r = w - x;
    let row_h = px(38.0, scale);
    let head_h = px(19.0, scale);
    let gap = px(10.0, scale);

    let title = rct(x + px(4.0, scale), px(10.0, scale), px(200.0, scale), px(34.0, scale));
    let timer_head = rct(x + px(4.0, scale), px(38.0, scale), px(160.0, scale), px(57.0, scale));
    let t0 = px(57.0, scale);
    let step_top = t0 + row_h;
    let overlay_label_top = step_top + row_h + px(8.0, scale);
    let overlay_label_bot = overlay_label_top + px(18.0, scale);
    let chip_y = overlay_label_bot + px(10.0, scale);
    let chip_h = px(26.0, scale);
    let timer_bot = chip_y + chip_h + px(20.0, scale);
    let timer_card = rct(x, t0, card_r, timer_bot);
    let sitting_row = rct(x, t0, card_r, t0 + row_h);
    let step_row = rct(x, step_top, card_r, step_top + row_h);

    let inner = (card_r - x) - px(16.0, scale);
    let chip_gap = px(4.0, scale);
    let cw = (inner - chip_gap * 4.0) / 5.0;
    let mut sizes = [rct(0.0, 0.0, 0.0, 0.0); 5];
    for i in 0..5 {
        let left = x + px(8.0, scale) + i as f32 * (cw + chip_gap);
        sizes[i] = rct(left, chip_y, left + cw, chip_y + chip_h);
    }

    let breaks_head_top = timer_bot + gap;
    let b0 = breaks_head_top + head_h;
    let idle_top = b0 + row_h;
    let breaks_bot = idle_top + row_h;
    let breaks_card = rct(x, b0, card_r, breaks_bot);
    let break_row = rct(x, b0, card_r, idle_top);
    let idle_row = rct(x, idle_top, card_r, breaks_bot);

    let alerts_head_top = breaks_bot + gap;
    let a0 = alerts_head_top + head_h;
    let sound = rct(x, a0, card_r, a0 + row_h);
    let sounds_y = a0 + row_h + px(4.0, scale);
    let mut sounds = [rct(0.0, 0.0, 0.0, 0.0); 5];
    for i in 0..5 {
        let left = x + px(8.0, scale) + i as f32 * (cw + chip_gap);
        sounds[i] = rct(left, sounds_y, left + cw, sounds_y + chip_h);
    }
    let duration_label_top = sounds_y + chip_h + px(8.0, scale);
    let duration_label_bot = duration_label_top + px(16.0, scale);
    let duration_y = duration_label_bot + px(4.0, scale);
    let d_inner = inner;
    let d_gap = px(4.0, scale);
    let dw = (d_inner - d_gap * 3.0) / 4.0;
    let mut durations = [rct(0.0, 0.0, 0.0, 0.0); 4];
    for i in 0..4 {
        let left = x + px(8.0, scale) + i as f32 * (dw + d_gap);
        durations[i] = rct(left, duration_y, left + dw, duration_y + chip_h);
    }
    let repeat_top = duration_y + chip_h + px(8.0, scale);
    let every_top = repeat_top + row_h;
    let alerts_bot = every_top + row_h;
    let alerts_card = rct(x, a0, card_r, alerts_bot);
    let every_row = rct(x, every_top, card_r, alerts_bot);

    let tog_w = px(38.0, scale);
    let preview = rct(
        card_r - px(12.0, scale) - tog_w - px(8.0, scale) - px(58.0, scale),
        a0 + px(8.0, scale),
        card_r - px(12.0, scale) - tog_w - px(8.0, scale),
        a0 + row_h - px(8.0, scale),
    );

    let snooze_head_top = alerts_bot + gap;
    let z0 = snooze_head_top + head_h;
    let snooze_chip_y = z0 + px(8.0, scale);
    let mut snoozes = [rct(0.0, 0.0, 0.0, 0.0); 4];
    let sw = (inner - d_gap * 3.0) / 4.0;
    for i in 0..4 {
        let left = x + px(8.0, scale) + i as f32 * (sw + d_gap);
        snoozes[i] = rct(left, snooze_chip_y, left + sw, snooze_chip_y + chip_h);
    }
    let snooze_row_top = snooze_chip_y + chip_h + px(8.0, scale);
    let snooze_bot = snooze_row_top + row_h;
    let snooze_card = rct(x, z0, card_r, snooze_bot);
    let snooze_row = rct(x, snooze_row_top, card_r, snooze_bot);

    let system_head_top = snooze_bot + gap;
    let s0 = system_head_top + head_h;
    let auto = rct(x, s0, card_r, s0 + row_h);

    let btn_h = px(28.0, scale);
    let btn_y = h - px(12.0, scale) - btn_h;
    let btn_w = px(72.0, scale);
    let save = rct(card_r - btn_w, btn_y, card_r, btn_y + btn_h);
    let cancel = rct(
        save.left - px(8.0, scale) - btn_w,
        btn_y,
        save.left - px(8.0, scale),
        btn_y + btn_h,
    );

    Lay {
        w,
        h,
        scale,
        close: rct(w - px(36.0, scale), px(8.0, scale), w - px(12.0, scale), px(32.0, scale)),
        title,
        timer_head,
        timer_card,
        sitting_row,
        step_row,
        overlay_label: rct(x + px(12.0, scale), overlay_label_top, px(200.0, scale), overlay_label_bot),
        sizes,
        breaks_head: rct(x + px(4.0, scale), breaks_head_top, px(160.0, scale), b0),
        breaks_card,
        break_row,
        idle_row,
        alerts_head: rct(x + px(4.0, scale), alerts_head_top, px(160.0, scale), a0),
        alerts_card,
        sound,
        preview,
        sounds,
        duration_label: rct(x + px(12.0, scale), duration_label_top, px(200.0, scale), duration_label_bot),
        durations,
        repeat: rct(x, repeat_top, card_r, every_top),
        every_row,
        snooze_head: rct(x + px(4.0, scale), snooze_head_top, px(160.0, scale), z0),
        snooze_card,
        snoozes,
        snooze_row,
        system_head: rct(x + px(4.0, scale), system_head_top, px(160.0, scale), s0),
        auto,
        today: rct(x, btn_y, cancel.left - px(8.0, scale), btn_y + btn_h),
        cancel,
        save,
        step_limit: stepper(sitting_row, scale),
        step_incr: stepper(step_row, scale),
        step_break: stepper(break_row, scale),
        step_idle: stepper(idle_row, scale),
        step_every: stepper(every_row, scale),
        step_snooze: stepper(snooze_row, scale),
    }
}

fn hit_step(l: &Lay, field: Field, x: f32, y: f32) -> Option<Hit> {
    let step = l.step(field);
    if contains(step.minus, x, y) {
        return Some(Hit::Minus(field));
    }
    if contains(step.plus, x, y) {
        return Some(Hit::Plus(field));
    }
    None
}

fn hit_at(x: f32, y: f32, scale: f32) -> Hit {
    let l = layout(scale);
    if contains(l.close, x, y) {
        return Hit::Close;
    }
    if contains(l.save, x, y) {
        return Hit::Save;
    }
    if contains(l.cancel, x, y) {
        return Hit::Cancel;
    }
    for field in [
        Field::Limit,
        Field::Step,
        Field::Break,
        Field::Idle,
        Field::Every,
        Field::Snooze,
    ] {
        if let Some(hit) = hit_step(&l, field, x, y) {
            return hit;
        }
    }
    for (i, rc) in l.sizes.iter().enumerate() {
        if contains(*rc, x, y) {
            return Hit::Size(i);
        }
    }
    if contains(l.preview, x, y) {
        return Hit::Preview;
    }
    for (i, rc) in l.sounds.iter().enumerate() {
        if contains(*rc, x, y) {
            return Hit::SoundKind(i);
        }
    }
    for (i, rc) in l.durations.iter().enumerate() {
        if contains(*rc, x, y) {
            return Hit::Duration(i);
        }
    }
    for (i, rc) in l.snoozes.iter().enumerate() {
        if contains(*rc, x, y) {
            return Hit::SnoozePreset(i);
        }
    }
    if contains(l.sound, x, y) {
        return Hit::Sound;
    }
    if contains(l.repeat, x, y) {
        return Hit::Repeat;
    }
    if contains(l.auto, x, y) {
        return Hit::Auto;
    }
    Hit::None
}

fn client_hit(hwnd: HWND, dlg: *mut Dialog, lp: LPARAM, screen: bool) -> Hit {
    unsafe {
        let mut x = (lp.0 as i16) as i32;
        let mut y = ((lp.0 >> 16) as i16) as i32;
        if screen {
            let mut pt = POINT { x, y };
            let _ = ScreenToClient(hwnd, &mut pt);
            x = pt.x;
            y = pt.y;
        }
        let scale = if dlg.is_null() { 1.0 } else { (*dlg).scale };
        hit_at(x as f32, y as f32, scale)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let cs = lp.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
        let app = if cs.is_null() {
            std::ptr::null_mut()
        } else {
            (*cs).lpCreateParams as *mut App
        };
        let dlg = build(hwnd, app);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(dlg) as isize);
        return LRESULT(0);
    }
    let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
    match msg {
        WM_NCHITTEST => {
            let hit = client_hit(hwnd, dlg, lp, true);
            if hit == Hit::None {
                LRESULT(HTCAPTION as isize)
            } else {
                LRESULT(HTCLIENT as isize)
            }
        }
        WM_MOUSEMOVE => {
            if !dlg.is_null() {
                let h = client_hit(hwnd, dlg, lp, false);
                if h != (*dlg).hover {
                    (*dlg).hover = h;
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP => {
            if msg == WM_LBUTTONDOWN && !dlg.is_null() {
                on_click(hwnd, dlg, client_hit(hwnd, dlg, lp, false));
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            if !dlg.is_null() {
                paint_ui(hwnd, dlg);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_CLOSE => {
            close(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u16;
            if vk == VK_ESCAPE.0 {
                close(hwnd);
            } else if vk == VK_RETURN.0 {
                save(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if !dlg.is_null() {
                let _ = Box::from_raw(dlg);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn build(hwnd: HWND, app: *mut App) -> Box<Dialog> {
    let (
        size,
        sound,
        repeat,
        auto,
        sitting_min,
        break_min,
        idle_sec,
        every_min,
        step,
        sound_kind,
        alert_duration,
        snooze_min,
    ) = if app.is_null() {
        (
            WidgetSize::Small,
            true,
            true,
            true,
            60,
            3,
            60,
            10,
            1,
            AlertSound::Chime,
            0,
            10,
        )
    } else {
        let s = &(*app).settings;
        (
            s.widget_size,
            s.sound_enabled,
            s.repeat_reminders,
            s.launch_with_windows,
            (s.sitting_limit_secs / 60).clamp(Field::Limit.min(), Field::Limit.max()),
            (s.break_duration_secs / 60).clamp(Field::Break.min(), Field::Break.max()),
            s.idle_after_secs.clamp(Field::Idle.min(), Field::Idle.max()),
            (s.repeat_every_secs / 60).clamp(Field::Every.min(), Field::Every.max()),
            s.step.clamp(Field::Step.min(), Field::Step.max()),
            s.alert_sound,
            s.alert_duration_secs.min(10),
            (s.snooze_secs / 60).clamp(Field::Snooze.min(), Field::Snooze.max()),
        )
    };
    let scale = dpi_scale(hwnd);
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        (WIN_W as f32 * scale).round() as i32,
        (WIN_H as f32 * scale).round() as i32,
        SWP_NOMOVE | SWP_NOACTIVATE,
    );
    Box::new(Dialog {
        app,
        size,
        revert_size: size,
        sound,
        repeat,
        auto,
        sitting_min,
        break_min,
        idle_sec,
        every_min,
        step,
        sound_kind,
        alert_duration,
        snooze_min,
        saved: false,
        hover: Hit::None,
        scale,
        factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok(),
        dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok(),
        rt: None,
    })
}

unsafe fn ensure_rt(hwnd: HWND, dlg: *mut Dialog) -> Option<ID2D1RenderTarget> {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let w = (rc.right - rc.left).max(1) as u32;
    let h = (rc.bottom - rc.top).max(1) as u32;
    if (*dlg).rt.is_none() {
        let factory = (*dlg).factory.as_ref()?;
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
        (*dlg).rt = factory.CreateHwndRenderTarget(&props, &hwnd_props).ok();
    } else if let Some(rt) = (*dlg).rt.as_ref() {
        let _ = rt.Resize(&D2D_SIZE_U { width: w, height: h });
    }
    (*dlg).rt.as_ref().and_then(|rt| rt.cast().ok())
}

unsafe fn fmt(
    dwrite: &IDWriteFactory,
    size: f32,
    weight: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    let f = dwrite
        .CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-us"),
        )
        .ok()
        .or_else(|| {
            dwrite
                .CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()
        })?;
    let _ = f.SetTextAlignment(align);
    let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    Some(f)
}

unsafe fn fill_round(rt: &ID2D1RenderTarget, rc: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F) {
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: rc,
                radiusX: radius,
                radiusY: radius,
            },
            &brush,
        );
    }
}

unsafe fn stroke_round(
    rt: &ID2D1RenderTarget,
    rc: D2D_RECT_F,
    radius: f32,
    color: D2D1_COLOR_F,
    width: f32,
) {
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        rt.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: rc,
                radiusX: radius,
                radiusY: radius,
            },
            &brush,
            width,
            None,
        );
    }
}

unsafe fn text(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    s: &str,
    rc: D2D_RECT_F,
    size: f32,
    weight: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    color: D2D1_COLOR_F,
) {
    let Some(format) = fmt(dwrite, size, weight, align) else {
        return;
    };
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
}

unsafe fn toggle(rt: &ID2D1RenderTarget, row: D2D_RECT_F, on: bool, scale: f32) {
    let h = px(22.0, scale);
    let w = px(38.0, scale);
    let y = (row.top + row.bottom - h) * 0.5;
    let x = row.right - px(12.0, scale) - w;
    let track = rct(x, y, x + w, y + h);
    fill_round(
        rt,
        track,
        h * 0.5,
        if on {
            rgb(0x30, 0xD1, 0x58, 1.0)
        } else {
            rgb(0x39, 0x39, 0x3D, 1.0)
        },
    );
    let knob = px(16.0, scale);
    let pad = px(3.0, scale);
    let kx = if on { x + w - knob - pad } else { x + pad };
    fill_round(
        rt,
        rct(kx, y + pad, kx + knob, y + pad + knob),
        knob * 0.5,
        rgb(255, 255, 255, 1.0),
    );
}

fn field_value(dlg: &Dialog, field: Field) -> u64 {
    match field {
        Field::Limit => dlg.sitting_min,
        Field::Step => dlg.step,
        Field::Break => dlg.break_min,
        Field::Idle => dlg.idle_sec,
        Field::Every => dlg.every_min,
        Field::Snooze => dlg.snooze_min,
    }
}

unsafe fn paint_step(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    dlg: &Dialog,
    l: &Lay,
    field: Field,
    hover: Hit,
) {
    let step = l.step(field);
    let s = l.scale;
    let r = px(6.0, s);
    let minus_on = hover == Hit::Minus(field);
    let plus_on = hover == Hit::Plus(field);
    fill_round(
        rt,
        step.minus,
        r,
        if minus_on {
            rgb(0x48, 0x48, 0x4A, 1.0)
        } else {
            rgb(0x3A, 0x3A, 0x3C, 1.0)
        },
    );
    text(
        rt,
        dwrite,
        "\u{2212}",
        step.minus,
        px(14.0, s),
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        rgb(0xF2, 0xF2, 0xF7, 1.0),
    );
    fill_round(rt, step.well, r, rgb(0x1C, 0x1C, 0x1E, 1.0));
    let n = field_value(dlg, field);
    let num = format!("{n}");
    if field.unit().is_empty() {
        text(
            rt,
            dwrite,
            &num,
            step.well,
            px(13.0, s),
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            rgb(0xF2, 0xF2, 0xF7, 1.0),
        );
    } else {
        let mid = step.well.left + (step.well.right - step.well.left) * 0.55;
        text(
            rt,
            dwrite,
            &num,
            rct(step.well.left + px(4.0, s), step.well.top, mid, step.well.bottom),
            px(13.0, s),
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
            rgb(0xF2, 0xF2, 0xF7, 1.0),
        );
        text(
            rt,
            dwrite,
            field.unit(),
            rct(mid + px(3.0, s), step.well.top, step.well.right - px(4.0, s), step.well.bottom),
            px(11.0, s),
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            rgb(0x8E, 0x8E, 0x93, 1.0),
        );
    }
    fill_round(
        rt,
        step.plus,
        r,
        if plus_on {
            rgb(0x48, 0x48, 0x4A, 1.0)
        } else {
            rgb(0x3A, 0x3A, 0x3C, 1.0)
        },
    );
    text(
        rt,
        dwrite,
        "+",
        step.plus,
        px(15.0, s),
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        rgb(0xF2, 0xF2, 0xF7, 1.0),
    );
}

unsafe fn paint_ui(hwnd: HWND, dlg: *mut Dialog) {
    let Some(rt) = ensure_rt(hwnd, dlg) else {
        return;
    };
    let Some(dwrite) = (*dlg).dwrite.as_ref() else {
        return;
    };
    let l = layout((*dlg).scale);
    let s = l.scale;
    let hover = (*dlg).hover;
    rt.BeginDraw();
    rt.Clear(Some(&rgb(0x1C, 0x1C, 0x1E, 1.0)));

    stroke_round(
        &rt,
        rct(0.5, 0.5, l.w - 0.5, l.h - 0.5),
        px(12.0, s),
        rgb(0x63, 0x63, 0x66, 0.90),
        px(1.0, s),
    );

    let head = rgb(0x8E, 0x8E, 0x93, 1.0);
    let label = rgb(0xF2, 0xF2, 0xF7, 1.0);
    let card = rgb(0x2C, 0x2C, 0x2E, 1.0);
    let sep = rgb(0x3A, 0x3A, 0x3C, 1.0);
    let head_sz = px(11.0, s);
    let label_sz = px(11.05, s);
    let chip_sz = px(10.0, s);
    let card_r = px(10.0, s);

    text(&rt, dwrite, "Unseat", l.title, px(16.0, s), DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, label);
    text(&rt, dwrite, "Timer", l.timer_head, head_sz, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, head);
    fill_round(&rt, l.timer_card, card_r, card);
    text(
        &rt,
        dwrite,
        "Limit",
        l.row_label(l.sitting_row, l.step_limit.minus.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    paint_step(&rt, dwrite, &*dlg, &l, Field::Limit, hover);
    fill_round(
        &rt,
        rct(l.timer_card.left + px(12.0, s), l.sitting_row.bottom, l.timer_card.right - px(12.0, s), l.sitting_row.bottom + px(1.0, s)),
        0.5,
        sep,
    );
    text(
        &rt,
        dwrite,
        "Step",
        l.row_label(l.step_row, l.step_incr.minus.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    paint_step(&rt, dwrite, &*dlg, &l, Field::Step, hover);
    fill_round(
        &rt,
        rct(l.timer_card.left + px(12.0, s), l.step_row.bottom, l.timer_card.right - px(12.0, s), l.step_row.bottom + px(1.0, s)),
        0.5,
        sep,
    );
    text(&rt, dwrite, "Overlay size", l.overlay_label, label_sz, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_LEADING, label);
    for (i, sz) in WidgetSize::ALL.iter().enumerate() {
        let on = *sz == (*dlg).size;
        fill_round(
            &rt,
            l.sizes[i],
            px(8.0, s),
            if on {
                rgb(0x0A, 0x84, 0xFF, 1.0)
            } else {
                rgb(0x3A, 0x3A, 0x3C, 1.0)
            },
        );
        text(
            &rt,
            dwrite,
            sz.chip_label(),
            l.sizes[i],
            chip_sz,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            if on {
                rgb(255, 255, 255, 1.0)
            } else {
                rgb(0xEB, 0xEB, 0xF5, 0.92)
            },
        );
    }

    text(&rt, dwrite, "Breaks", l.breaks_head, head_sz, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, head);
    fill_round(&rt, l.breaks_card, card_r, card);
    text(
        &rt,
        dwrite,
        "Break length",
        l.row_label(l.break_row, l.step_break.minus.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    paint_step(&rt, dwrite, &*dlg, &l, Field::Break, hover);
    fill_round(
        &rt,
        rct(l.breaks_card.left + px(12.0, s), l.idle_row.top, l.breaks_card.right - px(12.0, s), l.idle_row.top + px(1.0, s)),
        0.5,
        sep,
    );
    text(
        &rt,
        dwrite,
        "Idle starts break",
        l.row_label(l.idle_row, l.step_idle.minus.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    paint_step(&rt, dwrite, &*dlg, &l, Field::Idle, hover);

    text(&rt, dwrite, "Alerts", l.alerts_head, head_sz, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, head);
    fill_round(&rt, l.alerts_card, card_r, card);
    text(
        &rt,
        dwrite,
        "Sound",
        l.row_label(l.sound, l.preview.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    fill_round(
        &rt,
        l.preview,
        px(8.0, s),
        if hover == Hit::Preview {
            rgb(0x48, 0x48, 0x4A, 1.0)
        } else {
            rgb(0x3A, 0x3A, 0x3C, 1.0)
        },
    );
    text(
        &rt,
        dwrite,
        "Preview",
        l.preview,
        px(10.0, s),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        rgb(0xEB, 0xEB, 0xF5, 0.95),
    );
    toggle(&rt, l.sound, (*dlg).sound, s);
    for (i, kind) in AlertSound::ALL.iter().enumerate() {
        let on = *kind == (*dlg).sound_kind;
        fill_round(
            &rt,
            l.sounds[i],
            px(8.0, s),
            if on {
                rgb(0x0A, 0x84, 0xFF, 1.0)
            } else {
                rgb(0x3A, 0x3A, 0x3C, 1.0)
            },
        );
        text(
            &rt,
            dwrite,
            kind.chip_label(),
            l.sounds[i],
            px(10.0, s),
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            if on {
                rgb(255, 255, 255, 1.0)
            } else {
                rgb(0xEB, 0xEB, 0xF5, 0.92)
            },
        );
    }
    text(
        &rt,
        dwrite,
        "Alert length",
        l.duration_label,
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    for (i, secs) in ALERT_DURATION_PRESETS_SECS.iter().enumerate() {
        let on = (*dlg).alert_duration == *secs;
        let caption = if *secs == 0 {
            "Once".to_string()
        } else {
            format!("{secs}s")
        };
        fill_round(
            &rt,
            l.durations[i],
            px(8.0, s),
            if on {
                rgb(0x0A, 0x84, 0xFF, 1.0)
            } else {
                rgb(0x3A, 0x3A, 0x3C, 1.0)
            },
        );
        text(
            &rt,
            dwrite,
            &caption,
            l.durations[i],
            chip_sz,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            if on {
                rgb(255, 255, 255, 1.0)
            } else {
                rgb(0xEB, 0xEB, 0xF5, 0.92)
            },
        );
    }
    fill_round(
        &rt,
        rct(l.alerts_card.left + px(12.0, s), l.repeat.top, l.alerts_card.right - px(12.0, s), l.repeat.top + px(1.0, s)),
        0.5,
        sep,
    );
    text(
        &rt,
        dwrite,
        "Repeat while overdue",
        l.row_label(l.repeat, l.repeat.right - px(50.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    toggle(&rt, l.repeat, (*dlg).repeat, s);
    fill_round(
        &rt,
        rct(l.alerts_card.left + px(12.0, s), l.every_row.top, l.alerts_card.right - px(12.0, s), l.every_row.top + px(1.0, s)),
        0.5,
        sep,
    );
    text(
        &rt,
        dwrite,
        "Remind every",
        l.row_label(l.every_row, l.step_every.minus.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    paint_step(&rt, dwrite, &*dlg, &l, Field::Every, hover);

    text(&rt, dwrite, "Snooze", l.snooze_head, head_sz, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, head);
    fill_round(&rt, l.snooze_card, card_r, card);
    for (i, mins) in SNOOZE_PRESETS_SECS.iter().map(|secs| secs / 60).enumerate() {
        let on = (*dlg).snooze_min == mins;
        fill_round(
            &rt,
            l.snoozes[i],
            px(8.0, s),
            if on {
                rgb(0x0A, 0x84, 0xFF, 1.0)
            } else {
                rgb(0x3A, 0x3A, 0x3C, 1.0)
            },
        );
        text(
            &rt,
            dwrite,
            &format!("{mins}m"),
            l.snoozes[i],
            chip_sz,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            if on {
                rgb(255, 255, 255, 1.0)
            } else {
                rgb(0xEB, 0xEB, 0xF5, 0.92)
            },
        );
    }
    fill_round(
        &rt,
        rct(l.snooze_card.left + px(12.0, s), l.snooze_row.top, l.snooze_card.right - px(12.0, s), l.snooze_row.top + px(1.0, s)),
        0.5,
        sep,
    );
    text(
        &rt,
        dwrite,
        "Custom",
        l.row_label(l.snooze_row, l.step_snooze.minus.left - px(8.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    paint_step(&rt, dwrite, &*dlg, &l, Field::Snooze, hover);

    text(&rt, dwrite, "System", l.system_head, head_sz, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING, head);
    fill_round(&rt, l.auto, card_r, card);
    text(
        &rt,
        dwrite,
        "Launch with Windows",
        l.row_label(l.auto, l.auto.right - px(50.0, s)),
        label_sz,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        label,
    );
    toggle(&rt, l.auto, (*dlg).auto, s);

    let today = if (*dlg).app.is_null() {
        String::new()
    } else {
        format_today((*(*dlg).app).engine.snapshot().today_sitting)
    };
    text(&rt, dwrite, &today, l.today, px(10.0, s), DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_LEADING, head);

    let btn_r = px(BTN_RADIUS, s);
    fill_round(
        &rt,
        l.cancel,
        btn_r,
        if hover == Hit::Cancel {
            rgb(0x48, 0x48, 0x4A, 1.0)
        } else {
            rgb(0x3A, 0x3A, 0x3C, 1.0)
        },
    );
    text(&rt, dwrite, "Cancel", l.cancel, px(12.0, s), DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, label);
    fill_round(
        &rt,
        l.save,
        btn_r,
        if hover == Hit::Save {
            rgb(0x40, 0x9C, 0xFF, 1.0)
        } else {
            rgb(0x0A, 0x84, 0xFF, 1.0)
        },
    );
    text(
        &rt,
        dwrite,
        "Save",
        l.save,
        px(12.0, s),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        rgb(255, 255, 255, 1.0),
    );

    fill_round(&rt, l.close, px(12.0, s), rgb(0x3A, 0x3A, 0x3C, 1.0));
    text(
        &rt,
        dwrite,
        "\u{00D7}",
        l.close,
        px(14.0, s),
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_TEXT_ALIGNMENT_CENTER,
        rgb(0xEB, 0xEB, 0xF5, 1.0),
    );

    let _ = rt.EndDraw(None, None);
}

unsafe fn preview(dlg: *mut Dialog) {
    if dlg.is_null() || (*dlg).app.is_null() {
        return;
    }
    (*(*dlg).app).settings.widget_size = (*dlg).size;
    (*(*dlg).app).force_repaint();
}

unsafe fn set_field(dlg: *mut Dialog, field: Field, value: u64) {
    match field {
        Field::Limit => (*dlg).sitting_min = value,
        Field::Step => (*dlg).step = value,
        Field::Break => (*dlg).break_min = value,
        Field::Idle => (*dlg).idle_sec = value,
        Field::Every => (*dlg).every_min = value,
        Field::Snooze => (*dlg).snooze_min = value,
    }
}

unsafe fn step_field(dlg: *mut Dialog, field: Field, dir: i32) {
    let current = field_value(&*dlg, field);
    let delta = if field == Field::Step || field == Field::Snooze {
        1
    } else {
        (*dlg).step.max(1)
    };
    let next = if dir < 0 {
        current.saturating_sub(delta).max(field.min())
    } else {
        current.saturating_add(delta).min(field.max())
    };
    set_field(dlg, field, next);
}

unsafe fn on_click(hwnd: HWND, dlg: *mut Dialog, hit: Hit) {
    match hit {
        Hit::Close | Hit::Cancel => close(hwnd),
        Hit::Save => save(hwnd),
        Hit::Size(i) => {
            (*dlg).size = WidgetSize::from_index(i);
            preview(dlg);
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::Sound => {
            (*dlg).sound = !(*dlg).sound;
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::Preview => {
            if !(*dlg).app.is_null() {
                (*(*dlg).app).preview_sound((*dlg).sound_kind, (*dlg).alert_duration);
            }
        }
        Hit::SoundKind(i) => {
            (*dlg).sound_kind = AlertSound::from_index(i);
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::Duration(i) => {
            if let Some(secs) = ALERT_DURATION_PRESETS_SECS.get(i).copied() {
                (*dlg).alert_duration = secs;
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
        Hit::SnoozePreset(i) => {
            if let Some(secs) = SNOOZE_PRESETS_SECS.get(i).copied() {
                (*dlg).snooze_min = secs / 60;
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
        Hit::Repeat => {
            (*dlg).repeat = !(*dlg).repeat;
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::Auto => {
            (*dlg).auto = !(*dlg).auto;
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::Minus(field) => {
            step_field(dlg, field, -1);
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::Plus(field) => {
            step_field(dlg, field, 1);
            let _ = InvalidateRect(hwnd, None, false);
        }
        Hit::None => {}
    }
}

unsafe fn save(hwnd: HWND) {
    let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
    if dlg.is_null() || (*dlg).app.is_null() {
        return;
    }
    let app = (*dlg).app;
    (*app).settings.sitting_limit_secs = (*dlg).sitting_min.saturating_mul(60);
    (*app).settings.break_duration_secs = (*dlg).break_min.saturating_mul(60);
    (*app).settings.idle_after_secs = (*dlg).idle_sec;
    (*app).settings.repeat_every_secs = (*dlg).every_min.saturating_mul(60);
    (*app).settings.step = (*dlg).step;
    (*app).settings.sound_enabled = (*dlg).sound;
    (*app).settings.repeat_reminders = (*dlg).repeat;
    (*app).settings.alert_sound = (*dlg).sound_kind;
    (*app).settings.alert_duration_secs = (*dlg).alert_duration;
    (*app).settings.snooze_secs = (*dlg).snooze_min.saturating_mul(60);
    (*app).settings.launch_with_windows = (*dlg).auto;
    (*app).settings.widget_size = (*dlg).size;
    (*app).settings.clamp();
    (*app)
        .engine
        .set_config(unseat::EngineConfig::from_settings(&(*app).settings));
    crate::win::autostart::apply((*app).settings.launch_with_windows);
    (*app).save_settings();
    (*app).force_repaint();
    (*dlg).saved = true;
    close(hwnd);
}

unsafe fn close(hwnd: HWND) {
    let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
    if !dlg.is_null() && !(*dlg).saved && !(*dlg).app.is_null() {
        (*(*dlg).app).settings.widget_size = (*dlg).revert_size;
        (*(*dlg).app).force_repaint();
    }
    let _ = DestroyWindow(hwnd);
}

pub fn shutdown() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, None) {
            let _ = DestroyWindow(hwnd);
        }
    }
}
