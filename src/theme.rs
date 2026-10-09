//! Signal palettes (light and dark) shared by the overlay, settings, tray panel and cards.
//!
//! Colors live in the library so contrast can be checked by tests without Win32. In Signal
//! the overlay's whole surface carries the state: each state has its own background, a
//! lighter progress band that fills the pill, digit ink and tag ink.

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

/// How one overlay state is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayColors {
    pub bg: Rgba,
    /// The progress band that fills the pill from the leading edge.
    pub band: Rgba,
    pub digits: Rgba,
    pub tag: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    // Windows: settings, tray panel, alert card, flyout.
    pub window: Rgba,
    /// Row tiles and secondary buttons.
    pub card: Rgba,
    pub card_stroke: Rgba,
    pub separator: Rgba,
    pub text: Rgba,
    pub text_secondary: Rgba,
    pub text_disabled: Rgba,
    /// Unselected chips and stepper buttons.
    pub control: Rgba,
    pub control_hover: Rgba,
    pub control_pressed: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub on_accent: Rgba,
    /// Selected tab and chip fill, and the primary button.
    pub selected: Rgba,
    pub on_selected: Rgba,
    pub toggle_off: Rgba,
    pub knob: Rgba,
    pub focus: Rgba,
    pub window_stroke: Rgba,
    /// Alert card for "time is up".
    pub alarm: Rgba,
    pub on_alarm: Rgba,
    // Overlay.
    pub sit: OverlayColors,
    pub warn: OverlayColors,
    pub over: OverlayColors,
    pub brk: OverlayColors,
    pub idle: OverlayColors,
    pub pill_stroke: Rgba,
    pub shadow: Rgba,
}

pub const DARK: Palette = Palette {
    dark: true,
    window: Rgba::opaque(0x1A1918),
    card: Rgba::opaque(0x262422),
    card_stroke: Rgba(255, 255, 255, 12),
    separator: Rgba(255, 255, 255, 16),
    text: Rgba::opaque(0xF5F4F1),
    text_secondary: Rgba::opaque(0xA8A29E),
    text_disabled: Rgba::opaque(0x6B6560),
    control: Rgba::opaque(0x33302C),
    control_hover: Rgba::opaque(0x3D3A35),
    control_pressed: Rgba::opaque(0x2C2A27),
    accent: Rgba::opaque(0x2EC4A0),
    accent_hover: Rgba::opaque(0x45D1AE),
    on_accent: Rgba::opaque(0x08251D),
    selected: Rgba::opaque(0x2EC4A0),
    on_selected: Rgba::opaque(0x08251D),
    toggle_off: Rgba::opaque(0x4A4540),
    knob: Rgba::opaque(0xFFFFFF),
    focus: Rgba::opaque(0x7BE0C6),
    window_stroke: Rgba(255, 255, 255, 26),
    alarm: Rgba::opaque(0xC4371F),
    on_alarm: Rgba::opaque(0xFFFFFF),
    sit: OverlayColors {
        bg: Rgba::opaque(0x17211E),
        band: Rgba::opaque(0x1F3B33),
        digits: Rgba::opaque(0xEAF6F2),
        tag: Rgba::opaque(0x9CC9BC),
    },
    warn: OverlayColors {
        bg: Rgba::opaque(0x231C10),
        band: Rgba::opaque(0x4A3816),
        digits: Rgba::opaque(0xFFF4E0),
        tag: Rgba::opaque(0xF2C77E),
    },
    over: OverlayColors {
        bg: Rgba::opaque(0xC4371F),
        band: Rgba::opaque(0xC4371F),
        digits: Rgba::opaque(0xFFFFFF),
        tag: Rgba::opaque(0xFFF1EE),
    },
    brk: OverlayColors {
        bg: Rgba::opaque(0x161C2C),
        band: Rgba::opaque(0x25345A),
        digits: Rgba::opaque(0xE8EEFF),
        tag: Rgba::opaque(0xA9C2FF),
    },
    idle: OverlayColors {
        bg: Rgba::opaque(0x24221F),
        band: Rgba::opaque(0x33302C),
        digits: Rgba::opaque(0xF5F4F1),
        tag: Rgba::opaque(0xA8A29E),
    },
    pill_stroke: Rgba(255, 255, 255, 18),
    shadow: Rgba(0, 0, 0, 70),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    window: Rgba::opaque(0xFFFFFF),
    card: Rgba::opaque(0xF3F1EC),
    card_stroke: Rgba(0, 0, 0, 10),
    separator: Rgba(0, 0, 0, 16),
    text: Rgba::opaque(0x1A1918),
    text_secondary: Rgba::opaque(0x57534E),
    text_disabled: Rgba::opaque(0xA8A29E),
    control: Rgba::opaque(0xE7E4DD),
    control_hover: Rgba::opaque(0xDDD9D0),
    control_pressed: Rgba::opaque(0xD2CDC2),
    accent: Rgba::opaque(0x0F7D62),
    accent_hover: Rgba::opaque(0x0B6B54),
    on_accent: Rgba::opaque(0xFFFFFF),
    selected: Rgba::opaque(0x1A1918),
    on_selected: Rgba::opaque(0xFFFFFF),
    toggle_off: Rgba::opaque(0xC4BEB5),
    knob: Rgba::opaque(0xFFFFFF),
    focus: Rgba::opaque(0x0F7D62),
    window_stroke: Rgba(0, 0, 0, 34),
    alarm: Rgba::opaque(0xC4371F),
    on_alarm: Rgba::opaque(0xFFFFFF),
    sit: OverlayColors {
        bg: Rgba::opaque(0xEEF6F3),
        band: Rgba::opaque(0xD3EBE3),
        digits: Rgba::opaque(0x0C2A22),
        tag: Rgba::opaque(0x2E5C50),
    },
    warn: OverlayColors {
        bg: Rgba::opaque(0xFDF4E3),
        band: Rgba::opaque(0xF7DFAE),
        digits: Rgba::opaque(0x3D2A05),
        tag: Rgba::opaque(0x7A5410),
    },
    over: OverlayColors {
        bg: Rgba::opaque(0xC4371F),
        band: Rgba::opaque(0xC4371F),
        digits: Rgba::opaque(0xFFFFFF),
        tag: Rgba::opaque(0xFFF1EE),
    },
    brk: OverlayColors {
        bg: Rgba::opaque(0xEEF2FD),
        band: Rgba::opaque(0xD5E0FB),
        digits: Rgba::opaque(0x132355),
        tag: Rgba::opaque(0x3A4F8C),
    },
    idle: OverlayColors {
        bg: Rgba::opaque(0xF1EFEB),
        band: Rgba::opaque(0xE4E0D9),
        digits: Rgba::opaque(0x1A1918),
        tag: Rgba::opaque(0x57534E),
    },
    pill_stroke: Rgba(0, 0, 0, 22),
    shadow: Rgba(0, 0, 0, 40),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// Fraction of the limit after which a sitting session shows the "near limit" colors.
pub const WARN_AT: f32 = 0.85;

impl Palette {
    /// Overlay colors for a state; `progress` (0-1) picks the near-limit variant.
    pub fn overlay(&self, state: VisibleState, progress: f32) -> OverlayColors {
        match state {
            VisibleState::Overdue => self.over,
            VisibleState::Break => self.brk,
            VisibleState::Away | VisibleState::Paused => self.idle,
            VisibleState::Sitting if progress >= WARN_AT => self.warn,
            VisibleState::Sitting => self.sit,
        }
    }
}
