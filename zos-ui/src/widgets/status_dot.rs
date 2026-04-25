//! Status dot — small filled circle for state indication.
//!
//! Place next to text labels in lists, panels, switcher rows. Default
//! diameter is 8px which lines up with `font_size::SM` cap height.
//!
//! ```ignore
//! use zos_ui::widgets::StatusDot;
//! use zos_ui::theme;
//!
//! let _: iced::Element<'_, ()> = StatusDot::new(theme::GREEN).size(10.0).into();
//! ```

use iced::{
    Background, Border, Color, Element, Length,
    border::Radius,
    widget::{Space, container},
};

/// Solid circle of the given color.
pub struct StatusDot {
    color: Color,
    size: f32,
}

impl StatusDot {
    /// New 8px dot of the given color.
    pub fn new(color: Color) -> Self {
        Self { color, size: 8.0 }
    }

    /// Override the diameter (px).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl<'a, Msg: 'a> From<StatusDot> for Element<'a, Msg> {
    fn from(d: StatusDot) -> Self {
        let color = d.color;
        let size = d.size;

        container(
            Space::new()
                .width(Length::Fixed(size))
                .height(Length::Fixed(size)),
        )
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color,
                width: 0.0,
                radius: Radius::from(size / 2.0),
            },
            ..Default::default()
        })
        .into()
    }
}
