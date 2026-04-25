//! Pill — small rounded color tag with a label.
//!
//! ```ignore
//! use zos_ui::widgets::Pill;
//! use zos_ui::theme;
//!
//! let _: iced::Element<'_, ()> = Pill::new("Active").color(theme::GREEN).into();
//! ```
//!
//! The pill auto-picks a contrasting text color: dark Catppuccin colors
//! get TEXT, lighter accent colors get CRUST. The heuristic is "any
//! channel above 0.6" — good enough for the Catppuccin palette without
//! pulling in a full luminance computation.

use iced::{
    Background, Border, Color, Element, Padding,
    border::Radius,
    widget::{container, text},
};

use crate::theme;

/// Rounded color tag with a short label.
pub struct Pill {
    label: String,
    color: Color,
    text_color: Color,
}

impl Pill {
    /// Create a pill with default (`SURFACE1`) background.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            color: theme::SURFACE1,
            text_color: theme::TEXT,
        }
    }

    /// Set the background color. Auto-flips text color so light Catppuccin
    /// accents (GREEN, YELLOW, PEACH...) stay readable.
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        if c.r > 0.6 || c.g > 0.6 || c.b > 0.6 {
            self.text_color = theme::CRUST;
        } else {
            self.text_color = theme::TEXT;
        }
        self
    }
}

impl<'a, Msg: 'a> From<Pill> for Element<'a, Msg> {
    fn from(p: Pill) -> Self {
        let label = p.label;
        let bg = p.color;
        let fg = p.text_color;

        container(
            text(label)
                .size(theme::font_size::XS)
                .color(fg),
        )
        .padding(Padding::from([4.0_f32, 10.0]))
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: bg,
                width: 0.0,
                radius: Radius::from(theme::radius::FULL),
            },
            ..Default::default()
        })
        .into()
    }
}
