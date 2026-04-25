//! Card — a Catppuccin-themed surface with rounded corners, padding,
//! and an optional title row above the content.
//!
//! ```ignore
//! use zos_ui::prelude::*;
//! use zos_ui::widgets::Card;
//! use iced::widget::text;
//!
//! let _: iced::Element<'_, ()> = Card::new()
//!     .title("Audio")
//!     .push(text("Output: Built-in speakers"))
//!     .push(text("Input: Microphone"))
//!     .into();
//! ```
//!
//! The card paints SURFACE0 with a 1px SURFACE1 border and a MD radius
//! (the same surface treatment used everywhere in zOS settings/panel
//! UIs). Padding defaults to `space::X4` but can be overridden.

use iced::{
    Background, Border, Element, Length, Padding,
    border::Radius,
    widget::{Column, column, container, text},
};

use crate::theme;

/// Vertical surface with optional title and a vec of children.
///
/// Use the builder methods (`title`, `push`, `padding`) to compose,
/// then convert via `.into()` (or just place it in another widget that
/// expects an `Element`).
#[derive(Default)]
pub struct Card<'a, Msg> {
    title: Option<String>,
    content: Vec<Element<'a, Msg>>,
    padding: Padding,
}

impl<'a, Msg: 'a> Card<'a, Msg> {
    /// Construct an empty card with default padding.
    pub fn new() -> Self {
        Self {
            title: None,
            content: Vec::new(),
            padding: Padding::from(theme::space::X4),
        }
    }

    /// Set the heading rendered above the content. Pass `None` to clear.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Append a child element. Children stack vertically with `space::X3`
    /// gap between rows.
    pub fn push(mut self, child: impl Into<Element<'a, Msg>>) -> Self {
        self.content.push(child.into());
        self
    }

    /// Override the inner padding (default = `space::X4`).
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
}

impl<'a, Msg: 'a> From<Card<'a, Msg>> for Element<'a, Msg> {
    fn from(card: Card<'a, Msg>) -> Self {
        let mut col: Column<'a, Msg> = column![].spacing(theme::space::X3);

        if let Some(title) = card.title {
            col = col.push(
                text(title)
                    .size(theme::font_size::LG)
                    .color(theme::TEXT),
            );
        }

        for c in card.content {
            col = col.push(c);
        }

        container(col)
            .padding(card.padding)
            .style(|_theme: &iced::Theme| iced::widget::container::Style {
                background: Some(Background::Color(theme::SURFACE0)),
                border: Border {
                    color: theme::SURFACE1,
                    width: 1.0,
                    radius: Radius::from(theme::radius::MD),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    }
}
