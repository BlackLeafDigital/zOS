// === tui/theme.rs — Catppuccin Mocha color palette ===

use ratatui::style::{Color, Modifier, Style};

// --- Catppuccin Mocha palette ---
pub const BASE: Color = Color::Rgb(30, 30, 46);
pub const MANTLE: Color = Color::Rgb(24, 24, 37);
pub const CRUST: Color = Color::Rgb(17, 17, 27);
pub const SURFACE0: Color = Color::Rgb(49, 50, 68);
pub const SURFACE1: Color = Color::Rgb(69, 71, 90);
pub const TEXT: Color = Color::Rgb(205, 214, 244);
pub const SUBTEXT0: Color = Color::Rgb(166, 173, 200);
pub const SUBTEXT1: Color = Color::Rgb(186, 194, 222);
pub const BLUE: Color = Color::Rgb(137, 180, 250);
pub const GREEN: Color = Color::Rgb(166, 227, 161);
pub const RED: Color = Color::Rgb(243, 139, 168);
pub const YELLOW: Color = Color::Rgb(249, 226, 175);
pub const PURPLE: Color = Color::Rgb(203, 166, 247);
pub const TEAL: Color = Color::Rgb(148, 226, 213);
pub const PEACH: Color = Color::Rgb(250, 179, 135);

// --- Reusable styles ---

pub fn title_style() -> Style {
    Style::default().fg(BLUE).add_modifier(Modifier::BOLD)
}

pub fn text_style() -> Style {
    Style::default().fg(TEXT)
}

pub fn subtext_style() -> Style {
    Style::default().fg(SUBTEXT0)
}

pub fn highlight_style() -> Style {
    Style::default().fg(BASE).bg(BLUE)
}

pub fn pass_style() -> Style {
    Style::default().fg(GREEN)
}

pub fn fail_style() -> Style {
    Style::default().fg(RED)
}

pub fn warn_style() -> Style {
    Style::default().fg(YELLOW)
}

pub fn accent_style() -> Style {
    Style::default().fg(PURPLE)
}

pub fn keybind_style() -> Style {
    Style::default().fg(TEAL)
}

pub fn dimmed_style() -> Style {
    Style::default().fg(SURFACE1)
}
