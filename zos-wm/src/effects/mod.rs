//! Visual effects for zos-wm windows: rounded corners, shadows, blur,
//! opacity. Each effect is a wrapper around smithay's custom-shader
//! infrastructure.
//!
//! See `docs/research/phase-4-smithay-effects.md` for design.
//!
//! Effects depend on the GLES renderer, so the module is gated behind any
//! feature that pulls in `smithay/renderer_gl` (udev, winit, x11). Without
//! one of those features the GLES types this module references aren't in
//! scope and a build would fail with confusing errors.

#[cfg(any(feature = "udev", feature = "winit", feature = "x11"))]
pub mod rounded;

#[cfg(any(feature = "udev", feature = "winit", feature = "x11"))]
pub mod shadow;
