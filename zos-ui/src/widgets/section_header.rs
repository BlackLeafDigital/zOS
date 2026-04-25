//! Section header — a bold title with an optional subtitle.
//!
//! Used at the top of settings groups, sidebar buckets, and dialog
//! sections. Pure visual primitive — no interaction, no state.
//!
//! ```ignore
//! use zos_ui::widgets::SectionHeader;
//!
//! let _: iced::Element<'_, ()> = SectionHeader::new("Display")
//!     .subtitle("Resolution, refresh rate, scaling")
//!     .into();
//! ```

use std::marker::PhantomData;

use iced::{
    Element,
    widget::{column, text},
};

use crate::theme;

/// Title (`font_size::XL`, TEXT) optionally followed by a small
/// subtitle (`font_size::SM`, SUBTEXT0).
pub struct SectionHeader<'a, Msg> {
    title: String,
    subtitle: Option<String>,
    _phantom: PhantomData<&'a Msg>,
}

impl<'a, Msg: 'a> SectionHeader<'a, Msg> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            _phantom: PhantomData,
        }
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = Some(s.into());
        self
    }
}

impl<'a, Msg: 'a> From<SectionHeader<'a, Msg>> for Element<'a, Msg> {
    fn from(s: SectionHeader<'a, Msg>) -> Self {
        let mut col = column![
            text(s.title)
                .size(theme::font_size::XL)
                .color(theme::TEXT)
        ]
        .spacing(theme::space::X1);

        if let Some(sub) = s.subtitle {
            col = col.push(
                text(sub)
                    .size(theme::font_size::SM)
                    .color(theme::SUBTEXT0),
            );
        }

        col.into()
    }
}
