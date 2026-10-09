//! Light and dark palettes shared by the overlay, settings and flyout.
//!
//! Colors live in the library so contrast can be checked by tests without Win32.

use crate::engine::VisibleState;

/// sRGB color with 8-bit alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn opaque(hex: u32) -> Self {
        Self((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
    }

    pub const fn alpha(self, a: u8) -> Self {
        Self(self.0, self.1, self.2, a)
    }

    /// Composite over an opaque background.
    pub fn over(self, bg: Rgba) -> Rgba {
        let a = f32::from(self.3) / 255.0;
        let mix = |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
        Rgba(mix(self.0, bg.0), mix(self.1, bg.1), mix(self.2, bg.2), 255)
    }

    fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }
}

/// WCAG contrast ratio of two opaque colors.
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    // Settings window and flyout.
    pub window: Rgba,
    pub card: Rgba,
    pub card_stroke: Rgba,
    pub separator: Rgba,
    pub text: Rgba,
    pub text_secondary: Rgba,
    pub text_disabled: Rgba,
    pub control: Rgba,
    pub control_hover: Rgba,
    pub control_pressed: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub on_accent: Rgba,
    pub toggle_off: Rgba,
    pub knob: Rgba,
    pub focus: Rgba,
    pub window_stroke: Rgba,
    // Overlay.
    pub pill: Rgba,
    pub pill_overdue: Rgba,
    pub pill_stroke: Rgba,
    pub digits: Rgba,
    pub tag: Rgba,
    pub tag_sub: Rgba,
    pub bar_track: Rgba,
    pub bar_normal: Rgba,
    pub bar_warn: Rgba,
    pub bar_over: Rgba,
    pub bar_break: Rgba,
    pub bar_away: Rgba,
    pub shadow: Rgba,
}

pub const DARK: Palette = Palette {
    dark: true,
    window: Rgba::opaque(0x1F1F21),
    card: Rgba::opaque(0x2A2A2D),
    card_stroke: Rgba(255, 255, 255, 14),
    separator: Rgba(255, 255, 255, 20),
    text: Rgba::opaque(0xF3F3F5),
    text_secondary: Rgba::opaque(0xB4B4BA),
    text_disabled: Rgba::opaque(0x75757C),
    control: Rgba::opaque(0x3A3A3E),
    control_hover: Rgba::opaque(0x46464B),
    control_pressed: Rgba::opaque(0x323236),
    accent: Rgba::opaque(0x3D6CF2),
    accent_hover: Rgba::opaque(0x4A78F7),
    on_accent: Rgba::opaque(0xFFFFFF),
    toggle_off: Rgba::opaque(0x55555B),
    knob: Rgba::opaque(0xFFFFFF),
    focus: Rgba::opaque(0x8DB3FF),
    window_stroke: Rgba(255, 255, 255, 34),
    pill: Rgba(0x10, 0x12, 0x14, 248),
    pill_overdue: Rgba(0x2A, 0x10, 0x16, 248),
    pill_stroke: Rgba(255, 255, 255, 24),
    digits: Rgba::opaque(0xFCFCF8),
    tag: Rgba(0xEC, 0xE0, 0xCC, 242),
    tag_sub: Rgba(0xD2, 0xD0, 0xC8, 150),
    bar_track: Rgba(255, 255, 255, 26),
    bar_normal: Rgba::opaque(0x3D7BFF),
    bar_warn: Rgba::opaque(0xFF9F0A),
    bar_over: Rgba::opaque(0xFF453A),
    bar_break: Rgba::opaque(0x30D158),
    bar_away: Rgba::opaque(0x64D2FF),
    shadow: Rgba(0, 0, 0, 60),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    window: Rgba::opaque(0xF3F3F5),
    card: Rgba::opaque(0xFFFFFF),
    card_stroke: Rgba(0, 0, 0, 16),
    separator: Rgba(0, 0, 0, 18),
    text: Rgba::opaque(0x1B1B1F),
    text_secondary: Rgba::opaque(0x5C5C63),
    text_disabled: Rgba::opaque(0xA3A3AA),
    control: Rgba::opaque(0xEDEDF0),
    control_hover: Rgba::opaque(0xE2E2E6),
    control_pressed: Rgba::opaque(0xD7D7DC),
    accent: Rgba::opaque(0x2F5FE6),
    accent_hover: Rgba::opaque(0x2752CC),
    on_accent: Rgba::opaque(0xFFFFFF),
    toggle_off: Rgba::opaque(0xB9B9C0),
    knob: Rgba::opaque(0xFFFFFF),
    focus: Rgba::opaque(0x2F5FE6),
    window_stroke: Rgba(0, 0, 0, 40),
    pill: Rgba(0xFB, 0xFB, 0xFC, 250),
    pill_overdue: Rgba(0xFD, 0xEB, 0xEC, 250),
    pill_stroke: Rgba(0, 0, 0, 30),
    digits: Rgba::opaque(0x15171A),
    tag: Rgba(0x3A, 0x33, 0x29, 240),
    tag_sub: Rgba(0x55, 0x55, 0x5C, 200),
    bar_track: Rgba(0, 0, 0, 22),
    bar_normal: Rgba::opaque(0x2F5FE6),
    bar_warn: Rgba::opaque(0xD97706),
    bar_over: Rgba::opaque(0xDC2626),
    bar_break: Rgba::opaque(0x16A34A),
    bar_away: Rgba::opaque(0x0891B2),
    shadow: Rgba(0, 0, 0, 34),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

impl Palette {
    pub fn pill_for(&self, state: VisibleState) -> Rgba {
        if state == VisibleState::Overdue {
            self.pill_overdue
        } else {
            self.pill
        }
    }

    /// Progress bar color: blue while sitting, amber near the limit, red when overdue,
    /// green during a break and cyan while away.
    pub fn bar_for(&self, state: VisibleState, progress: f32) -> Rgba {
        match state {
            VisibleState::Overdue => self.bar_over,
            VisibleState::Break => self.bar_break,
            VisibleState::Away => self.bar_away,
            _ if progress >= 0.85 => self.bar_warn,
            _ => self.bar_normal,
        }
    }
}
