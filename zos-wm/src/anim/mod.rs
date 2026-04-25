//! Animation primitives for zos-wm.
//!
//! Provides:
//! - [`BezierCurve`]: cubic bezier with 255-point baking + binary-search eval.
//! - [`Animatable`]: trait for types that can be linearly interpolated.
//! - [`AnimatedValue<T>`]: time-driven animated value with `animate_to` /
//!   `warp_to` / `tick` API.
//! - [`AnimationManager`]: registry of named curves + per-property config.
//!
//! See `docs/research/phase-4-hyprland-animations.md` for the design rationale.

pub mod animatable;
pub mod bezier;
pub mod manager;
pub mod value;

pub use animatable::Animatable;
pub use bezier::BezierCurve;
pub use manager::{AnimationManager, AnimationProperty, AnimationStyle};
pub use value::AnimatedValue;
