use crate::win::gfx::{self, glyph, Family, Fonts};
use std::cell::RefCell;
use unseat::{
    palette, progress, tag_text, timer_digit_px, timer_face_digits, widget_layout,
    widget_pixel_size, Palette, Rgba, Snapshot, TimerDisplayMode, TimerShape, VisibleState,
};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    FF_DONTCARE, FW_BOLD, HBITMAP, HDC, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

/// Device-independent Direct2D resources plus the overlay's reusable back buffer.
pub struct D2d {
    factory: ID2D1Factory,
    pub fonts: Fonts,
    surface: RefCell<Option<Surface>>,
}

impl D2d {
    pub fn new() -> Option<Self> {
        unsafe {
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let dwrite = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            Some(Self {
                factory,
                fonts: Fonts::new(dwrite),
                surface: RefCell::new(None),
            })
        }
    }

    pub fn factory(&self) -> &ID2D1Factory {
        &self.factory
    }
}

/// A 32-bit DIB selected into a memory DC, with a DC render target bound to it. Rebuilt only
/// when the overlay size changes, instead of once per second.
struct Surface {
    w: i32,
    h: i32,
    hdc: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    bits: *mut core::ffi::c_void,
    rt: Option<ID2D1DCRenderTarget>,
}

impl Surface {
    unsafe fn new(factory: Option<&ID2D1Factory>, w: i32, h: i32) -> Option<Self> {
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
        if s.bits.is_null() {
            return None;
        }
        let len = (s.w * s.h * 4) as usize;
        let bytes = unsafe { std::slice::from_raw_parts(s.bits as *const u8, len) }.to_vec();
        Some((s.w, s.h, bytes))
    }
}

pub fn widget_size(shape: TimerShape, scale: f32) -> (i32, i32) {
    widget_pixel_size(shape, scale)
}

#[allow(clippy::too_many_arguments)]
pub fn paint(
    d2d: Option<&D2d>,
    hwnd: HWND,
    shape: TimerShape,
    timer_display_mode: TimerDisplayMode,
    snap: Snapshot,
    dark: bool,
    hover: bool,
    limit: std::time::Duration,
    break_dur: std::time::Duration,
    scale: f32,
) {
    let (cx, cy) = widget_size(shape, scale);
    if cx <= 0 || cy <= 0 {
        return;
    }
    let pal = palette(dark);
    let fallback_surface;
    let mut cache = d2d.map(|d| d.surface.borrow_mut());
    unsafe {
        let surface = match cache.as_deref_mut() {
            Some(slot) => {
                if slot.as_ref().map_or(true, |s| s.w != cx || s.h != cy) {
                    *slot = Surface::new(d2d.map(|d| &d.factory), cx, cy);
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
                draw_pill(
                    &rt,
                    d2d,
                    pal,
                    timer_display_mode,
                    snap,
                    hover,
                    limit,
                    break_dur,
                    scale,
                );
            }
            drew = rt.EndDraw(None, None).is_ok();
        }
        if !drew {
            gdi_fallback(surface.hdc, surface.bits, cx, cy, pal, timer_display_mode, snap);
        }
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx, cy };
        let src = POINT { x: 0, y: 0 };
        let _ = UpdateLayeredWindow(
            hwnd,
            HDC::default(),
            None,
            Some(&size as *const SIZE),
            surface.hdc,
            Some(&src as *const POINT),
            COLORREF(0),
            Some(&blend as *const BLENDFUNCTION),
            ULW_ALPHA,
        );
    }
}

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

#[allow(clippy::too_many_arguments)]
unsafe fn draw_pill(
    rt: &ID2D1RenderTarget,
    d2d: &D2d,
    pal: &Palette,
    timer_display_mode: TimerDisplayMode,
    snap: Snapshot,
    hover: bool,
    limit: std::time::Duration,
    break_dur: std::time::Duration,
    s: f32,
) {
    let fonts = &d2d.fonts;
    let l = widget_layout(s);
    let radius = 11.0 * s;
    let pill = gfx::rect(l.pill_x, l.pill_y, l.pill_x + l.pill_w, l.pill_y + l.pill_h);

    // Two soft shadow layers read better than one hard offset.
    for (dy, grow, k) in [(2.5, 1.0, 0.55), (1.0, 0.0, 0.45)] {
        let shadow = gfx::rect(
            pill.left - grow * s,
            pill.top + dy * s - grow * s,
            pill.right + grow * s,
            pill.bottom + dy * s + grow * s,
        );
        gfx::fill_round(rt, shadow, radius + grow * s, gfx::fade(pal.shadow, k));
    }
    gfx::fill_round(rt, pill, radius, pal.pill_for(snap.state));
    gfx::stroke_round(
        rt,
        gfx::inflate(pill, -0.5 * s),
        radius,
        pal.pill_stroke,
        1.0 * s,
    );

    let paused = snap.state == VisibleState::Paused;
    let away = snap.state == VisibleState::Away;
    let digits = timer_face_digits(timer_display_mode, snap);
    let digit_px = timer_digit_px(&digits, l.digit_px, l.digits_r - l.digits_l);
    let digit_alpha = if paused {
        0.30
    } else if away {
        0.55
    } else {
        1.0
    };
    gfx::text(
        rt,
        fonts,
        Family::Ui,
        &digits,
        gfx::rect(l.digits_l, l.digits_t, l.digits_r, l.digits_b),
        digit_px,
        gfx::BOLD,
        gfx::LEADING,
        gfx::fade(pal.digits, digit_alpha),
    );

    let fill = progress(
        snap.state,
        snap.sitting_elapsed,
        limit,
        snap.break_elapsed,
        break_dur,
    );
    let bar = gfx::rect(l.bar_x, l.bar_y, l.bar_x + l.bar_w, l.bar_y + l.bar_h);
    gfx::fill_round(rt, bar, l.bar_h / 2.0, pal.bar_track);
    if fill > 0.0 {
        let fw = (l.bar_w * fill).max(l.bar_h).min(l.bar_w);
        gfx::fill_round(
            rt,
            gfx::rect(bar.left, bar.top, bar.left + fw, bar.bottom),
            l.bar_h / 2.0,
            gfx::fade(pal.bar_for(snap.state, fill), if paused { 0.4 } else { 1.0 }),
        );
    }

    if paused {
        let (cx, cy, r) = l.play;
        fill_play_triangle(rt, d2d.factory(), cx, cy, r * 1.05, gfx::fade(pal.digits, 0.95));
    }

    if hover {
        let (px, py) = l.pause;
        let (rx, ry) = l.reset;
        let cr = l.control_r;
        let ink = gfx::fade(pal.digits, 0.92);
        let chip = gfx::fade(pal.digits, 0.08);
        if !paused {
            gfx::fill_round(rt, circle(px, py, cr), cr, chip);
            draw_icon(rt, fonts, glyph::PAUSE, px, py, cr * 0.85, ink);
        }
        gfx::fill_round(rt, circle(rx, ry, cr), cr, chip);
        draw_icon(rt, fonts, glyph::RESET, rx, ry, cr * 0.85, ink);
        draw_close_chip(rt, pal, l.close.0, l.close.1, l.close_r);
    } else {
        let (n, unit) = tag_text(snap, limit, break_dur);
        let mid = l.tag_y + l.tag_h * 0.48;
        let value_px = if n.chars().count() > 2 { 11.5 } else { 13.0 };
        gfx::text(
            rt,
            fonts,
            Family::Ui,
            &n,
            gfx::rect(l.tag_x - 4.0 * s, mid - 15.0 * s, l.tag_x + l.tag_w + 4.0 * s, mid + 3.0 * s),
            value_px * s,
            gfx::SEMIBOLD,
            gfx::CENTER,
            if snap.state == VisibleState::Overdue {
                pal.bar_over
            } else {
                pal.tag
            },
        );
        gfx::text(
            rt,
            fonts,
            Family::Ui,
            unit,
            gfx::rect(l.tag_x - 4.0 * s, mid + 1.0 * s, l.tag_x + l.tag_w + 4.0 * s, mid + 13.0 * s),
            8.5 * s,
            gfx::MEDIUM,
            gfx::CENTER,
            pal.tag_sub,
        );
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

unsafe fn fill_play_triangle(
    rt: &ID2D1RenderTarget,
    factory: &ID2D1Factory,
    cx: f32,
    cy: f32,
    size: f32,
    c: Rgba,
) {
    let Ok(geo) = factory.CreatePathGeometry() else {
        return;
    };
    {
        let Ok(sink) = geo.Open() else {
            return;
        };
        let h = size;
        let w = size * 0.84;
        let left = cx - w * 0.28;
        sink.BeginFigure(
            D2D_POINT_2F {
                x: left,
                y: cy - h * 0.5,
            },
            D2D1_FIGURE_BEGIN_FILLED,
        );
        sink.AddLines(&[
            D2D_POINT_2F { x: left + w, y: cy },
            D2D_POINT_2F {
                x: left,
                y: cy + h * 0.5,
            },
        ]);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        let _ = sink.Close();
    }
    if let Ok(brush) = rt.CreateSolidColorBrush(&gfx::color(c), None) {
        rt.FillGeometry(&geo, &brush, None);
    }
}

unsafe fn draw_close_chip(rt: &ID2D1RenderTarget, pal: &Palette, cx: f32, cy: f32, r: f32) {
    gfx::fill_round(rt, circle(cx, cy, r), r, gfx::fade(pal.digits, 0.10));
    let arm = r * 0.34;
    let thick = (r * 0.20).clamp(1.15, 2.0);
    let ink = gfx::fade(pal.digits, 0.80);
    gfx::line(rt, (cx - arm, cy - arm), (cx + arm, cy + arm), ink, thick);
    gfx::line(rt, (cx + arm, cy - arm), (cx - arm, cy + arm), ink, thick);
}

/// Plain GDI rendering for systems where Direct2D cannot start.
unsafe fn gdi_fallback(
    hdc: HDC,
    bits: *mut core::ffi::c_void,
    cx: i32,
    cy: i32,
    pal: &Palette,
    timer_display_mode: TimerDisplayMode,
    snap: Snapshot,
) {
    if bits.is_null() || cx <= 0 || cy <= 0 {
        return;
    }
    let stride = cx * 4;
    let ptr = bits as *mut u8;
    let Rgba(r, g, b, _) = pal.pill_for(snap.state);
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
    let digits = timer_face_digits(timer_display_mode, snap);
    let mut text: Vec<u16> = digits.encode_utf16().chain(std::iter::once(0)).collect();
    SetBkMode(hdc, TRANSPARENT);
    let Rgba(tr, tg, tb, _) = pal.digits;
    SetTextColor(
        hdc,
        COLORREF(u32::from(tr) | u32::from(tg) << 8 | u32::from(tb) << 16),
    );
    let scale = cy as f32 / unseat::WIDGET_H;
    let layout = widget_layout(scale);
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
