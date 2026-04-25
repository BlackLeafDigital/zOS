//! Canonical re-exports for zOS UI code.
//!
//! `use zos_ui::prelude::*;` brings in everything needed for typical app
//! code — iced types, the theme, signals, components, and the built-in
//! widget set.

pub use crate::config::{
    AnimationOverrides, BezierCurveOverride, PropertyOverride, ThemeOverrides, load_animations,
    load_theme_overrides,
};
pub use crate::signal::{
    Effect, Interval, Memo, Signal, Timeout, tick_timers, use_interval, use_timeout,
};
pub use crate::theme::{self, Tokens, zos_theme};
pub use crate::widgets::{Card, Pill, SectionHeader, StatusDot};
pub use crate::{Component, View};

// Layer-shell helpers (TopBar / BottomDock / CenteredPopup builders).
// Behind the default `layer-shell` feature; the prelude transparently
// re-exports them when enabled.
#[cfg(feature = "layer-shell")]
pub use crate::layer::layer_shell;

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
