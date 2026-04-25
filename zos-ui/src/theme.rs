//! Catppuccin Mocha theme + tokens.
//!
//! All zOS UIs reference this module. The palette is the canonical set;
//! typography/spacing/radius are tokens that compose into per-component
//! styles via the `style!` helper (separate task) or direct iced styling.
//!
//! ## Naming
//!
//! Two distinct types are involved:
//!
//! - [`iced::Theme`] — the renderer-level theme passed to `Application` /
//!   `Program`. Built via [`zos_theme()`].
//! - [`Tokens`] — a zero-sized accessor for theme-aware values used by
//!   custom components in this crate. Methods on `Tokens` return the
//!   exact `Color` values from the palette below.
//!
//! Component code that just needs a color reads via `Tokens`; code that
//! constructs an iced `Application` calls [`zos_theme()`] once at startup.

use iced::theme::Palette;
use iced::{Color, Theme, color};

// --- Catppuccin Mocha palette (https://catppuccin.com/palette) ---
pub const BASE: Color = color!(0x1e1e2e);
pub const MANTLE: Color = color!(0x181825);
pub const CRUST: Color = color!(0x11111b);
pub const SURFACE0: Color = color!(0x313244);
pub const SURFACE1: Color = color!(0x45475a);
pub const SURFACE2: Color = color!(0x585b70);
pub const OVERLAY0: Color = color!(0x6c7086);
pub const OVERLAY1: Color = color!(0x7f849c);
pub const OVERLAY2: Color = color!(0x9399b2);
pub const TEXT: Color = color!(0xcdd6f4);
pub const SUBTEXT0: Color = color!(0xa6adc8);
pub const SUBTEXT1: Color = color!(0xbac2de);
pub const BLUE: Color = color!(0x89b4fa);
pub const LAVENDER: Color = color!(0xb4befe);
pub const MAUVE: Color = color!(0xcba6f7);
pub const PINK: Color = color!(0xf5c2e7);
pub const GREEN: Color = color!(0xa6e3a1);
pub const YELLOW: Color = color!(0xf9e2af);
pub const RED: Color = color!(0xf38ba8);
pub const PEACH: Color = color!(0xfab387);
pub const SAPPHIRE: Color = color!(0x74c7ec);
pub const SKY: Color = color!(0x89dceb);
pub const TEAL: Color = color!(0x94e2d5);
pub const ROSEWATER: Color = color!(0xf5e0dc);
pub const FLAMINGO: Color = color!(0xf2cdcd);
pub const MAROON: Color = color!(0xeba0ac);

// --- Typography scale (rem-style float pixels) ---
pub mod font_size {
    pub const XS: f32 = 11.0;
    pub const SM: f32 = 13.0;
    pub const BASE: f32 = 14.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 18.0;
    pub const X2L: f32 = 22.0;
    pub const X3L: f32 = 28.0;
}

// --- Spacing tokens (px) ---
pub mod space {
    pub const X1: f32 = 4.0;
    pub const X2: f32 = 8.0;
    pub const X3: f32 = 12.0;
    pub const X4: f32 = 16.0;
    pub const X6: f32 = 24.0;
    pub const X8: f32 = 32.0;
}

// --- Radius tokens (px) ---
pub mod radius {
    pub const SM: f32 = 4.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const FULL: f32 = 9999.0;
}

// --- Transition durations (ms) ---
pub mod duration {
    pub const FAST: u64 = 150;
    pub const NORMAL: u64 = 250;
    pub const SLOW: u64 = 400;
}

/// Build the canonical zOS iced theme.
///
/// Pass the result to `iced::Application::theme()` (or `Program::theme()`)
/// once at startup. The palette below maps Catppuccin Mocha onto iced's
/// six semantic slots; component code that needs more nuance reads
/// from [`Tokens`] or the constants directly.
pub fn zos_theme() -> Theme {
    Theme::custom(
        "zOS Mocha".to_string(),
        Palette {
            background: BASE,
            text: TEXT,
            primary: BLUE,
            success: GREEN,
            warning: YELLOW,
            danger: RED,
        },
    )
}

/// Theme-aware accessor used by custom components.
///
/// Zero-sized; clone-and-pass-by-value. Methods return concrete `Color`
/// values from the Catppuccin Mocha palette. When runtime theme swap
/// lands, this struct will hold a handle into the active palette and
/// these methods will resolve through it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tokens;

impl Tokens {
    pub fn bg(&self) -> Color {
        BASE
    }
    pub fn mantle(&self) -> Color {
        MANTLE
    }
    pub fn crust(&self) -> Color {
        CRUST
    }
    pub fn surface(&self) -> Color {
        SURFACE0
    }
    pub fn surface_alt(&self) -> Color {
        SURFACE1
    }
    pub fn overlay(&self) -> Color {
        OVERLAY0
    }
    pub fn text(&self) -> Color {
        TEXT
    }
    pub fn subtext(&self) -> Color {
        SUBTEXT0
    }
    pub fn accent(&self) -> Color {
        BLUE
    }
    pub fn accent_purple(&self) -> Color {
        MAUVE
    }
    pub fn success(&self) -> Color {
        GREEN
    }
    pub fn warning(&self) -> Color {
        YELLOW
    }
    pub fn danger(&self) -> Color {
        RED
    }
}
