//! zOS shared UI framework.
//!
//! All zOS apps (panel, dock, settings, switcher, notification daemon)
//! consume this crate. Goals:
//!
//! - Ergonomic component API — easy to write apps without ceremony.
//! - Catppuccin Mocha theme baked in, with hooks for runtime theme swap.
//! - Layer-shell-friendly primitives.
//! - Reactive signals so UI updates are surgical, not full-tree rebuilds.
//!
//! ## Architecture
//!
//! Built on top of [`iced`]; we don't reimplement layout, text, or rendering.
//! What `zos-ui` adds:
//! - Theme module — Catppuccin Mocha palette + typography + spacing tokens.
//! - Reactive signal/effect primitives (separate task — `signal` module).
//! - `Component` trait — composition-friendly per-component API.
//! - `#[component]` proc-macro — turns a function into a Component struct.
//! - Layer-shell wrappers — `TopBar`, `BottomDock`, etc. (separate task).
//! - Built-in widgets — `Card`, `SectionHeader`, `Pill`, etc. (separate task).
//!
//! See [`prelude`] for the canonical import set.
//!
//! ```no_run
//! use zos_ui::prelude::*;
//!
//! // example app code goes here once the framework is built up.
//! let _theme = zos_theme();
//! let _tokens = Tokens;
//! ```

// Make `::zos_ui::...` paths inside this crate's tests resolve to ourselves.
// The `#[component]` macro emits absolute `::zos_ui::Component` paths so it
// works for downstream crates; this alias makes the smoke tests below compile
// inside our own crate.
#[cfg(test)]
extern crate self as zos_ui;

pub mod config;
pub mod layer;
pub mod prelude;
pub mod signal;
pub mod theme;
pub mod widgets;

// Re-export the proc-macros under the `zos_ui` namespace so app code only
// needs `use zos_ui::prelude::*;` — never `use zos_ui_macros::*`.
pub use zos_ui_macros::{component, panel_module, taskbar_icon};

/// Anything that can become an iced `Element`.
///
/// Used as the return type of [`Component::view`]. Blanket-impl'd for every
/// `T: Into<iced::Element<'static, ()>>` so users can return widgets directly
/// from a `#[component]` body without writing `.into()`.
///
/// The message type is fixed to `()` for v1; a later task will generalize
/// `Component` over a `Message` type so apps can wire interactions.
pub trait View {
    fn into_element(self) -> ::iced::Element<'static, ()>;
}

impl<T> View for T
where
    T: Into<::iced::Element<'static, ()>>,
{
    fn into_element(self) -> ::iced::Element<'static, ()> {
        self.into()
    }
}

/// A composable UI component.
///
/// `#[component]` generates an impl of this trait for any function it's
/// applied to. The function body becomes the body of `view`, with arguments
/// destructured back to local names so the body reads identically to the
/// original function.
pub trait Component {
    fn view(self) -> impl View;
}

#[cfg(test)]
mod component_smoke {
    use super::*;

    #[component]
    fn HelloWorld(name: String) -> impl View {
        ::iced::widget::text(format!("hello {}", name))
    }

    #[test]
    fn macro_expands_and_compiles() {
        let h = HelloWorld::new("zach".into());
        // Builder method exists.
        let h = h.name("world".into());
        // The view trait is implemented and produces something convertible
        // to an iced Element.
        let _: ::iced::Element<'static, ()> = h.view().into_element();
    }

    #[component]
    fn NoArgs() -> impl View {
        ::iced::widget::text("static")
    }

    #[test]
    fn macro_handles_zero_args() {
        let n = NoArgs::new();
        let _: ::iced::Element<'static, ()> = n.view().into_element();
    }
}
