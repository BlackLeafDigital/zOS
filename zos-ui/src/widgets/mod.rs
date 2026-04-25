//! Built-in zOS widgets — Catppuccin-themed primitives composing iced.
//!
//! Each module here is a small, focused widget. None of these are
//! reactive yet — they consume regular iced `Element`s and produce
//! regular iced `Element`s, just with the zOS theme baked in.
//!
//! When the reactive `signal` integration lands, these widgets will gain
//! `Signal<T>`-aware constructors, but the existing API will continue to
//! work as-is for static data.

pub mod card;
pub mod pill;
pub mod section_header;
pub mod status_dot;

pub use card::Card;
pub use pill::Pill;
pub use section_header::SectionHeader;
pub use status_dot::StatusDot;
