// === zos-settings theme — Catppuccin Mocha palette ===

use iced::theme::Palette;
use iced::{color, Color, Theme};

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
