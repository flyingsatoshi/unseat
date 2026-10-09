//! Direct2D/DirectWrite helpers shared by the overlay, flyout and settings window.

use std::cell::RefCell;
use unseat::Rgba;
use windows::core::{w, PCWSTR};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, D2D1_ROUNDED_RECT};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
};

pub use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_BOLD as BOLD, DWRITE_FONT_WEIGHT_MEDIUM as MEDIUM,
    DWRITE_FONT_WEIGHT_NORMAL as REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD as SEMIBOLD,
    DWRITE_TEXT_ALIGNMENT_CENTER as CENTER, DWRITE_TEXT_ALIGNMENT_LEADING as LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING as TRAILING,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Segoe UI Variable Display, falling back to Segoe UI.
    Ui,
    /// Segoe Fluent Icons, falling back to Segoe MDL2 Assets.
    Icons,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FormatKey {
    family: Family,
    wrap: bool,
    size_centi: u32,
    weight: i32,
    align: i32,
}

/// DirectWrite text formats are immutable and cheap to keep, so each distinct
/// (family, size, weight, alignment) is created once and reused for every repaint.
pub struct Fonts {
    pub dwrite: IDWriteFactory,
    cache: RefCell<Vec<(FormatKey, IDWriteTextFormat)>>,
}

impl Fonts {
    pub fn new(dwrite: IDWriteFactory) -> Self {
        Self {
            dwrite,
            cache: RefCell::new(Vec::new()),
        }
    }

    pub fn format(
        &self,
        family: Family,
        size: f32,
        weight: DWRITE_FONT_WEIGHT,
        align: DWRITE_TEXT_ALIGNMENT,
    ) -> Option<IDWriteTextFormat> {
        self.format_with(family, false, size, weight, align)
    }

    fn format_with(
        &self,
        family: Family,
        wrap: bool,
        size: f32,
        weight: DWRITE_FONT_WEIGHT,
        align: DWRITE_TEXT_ALIGNMENT,
    ) -> Option<IDWriteTextFormat> {
        let key = FormatKey {
            family,
            wrap,
            size_centi: (size.max(1.0) * 100.0).round() as u32,
            weight: weight.0,
            align: align.0,
        };
        if let Some((_, f)) = self.cache.borrow().iter().find(|(k, _)| *k == key) {
            return Some(f.clone());
        }
        let names: [PCWSTR; 2] = match family {
            Family::Ui => [w!("Segoe UI Variable Display"), w!("Segoe UI")],
            Family::Icons => [w!("Segoe Fluent Icons"), w!("Segoe MDL2 Assets")],
        };
        let size = key.size_centi as f32 / 100.0;
        let format = names.iter().find_map(|name| unsafe {
            self.dwrite
                .CreateTextFormat(
                    *name,
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()
        })?;
        unsafe {
            let _ = format.SetTextAlignment(align);
            if wrap {
                let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
                let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            } else {
                let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            }
        }
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 64 {
            cache.clear();
        }
        cache.push((key, format.clone()));
        Some(format)
    }
}

pub fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(c.0) / 255.0,
        g: f32::from(c.1) / 255.0,
        b: f32::from(c.2) / 255.0,
        a: f32::from(c.3) / 255.0,
    }
}

/// `c` with its alpha multiplied by `k` (0–1).
pub fn fade(c: Rgba, k: f32) -> Rgba {
    Rgba(c.0, c.1, c.2, (f32::from(c.3) * k.clamp(0.0, 1.0)).round() as u8)
}

pub fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

pub fn inflate(rc: D2D_RECT_F, by: f32) -> D2D_RECT_F {
    rect(rc.left - by, rc.top - by, rc.right + by, rc.bottom + by)
}

pub fn contains(rc: D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

pub unsafe fn fill_round(rt: &ID2D1RenderTarget, rc: D2D_RECT_F, radius: f32, c: Rgba) {
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w <= 0.0 || h <= 0.0 || c.3 == 0 {
        return;
    }
    let r = radius.min(w / 2.0).min(h / 2.0);
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: rc,
                radiusX: r,
                radiusY: r,
            },
            &brush,
        );
    }
}

pub unsafe fn stroke_round(
    rt: &ID2D1RenderTarget,
    rc: D2D_RECT_F,
    radius: f32,
    c: Rgba,
    width: f32,
) {
    if c.3 == 0 {
        return;
    }
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
        rt.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: rc,
                radiusX: r,
                radiusY: r,
            },
            &brush,
            width,
            None,
        );
    }
}

pub unsafe fn line(rt: &ID2D1RenderTarget, a: (f32, f32), b: (f32, f32), c: Rgba, width: f32) {
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
        rt.DrawLine(
            D2D_POINT_2F { x: a.0, y: a.1 },
            D2D_POINT_2F { x: b.0, y: b.1 },
            &brush,
            width,
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub unsafe fn text(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    family: Family,
    s: &str,
    rc: D2D_RECT_F,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    c: Rgba,
) {
    if s.is_empty() || c.3 == 0 {
        return;
    }
    let Some(format) = fonts.format(family, size, weight, align) else {
        return;
    };
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
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

/// Top-aligned text that wraps inside `rc`.
pub unsafe fn text_wrapped(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    s: &str,
    rc: D2D_RECT_F,
    size: f32,
    c: Rgba,
) {
    let Some(format) = fonts.format_with(Family::Ui, true, size, REGULAR, LEADING) else {
        return;
    };
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
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

/// Icon glyphs shared by Segoe Fluent Icons and Segoe MDL2 Assets.
pub mod glyph {
    pub const PAUSE: &str = "\u{E769}";
    pub const RESET: &str = "\u{E72C}";
    pub const VOLUME: &str = "\u{E767}";
}
