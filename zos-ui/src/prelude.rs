//! Canonical re-exports for zOS UI code.
//!
//! `use zos_ui::prelude::*;` brings in everything needed for typical app
//! code — iced types, the theme, and (later) signals + components.

pub use crate::signal::{Effect, Memo, Signal};
pub use crate::theme::{self, Tokens, zos_theme};
pub use crate::{Component, View};

// Proc-macros. Re-exported here so `use zos_ui::prelude::*;` is a one-stop
// shop for app authors.
pub use zos_ui_macros::{component, panel_module, taskbar_icon};

// Re-export common iced types so users only need one prelude import.
pub use iced::{
    Element, Length, Padding, Renderer,
    Theme as IcedTheme,
    alignment::{Horizontal, Vertical},
    widget::{
        Button, Column, Container, Row, Scrollable, Space, Text, button, column, container, row,
        scrollable, text,
    },
};
