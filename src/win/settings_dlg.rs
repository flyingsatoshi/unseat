//! Settings window: five tabs of setting tiles, drawn with Direct2D in the Signal style.
//!
//! The UI is data-driven: `sections()` lists each tab's rows, `layout()` turns them into
//! rectangles, and painting, hit testing and keyboard focus all walk the same layout, so a
//! control is defined once. The dialog edits a draft copy of the settings; size, theme and
//! always-on-top preview live on the overlay and revert on Cancel.

use crate::win::app::App;
use crate::win::gfx::{self, glyph, Family, Fonts};
use unseat::{
    average_secs, format_today_hm, palette, recent_days, status_line, weekday_initial, AlertSound,
    CivilDate, DockEdge, DockReveal, InactivityBehavior, Palette, Rgba, Settings, ThemePreference,
    TimerDisplayMode, VisibleState, WidgetSize, ALERT_DURATION_PRESETS_SECS, SNOOZE_PRESETS_SECS,
};
use windows::core::{w, Interface};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, ScreenToClient,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
    VIRTUAL_KEY, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, LoadCursorW, RegisterClassW, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, CREATESTRUCTW, GWLP_USERDATA, HTCAPTION, HTCLIENT, HWND_TOPMOST,
    IDC_ARROW, SWP_NOACTIVATE, SW_SHOW, WM_CHAR, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_PAINT,
    WM_SYSKEYDOWN, WNDCLASSW, WS_EX_TOPMOST, WS_POPUP,
};

pub const CLASS: windows::core::PCWSTR = w!("UnseatSettings");

const WIN_W: f32 = 400.0;
const PAD: f32 = 16.0;
const HEADER_H: f32 = 60.0;
const TABS_H: f32 = 34.0;
const BODY_TOP: f32 = HEADER_H + TABS_H + 10.0;
const FOOTER_H: f32 = 68.0;
const SECTION_HEAD: f32 = 26.0;
const SECTION_GAP: f32 = 8.0;
const ROW_GAP: f32 = 6.0;
const TILE_R: f32 = 10.0;
const CHIP_R: f32 = 8.0;
const WORK_AREA_MARGIN: i32 = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Timer,
    Breaks,
    Alerts,
    Dock,
    General,
}

impl Tab {
    const ALL: [Self; 5] = [Self::Timer, Self::Breaks, Self::Alerts, Self::Dock, Self::General];

    fn label(self) -> &'static str {
        match self {
            Self::Timer => "Timer",
            Self::Breaks => "Breaks",
            Self::Alerts => "Alerts",
            Self::Dock => "Dock",
            Self::General => "General",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Limit,
    Step,
    Break,
    Idle,
    Every,
    Snooze,
    HideAfter,
    RevealFor,
}

impl Field {
    fn unit(self) -> &'static str {
        match self {
            Self::Idle | Self::HideAfter | Self::RevealFor => "sec",
            _ => "min",
        }
    }

    fn range(self) -> (u64, u64) {
        match self {
            Self::Limit => (5, 240),
            Self::Step => (1, 30),
            Self::Break => (1, 30),
            Self::Idle => (15, 600),
            Self::Every => (2, 60),
            Self::Snooze => (1, 60),
            Self::HideAfter => (1, 30),
            Self::RevealFor => (3, 60),
        }
    }

    fn get(self, s: &Settings) -> u64 {
        match self {
            Self::Limit => s.sitting_limit_secs / 60,
            Self::Step => s.step,
            Self::Break => s.break_duration_secs / 60,
            Self::Idle => s.idle_after_secs,
            Self::Every => s.repeat_every_secs / 60,
            Self::Snooze => s.snooze_secs / 60,
            Self::HideAfter => s.dock_hide_after_secs,
            Self::RevealFor => s.dock_reveal_secs,
        }
    }

    fn set(self, s: &mut Settings, v: u64) {
        let (lo, hi) = self.range();
        let v = v.clamp(lo, hi);
        match self {
            Self::Limit => s.sitting_limit_secs = v * 60,
            Self::Step => s.step = v,
            Self::Break => s.break_duration_secs = v * 60,
            Self::Idle => s.idle_after_secs = v,
            Self::Every => s.repeat_every_secs = v * 60,
            Self::Snooze => s.snooze_secs = v * 60,
            Self::HideAfter => s.dock_hide_after_secs = v,
            Self::RevealFor => s.dock_reveal_secs = v,
        }
    }

    /// The limit moves by the user's adjust step; seconds move by 5; others by 1.
    fn delta(self, s: &Settings) -> u64 {
        match self {
            Self::Limit => s.step.max(1),
            Self::Idle => 5,
            _ => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flag {
    PauseInactive,
    Sound,
    Repeat,
    Notify,
    Auto,
    OnTop,
    Hotkeys,
    Stats,
    Dock,
    DockAutohide,
    DockFullscreen,
}

impl Flag {
    fn get(self, s: &Settings) -> bool {
        match self {
            Self::PauseInactive => s.inactivity_behavior == InactivityBehavior::Pause,
            Self::Sound => s.sound_enabled,
            Self::Repeat => s.repeat_reminders,
            Self::Notify => s.notify_enabled,
            Self::Auto => s.launch_with_windows,
            Self::OnTop => s.always_on_top,
            Self::Hotkeys => s.hotkeys_enabled,
            Self::Stats => s.stats_enabled,
            Self::Dock => s.dock_enabled,
            Self::DockAutohide => s.dock_autohide,
            Self::DockFullscreen => s.dock_hide_fullscreen,
        }
    }

    fn set(self, s: &mut Settings, on: bool) {
        match self {
            Self::PauseInactive => {
                s.inactivity_behavior = if on {
                    InactivityBehavior::Pause
                } else {
                    InactivityBehavior::Continue
                }
            }
            Self::Sound => s.sound_enabled = on,
            Self::Repeat => s.repeat_reminders = on,
            Self::Notify => s.notify_enabled = on,
            Self::Auto => s.launch_with_windows = on,
            Self::OnTop => s.always_on_top = on,
            Self::Hotkeys => s.hotkeys_enabled = on,
            Self::Stats => s.stats_enabled = on,
            Self::Dock => s.dock_enabled = on,
            Self::DockAutohide => s.dock_autohide = on,
            Self::DockFullscreen => s.dock_hide_fullscreen = on,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Display,
    Size,
    SoundKind,
    Length,
    Snooze,
    Theme,
    DockEdge,
    DockReveal,
}

impl Group {
    fn options(self) -> Vec<String> {
        match self {
            Self::Display => TimerDisplayMode::ALL
                .iter()
                .map(|m| m.chip_label().to_string())
                .collect(),
            Self::Size => WidgetSize::ALL
                .iter()
                .map(|s| s.chip_label().to_string())
                .collect(),
            Self::SoundKind => AlertSound::ALL
                .iter()
                .map(|s| s.chip_label().to_string())
                .collect(),
            Self::Length => ALERT_DURATION_PRESETS_SECS
                .iter()
                .map(|&s| if s == 0 { "Once".into() } else { format!("{s}s") })
                .collect(),
            Self::Snooze => SNOOZE_PRESETS_SECS
                .iter()
                .map(|s| format!("{}m", s / 60))
                .collect(),
            Self::Theme => ThemePreference::ALL
                .iter()
                .map(|t| t.chip_label().to_string())
                .collect(),
            Self::DockEdge => DockEdge::ALL
                .iter()
                .map(|e| e.chip_label().to_string())
                .collect(),
            Self::DockReveal => DockReveal::ALL
                .iter()
                .map(|r| r.chip_label().to_string())
                .collect(),
        }
    }

    fn segment_w(self) -> f32 {
        match self {
            Self::Display => 84.0,
            Self::Size => 36.0,
            Self::Length | Self::Snooze => 46.0,
            Self::Theme => 64.0,
            Self::DockEdge => 58.0,
            Self::DockReveal => 70.0,
            Self::SoundKind => 0.0,
        }
    }

    /// Selected option, or `None` when the value matches no preset (custom snooze).
    fn get(self, s: &Settings) -> Option<usize> {
        match self {
            Self::Display => TimerDisplayMode::ALL
                .iter()
                .position(|&m| m == s.timer_display_mode),
            Self::Size => Some(s.widget_size.index()),
            Self::SoundKind => Some(s.alert_sound.index()),
            Self::Length => ALERT_DURATION_PRESETS_SECS
                .iter()
                .position(|&d| d == s.alert_duration_secs),
            Self::Snooze => SNOOZE_PRESETS_SECS.iter().position(|&d| d == s.snooze_secs),
            Self::Theme => ThemePreference::ALL.iter().position(|&t| t == s.theme),
            Self::DockEdge => DockEdge::ALL
                .iter()
                .position(|&e| e == s.dock_edge.unwrap_or_default()),
            Self::DockReveal => DockReveal::ALL.iter().position(|&r| r == s.dock_reveal),
        }
    }

    fn set(self, s: &mut Settings, i: usize) {
        match self {
            Self::Display => s.timer_display_mode = TimerDisplayMode::from_index(i),
            Self::Size => s.widget_size = WidgetSize::from_index(i),
            Self::SoundKind => s.alert_sound = AlertSound::from_index(i),
            Self::Length => {
                if let Some(&d) = ALERT_DURATION_PRESETS_SECS.get(i) {
                    s.alert_duration_secs = d;
                }
            }
            Self::Snooze => {
                if let Some(&d) = SNOOZE_PRESETS_SECS.get(i) {
                    s.snooze_secs = d;
                }
            }
            Self::Theme => s.theme = ThemePreference::from_index(i),
            Self::DockEdge => s.dock_edge = Some(DockEdge::from_index(i)),
            Self::DockReveal => s.dock_reveal = DockReveal::from_index(i),
        }
    }
}

#[derive(Clone, Copy)]
enum Row {
    Step(&'static str, Field, Option<&'static str>),
    Flag(&'static str, Flag, Option<&'static str>),
    Group(&'static str, Group),
    /// Options on their own line under the label.
    WideGroup(&'static str, Group),
    Volume,
    Note(&'static str),
    Stats,
    Defaults,
}

impl Row {
    fn height(self) -> f32 {
        match self {
            Row::Step(_, _, Some(_)) | Row::Flag(_, _, Some(_)) => 58.0,
            Row::Step(..) | Row::Flag(..) | Row::Group(..) => 46.0,
            Row::WideGroup(..) => 80.0,
            Row::Volume => 48.0,
            Row::Note(_) => 38.0,
            Row::Stats => 112.0,
            Row::Defaults => 48.0,
        }
    }

    /// Notes sit under the tile above them rather than in a tile of their own.
    fn tiled(self) -> bool {
        !matches!(self, Row::Note(_))
    }
}

type Section = (&'static str, &'static [Row]);

fn sections(tab: Tab) -> &'static [Section] {
    match tab {
        Tab::Timer => &[
            (
                "Session",
                &[
                    Row::Step("Sitting limit", Field::Limit, None),
                    Row::Step("Adjust step", Field::Step, Some("How far − and + move the limit")),
                    Row::Group("Display", Group::Display),
                ],
            ),
            (
                "Overlay",
                &[
                    Row::Group("Size", Group::Size),
                    Row::Flag("Always on top", Flag::OnTop, Some("Keep the timer above other windows")),
                ],
            ),
        ],
        Tab::Breaks => &[
            (
                "Breaks",
                &[
                    Row::Step("Break length", Field::Break, None),
                    Row::Note("Start one from the overlay, the tray or Ctrl+Alt+Shift+B. A fresh session follows."),
                ],
            ),
            (
                "Inactivity",
                &[
                    Row::Flag(
                        "Pause when inactive",
                        Flag::PauseInactive,
                        Some("Off: the timer keeps running while you're away"),
                    ),
                    Row::Step("Inactive after", Field::Idle, Some("No input, or a locked screen")),
                ],
            ),
            (
                "Snooze",
                &[
                    Row::Group("Snooze for", Group::Snooze),
                    Row::Step("Custom", Field::Snooze, None),
                ],
            ),
        ],
        Tab::Alerts => &[
            (
                "Sound",
                &[
                    Row::Flag("Play a sound", Flag::Sound, None),
                    Row::WideGroup("Sound", Group::SoundKind),
                    Row::Volume,
                    Row::Group("Alert length", Group::Length),
                ],
            ),
            (
                "Reminders",
                &[
                    Row::Flag("Show notification", Flag::Notify, Some("When time is up and when a break ends")),
                    Row::Flag("Repeat while overdue", Flag::Repeat, None),
                    Row::Step("Remind every", Field::Every, None),
                ],
            ),
        ],
        Tab::Dock => &[
            (
                "Edge",
                &[
                    Row::Flag("Dock to a screen edge", Flag::Dock, Some("Or drag the overlay against any edge")),
                    Row::Group("Edge", Group::DockEdge),
                ],
            ),
            (
                "Auto-hide",
                &[
                    Row::Flag("Slide out when not in use", Flag::DockAutohide, Some("Off keeps it open, like the pin")),
                    Row::Step("Hide after", Field::HideAfter, None),
                ],
            ),
            (
                "Alerts",
                &[
                    Row::Group("Reveal for", Group::DockReveal),
                    Row::Step("Stay revealed", Field::RevealFor, None),
                    Row::Flag(
                        "Stay hidden over fullscreen apps",
                        Flag::DockFullscreen,
                        Some("Games, presentations and video"),
                    ),
                ],
            ),
        ],
        Tab::General => &[
            ("Appearance", &[Row::Group("Theme", Group::Theme)]),
            (
                "System",
                &[
                    Row::Flag("Launch with Windows", Flag::Auto, None),
                    Row::Flag("Keyboard shortcuts", Flag::Hotkeys, Some("Ctrl+Alt+Shift + P, R, S, B or U")),
                ],
            ),
            (
                "Statistics",
                &[
                    Row::Flag("Daily statistics", Flag::Stats, Some("Kept on this PC for 14 days")),
                    Row::Stats,
                ],
            ),
            ("", &[Row::Defaults]),
        ],
    }
}

/// A focusable or clickable control.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ctl {
    Close,
    Tabs,
    Step(Field),
    Flag(Flag),
    Group(Group),
    Volume,
    Preview,
    Defaults,
    Cancel,
    Save,
}

/// Which piece of a control the pointer is over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Whole,
    Minus,
    Plus,
    Option(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Target {
    ctl: Ctl,
    part: Part,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TextKind {
    Title,
    Head,
    Label,
    Hint,
    Note,
}

struct Label {
    text: &'static str,
    rect: D2D_RECT_F,
    kind: TextKind,
    /// Dim the label when this control is disabled.
    owner: Option<Ctl>,
}

struct Placed {
    ctl: Ctl,
    rect: D2D_RECT_F,
    parts: Vec<(Part, D2D_RECT_F)>,
    in_body: bool,
}

struct Lay {
    scale: f32,
    w: f32,
    h: f32,
    body_top: f32,
    body_bottom: f32,
    content_h: f32,
    tiles: Vec<D2D_RECT_F>,
    labels: Vec<Label>,
    controls: Vec<Placed>,
    stats: Option<D2D_RECT_F>,
    status: D2D_RECT_F,
}

impl Lay {
    fn find(&self, ctl: Ctl) -> Option<&Placed> {
        self.controls.iter().find(|p| p.ctl == ctl)
    }
}

fn px(v: f32, scale: f32) -> f32 {
    v * scale
}

fn row_visible(row: Row, draft: &Settings) -> bool {
    !matches!(row, Row::Stats) || draft.stats_enabled
}

fn content_height(tab: Tab, draft: &Settings) -> f32 {
    let mut y = 0.0;
    for (title, rows) in sections(tab) {
        if !title.is_empty() {
            y += SECTION_HEAD;
        }
        let visible: Vec<Row> = rows.iter().copied().filter(|r| row_visible(*r, draft)).collect();
        y += visible.iter().map(|r| r.height()).sum::<f32>();
        y += ROW_GAP * visible.len().saturating_sub(1) as f32;
        y += SECTION_GAP;
    }
    y + 6.0
}

/// Window height that fits the tallest tab, so switching tabs never resizes the window.
fn window_logical_h() -> f32 {
    let mut tallest = Settings::default();
    tallest.stats_enabled = true;
    let body = Tab::ALL
        .iter()
        .map(|&t| content_height(t, &tallest))
        .fold(0.0, f32::max);
    BODY_TOP + body + FOOTER_H
}

fn layout(tab: Tab, draft: &Settings, scale: f32, win_h: f32, scroll: f32) -> Lay {
    let s = scale;
    let w = px(WIN_W, s);
    let h = win_h;
    let x0 = px(PAD, s);
    let x1 = w - px(PAD, s);
    let body_top = px(BODY_TOP, s);
    let body_bottom = h - px(FOOTER_H, s);
    let mut labels = Vec::new();
    let mut controls = Vec::new();
    let mut tiles = Vec::new();
    let mut stats = None;

    // Header: the tab's name, a live status chip and close.
    labels.push(Label {
        text: tab.label(),
        rect: gfx::rect(x0 + px(2.0, s), px(14.0, s), x0 + px(170.0, s), px(50.0, s)),
        kind: TextKind::Title,
        owner: None,
    });
    let close = gfx::rect(x1 - px(32.0, s), px(17.0, s), x1, px(49.0, s));
    controls.push(Placed {
        ctl: Ctl::Close,
        rect: close,
        parts: vec![],
        in_body: false,
    });
    let status = gfx::rect(close.left - px(8.0, s) - px(150.0, s), px(19.0, s), close.left - px(8.0, s), px(47.0, s));

    let tabs = gfx::rect(x0, px(HEADER_H, s), x1, px(HEADER_H + TABS_H, s));
    let gap = px(4.0, s);
    let n = Tab::ALL.len() as f32;
    let seg = (tabs.right - tabs.left - gap * (n - 1.0)) / n;
    controls.push(Placed {
        ctl: Ctl::Tabs,
        rect: tabs,
        parts: (0..Tab::ALL.len())
            .map(|i| {
                let l = tabs.left + (seg + gap) * i as f32;
                (Part::Option(i), gfx::rect(l, tabs.top, l + seg, tabs.bottom))
            })
            .collect(),
        in_body: false,
    });

    let mut y = body_top - scroll;
    for (title, rows) in sections(tab) {
        if !title.is_empty() {
            labels.push(Label {
                text: title,
                rect: gfx::rect(x0 + px(4.0, s), y, x1, y + px(SECTION_HEAD, s)),
                kind: TextKind::Head,
                owner: None,
            });
            y += px(SECTION_HEAD, s);
        }
        let mut first = true;
        for &row in rows.iter().filter(|r| row_visible(**r, draft)) {
            if !first {
                y += px(ROW_GAP, s);
            }
            first = false;
            let rh = px(row.height(), s);
            let rc = gfx::rect(x0, y, x1, y + rh);
            if row.tiled() {
                tiles.push(rc);
            }
            place_row(row, rc, s, &mut labels, &mut controls, &mut stats);
            y += rh;
        }
        y += px(SECTION_GAP, s);
    }

    let btn_h = px(40.0, s);
    let by = h - px(FOOTER_H, s) + (px(FOOTER_H, s) - btn_h) * 0.5;
    let mid = (x0 + x1) / 2.0;
    controls.push(Placed {
        ctl: Ctl::Cancel,
        rect: gfx::rect(x0, by, mid - px(4.0, s), by + btn_h),
        parts: vec![],
        in_body: false,
    });
    controls.push(Placed {
        ctl: Ctl::Save,
        rect: gfx::rect(mid + px(4.0, s), by, x1, by + btn_h),
        parts: vec![],
        in_body: false,
    });

    Lay {
        scale,
        w,
        h,
        body_top,
        body_bottom,
        content_h: px(content_height(tab, draft), s),
        tiles,
        labels,
        controls,
        stats,
        status,
    }
}

fn place_row(
    row: Row,
    rc: D2D_RECT_F,
    s: f32,
    labels: &mut Vec<Label>,
    controls: &mut Vec<Placed>,
    stats: &mut Option<D2D_RECT_F>,
) {
    let inner_l = rc.left + px(14.0, s);
    let inner_r = rc.right - px(12.0, s);
    let mid = (rc.top + rc.bottom) * 0.5;
    let mut label = |text: &'static str, hint: Option<&'static str>, right: f32, owner: Ctl| {
        match hint {
            Some(hint) => {
                labels.push(Label {
                    text,
                    rect: gfx::rect(inner_l, rc.top + px(9.0, s), right, rc.top + px(29.0, s)),
                    kind: TextKind::Label,
                    owner: Some(owner),
                });
                labels.push(Label {
                    text: hint,
                    rect: gfx::rect(inner_l, rc.top + px(28.0, s), right, rc.top + px(46.0, s)),
                    kind: TextKind::Hint,
                    owner: Some(owner),
                });
            }
            None => labels.push(Label {
                text,
                rect: gfx::rect(inner_l, rc.top, right, rc.bottom),
                kind: TextKind::Label,
                owner: Some(owner),
            }),
        }
    };
    match row {
        Row::Step(text, field, hint) => {
            let bh = px(28.0, s);
            let bw = px(28.0, s);
            let well = px(66.0, s);
            let gap = px(4.0, s);
            let top = mid - bh * 0.5;
            let plus = gfx::rect(inner_r - bw, top, inner_r, top + bh);
            let well_rc = gfx::rect(plus.left - gap - well, top, plus.left - gap, top + bh);
            let minus = gfx::rect(well_rc.left - gap - bw, top, well_rc.left - gap, top + bh);
            label(text, hint, minus.left - px(8.0, s), Ctl::Step(field));
            controls.push(Placed {
                ctl: Ctl::Step(field),
                rect: gfx::rect(minus.left, top, plus.right, top + bh),
                parts: vec![
                    (Part::Minus, minus),
                    (Part::Whole, well_rc),
                    (Part::Plus, plus),
                ],
                in_body: true,
            });
        }
        Row::Flag(text, flag, hint) => {
            let tw = px(40.0, s);
            let th = px(22.0, s);
            let track = gfx::rect(inner_r - tw, mid - th * 0.5, inner_r, mid + th * 0.5);
            label(text, hint, track.left - px(8.0, s), Ctl::Flag(flag));
            // The whole row toggles, like Windows settings.
            controls.push(Placed {
                ctl: Ctl::Flag(flag),
                rect: rc,
                parts: vec![(Part::Whole, track)],
                in_body: true,
            });
        }
        Row::Group(text, group) => {
            let n = group.options().len();
            let sw = px(group.segment_w(), s);
            let sh = px(28.0, s);
            let left = inner_r - sw * n as f32;
            let top = mid - sh * 0.5;
            label(text, None, left - px(8.0, s), Ctl::Group(group));
            controls.push(Placed {
                ctl: Ctl::Group(group),
                rect: gfx::rect(left, top, inner_r, top + sh),
                parts: (0..n)
                    .map(|i| {
                        let l = left + sw * i as f32;
                        (Part::Option(i), gfx::rect(l, top, l + sw, top + sh))
                    })
                    .collect(),
                in_body: true,
            });
        }
        Row::WideGroup(text, group) => {
            labels.push(Label {
                text,
                rect: gfx::rect(inner_l, rc.top + px(4.0, s), inner_r, rc.top + px(36.0, s)),
                kind: TextKind::Label,
                owner: Some(Ctl::Group(group)),
            });
            let n = group.options().len();
            let left = rc.left + px(10.0, s);
            let right = rc.right - px(10.0, s);
            let top = rc.top + px(36.0, s);
            let sh = px(30.0, s);
            let sw = (right - left) / n as f32;
            controls.push(Placed {
                ctl: Ctl::Group(group),
                rect: gfx::rect(left, top, right, top + sh),
                parts: (0..n)
                    .map(|i| {
                        let l = left + sw * i as f32;
                        (Part::Option(i), gfx::rect(l, top, l + sw, top + sh))
                    })
                    .collect(),
                in_body: true,
            });
        }
        Row::Volume => {
            let bh = px(28.0, s);
            let top = mid - bh * 0.5;
            let preview = gfx::rect(inner_r - px(76.0, s), top, inner_r, top + bh);
            let track_l = inner_l + px(70.0, s);
            let track_r = preview.left - px(52.0, s);
            label("Volume", None, track_l - px(12.0, s), Ctl::Volume);
            controls.push(Placed {
                ctl: Ctl::Volume,
                rect: gfx::rect(track_l, top, track_r, top + bh),
                parts: vec![],
                in_body: true,
            });
            controls.push(Placed {
                ctl: Ctl::Preview,
                rect: preview,
                parts: vec![],
                in_body: true,
            });
        }
        Row::Note(text) => labels.push(Label {
            text,
            rect: gfx::rect(rc.left + px(4.0, s), rc.top + px(2.0, s), inner_r, rc.bottom),
            kind: TextKind::Note,
            owner: None,
        }),
        Row::Stats => {
            *stats = Some(gfx::rect(
                inner_l,
                rc.top + px(8.0, s),
                inner_r,
                rc.bottom - px(8.0, s),
            ))
        }
        Row::Defaults => {
            let bh = px(30.0, s);
            let top = mid - bh * 0.5;
            let button = gfx::rect(inner_r - px(96.0, s), top, inner_r, top + bh);
            labels.push(Label {
                text: "Restore default settings",
                rect: gfx::rect(inner_l, rc.top, button.left - px(8.0, s), rc.bottom),
                kind: TextKind::Label,
                owner: None,
            });
            controls.push(Placed {
                ctl: Ctl::Defaults,
                rect: button,
                parts: vec![],
                in_body: true,
            });
        }
    }
}

fn enabled(ctl: Ctl, d: &Settings) -> bool {
    let docking = d.dock_enabled;
    match ctl {
        Ctl::Step(Field::Idle) => d.inactivity_behavior == InactivityBehavior::Pause,
        Ctl::Step(Field::Every) => d.repeat_reminders,
        Ctl::Group(Group::SoundKind) | Ctl::Group(Group::Length) | Ctl::Volume | Ctl::Preview => {
            d.sound_enabled
        }
        Ctl::Step(Field::HideAfter) => docking && d.dock_autohide,
        Ctl::Group(Group::DockEdge)
        | Ctl::Group(Group::DockReveal)
        | Ctl::Step(Field::RevealFor)
        | Ctl::Flag(Flag::DockAutohide)
        | Ctl::Flag(Flag::DockFullscreen) => docking,
        _ => true,
    }
}

/// Keyboard focus order: tab strip, the active tab's controls top to bottom, then the footer.
fn focus_order(l: &Lay, d: &Settings) -> Vec<Ctl> {
    let mut order = vec![Ctl::Tabs];
    order.extend(
        l.controls
            .iter()
            .filter(|p| p.in_body && enabled(p.ctl, d))
            .map(|p| p.ctl),
    );
    order.extend([Ctl::Cancel, Ctl::Save]);
    order
}

fn hit(l: &Lay, x: f32, y: f32) -> Option<Target> {
    for p in &l.controls {
        if p.in_body && (y < l.body_top || y >= l.body_bottom) {
            continue;
        }
        for (part, rc) in &p.parts {
            if gfx::contains(*rc, x, y) {
                return Some(Target {
                    ctl: p.ctl,
                    part: *part,
                });
            }
        }
        if gfx::contains(p.rect, x, y) {
            return Some(Target {
                ctl: p.ctl,
                part: Part::Whole,
            });
        }
    }
    None
}

fn fit_window_to_work_area(work: RECT, scale: f32) -> (i32, i32, i32, i32) {
    let available_w = (work.right - work.left - WORK_AREA_MARGIN * 2).max(1);
    let available_h = (work.bottom - work.top - WORK_AREA_MARGIN * 2).max(1);
    let width = ((WIN_W * scale).round() as i32).min(available_w);
    let height = ((window_logical_h() * scale).round() as i32).min(available_h);
    let min_x = work.left + WORK_AREA_MARGIN;
    let min_y = work.top + WORK_AREA_MARGIN;
    let max_x = (work.right - WORK_AREA_MARGIN - width).max(min_x);
    let max_y = (work.bottom - WORK_AREA_MARGIN - height).max(min_y);
    let x = work.left + (work.right - work.left - width) / 2;
    let y = work.top + (work.bottom - work.top - height) / 2;
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y), width, height)
}

fn max_scroll(l: &Lay) -> f32 {
    (l.content_h - (l.body_bottom - l.body_top)).max(0.0)
}

fn scroll_after_wheel(current: f32, wheel_delta: i16, scale: f32, max_scroll: f32) -> f32 {
    let step = px(60.0, scale) * wheel_delta as f32 / 120.0;
    (current - step).clamp(0.0, max_scroll)
}

struct Dialog {
    app: *mut App,
    draft: Settings,
    /// Overlay appearance to restore if the dialog is cancelled.
    revert: (WidgetSize, ThemePreference, bool),
    saved: bool,
    tab: Tab,
    hover: Option<Target>,
    pressed: Option<Target>,
    focus: Ctl,
    focus_visible: bool,
    dragging_volume: bool,
    tracking: bool,
    scale: f32,
    scroll: f32,
    factory: Option<ID2D1Factory>,
    fonts: Option<Fonts>,
    rt: Option<ID2D1HwndRenderTarget>,
}

impl Dialog {
    fn lay(&self, hwnd: HWND) -> Lay {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
        }
        let h = (rc.bottom - rc.top).max(1) as f32;
        layout(self.tab, &self.draft, self.scale, h, self.scroll)
    }

    fn dark(&self) -> bool {
        let system_dark = if self.app.is_null() {
            true
        } else {
            unsafe { (*self.app).system_dark }
        };
        self.draft.theme.is_dark(system_dark)
    }
}

fn title() -> windows::core::HSTRING {
    crate::win::testhook::title("Unseat Settings")
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

pub fn open(app: *mut App) {
    unsafe {
        if let Ok(existing) = FindWindowW(CLASS, &title()) {
            let _ = ShowWindow(existing, SW_SHOW);
            let _ = SetForegroundWindow(existing);
            return;
        }
        if let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOPMOST,
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
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&pref) as u32,
            );
            crate::win::icon::apply(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
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

/// Re-theme an open dialog after the system theme changes.
pub fn refresh() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(CLASS, &title()) {
            let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
            if !dlg.is_null() {
                apply_frame_theme(hwnd, (*dlg).dark());
            }
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

unsafe fn apply_frame_theme(hwnd: HWND, dark: bool) {
    let value: i32 = dark.into();
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &value as *const _ as *const core::ffi::c_void,
        4,
    );
}

unsafe fn dpi_scale(hwnd: HWND) -> f32 {
    match GetDpiForWindow(hwnd) {
        0 => 1.0,
        d => d as f32 / 96.0,
    }
}

/// Size the window for `scale` and center it on the work area of the monitor nearest `near`.
unsafe fn place_window(hwnd: HWND, scale: f32, near: POINT) {
    let monitor = MonitorFromPoint(near, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 800,
        }
    };
    let (x, y, w, h) = fit_window_to_work_area(work, scale);
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
}

fn lparam_point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 as i16) as i32, ((lp.0 >> 16) as i16) as i32)
}

unsafe fn key_down(vk: VIRTUAL_KEY) -> bool {
    GetKeyState(vk.0 as i32) < 0
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let cs = lp.0 as *const CREATESTRUCTW;
        let app = if cs.is_null() {
            std::ptr::null_mut()
        } else {
            (*cs).lpCreateParams as *mut App
        };
        let dlg = build(hwnd, app);
        apply_frame_theme(hwnd, dlg.dark());
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(dlg) as isize);
        return LRESULT(0);
    }
    let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
    if dlg.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let d = &mut *dlg;
    match msg {
        WM_NCHITTEST => {
            let (sx, sy) = lparam_point(lp);
            let mut pt = POINT { x: sx, y: sy };
            let _ = ScreenToClient(hwnd, &mut pt);
            let l = d.lay(hwnd);
            let in_chrome = (pt.y as f32) < l.body_top || (pt.y as f32) >= l.body_bottom;
            if in_chrome && hit(&l, pt.x as f32, pt.y as f32).is_none() {
                LRESULT(HTCAPTION as isize)
            } else {
                LRESULT(HTCLIENT as isize)
            }
        }
        WM_MOUSEMOVE => {
            if !d.tracking {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                d.tracking = TrackMouseEvent(&mut tme).is_ok();
            }
            let (x, y) = lparam_point(lp);
            if d.dragging_volume {
                set_volume_from_x(d, hwnd, x as f32);
                return LRESULT(0);
            }
            let h = hit(&d.lay(hwnd), x as f32, y as f32);
            if h != d.hover {
                d.hover = h;
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            d.tracking = false;
            if d.hover.is_some() {
                d.hover = None;
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let l = d.lay(hwnd);
            let delta = ((wp.0 >> 16) as u16) as i16;
            let next = scroll_after_wheel(d.scroll, delta, d.scale, max_scroll(&l));
            if next != d.scroll {
                d.scroll = next;
                d.hover = None;
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_point(lp);
            let target = hit(&d.lay(hwnd), x as f32, y as f32);
            d.focus_visible = false;
            d.pressed = target;
            if let Some(t) = target {
                if t.ctl != Ctl::Close {
                    d.focus = t.ctl;
                }
                if t.ctl == Ctl::Volume && enabled(Ctl::Volume, &d.draft) {
                    d.dragging_volume = true;
                    let _ = SetCapture(hwnd);
                    set_volume_from_x(d, hwnd, x as f32);
                } else {
                    activate(hwnd, dlg, t);
                    return LRESULT(0);
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if d.dragging_volume {
                d.dragging_volume = false;
                let _ = ReleaseCapture();
            }
            d.pressed = None;
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            on_key(hwnd, dlg, VIRTUAL_KEY(wp.0 as u16));
            LRESULT(0)
        }
        // Keys are fully handled in WM_KEYDOWN; swallow the characters (no beeps).
        WM_CHAR => LRESULT(0),
        WM_DPICHANGED => {
            d.scale = (wp.0 & 0xFFFF) as f32 / 96.0;
            d.rt = None;
            d.scroll = 0.0;
            let suggested = *(lp.0 as *const RECT);
            place_window(
                hwnd,
                d.scale,
                POINT {
                    x: (suggested.left + suggested.right) / 2,
                    y: (suggested.top + suggested.bottom) / 2,
                },
            );
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            paint_ui(hwnd, d);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_CLOSE => {
            close(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(dlg));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn build(hwnd: HWND, app: *mut App) -> Box<Dialog> {
    let draft = if app.is_null() {
        Settings::default()
    } else {
        let mut draft = (*app).settings.clone();
        // Show the real startup state: the entry may have been removed or disabled elsewhere.
        draft.launch_with_windows = crate::win::autostart::is_enabled();
        draft
    };
    let mut cursor = POINT::default();
    let _ = GetCursorPos(&mut cursor);
    place_window(hwnd, dpi_scale(hwnd), cursor);
    // The target monitor may use a different DPI than where the window was created.
    let scale = dpi_scale(hwnd);
    place_window(hwnd, scale, cursor);
    Box::new(Dialog {
        app,
        revert: (draft.widget_size, draft.theme, draft.always_on_top),
        draft,
        saved: false,
        tab: Tab::Timer,
        hover: None,
        pressed: None,
        focus: Ctl::Tabs,
        focus_visible: false,
        dragging_volume: false,
        tracking: false,
        scale,
        scroll: 0.0,
        factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok(),
        fonts: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
            .ok()
            .map(Fonts::new),
        rt: None,
    })
}

unsafe fn set_volume_from_x(d: &mut Dialog, hwnd: HWND, x: f32) {
    let l = d.lay(hwnd);
    if let Some(p) = l.find(Ctl::Volume) {
        let t = ((x - p.rect.left) / (p.rect.right - p.rect.left)).clamp(0.0, 1.0);
        let v = ((t * 100.0 / 5.0).round() * 5.0) as u8;
        if v != d.draft.alert_volume {
            d.draft.alert_volume = v;
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

/// Push appearance choices to the overlay right away so the user sees them.
unsafe fn preview_appearance(d: &Dialog) {
    if !d.app.is_null() {
        (*d.app).preview_appearance(d.draft.widget_size, d.draft.theme, d.draft.always_on_top);
    }
}

unsafe fn activate(hwnd: HWND, dlg: *mut Dialog, t: Target) {
    let d = &mut *dlg;
    if !enabled(t.ctl, &d.draft) {
        return;
    }
    match (t.ctl, t.part) {
        (Ctl::Close, _) | (Ctl::Cancel, _) => {
            close(hwnd);
            return;
        }
        (Ctl::Save, _) => {
            save(hwnd);
            return;
        }
        (Ctl::Tabs, Part::Option(i)) => select_tab(d, Tab::ALL[i]),
        (Ctl::Step(field), Part::Minus) => step(d, field, -1),
        (Ctl::Step(field), Part::Plus) => step(d, field, 1),
        (Ctl::Flag(flag), _) => {
            let next = !flag.get(&d.draft);
            flag.set(&mut d.draft, next);
            if flag == Flag::OnTop {
                preview_appearance(d);
            }
        }
        (Ctl::Group(group), Part::Option(i)) => choose(hwnd, d, group, i),
        (Ctl::Preview, _) => {
            if !d.app.is_null() {
                (*d.app).preview_sound(
                    d.draft.alert_sound,
                    d.draft.alert_duration_secs,
                    d.draft.alert_volume,
                );
            }
        }
        (Ctl::Defaults, _) => {
            d.draft = d.draft.with_default_preferences();
            preview_appearance(d);
            apply_frame_theme(hwnd, d.dark());
        }
        _ => {}
    }
    let _ = InvalidateRect(hwnd, None, false);
}

fn select_tab(d: &mut Dialog, tab: Tab) {
    if d.tab != tab {
        d.tab = tab;
        d.scroll = 0.0;
        d.hover = None;
    }
}

fn step(d: &mut Dialog, field: Field, dir: i64) {
    let current = field.get(&d.draft);
    let delta = field.delta(&d.draft);
    let next = if dir < 0 {
        current.saturating_sub(delta)
    } else {
        current.saturating_add(delta)
    };
    field.set(&mut d.draft, next);
}

unsafe fn choose(hwnd: HWND, d: &mut Dialog, group: Group, i: usize) {
    group.set(&mut d.draft, i);
    match group {
        Group::Size => preview_appearance(d),
        Group::Theme => {
            preview_appearance(d);
            apply_frame_theme(hwnd, d.dark());
        }
        _ => {}
    }
}

unsafe fn on_key(hwnd: HWND, dlg: *mut Dialog, vk: VIRTUAL_KEY) {
    let d = &mut *dlg;
    let ctrl = key_down(VK_CONTROL);
    let shift = key_down(VK_SHIFT);
    let order = focus_order(&d.lay(hwnd), &d.draft);
    let pos = order.iter().position(|c| *c == d.focus);
    let n_tabs = Tab::ALL.len();
    let tab_i = d.tab.index();
    match vk {
        VK_ESCAPE => {
            close(hwnd);
            return;
        }
        VK_TAB if ctrl => {
            select_tab(
                d,
                Tab::ALL[if shift {
                    (tab_i + n_tabs - 1) % n_tabs
                } else {
                    (tab_i + 1) % n_tabs
                }],
            );
            d.focus = Ctl::Tabs;
            d.focus_visible = true;
        }
        VK_NEXT | VK_PRIOR if ctrl => {
            let next = if vk == VK_PRIOR {
                (tab_i + n_tabs - 1) % n_tabs
            } else {
                (tab_i + 1) % n_tabs
            };
            select_tab(d, Tab::ALL[next]);
        }
        VK_TAB | VK_DOWN | VK_UP
            if vk == VK_TAB || !matches!(d.focus, Ctl::Volume | Ctl::Step(_)) =>
        {
            let forward = if vk == VK_TAB { !shift } else { vk == VK_DOWN };
            let n = order.len();
            let i = pos.map_or(0, |p| if forward { (p + 1) % n } else { (p + n - 1) % n });
            d.focus = order[i];
            d.focus_visible = true;
        }
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_HOME | VK_END => {
            let dir: i64 = if matches!(vk, VK_LEFT | VK_DOWN | VK_HOME) {
                -1
            } else {
                1
            };
            d.focus_visible = true;
            match d.focus {
                Ctl::Tabs => {
                    let i = (tab_i as i64 + dir).rem_euclid(n_tabs as i64) as usize;
                    select_tab(d, Tab::ALL[i]);
                }
                Ctl::Step(field) => step(d, field, dir),
                Ctl::Group(group) => {
                    let n = group.options().len() as i64;
                    let i = group.get(&d.draft).map_or(0, |i| i as i64);
                    choose(hwnd, d, group, (i + dir).clamp(0, n - 1) as usize);
                }
                Ctl::Volume => {
                    let v = i64::from(d.draft.alert_volume);
                    d.draft.alert_volume = match vk {
                        VK_HOME => 0,
                        VK_END => 100,
                        _ => (v + dir * 5).clamp(0, 100) as u8,
                    };
                }
                Ctl::Flag(flag) if matches!(vk, VK_LEFT | VK_RIGHT) => {
                    flag.set(&mut d.draft, vk == VK_RIGHT);
                    if flag == Flag::OnTop {
                        preview_appearance(d);
                    }
                }
                _ => {}
            }
        }
        VK_SPACE => {
            d.focus_visible = true;
            if !matches!(
                d.focus,
                Ctl::Tabs | Ctl::Step(_) | Ctl::Group(_) | Ctl::Volume
            ) {
                activate(
                    hwnd,
                    dlg,
                    Target {
                        ctl: d.focus,
                        part: Part::Whole,
                    },
                );
                return;
            }
        }
        VK_RETURN => {
            if matches!(
                d.focus,
                Ctl::Cancel | Ctl::Close | Ctl::Defaults | Ctl::Preview
            ) {
                activate(
                    hwnd,
                    dlg,
                    Target {
                        ctl: d.focus,
                        part: Part::Whole,
                    },
                );
            } else {
                save(hwnd);
            }
            return;
        }
        // Ctrl+1..5 jumps to a tab.
        _ if ctrl && (0x31..=0x35).contains(&vk.0) => {
            select_tab(d, Tab::ALL[usize::from(vk.0 - 0x31)]);
        }
        _ => return,
    }
    ensure_focus_visible(hwnd, d);
    let _ = InvalidateRect(hwnd, None, false);
}

/// Scroll so the focused body control is inside the viewport.
unsafe fn ensure_focus_visible(hwnd: HWND, d: &mut Dialog) {
    let l = d.lay(hwnd);
    let Some(p) = l.find(d.focus).filter(|p| p.in_body) else {
        return;
    };
    let margin = px(8.0, d.scale);
    if p.rect.top < l.body_top + margin {
        d.scroll = (d.scroll - (l.body_top + margin - p.rect.top)).max(0.0);
    } else if p.rect.bottom > l.body_bottom - margin {
        d.scroll = (d.scroll + p.rect.bottom - (l.body_bottom - margin)).min(max_scroll(&l));
    }
}

unsafe fn ensure_rt(hwnd: HWND, d: &mut Dialog) -> Option<ID2D1RenderTarget> {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let size = D2D_SIZE_U {
        width: (rc.right - rc.left).max(1) as u32,
        height: (rc.bottom - rc.top).max(1) as u32,
    };
    if d.rt.is_none() {
        let factory = d.factory.as_ref()?;
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
        d.rt = factory.CreateHwndRenderTarget(&props, &hwnd_props).ok();
    } else if let Some(rt) = d.rt.as_ref() {
        let _ = rt.Resize(&size);
    }
    d.rt.as_ref().and_then(|rt| rt.cast().ok())
}

fn label_style(
    kind: TextKind,
    s: f32,
) -> (f32, windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT) {
    match kind {
        TextKind::Title => (px(24.0, s), gfx::SEMIBOLD),
        TextKind::Head => (px(11.5, s), gfx::SEMIBOLD),
        TextKind::Label => (px(13.0, s), gfx::MEDIUM),
        TextKind::Hint | TextKind::Note => (px(11.0, s), gfx::REGULAR),
    }
}

unsafe fn paint_ui(hwnd: HWND, d: &mut Dialog) {
    let Some(rt) = ensure_rt(hwnd, d) else {
        return;
    };
    let scroll_max = max_scroll(&d.lay(hwnd));
    d.scroll = d.scroll.min(scroll_max);
    let l = d.lay(hwnd);
    let pal = palette(d.dark());
    let d = &*d;
    let Some(fonts) = d.fonts.as_ref() else {
        return;
    };
    let s = l.scale;
    rt.BeginDraw();
    rt.SetTransform(&Matrix3x2::identity());
    rt.Clear(Some(&gfx::color(pal.window)));

    // Body, clipped between the tab chips and the footer. Every setting is its own tile.
    rt.PushAxisAlignedClip(
        &gfx::rect(0.0, l.body_top, l.w, l.body_bottom),
        D2D1_ANTIALIAS_MODE_ALIASED,
    );
    for tile in &l.tiles {
        gfx::fill_round(&rt, *tile, px(TILE_R, s), pal.card);
    }
    if let Some(rc) = l.stats {
        paint_stats(&rt, fonts, pal, d, rc, s);
    }
    for p in l.controls.iter().filter(|p| p.in_body) {
        paint_control(&rt, fonts, pal, d, p, s);
    }
    for label in l.labels.iter().filter(|lb| lb.kind != TextKind::Title) {
        paint_label(&rt, fonts, pal, d, label, s);
    }
    rt.PopAxisAlignedClip();

    // Chrome: title, live status chip, close, tabs and footer.
    for label in l.labels.iter().filter(|lb| lb.kind == TextKind::Title) {
        paint_label(&rt, fonts, pal, d, label, s);
    }
    if !d.app.is_null() {
        let snap = (*d.app).engine.snapshot();
        let dot = match snap.state {
            VisibleState::Overdue => pal.alarm,
            VisibleState::Break => pal.brk.tag,
            VisibleState::Sitting => pal.accent,
            VisibleState::Away | VisibleState::Paused => pal.text_disabled,
        };
        let line = status_line(snap);
        let text_w = gfx::measure(fonts, &line, px(12.0, s), gfx::MEDIUM);
        let chip_w = (text_w + px(34.0, s)).min(l.status.right - l.status.left);
        let chip = gfx::rect(l.status.right - chip_w, l.status.top, l.status.right, l.status.bottom);
        gfx::fill_round(&rt, chip, px(CHIP_R, s), pal.card);
        let cy = (chip.top + chip.bottom) / 2.0;
        let r = px(4.0, s);
        gfx::fill_round(
            &rt,
            gfx::rect(chip.left + px(12.0, s), cy - r, chip.left + px(12.0, s) + 2.0 * r, cy + r),
            r,
            dot,
        );
        gfx::text(
            &rt,
            fonts,
            Family::Ui,
            &line,
            gfx::rect(chip.left + px(26.0, s), chip.top, chip.right - px(8.0, s), chip.bottom),
            px(12.0, s),
            gfx::MEDIUM,
            gfx::LEADING,
            pal.text,
        );
    }
    for p in l.controls.iter().filter(|p| !p.in_body) {
        paint_control(&rt, fonts, pal, d, p, s);
    }
    if scroll_max > 0.0 {
        let view = l.body_bottom - l.body_top;
        let thumb_h = (view * view / l.content_h).max(px(28.0, s));
        let thumb_y = l.body_top + d.scroll / scroll_max * (view - thumb_h);
        gfx::fill_round(
            &rt,
            gfx::rect(l.w - px(6.0, s), thumb_y, l.w - px(3.0, s), thumb_y + thumb_h),
            px(1.5, s),
            gfx::fade(pal.text_secondary, 0.6),
        );
    }
    gfx::stroke_round(
        &rt,
        gfx::rect(0.5, 0.5, l.w - 0.5, l.h - 0.5),
        px(8.0, s),
        pal.window_stroke,
        1.0,
    );
    if rt.EndDraw(None, None).is_err() {
        // Device lost: rebuild the render target on the next paint.
        (*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog)).rt = None;
    }
}

unsafe fn paint_label(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    pal: &Palette,
    d: &Dialog,
    label: &Label,
    s: f32,
) {
    let (size, weight) = label_style(label.kind, s);
    let on = label.owner.map_or(true, |c| enabled(c, &d.draft));
    let color = match label.kind {
        _ if !on => pal.text_disabled,
        TextKind::Head | TextKind::Hint | TextKind::Note => pal.text_secondary,
        _ => pal.text,
    };
    if label.kind == TextKind::Note {
        gfx::text_wrapped(rt, fonts, label.text, label.rect, size, color);
    } else {
        gfx::text(
            rt,
            fonts,
            Family::Ui,
            label.text,
            label.rect,
            size,
            weight,
            gfx::LEADING,
            color,
        );
    }
}

unsafe fn paint_focus(rt: &ID2D1RenderTarget, pal: &Palette, rc: D2D_RECT_F, radius: f32, s: f32) {
    gfx::stroke_round(
        rt,
        gfx::inflate(rc, px(2.5, s)),
        radius + px(2.5, s),
        pal.focus,
        px(2.0, s),
    );
}

fn hovered(d: &Dialog, ctl: Ctl, part: Part) -> bool {
    d.hover == Some(Target { ctl, part })
}

fn hovered_any(d: &Dialog, ctl: Ctl) -> bool {
    d.hover.is_some_and(|t| t.ctl == ctl)
}

fn button_fill(pal: &Palette, d: &Dialog, ctl: Ctl, part: Part) -> Rgba {
    if d.pressed == Some(Target { ctl, part }) {
        pal.control_pressed
    } else if hovered(d, ctl, part) {
        pal.control_hover
    } else {
        pal.control
    }
}

fn center(rc: D2D_RECT_F) -> (f32, f32) {
    ((rc.left + rc.right) / 2.0, (rc.top + rc.bottom) / 2.0)
}

unsafe fn paint_control(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    pal: &Palette,
    d: &Dialog,
    p: &Placed,
    s: f32,
) {
    let draft = &d.draft;
    let on = enabled(p.ctl, draft);
    let ink = if on { pal.text } else { pal.text_disabled };
    let focused = d.focus_visible && d.focus == p.ctl;
    let r = px(CHIP_R, s);
    match p.ctl {
        Ctl::Close => {
            if hovered_any(d, Ctl::Close) {
                gfx::fill_round(rt, p.rect, r, pal.control_hover);
            }
            let (cx, cy) = center(p.rect);
            let arm = px(5.0, s);
            let w = px(1.5, s);
            gfx::line(rt, (cx - arm, cy - arm), (cx + arm, cy + arm), pal.text_secondary, w);
            gfx::line(rt, (cx + arm, cy - arm), (cx - arm, cy + arm), pal.text_secondary, w);
        }
        Ctl::Tabs => {
            for (part, rc) in &p.parts {
                let Part::Option(i) = *part else { continue };
                let selected = Tab::ALL[i] == d.tab;
                let fill = if selected {
                    pal.selected
                } else if hovered(d, Ctl::Tabs, *part) {
                    pal.control_hover
                } else {
                    pal.card
                };
                gfx::fill_round(rt, *rc, r, fill);
                gfx::text(
                    rt,
                    fonts,
                    Family::Ui,
                    Tab::ALL[i].label(),
                    *rc,
                    px(12.5, s),
                    if selected { gfx::SEMIBOLD } else { gfx::MEDIUM },
                    gfx::CENTER,
                    if selected { pal.on_selected } else { pal.text },
                );
            }
            if focused {
                paint_focus(rt, pal, p.rect, r, s);
            }
        }
        Ctl::Step(field) => {
            for (part, rc) in &p.parts {
                if matches!(part, Part::Minus | Part::Plus) {
                    let fill = if on {
                        button_fill(pal, d, p.ctl, *part)
                    } else {
                        pal.control
                    };
                    gfx::fill_round(rt, *rc, r, fill);
                    let glyph = if *part == Part::Minus { "\u{2212}" } else { "+" };
                    gfx::text(rt, fonts, Family::Ui, glyph, *rc, px(15.0, s), gfx::MEDIUM, gfx::CENTER, ink);
                } else {
                    let mid = rc.left + (rc.right - rc.left) * 0.56;
                    gfx::digits(
                        rt,
                        fonts,
                        &field.get(draft).to_string(),
                        gfx::rect(rc.left, rc.top, mid, rc.bottom),
                        px(14.0, s),
                        gfx::SEMIBOLD,
                        gfx::TRAILING,
                        ink,
                    );
                    gfx::text(
                        rt,
                        fonts,
                        Family::Ui,
                        field.unit(),
                        gfx::rect(mid + px(4.0, s), rc.top, rc.right, rc.bottom),
                        px(11.5, s),
                        gfx::REGULAR,
                        gfx::LEADING,
                        if on { pal.text_secondary } else { pal.text_disabled },
                    );
                }
            }
            if focused {
                paint_focus(rt, pal, p.rect, r, s);
            }
        }
        Ctl::Flag(flag) => {
            let value = flag.get(draft);
            let track = p.parts[0].1;
            let h = track.bottom - track.top;
            let hot = on && hovered_any(d, p.ctl);
            let fill = match (on, value, hot) {
                (false, true, _) => pal.text_disabled,
                (false, false, _) => gfx::fade(pal.toggle_off, 0.6),
                (true, true, true) => pal.accent_hover,
                (true, true, false) => pal.accent,
                (true, false, true) => pal.text_secondary,
                (true, false, false) => pal.toggle_off,
            };
            gfx::fill_round(rt, track, h / 2.0, fill);
            let knob = h - px(6.0, s);
            let kx = if value {
                track.right - px(3.0, s) - knob
            } else {
                track.left + px(3.0, s)
            };
            let ky = track.top + px(3.0, s);
            gfx::fill_round(rt, gfx::rect(kx, ky, kx + knob, ky + knob), knob / 2.0, pal.knob);
            if focused {
                paint_focus(rt, pal, track, h / 2.0, s);
            }
        }
        Ctl::Group(group) => {
            let selected = group.get(draft);
            let options = group.options();
            for (part, rc) in &p.parts {
                let Part::Option(i) = *part else { continue };
                let chip = gfx::inflate(*rc, -px(1.5, s));
                let is_sel = selected == Some(i);
                let fill = if is_sel {
                    if on { pal.selected } else { pal.toggle_off }
                } else if on && hovered(d, p.ctl, *part) {
                    pal.control_hover
                } else {
                    pal.control
                };
                gfx::fill_round(rt, chip, r, fill);
                gfx::text(
                    rt,
                    fonts,
                    Family::Ui,
                    &options[i],
                    *rc,
                    px(12.0, s),
                    if is_sel { gfx::SEMIBOLD } else { gfx::MEDIUM },
                    gfx::CENTER,
                    if is_sel { pal.on_selected } else { ink },
                );
            }
            if focused {
                paint_focus(rt, pal, p.rect, r, s);
            }
        }
        Ctl::Volume => {
            let (_, mid) = center(p.rect);
            let th = px(8.0, s);
            let track = gfx::rect(p.rect.left, mid - th / 2.0, p.rect.right, mid + th / 2.0);
            let kx = track.left + (track.right - track.left) * f32::from(draft.alert_volume) / 100.0;
            gfx::fill_round(rt, track, th / 2.0, pal.control);
            gfx::fill_round(
                rt,
                gfx::rect(track.left, track.top, kx.max(track.left + th), track.bottom),
                th / 2.0,
                if on { pal.accent } else { pal.text_disabled },
            );
            let kr = px(if hovered_any(d, Ctl::Volume) || d.dragging_volume { 9.0 } else { 8.0 }, s);
            let knob = gfx::rect(kx - kr, mid - kr, kx + kr, mid + kr);
            gfx::fill_round(rt, knob, kr, pal.knob);
            gfx::stroke_round(rt, knob, kr, pal.window_stroke, 1.0);
            gfx::digits(
                rt,
                fonts,
                &format!("{}%", draft.alert_volume),
                gfx::rect(p.rect.right + px(10.0, s), p.rect.top, p.rect.right + px(48.0, s), p.rect.bottom),
                px(12.0, s),
                gfx::MEDIUM,
                gfx::LEADING,
                if on { pal.text_secondary } else { pal.text_disabled },
            );
            if focused {
                paint_focus(rt, pal, knob, kr, s);
            }
        }
        Ctl::Preview | Ctl::Defaults | Ctl::Cancel | Ctl::Save => {
            let primary = p.ctl == Ctl::Save;
            let br = px(9.0, s);
            let fill = if primary {
                pal.selected
            } else if p.ctl == Ctl::Cancel {
                if hovered_any(d, p.ctl) { pal.control_hover } else { pal.card }
            } else if on {
                button_fill(pal, d, p.ctl, Part::Whole)
            } else {
                pal.control
            };
            gfx::fill_round(rt, p.rect, br, fill);
            if primary && hovered_any(d, p.ctl) {
                gfx::fill_round(rt, p.rect, br, gfx::fade(pal.on_selected, 0.10));
            }
            let caption = match p.ctl {
                Ctl::Preview => "Test",
                Ctl::Defaults => "Restore",
                Ctl::Cancel => "Cancel",
                _ => "Save",
            };
            let color = if primary { pal.on_selected } else { ink };
            if p.ctl == Ctl::Preview {
                let icon = gfx::rect(p.rect.left + px(8.0, s), p.rect.top, p.rect.left + px(28.0, s), p.rect.bottom);
                gfx::text(rt, fonts, Family::Icons, glyph::VOLUME, icon, px(13.0, s), gfx::REGULAR, gfx::CENTER, color);
                let label = gfx::rect(icon.right, p.rect.top, p.rect.right - px(6.0, s), p.rect.bottom);
                gfx::text(rt, fonts, Family::Ui, caption, label, px(12.5, s), gfx::SEMIBOLD, gfx::CENTER, color);
            } else {
                gfx::text(rt, fonts, Family::Ui, caption, p.rect, px(13.5, s), gfx::SEMIBOLD, gfx::CENTER, color);
            }
            if focused {
                paint_focus(rt, pal, p.rect, br, s);
            }
        }
    }
}

unsafe fn paint_stats(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    pal: &Palette,
    d: &Dialog,
    rc: D2D_RECT_F,
    s: f32,
) {
    let (today, today_secs, history) = if d.app.is_null() {
        (
            CivilDate {
                year: 2026,
                month: 1,
                day: 1,
            },
            0,
            Vec::new(),
        )
    } else {
        let app = &*d.app;
        (
            crate::win::app::local_date(),
            app.engine.snapshot().today_sitting.as_secs(),
            app.settings.history.clone(),
        )
    };
    let bars = recent_days(&history, today, today_secs, 7);
    let caption = format!(
        "Today {}   ·   7-day average {}",
        format_today_hm(std::time::Duration::from_secs(today_secs)),
        format_today_hm(std::time::Duration::from_secs(average_secs(&bars)))
    );
    gfx::text(
        rt,
        fonts,
        Family::Ui,
        &caption,
        gfx::rect(rc.left, rc.top, rc.right, rc.top + px(20.0, s)),
        px(12.0, s),
        gfx::REGULAR,
        gfx::LEADING,
        pal.text_secondary,
    );
    let chart = gfx::rect(rc.left, rc.top + px(28.0, s), rc.right, rc.bottom - px(16.0, s));
    let tallest = bars.iter().map(|b| b.sitting_secs).max().unwrap_or(0).max(3600) as f32;
    let slot = (chart.right - chart.left) / bars.len() as f32;
    let bw = (slot * 0.46).min(px(22.0, s));
    let full = chart.bottom - chart.top;
    for (i, bar) in bars.iter().enumerate() {
        let cx = chart.left + slot * (i as f32 + 0.5);
        let h = (bar.sitting_secs as f32 / tallest * full).max(px(3.0, s));
        gfx::fill_round(
            rt,
            gfx::rect(cx - bw / 2.0, chart.top, cx + bw / 2.0, chart.bottom),
            px(4.0, s),
            pal.control,
        );
        gfx::fill_round(
            rt,
            gfx::rect(cx - bw / 2.0, chart.bottom - h, cx + bw / 2.0, chart.bottom),
            px(3.0, s),
            if bar.is_today {
                pal.accent
            } else {
                gfx::fade(pal.accent, 0.5)
            },
        );
        gfx::text(
            rt,
            fonts,
            Family::Ui,
            weekday_initial(bar.date),
            gfx::rect(cx - slot / 2.0, chart.bottom + px(2.0, s), cx + slot / 2.0, chart.bottom + px(16.0, s)),
            px(10.5, s),
            if bar.is_today { gfx::SEMIBOLD } else { gfx::REGULAR },
            gfx::CENTER,
            if bar.is_today { pal.text } else { pal.text_secondary },
        );
    }
}

unsafe fn save(hwnd: HWND) {
    let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
    if dlg.is_null() || (*dlg).app.is_null() {
        return;
    }
    let mut next = (*dlg).draft.clone();
    next.clamp();
    (*(*dlg).app).apply_settings(next);
    (*dlg).saved = true;
    close(hwnd);
}

unsafe fn close(hwnd: HWND) {
    let dlg = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog;
    if !dlg.is_null() && !(*dlg).saved && !(*dlg).app.is_null() {
        let (size, theme, on_top) = (*dlg).revert;
        (*(*dlg).app).preview_appearance(size, theme, on_top);
    }
    let _ = DestroyWindow(hwnd);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_layouts(scale: f32) -> Vec<(Tab, Lay)> {
        let d = Settings::default();
        let h = window_logical_h() * scale;
        Tab::ALL
            .iter()
            .map(|&t| (t, layout(t, &d, scale, h, 0.0)))
            .collect()
    }

    fn overlaps(a: D2D_RECT_F, b: D2D_RECT_F) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    #[test]
    fn settings_window_is_capped_and_clamped_to_the_monitor_work_area() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 900,
        };
        let (x, y, w, h) = fit_window_to_work_area(work, 1.5);
        assert_eq!(w, 600);
        assert_eq!(h, 900 - WORK_AREA_MARGIN * 2);
        assert!(x >= WORK_AREA_MARGIN && x + w <= 1920 - WORK_AREA_MARGIN);
        assert_eq!(y, WORK_AREA_MARGIN);
    }

    #[test]
    fn tallest_tab_fits_a_768_pixel_high_laptop_screen_at_100_percent() {
        assert!(window_logical_h() <= 700.0, "{}", window_logical_h());
    }

    #[test]
    fn scrolling_clamps_at_both_ends() {
        assert_eq!(scroll_after_wheel(0.0, -120, 1.0, 100.0), 60.0);
        assert_eq!(scroll_after_wheel(90.0, -120, 1.0, 100.0), 100.0);
        assert_eq!(scroll_after_wheel(30.0, 120, 1.0, 100.0), 0.0);
    }

    #[test]
    fn every_tab_fits_without_scrolling_at_common_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for (tab, l) in all_layouts(scale) {
                assert!(max_scroll(&l) < 0.5, "{tab:?} scrolls at {scale}");
            }
        }
    }

    #[test]
    fn controls_stay_inside_the_window_and_never_overlap() {
        for scale in [1.0, 1.5] {
            for (tab, l) in all_layouts(scale) {
                for p in &l.controls {
                    assert!(p.rect.left >= 0.0 && p.rect.right <= l.w, "{tab:?} {:?}", p.ctl);
                    assert!(p.rect.top >= 0.0 && p.rect.bottom <= l.h, "{tab:?} {:?}", p.ctl);
                }
                // A toggle row's hit area is the whole row; its track is what is drawn.
                let visual = |p: &Placed| {
                    if matches!(p.ctl, Ctl::Flag(_)) {
                        p.parts[0].1
                    } else {
                        p.rect
                    }
                };
                for (i, a) in l.controls.iter().enumerate() {
                    for b in &l.controls[i + 1..] {
                        assert!(
                            !overlaps(visual(a), visual(b)),
                            "{tab:?}: {:?} overlaps {:?}",
                            a.ctl,
                            b.ctl
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn labels_never_run_into_their_controls() {
        for (tab, l) in all_layouts(1.0) {
            for label in l.labels.iter().filter(|lb| lb.kind == TextKind::Label) {
                for p in l.controls.iter() {
                    let rc = if matches!(p.ctl, Ctl::Flag(_)) {
                        p.parts[0].1
                    } else {
                        p.rect
                    };
                    assert!(
                        !overlaps(label.rect, rc),
                        "{tab:?}: '{}' hits {:?}",
                        label.text,
                        p.ctl
                    );
                }
            }
        }
    }

    #[test]
    fn every_setting_is_reachable_by_keyboard() {
        let d = Settings {
            inactivity_behavior: InactivityBehavior::Pause,
            dock_enabled: true,
            ..Settings::default()
        };
        let mut seen = Vec::new();
        for tab in Tab::ALL {
            let l = layout(tab, &d, 1.0, window_logical_h(), 0.0);
            seen.extend(focus_order(&l, &d));
        }
        for ctl in [
            Ctl::Tabs,
            Ctl::Step(Field::Limit),
            Ctl::Step(Field::Step),
            Ctl::Step(Field::Break),
            Ctl::Step(Field::Idle),
            Ctl::Step(Field::Every),
            Ctl::Step(Field::Snooze),
            Ctl::Step(Field::HideAfter),
            Ctl::Step(Field::RevealFor),
            Ctl::Flag(Flag::Dock),
            Ctl::Flag(Flag::DockAutohide),
            Ctl::Flag(Flag::DockFullscreen),
            Ctl::Group(Group::DockEdge),
            Ctl::Group(Group::DockReveal),
            Ctl::Flag(Flag::PauseInactive),
            Ctl::Flag(Flag::Sound),
            Ctl::Flag(Flag::Repeat),
            Ctl::Flag(Flag::Notify),
            Ctl::Flag(Flag::Auto),
            Ctl::Flag(Flag::OnTop),
            Ctl::Flag(Flag::Hotkeys),
            Ctl::Flag(Flag::Stats),
            Ctl::Group(Group::Display),
            Ctl::Group(Group::Size),
            Ctl::Group(Group::SoundKind),
            Ctl::Group(Group::Length),
            Ctl::Group(Group::Snooze),
            Ctl::Group(Group::Theme),
            Ctl::Volume,
            Ctl::Preview,
            Ctl::Defaults,
            Ctl::Cancel,
            Ctl::Save,
        ] {
            assert!(seen.contains(&ctl), "{ctl:?} has no keyboard focus");
        }
    }

    #[test]
    fn dependent_controls_are_disabled_and_skipped_by_focus() {
        let d = Settings::default();
        assert!(!enabled(Ctl::Step(Field::Idle), &d), "idle threshold is moot while running");
        let l = layout(Tab::Breaks, &d, 1.0, window_logical_h(), 0.0);
        assert!(!focus_order(&l, &d).contains(&Ctl::Step(Field::Idle)));

        assert!(!enabled(Ctl::Group(Group::DockEdge), &d), "docking options wait for docking");
        let mut docked = Settings::default();
        docked.dock_enabled = true;
        docked.dock_autohide = false;
        assert!(enabled(Ctl::Group(Group::DockEdge), &docked));
        assert!(!enabled(Ctl::Step(Field::HideAfter), &docked), "no delay while pinned");

        let mut quiet = Settings::default();
        quiet.sound_enabled = false;
        assert!(!enabled(Ctl::Volume, &quiet));
        assert!(!enabled(Ctl::Group(Group::SoundKind), &quiet));
    }

    #[test]
    fn steppers_clamp_and_the_limit_moves_by_the_adjust_step() {
        let mut s = Settings::default();
        s.step = 5;
        let next = Field::Limit.get(&s) + Field::Limit.delta(&s);
        Field::Limit.set(&mut s, next);
        assert_eq!(s.sitting_limit_secs, 65 * 60);
        Field::Limit.set(&mut s, 1);
        assert_eq!(s.sitting_limit_secs, 5 * 60);
        Field::Idle.set(&mut s, 10_000);
        assert_eq!(s.idle_after_secs, 600);
        assert_eq!(Field::Idle.delta(&s), 5);
    }

    #[test]
    fn hit_testing_finds_stepper_parts_and_ignores_the_footer_gap() {
        let d = Settings::default();
        let l = layout(Tab::Timer, &d, 1.0, window_logical_h(), 0.0);
        let p = l.find(Ctl::Step(Field::Limit)).unwrap();
        let plus = p.parts.iter().find(|(part, _)| *part == Part::Plus).unwrap().1;
        let (x, y) = center(plus);
        assert_eq!(
            hit(&l, x, y),
            Some(Target {
                ctl: Ctl::Step(Field::Limit),
                part: Part::Plus
            })
        );
        assert_eq!(hit(&l, 2.0, l.body_bottom + 2.0), None);
    }

    #[test]
    fn groups_and_flags_round_trip_through_settings() {
        let mut s = Settings::default();
        Group::Theme.set(&mut s, 2);
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(Group::Theme.get(&s), Some(2));
        s.snooze_secs = 7 * 60;
        assert_eq!(Group::Snooze.get(&s), None, "custom snooze selects no preset");
        Flag::PauseInactive.set(&mut s, true);
        assert_eq!(s.inactivity_behavior, InactivityBehavior::Pause);
        assert!(!Flag::PauseInactive.get(&Settings::default()));
    }
}
