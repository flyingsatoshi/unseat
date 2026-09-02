use unseat::{
    control_centers, format_break_remaining, format_elapsed, format_goal_parts, play_center,
    progress, widget_pixel_size, Snapshot, TimerShape, VisibleState,
};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,     DWRITE_FONT_WEIGHT_BLACK, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, DrawTextW, GetDC, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HDC,
    TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
use windows::core::{w, Interface};

pub struct D2d {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
}

impl D2d {
    pub fn new() -> Option<Self> {
        unsafe {
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            Some(Self { factory, dwrite })
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

pub fn widget_size(shape: TimerShape, scale: f32) -> (i32, i32) {
    widget_pixel_size(shape, scale)
}

pub fn paint(
    d2d: Option<&D2d>,
    hwnd: HWND,
    shape: TimerShape,
    snap: Snapshot,
    _dark: bool,
    hover: bool,
    limit: std::time::Duration,
    break_dur: std::time::Duration,
    scale: f32,
) {
    let (cx, cy) = widget_size(shape, scale);
    if cx <= 0 || cy <= 0 {
        return;
    }
    unsafe {
        let screen = GetDC(HWND::default());
        let hdc = CreateCompatibleDC(screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: cx,
                biHeight: -cy,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(HDC::default(), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .unwrap_or_default();
        let old = SelectObject(hdc, dib);
        let mut drew = false;
        if let Some(d2d) = d2d {
            if let Ok(rt) = create_rt(&d2d.factory) {
                let rect = RECT {
                    left: 0,
                    top: 0,
                    right: cx,
                    bottom: cy,
                };
                if rt.BindDC(hdc, &rect).is_ok() {
                    let _ = rt.BeginDraw();
                    rt.Clear(Some(&rgb(0, 0, 0, 0.0)));
                    draw_pill(
                        &rt,
                        &d2d.factory,
                        &d2d.dwrite,
                        snap,
                        hover,
                        limit,
                        break_dur,
                        scale,
                    );
                    drew = rt.EndDraw(None, None).is_ok();
                }
            }
        }
        if !drew {
            gdi_fallback(hdc, bits, cx, cy, snap);
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
            hdc,
            Some(&src as *const POINT),
            COLORREF(0),
            Some(&blend as *const BLENDFUNCTION),
            ULW_ALPHA,
        );
        SelectObject(hdc, old);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(hdc);
        ReleaseDC(HWND::default(), screen);
    }
}

fn create_rt(factory: &ID2D1Factory) -> windows::core::Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe { factory.CreateDCRenderTarget(&props) }
}

fn as_rt(rt: &ID2D1DCRenderTarget) -> ID2D1RenderTarget {
    rt.cast().expect("d2d render target")
}

fn draw_pill(
    dc: &ID2D1DCRenderTarget,
    factory: &ID2D1Factory,
    dwrite: &IDWriteFactory,
    snap: Snapshot,
    hover: bool,
    limit: std::time::Duration,
    break_dur: std::time::Duration,
    scale: f32,
) {
    let s = scale;
    let x = 8.0 * s;
    let y = 12.0 * s;
    let w = 204.0 * s;
    let h = 64.0 * s;
    let radius = 10.0 * s;
    let rt = as_rt(dc);
    unsafe {
        fill_round(&rt, x + 1.0 * s, y + 3.0 * s, w, h, radius, rgb(0, 0, 0, 0.22));
        fill_round(&rt, x, y, w, h, radius, rgb(0x10, 0x12, 0x14, 0.98));
        stroke_round(&rt, x, y, w, h, radius, rgb(255, 255, 255, 0.10), 1.0 * s);

        let digits = match snap.state {
            VisibleState::Break => format_break_remaining(snap.break_remaining),
            _ => format_elapsed(snap.sitting_elapsed),
        };
        let goal = match snap.state {
            VisibleState::Break => break_dur,
            _ => limit,
        };
        let paused = snap.state == VisibleState::Paused;
        let ((px, py), (rx, ry), cr) = control_centers(TimerShape::Capsule, s);
        let tag_pad = 5.0 * s;
        let tag_left = px - cr - tag_pad;
        let tag_top = py - cr - tag_pad;
        let tag_w = (cr + tag_pad) * 2.0;
        let tag_h = (ry + cr + tag_pad) - tag_top;
        let text_left = x + 14.0 * s;
        let text_right = tag_left - 10.0 * s;
        let bar_h = 4.0 * s;
        let bar_y = y + h - 12.0 * s;
        let fill = progress(
            snap.state,
            snap.sitting_elapsed,
            limit,
            snap.break_elapsed,
            break_dur,
        );

        if let Some(fmt) = digit_format(dwrite, 36.0 * s, DWRITE_TEXT_ALIGNMENT_LEADING) {
            draw_text(
                &rt,
                &fmt,
                &digits,
                D2D_RECT_F {
                    left: text_left,
                    top: y,
                    right: text_right,
                    bottom: bar_y - 1.0 * s,
                },
                if paused {
                    rgb(252, 252, 248, 0.22)
                } else {
                    rgb(252, 252, 248, 1.0)
                },
            );
        }

        draw_progress(
            &rt,
            text_left,
            bar_y,
            (tag_left - 4.0 * s - text_left).max(8.0 * s),
            bar_h,
            fill,
            progress_color(snap.state, fill, paused),
        );

        if paused {
            fill_round(
                &rt,
                x,
                y,
                (tag_left - x).max(8.0 * s),
                h,
                radius,
                rgb(8, 10, 12, 0.28),
            );
            let (cx, cy, r) = play_center(s);
            fill_play_triangle(&rt, factory, cx, cy, r * 1.05, rgb(252, 252, 248, 0.96));
        }

        fill_round(
            &rt,
            tag_left,
            tag_top,
            tag_w,
            tag_h,
            8.0 * s,
            rgb(28, 32, 34, 0.98),
        );
        stroke_round(
            &rt,
            tag_left,
            tag_top,
            tag_w,
            tag_h,
            8.0 * s,
            rgb(255, 255, 255, 0.10),
            1.0 * s,
        );
        fill_round(
            &rt,
            tag_left + 1.0 * s,
            tag_top + 8.0 * s,
            1.5 * s,
            tag_h - 16.0 * s,
            0.75 * s,
            rgb(212, 196, 168, 0.35),
        );

        if hover {
            let icon = rgb(252, 252, 248, 1.0);
            if !paused {
                draw_icon(&rt, dwrite, "\u{E769}", px, py, cr * 0.95, icon);
            }
            draw_icon(&rt, dwrite, "\u{E72C}", rx, ry, cr * 0.95, icon);
        } else {
            let (n, unit) = format_goal_parts(goal);
            let mid = tag_top + tag_h * 0.48;
            if let Some(fmt) = digit_format(dwrite, 16.0 * s, DWRITE_TEXT_ALIGNMENT_CENTER) {
                draw_text(
                    &rt,
                    &fmt,
                    &n,
                    D2D_RECT_F {
                        left: tag_left,
                        top: mid - 15.0 * s,
                        right: tag_left + tag_w,
                        bottom: mid + 3.0 * s,
                    },
                    rgb(236, 224, 204, 0.95),
                );
            }
            if let Some(fmt) = label_format(dwrite, 9.0 * s, DWRITE_TEXT_ALIGNMENT_CENTER) {
                draw_text(
                    &rt,
                    &fmt,
                    unit,
                    D2D_RECT_F {
                        left: tag_left,
                        top: mid + 1.0 * s,
                        right: tag_left + tag_w,
                        bottom: mid + 13.0 * s,
                    },
                    rgb(210, 208, 200, 0.50),
                );
            }
        }
    }
}

fn progress_color(state: VisibleState, t: f32, paused: bool) -> D2D1_COLOR_F {
    let a = if paused { 0.35 } else { 0.96 };
    match state {
        VisibleState::Overdue => rgb(0xFF, 0x45, 0x3A, a),
        VisibleState::Break => rgb(0x30, 0xD1, 0x58, a),
        _ if t >= 0.85 => rgb(0xFF, 0x9F, 0x0A, a),
        _ => rgb(0x0A, 0x84, 0xFF, a),
    }
}

unsafe fn digit_format(
    dwrite: &IDWriteFactory,
    size: f32,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    let fmt = dwrite
        .CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            DWRITE_FONT_WEIGHT_BLACK,
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
                    DWRITE_FONT_WEIGHT_BLACK,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()
        })?;
    let _ = fmt.SetTextAlignment(align);
    let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    Some(fmt)
}

unsafe fn label_format(
    dwrite: &IDWriteFactory,
    size: f32,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Option<IDWriteTextFormat> {
    let fmt = dwrite
        .CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_MEDIUM,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("en-us"),
        )
        .ok()?;
    let _ = fmt.SetTextAlignment(align);
    let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    Some(fmt)
}

unsafe fn draw_icon(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    glyph: &str,
    cx: f32,
    cy: f32,
    size: f32,
    color: D2D1_COLOR_F,
) {
    let Ok(fmt) = dwrite.CreateTextFormat(
        w!("Segoe MDL2 Assets"),
        None,
        DWRITE_FONT_WEIGHT_MEDIUM,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        size,
        w!("en-us"),
    ) else {
        return;
    };
    let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    draw_text(
        rt,
        &fmt,
        glyph,
        D2D_RECT_F {
            left: cx - size,
            top: cy - size,
            right: cx + size,
            bottom: cy + size,
        },
        color,
    );
}

unsafe fn draw_text(
    rt: &ID2D1RenderTarget,
    format: &IDWriteTextFormat,
    text: &str,
    rect: D2D_RECT_F,
    color: D2D1_COLOR_F,
) {
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        rt.DrawText(
            &wide,
            format,
            &rect,
            &brush,
            Default::default(),
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

unsafe fn fill_play_triangle(
    rt: &ID2D1RenderTarget,
    factory: &ID2D1Factory,
    cx: f32,
    cy: f32,
    size: f32,
    color: D2D1_COLOR_F,
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
            D2D_POINT_2F {
                x: left + w,
                y: cy,
            },
            D2D_POINT_2F {
                x: left,
                y: cy + h * 0.5,
            },
        ]);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        let _ = sink.Close();
    }
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        let _ = rt.FillGeometry(&geo, &brush, None);
    }
}

unsafe fn fill_round(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: D2D1_COLOR_F,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let radius = radius.min(h / 2.0).min(w / 2.0);
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                radiusX: radius,
                radiusY: radius,
            },
            &brush,
        );
    }
}

unsafe fn draw_progress(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    t: f32,
    color: D2D1_COLOR_F,
) {
    let radius = h / 2.0;
    fill_round(rt, x, y, w, h, radius, rgb(255, 255, 255, 0.10));
    if t <= 0.0 {
        return;
    }
    let fw = (w * t).max(h).min(w);
    fill_round(rt, x, y, fw, h, radius, color);
}

unsafe fn stroke_round(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: D2D1_COLOR_F,
    width: f32,
) {
    if let Ok(brush) = rt.CreateSolidColorBrush(&color as *const _, None) {
        rt.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                radiusX: radius,
                radiusY: radius,
            },
            &brush,
            width,
            None,
        );
    }
}

unsafe fn gdi_fallback(hdc: HDC, bits: *mut core::ffi::c_void, cx: i32, cy: i32, snap: Snapshot) {
    if bits.is_null() || cx <= 0 || cy <= 0 {
        return;
    }
    let stride = cx * 4;
    let ptr = bits as *mut u8;
    for y in 0..cy {
        for x in 0..cx {
            let inside = rounded_contains(x as f32, y as f32, cx as f32, cy as f32, 10.0);
            let i = (y * stride + x * 4) as isize;
            if inside {
                *ptr.offset(i) = 0x1C;
                *ptr.offset(i + 1) = 0x19;
                *ptr.offset(i + 2) = 0x15;
                *ptr.offset(i + 3) = 240;
            } else {
                *ptr.offset(i) = 0;
                *ptr.offset(i + 1) = 0;
                *ptr.offset(i + 2) = 0;
                *ptr.offset(i + 3) = 0;
            }
        }
    }
    let digits = match snap.state {
        VisibleState::Break => format_break_remaining(snap.break_remaining),
        _ => format_elapsed(snap.sitting_elapsed),
    };
    let mut text: Vec<u16> = digits.encode_utf16().chain(std::iter::once(0)).collect();
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(0x00F2F5F5));
    let mut rc = RECT {
        left: 16,
        top: 0,
        right: cx - 16,
        bottom: cy,
    };
    let _ = DrawTextW(hdc, &mut text, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    for y in 0..cy {
        for x in 0..cx {
            let i = (y * stride + x * 4) as isize;
            let b0 = *ptr.offset(i);
            let g0 = *ptr.offset(i + 1);
            let r0 = *ptr.offset(i + 2);
            if b0 != 0 || g0 != 0 || r0 != 0 {
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
