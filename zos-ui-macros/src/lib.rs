//! Proc-macros for the zos-ui framework.
//!
//! - `#[component]` — turns a function into a Component struct + impl.
//! - `#[panel_module]` — `#[component]` plus a stub PanelModule impl (TODO).
//! - `#[taskbar_icon]` — `#[component]` plus a stub TaskbarIconWidget impl (TODO).
//!
//! See `zos-ui` for the runtime traits these expand to.

use proc_macro::TokenStream;

mod component;

#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(attr.into(), item.into(), component::Variant::Plain).into()
}

#[proc_macro_attribute]
pub fn panel_module(attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(attr.into(), item.into(), component::Variant::PanelModule).into()
}

#[proc_macro_attribute]
pub fn taskbar_icon(attr: TokenStream, item: TokenStream) -> TokenStream {
    component::expand(attr.into(), item.into(), component::Variant::TaskbarIcon).into()
}
