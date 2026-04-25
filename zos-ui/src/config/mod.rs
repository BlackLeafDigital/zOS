//! User config files for zOS UI.
//!
//! - `~/.config/zos/animations.toml` — animation curves + speeds
//! - `~/.config/zos/theme.toml` — palette overrides, font sizes, spacing
//!
//! Loaders are infallible from the caller's perspective — missing files
//! and parse errors degrade to "no overrides" + a `tracing::warn!`. This
//! is deliberate: a typo in the user's TOML must never block the desktop
//! from coming up.
//!
//! ## Composition model
//!
//! These types are *overrides*, not full theme/animation values. The
//! intended consumer pattern is:
//!
//! ```ignore
//! let overrides = zos_ui::config::load_animations();
//! let manager = AnimationManager::with_overrides(overrides); // in zos-wm
//! ```
//!
//! Every field is optional so callers can `.unwrap_or_default()` or
//! merge field-by-field.

pub mod animations;
pub mod theme_overrides;

pub use animations::{
    AnimationOverrides, BezierCurveOverride, PropertyOverride, load_animations,
    load_animations_from,
};
pub use theme_overrides::{ThemeOverrides, load_theme_overrides, load_theme_overrides_from};

use std::path::PathBuf;

/// Default config directory: `$XDG_CONFIG_HOME/zos` or `~/.config/zos`.
///
/// Falls back to `/tmp/.config/zos` if neither `XDG_CONFIG_HOME` nor
/// `HOME` is set — that path will simply not exist, and the loaders will
/// treat it as "no overrides", which is the desired behaviour.
pub fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("zos");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/zos")
}
