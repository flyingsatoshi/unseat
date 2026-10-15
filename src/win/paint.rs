use crate::win::gfx::{self, glyph, Family, Fonts};
use std::cell::RefCell;
use unseat::{
    chip_center, palette, progress, tag_text, timer_digit_px, timer_face_digits, widget_layout,
    widget_pixel_size, DockDraw, DockEdge, Palette, Rgba, Snapshot, TimerDisplayMode, TimerShape,
    VisibleState,
};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE, D2D1_ANTIALIAS_MODE_ALIASED,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    FF_DONTCARE, FW_BOLD, HBITMAP, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

/// The shared Direct2D resources plus the overlay's reusable back buffer.
pub struct D2d {
    gfx: &'static gfx::Shared,
    pub fonts: &'static Fonts,
    surface: RefCell<Option<Surface>>,
}

impl D2d {
    pub fn new() -> Option<Self> {
        let gfx = gfx::shared()?;
        Some(Self {
            gfx,
            fonts: &gfx.fonts,
            surface: RefCell::new(None),
        })
    }
}

/// A 32-bit DIB selected into a memory DC, with a DC render target bound to it, for a
/// layered (per-pixel alpha) window: the overlay and the prompt under it. Rebuilt only when
/// the size changes, instead of once per second.
pub(crate) struct Surface {
    pub(crate) w: i32,
    pub(crate) h: i32,
    hdc: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    bits: *mut core::ffi::c_void,
    rt: Option<ID2D1DCRenderTarget>,
}

impl Surface {
    pub(crate) unsafe fn new(factory: Option<&ID2D1Factory>, w: i32, h: i32) -> Option<Self> {
        let screen = GetDC(HWND::default());
        let hdc = CreateCompatibleDC(screen);
        ReleaseDC(HWND::default(), screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(HDC::default(), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(hdc);
            return None;
        };
        let old = SelectObject(hdc, dib);
        let rt = factory.and_then(|f| create_rt(f).ok()).filter(|rt| {
            let rc = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
            rt.BindDC(hdc, &rc).is_ok()
        });
        Some(Self {
            w,
            h,
            hdc,
            dib,
            old,
            bits,
            rt,
        })
    }
}

impl Surface {
    /// The Direct2D target drawing into this surface, if hardware drawing is available.
    pub(crate) fn target(&self) -> Option<ID2D1RenderTarget> {
        self.rt.as_ref().and_then(as_rt)
    }

    /// Raw premultiplied BGRA pixels, for test screenshots.
    pub(crate) fn pixels(&self) -> Option<Vec<u8>> {
        if self.bits.is_null() {
            return None;
        }
        let len = (self.w * self.h * 4) as usize;
        Some(unsafe { std::slice::from_raw_parts(self.bits as *const u8, len) }.to_vec())
    }

    /// Show the painted pixels in the layered window `hwnd`, moved to `at` (screen pixels)
    /// if given, and faded to `alpha`.
    pub(crate) unsafe fn present(&self, hwnd: HWND, at: Option<POINT>, alpha: u8) {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx: self.w, cy: self.h };
        let src = POINT { x: 0, y: 0 };
        let _ = UpdateLayeredWindow(
            hwnd,
            HDC::default(),
            at.as_ref().map(|p| p as *const POINT),
            Some(&size as *const SIZE),
            self.hdc,
            Some(&src as *const POINT),
            COLORREF(0),
            Some(&blend as *const BLENDFUNCTION),
            ULW_ALPHA,
        );
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old);
            let _ = DeleteObject(self.dib);
            let _ = DeleteDC(self.hdc);
        }
    }
}

impl D2d {
    /// Raw copy of the last painted overlay frame, for test screenshots.
    pub fn snapshot(&self) -> Option<(i32, i32, Vec<u8>)> {
        let surface = self.surface.borrow();
        let s = surface.as_ref()?;
        Some((s.w, s.h, s.pixels()?))
    }
}

pub fn widget_size(shape: TimerShape, scale: f32) -> (i32, i32) {
    widget_pixel_size(shape, scale)
}

/// Everything one overlay frame depends on.
pub struct OverlayPaint {
    pub mode: TimerDisplayMode,
    pub snap: Snapshot,
    pub dark: bool,
    pub hover: bool,
    pub limit: std::time::Duration,
    pub break_dur: std::time::Duration,
    pub scale: f32,
    /// Window size in pixels (smaller than the full overlay while docked or sliding).
    pub size: (i32, i32),
    pub dock: Option<(DockEdge, DockDraw)>,
    pub pinned: bool,
}

pub fn paint(d2d: Option<&D2d>, hwnd: HWND, p: &OverlayPaint) {
    let (cx, cy) = p.size;
    if cx <= 0 || cy <= 0 {
        return;
    }
    let pal = palette(p.dark);
    let fallback_surface;
    let mut cache = d2d.map(|d| d.surface.borrow_mut());
    unsafe {
        let surface = match cache.as_deref_mut() {
            Some(slot) => {
                if slot.as_ref().map_or(true, |s| s.w != cx || s.h != cy) {
                    *slot = Surface::new(d2d.map(|d| &d.gfx.factory), cx, cy);
                }
                slot.as_ref()
            }
            None => {
                fallback_surface = Surface::new(None, cx, cy);
                fallback_surface.as_ref()
            }
        };
        let Some(surface) = surface else {
            return;
        };
        let mut drew = false;
        if let (Some(d2d), Some(rt)) = (d2d, surface.rt.as_ref()) {
            rt.BeginDraw();
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.Clear(Some(&gfx::color(Rgba(0, 0, 0, 0))));
            if let Some(rt) = as_rt(rt) {
                let size = (cx as f32, cy as f32);
                match p.dock {
                    Some((edge, DockDraw::Tab)) => draw_tab(&rt, d2d, pal, p, edge, size, false),
                    Some((edge, DockDraw::Peek)) => draw_tab(&rt, d2d, pal, p, edge, size, true),
                    Some((edge, DockDraw::Pill { dx, dy })) => {
                        draw_pill(&rt, d2d, pal, p, Some(edge), dx, dy)
                    }
                    None => draw_pill(&rt, d2d, pal, p, None, 0.0, 0.0),
                }
                if crate::win::testhook::badge() {
                    draw_test_badge(&rt, d2d.fonts, cx as f32, cy as f32);
                }
            }
            drew = rt.EndDraw(None, None).is_ok();
        }
        if !drew {
            gdi_fallback(surface.hdc, surface.bits, cx, cy, pal, p);
        }
        surface.present(hwnd, None, 255);
    }
}

/// Hardware rendering. A software target saves ~24 MB of private memory (no Direct3D
/// device) but measured about 3x the idle CPU, because text is re-rasterized every second.
fn create_rt(factory: &ID2D1Factory) -> windows::core::Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe { factory.CreateDCRenderTarget(&props) }
}

fn as_rt(rt: &ID2D1DCRenderTarget) -> Option<ID2D1RenderTarget> {
    use windows::core::Interface;
    rt.cast().ok()
}

fn fraction(p: &OverlayPaint) -> f32 {
    progress(
        p.snap.state,
        p.snap.sitting_elapsed,
        p.limit,
        p.snap.break_elapsed,
        p.break_dur,
    )
}

/// Corner radii [tl, tr, br, bl] with the corners on the docked edge squared off.
fn radii(r: f32, edge: Option<DockEdge>) -> [f32; 4] {
    match edge {
        None => [r; 4],
        Some(DockEdge::Left) => [0.0, r, r, 0.0],
        Some(DockEdge::Right) => [r, 0.0, 0.0, r],
        Some(DockEdge::Top) => [0.0, 0.0, r, r],
        Some(DockEdge::Bottom) => [r, r, 0.0, 0.0],
    }
}

fn digit_alpha(state: VisibleState) -> f32 {
    match state {
        VisibleState::Paused => 0.45,
        VisibleState::Away => 0.6,
        _ => 1.0,
    }
}

unsafe fn draw_pill(
    rt: &ID2D1RenderTarget,
    d2d: &D2d,
    pal: &Palette,
    p: &OverlayPaint,
    edge: Option<DockEdge>,
    dx: f32,
    dy: f32,
) {
    let fonts = d2d.fonts;
    let s = p.scale;
    let l = widget_layout(s);
    let fill = fraction(p);
    let c = pal.overlay(p.snap.state, fill);
    let pill = gfx::rect(
        l.pill_x + dx,
        l.pill_y + dy,
        l.pill_x + l.pill_w + dx,
        l.pill_y + l.pill_h + dy,
    );
    let corners = radii(l.radius, edge);

    // Soft two-layer shadow below the pill.
    for (off, grow, k) in [(2.5, 1.0, 0.55), (1.0, 0.0, 0.45)] {
        let shadow = gfx::rect(
            pill.left - grow * s,
            pill.top + off * s - grow * s,
            pill.right + grow * s,
            pill.bottom + off * s + grow * s,
        );
        gfx::fill_corners(rt, shadow, corners.map(|r| if r > 0.0 { r + grow * s } else { 0.0 }), gfx::fade(pal.shadow, k));
    }
    gfx::fill_corners(rt, pill, corners, c.bg);
    // The pill is the progress bar: a lighter band fills it from the leading edge.
    if fill > 0.0 && c.band != c.bg {
        let band_r = pill.left + (pill.right - pill.left) * fill;
        rt.PushAxisAlignedClip(
            &gfx::rect(pill.left, pill.top, band_r, pill.bottom),
            D2D1_ANTIALIAS_MODE_ALIASED,
        );
        gfx::fill_corners(rt, pill, corners, c.band);
        rt.PopAxisAlignedClip();
    }
    if edge.is_none() && p.snap.state != VisibleState::Overdue {
        gfx::stroke_round(rt, gfx::inflate(pill, -0.5 * s), l.radius, pal.pill_stroke, 1.0 * s);
    }

    let digits = timer_face_digits(p.mode, p.snap);
    let tag = tag_text(p.snap, p.mode, p.limit);
    let tag_w = gfx::measure(fonts, &tag, l.tag_px, gfx::SEMIBOLD);
    let lane_r = if p.hover {
        l.digits_hover_r
    } else {
        (l.tag_r - tag_w - 8.0 * s).min(l.digits_r)
    } + dx;
    let lane_l = l.digits_l + dx;
    gfx::digits(
        rt,
        fonts,
        &digits,
        gfx::rect(lane_l, pill.top, lane_r, pill.bottom),
        l.digit_px,
        gfx::SEMIBOLD,
        gfx::LEADING,
        gfx::fade(c.digits, digit_alpha(p.snap.state)),
    );

    if p.hover {
        let ink = c.digits;
        let chip = gfx::fade(c.digits, 0.14);
        let r = l.control_r;
        let paused = p.snap.state == VisibleState::Paused;
        for (center, kind) in [(l.pause, 0), (l.brk, 1), (l.reset, 2)] {
            let (cx, cy) = (center.0 + dx, center.1 + dy);
            gfx::fill_round(rt, circle(cx, cy, r), r, chip);
            match kind {
                0 => draw_icon(rt, fonts, if paused { glyph::PLAY } else { glyph::PAUSE }, cx, cy, r * 0.8, ink),
                1 => draw_cup(rt, cx, cy, r, ink),
                _ => draw_icon(rt, fonts, glyph::RESET, cx, cy, r * 0.8, ink),
            }
        }
        let (kx, ky) = chip_center(s, edge);
        draw_corner_chip(rt, fonts, pal, kx, ky, l.close_r, edge.map(|_| p.pinned));
    } else {
        gfx::text(
            rt,
            fonts,
            Family::Ui,
            &tag,
            gfx::rect(l.tag_r - tag_w - 2.0 * s + dx, pill.top, l.tag_r + dx, pill.bottom),
            l.tag_px,
            gfx::SEMIBOLD,
            gfx::TRAILING,
            c.tag,
        );
    }
}

/// The docked tab: a thin bar flush with the edge, filled by progress along its length.
/// Peeking widens it and shows the minutes left.
unsafe fn draw_tab(
    rt: &ID2D1RenderTarget,
    d2d: &D2d,
    pal: &Palette,
    p: &OverlayPaint,
    edge: DockEdge,
    (w, h): (f32, f32),
    peek: bool,
) {
    let s = p.scale;
    let fill = fraction(p);
    let c = pal.overlay(p.snap.state, fill);
    let thick = if matches!(edge, DockEdge::Left | DockEdge::Right) { w } else { h };
    let r = (thick / 2.0).min(if peek { 10.0 * s } else { thick });
    let corners = radii(r, Some(edge));
    let full = gfx::rect(0.0, 0.0, w, h);
    gfx::fill_corners(rt, full, corners, c.bg);
    if fill > 0.0 && c.band != c.bg {
        let band = match edge {
            DockEdge::Left | DockEdge::Right => gfx::rect(0.0, h * (1.0 - fill), w, h),
            DockEdge::Top | DockEdge::Bottom => gfx::rect(0.0, 0.0, w * fill, h),
        };
        rt.PushAxisAlignedClip(&band, D2D1_ANTIALIAS_MODE_ALIASED);
        gfx::fill_corners(rt, full, corners, c.band);
        rt.PopAxisAlignedClip();
    }
    if !peek {
        return;
    }
    let snap = p.snap;
    let (value, unit) = match snap.state {
        VisibleState::Overdue => (format!("+{}", snap.overdue_by.as_secs() / 60), "over"),
        VisibleState::Break => (snap.break_remaining.as_secs().div_ceil(60).to_string(), "break"),
        _ => (snap.timer_remaining.as_secs().div_ceil(60).to_string(), "min"),
    };
    let ink = gfx::fade(c.digits, digit_alpha(snap.state));
    let fonts = d2d.fonts;
    if matches!(edge, DockEdge::Left | DockEdge::Right) {
        let mid = h / 2.0;
        gfx::digits(rt, fonts, &value, gfx::rect(0.0, mid - 16.0 * s, w, mid + 4.0 * s), 17.0 * s, gfx::SEMIBOLD, gfx::CENTER, ink);
        gfx::text(rt, fonts, Family::Ui, unit, gfx::rect(0.0, mid + 3.0 * s, w, mid + 16.0 * s), 9.5 * s, gfx::MEDIUM, gfx::CENTER, c.tag);
    } else {
        let label = format!("{value} {unit}");
        gfx::digits(rt, fonts, &label, full, 13.0 * s, gfx::SEMIBOLD, gfx::CENTER, ink);
    }
}

fn circle(cx: f32, cy: f32, r: f32) -> windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
    gfx::rect(cx - r, cy - r, cx + r, cy + r)
}

unsafe fn draw_icon(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    glyph: &str,
    cx: f32,
    cy: f32,
    size: f32,
    c: Rgba,
) {
    gfx::text(
        rt,
        fonts,
        Family::Icons,
        glyph,
        circle(cx, cy, size),
        size,
        gfx::REGULAR,
        gfx::CENTER,
        c,
    );
}

/// A small cup for "take a break": body with a rounded bottom and a handle.
unsafe fn draw_cup(rt: &ID2D1RenderTarget, cx: f32, cy: f32, r: f32, c: Rgba) {
    let u = r / 12.0;
    let w = 1.5 * u;
    let body = gfx::rect(cx - 6.0 * u, cy - 4.0 * u, cx + 3.5 * u, cy + 5.0 * u);
    gfx::stroke_round(rt, body, 2.5 * u, c, w);
    gfx::stroke_round(
        rt,
        gfx::rect(cx + 3.5 * u, cy - 2.0 * u, cx + 7.0 * u, cy + 2.5 * u),
        2.2 * u,
        c,
        w,
    );
}

/// "TEST" in the bottom-left corner of a test instance's overlay, so it can't be mistaken for
/// the user's own Unseat.
unsafe fn draw_test_badge(rt: &ID2D1RenderTarget, fonts: &Fonts, w: f32, h: f32) {
    let bh = (h * 0.26).clamp(11.0, 20.0);
    let bw = bh * 2.7;
    let rc = gfx::rect(2.0, h - bh - 2.0, (2.0 + bw).min(w), h - 2.0);
    gfx::fill_round(rt, rc, bh * 0.3, Rgba(200, 30, 120, 235));
    gfx::text(rt, fonts, Family::Ui, "TEST", rc, bh * 0.62, gfx::SEMIBOLD, gfx::CENTER, Rgba(255, 255, 255, 255));
}

/// Corner chip: × to hide the overlay, or a pin when docked (filled while pinned open).
unsafe fn draw_corner_chip(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    pal: &Palette,
    cx: f32,
    cy: f32,
    r: f32,
    pinned: Option<bool>,
) {
    gfx::fill_round(rt, circle(cx, cy, r), r, pal.control);
    let ink = pal.text;
    match pinned {
        Some(on) => draw_icon(
            rt,
            fonts,
            if on { glyph::PINNED } else { glyph::PIN },
            cx,
            cy,
            r * 0.9,
            ink,
        ),
        None => {
            let arm = r * 0.34;
            let thick = (r * 0.20).clamp(1.15, 2.0);
            gfx::line(rt, (cx - arm, cy - arm), (cx + arm, cy + arm), ink, thick);
            gfx::line(rt, (cx + arm, cy - arm), (cx - arm, cy + arm), ink, thick);
        }
    }
}

/// Plain GDI rendering for systems where Direct2D cannot start.
unsafe fn gdi_fallback(
    hdc: HDC,
    bits: *mut core::ffi::c_void,
    cx: i32,
    cy: i32,
    pal: &Palette,
    p: &OverlayPaint,
) {
    if bits.is_null() || cx <= 0 || cy <= 0 {
        return;
    }
    let colors = pal.overlay(p.snap.state, fraction(p));
    let stride = cx * 4;
    let ptr = bits as *mut u8;
    let Rgba(r, g, b, _) = colors.bg;
    for y in 0..cy {
        for x in 0..cx {
            let inside = rounded_contains(x as f32, y as f32, cx as f32, cy as f32, 10.0);
            let i = (y * stride + x * 4) as isize;
            let (pb, pg, pr, pa) = if inside { (b, g, r, 240) } else { (0, 0, 0, 0) };
            *ptr.offset(i) = pb;
            *ptr.offset(i + 1) = pg;
            *ptr.offset(i + 2) = pr;
            *ptr.offset(i + 3) = pa;
        }
    }
    let digits = timer_face_digits(p.mode, p.snap);
    let mut text: Vec<u16> = digits.encode_utf16().chain(std::iter::once(0)).collect();
    SetBkMode(hdc, TRANSPARENT);
    let Rgba(tr, tg, tb, _) = colors.digits;
    SetTextColor(
        hdc,
        COLORREF(u32::from(tr) | u32::from(tg) << 8 | u32::from(tb) << 16),
    );
    let layout = widget_layout(p.scale);
    let digit_px = timer_digit_px(&digits, layout.digit_px, layout.digits_r - layout.digits_l);
    let font = CreateFontW(
        -digit_px.round() as i32,
        0,
        0,
        0,
        FW_BOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        w!("Segoe UI"),
    );
    let old_font = if font.is_invalid() {
        None
    } else {
        Some(SelectObject(hdc, font))
    };
    let mut rc = RECT {
        left: 16,
        top: 0,
        right: cx - 16,
        bottom: cy,
    };
    let _ = DrawTextW(
        hdc,
        &mut text,
        &mut rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    if let Some(old_font) = old_font {
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font);
    }
    // GDI text leaves alpha at zero; make every painted pixel opaque.
    for y in 0..cy {
        for x in 0..cx {
            let i = (y * stride + x * 4) as isize;
            if *ptr.offset(i + 3) == 0
                && (*ptr.offset(i) != 0 || *ptr.offset(i + 1) != 0 || *ptr.offset(i + 2) != 0)
            {
                *ptr.offset(i + 3) = 240;
            }
        }
    }
}

fn rounded_contains(x: f32, y: f32, w: f32, h: f32, radius: f32) -> bool {
    let r = radius.min(h / 2.0).min(w / 2.0);
    let px = x.clamp(r, w - r);
    let py = y.clamp(r, h - r);
    if x >= r && x <= w - r {
        return y >= 0.0 && y <= h;
    }
    let dx = x - px;
    let dy = y - py;
    dx * dx + dy * dy <= r * r
}
