//! Direct2D/DirectWrite helpers shared by the overlay, flyout and settings window.

use std::cell::{OnceCell, RefCell};
use unseat::Rgba;
use windows::core::{w, PCWSTR};
use windows::core::Interface;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget, D2D1_ARC_SEGMENT,
    D2D1_ARC_SIZE_SMALL, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED,
    IDWriteFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontCollection1,
    IDWriteFontSetBuilder1, IDWriteInMemoryFontFileLoader, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FONT_FEATURE,
    DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_TEXT_RANGE,
    DWRITE_FONT_WEIGHT, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP,
};

pub use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_MEDIUM as MEDIUM,
    DWRITE_FONT_WEIGHT_NORMAL as REGULAR, DWRITE_FONT_WEIGHT_SEMI_BOLD as SEMIBOLD,
    DWRITE_TEXT_ALIGNMENT_CENTER as CENTER, DWRITE_TEXT_ALIGNMENT_LEADING as LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING as TRAILING,
};

/// The UI thread's Direct2D factory and text formats, used by every window. Hardware render
/// targets from one factory share one Direct3D device; a factory per window cost 6–22 MB of
/// private memory each, kept for as long as the window existed (hidden ones included).
pub struct Shared {
    pub factory: ID2D1Factory,
    pub fonts: Fonts,
}

/// The shared factory, created on first use. `None` if Direct2D or DirectWrite cannot start.
///
/// It lives until the process exits and is never released: every window is created on the UI
/// thread and may draw until the message loop ends, and releasing COM objects from a
/// thread-local destructor during process teardown is not safe.
pub fn shared() -> Option<&'static Shared> {
    thread_local! {
        static SHARED: OnceCell<Option<&'static Shared>> = const { OnceCell::new() };
    }
    SHARED.with(|cell| {
        *cell.get_or_init(|| unsafe {
            let factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let dwrite = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            Some(&*Box::leak(Box::new(Shared {
                factory,
                fonts: Fonts::new(dwrite),
            })))
        })
    })
}

/// The window's DPI as a scale factor (1.0 at 96 DPI).
pub fn dpi_scale(hwnd: HWND) -> f32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 1.0,
        d => d as f32 / 96.0,
    }
}

/// The render target of an ordinary (opaque) window, from the shared factory: created on first
/// use, resized to the client area after that. `slot` holds it between paints.
pub unsafe fn window_target(
    hwnd: HWND,
    slot: &mut Option<ID2D1HwndRenderTarget>,
) -> Option<ID2D1RenderTarget> {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let size = D2D_SIZE_U {
        width: (rc.right - rc.left).max(1) as u32,
        height: (rc.bottom - rc.top).max(1) as u32,
    };
    match slot.as_ref() {
        Some(rt) => {
            let _ = rt.Resize(&size);
        }
        None => {
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
            *slot = shared()?.factory.CreateHwndRenderTarget(&props, &hwnd_props).ok();
        }
    }
    slot.as_ref().and_then(|rt| rt.cast().ok())
}

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
        let size = key.size_centi as f32 / 100.0;
        let create = |name: PCWSTR, collection: Option<&IDWriteFontCollection>| unsafe {
            self.dwrite
                .CreateTextFormat(
                    name,
                    collection,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()
        };
        let format = match family {
            Family::Ui => sora(&self.dwrite)
                .and_then(|c| create(w!("Sora"), Some(&c)))
                .or_else(|| create(w!("Segoe UI Variable Display"), None))
                .or_else(|| create(w!("Segoe UI"), None)),
            Family::Icons => create(w!("Segoe Fluent Icons"), None)
                .or_else(|| create(w!("Segoe MDL2 Assets"), None)),
        }?;
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

static SORA_TTF: &[u8] = include_bytes!("../../assets/fonts/Sora.ttf");

/// The embedded Sora typeface (SIL OFL 1.1) as a private font collection, loaded once per
/// thread from memory. `None` on systems without in-memory font loading (pre-1703), in
/// which case text falls back to Segoe UI.
fn sora(dwrite: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    thread_local! {
        static SORA: RefCell<Option<Option<IDWriteFontCollection>>> = const { RefCell::new(None) };
    }
    SORA.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| unsafe { load_sora(dwrite) })
            .clone()
    })
}

unsafe fn load_sora(dwrite: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    let factory: IDWriteFactory5 = dwrite.cast().ok()?;
    let loader: IDWriteInMemoryFontFileLoader = factory.CreateInMemoryFontFileLoader().ok()?;
    factory.RegisterFontFileLoader(&loader).ok()?;
    let file = loader
        .CreateInMemoryFontFileReference(
            &factory,
            SORA_TTF.as_ptr() as *const core::ffi::c_void,
            SORA_TTF.len() as u32,
            None,
        )
        .ok()?;
    let builder: IDWriteFontSetBuilder1 = factory.CreateFontSetBuilder().ok()?.cast().ok()?;
    builder.AddFontFile(&file).ok()?;
    let set = builder.CreateFontSet().ok()?;
    let collection: IDWriteFontCollection1 = factory.CreateFontCollectionFromFontSet(&set).ok()?;
    collection.cast().ok()
}

unsafe fn tabular_layout(
    fonts: &Fonts,
    s: &str,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    w: f32,
    h: f32,
) -> Option<IDWriteTextLayout> {
    let format = fonts.format(Family::Ui, size, weight, align)?;
    let wide: Vec<u16> = s.encode_utf16().collect();
    let layout = fonts
        .dwrite
        .CreateTextLayout(&wide, &format, w.max(1.0), h.max(1.0))
        .ok()?;
    if let Ok(typography) = fonts.dwrite.CreateTypography() {
        let _ = typography.AddFontFeature(DWRITE_FONT_FEATURE {
            nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
            parameter: 1,
        });
        let _ = layout.SetTypography(
            &typography,
            DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: wide.len() as u32,
            },
        );
    }
    Some(layout)
}

/// Width of `s` set in the UI face (tabular figures) at `size`.
pub unsafe fn measure(fonts: &Fonts, s: &str, size: f32, weight: DWRITE_FONT_WEIGHT) -> f32 {
    tabular_layout(fonts, s, size, weight, LEADING, 10_000.0, size * 2.0)
        .map_or(0.0, |layout| layout_width(&layout))
}

/// Width of one line of laid-out text. It never wraps, so the box width doesn't matter.
unsafe fn layout_width(layout: &IDWriteTextLayout) -> f32 {
    let mut m = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS::default();
    if layout.GetMetrics(&mut m).is_ok() {
        m.widthIncludingTrailingWhitespace
    } else {
        0.0
    }
}

/// Draw digits with tabular figures so a running countdown never shifts sideways. The size
/// shrinks, if needed, so the text fits `rc` exactly as the font measures it.
#[allow(clippy::too_many_arguments)]
pub unsafe fn digits(
    rt: &ID2D1RenderTarget,
    fonts: &Fonts,
    s: &str,
    rc: D2D_RECT_F,
    max_size: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
    c: Rgba,
) {
    let lane = (rc.right - rc.left).max(1.0);
    let h = rc.bottom - rc.top;
    // Lay out at full size; only text too wide for the lane needs a second, smaller layout.
    let Some(mut layout) = tabular_layout(fonts, s, max_size, weight, align, lane, h) else {
        return;
    };
    let natural = layout_width(&layout);
    if natural > lane {
        match tabular_layout(fonts, s, max_size * lane / natural, weight, align, lane, h) {
            Some(smaller) => layout = smaller,
            None => return,
        }
    }
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
        rt.DrawTextLayout(
            D2D_POINT_2F {
                x: rc.left,
                y: rc.top,
            },
            &layout,
            &brush,
            Default::default(),
        );
    }
}

/// Rounded rectangle with its own radius per corner: [top-left, top-right, bottom-right,
/// bottom-left]. Used for docked shapes whose edge-side corners are square.
pub unsafe fn fill_corners(rt: &ID2D1RenderTarget, rc: D2D_RECT_F, radii: [f32; 4], c: Rgba) {
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w <= 0.0 || h <= 0.0 || c.3 == 0 {
        return;
    }
    let cap = (w / 2.0).min(h / 2.0);
    let [tl, tr, br, bl] = radii.map(|r| r.clamp(0.0, cap));
    let Ok(factory) = rt.GetFactory() else {
        return;
    };
    let Ok(geo) = factory.CreatePathGeometry() else {
        return;
    };
    let Ok(sink) = geo.Open() else {
        return;
    };
    let pt = |x: f32, y: f32| D2D_POINT_2F { x, y };
    let arc = |to: D2D_POINT_2F, r: f32| D2D1_ARC_SEGMENT {
        point: to,
        size: D2D_SIZE_F { width: r, height: r },
        rotationAngle: 0.0,
        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
        arcSize: D2D1_ARC_SIZE_SMALL,
    };
    sink.BeginFigure(pt(rc.left + tl, rc.top), D2D1_FIGURE_BEGIN_FILLED);
    sink.AddLine(pt(rc.right - tr, rc.top));
    if tr > 0.0 {
        sink.AddArc(&arc(pt(rc.right, rc.top + tr), tr));
    }
    sink.AddLine(pt(rc.right, rc.bottom - br));
    if br > 0.0 {
        sink.AddArc(&arc(pt(rc.right - br, rc.bottom), br));
    }
    sink.AddLine(pt(rc.left + bl, rc.bottom));
    if bl > 0.0 {
        sink.AddArc(&arc(pt(rc.left, rc.bottom - bl), bl));
    }
    sink.AddLine(pt(rc.left, rc.top + tl));
    if tl > 0.0 {
        sink.AddArc(&arc(pt(rc.left + tl, rc.top), tl));
    }
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    let _ = sink.Close();
    if let Ok(brush) = rt.CreateSolidColorBrush(&color(c), None) {
        rt.FillGeometry(&geo, &brush, None);
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
    pub const PLAY: &str = "\u{E768}";
    pub const PIN: &str = "\u{E718}";
    pub const PINNED: &str = "\u{E840}";
}
