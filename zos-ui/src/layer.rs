//! Layer-shell helpers for zOS apps that anchor to screen edges
//! (panel/top bar, dock, popups, switcher, notification daemon).
//!
//! Gated behind the default `layer-shell` cargo feature so consumers
//! that build only "regular" iced windows (e.g. zos-settings dialogs)
//! can opt out and skip the wayland-protocols dep tree.
//!
//! These wrappers produce `LayerShellSettings` values that callers feed
//! into `iced_layershell::application(...).settings(Settings { layer_settings,
//! ..Default::default() })`. They intentionally do NOT own the iced
//! Application lifecycle — apps still call `iced_layershell::application`
//! or `iced_layershell::daemon` themselves so they can attach their own
//! `update` / `view` / `subscription` closures.
//!
//! ```ignore
//! use zos_ui::layer::layer_shell;
//! use iced_layershell::settings::Settings;
//!
//! let settings = Settings {
//!     layer_settings: layer_shell::top_bar(36),
//!     ..Default::default()
//! };
//! ```

#[cfg(feature = "layer-shell")]
pub use self::layer_shell::*;

/// Re-exports of the iced_layershell types needed to construct
/// `LayerShellSettings` directly. Kept under a sub-module so callers
/// don't have to add iced_layershell as a direct dependency just to
/// reference the types we hand back.
#[cfg(feature = "layer-shell")]
pub mod layer_shell {
    pub use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    pub use iced_layershell::settings::{LayerShellSettings, StartMode};

    /// Settings for a top bar (panel) — anchored Top/Left/Right with the
    /// given `height` reserved as exclusive zone, so other windows tile
    /// below it. No keyboard input by default.
    ///
    /// ```ignore
    /// use zos_ui::layer::layer_shell;
    /// let settings = layer_shell::top_bar(36);
    /// assert_eq!(settings.exclusive_zone, 36);
    /// ```
    pub fn top_bar(height: u32) -> LayerShellSettings {
        LayerShellSettings {
            anchor: Anchor::Top | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            exclusive_zone: height as i32,
            size: Some((0, height)),
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::Active,
            events_transparent: false,
        }
    }

    /// Settings for a bottom dock — anchored Bottom/Left/Right with the
    /// given `height` reserved as exclusive zone.
    pub fn bottom_dock(height: u32) -> LayerShellSettings {
        LayerShellSettings {
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            exclusive_zone: height as i32,
            size: Some((0, height)),
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::None,
            start_mode: StartMode::Active,
            events_transparent: false,
        }
    }

    /// Settings for a centered popup (dialog overlay) — no anchor edges,
    /// fixed `width` x `height`, on the Overlay layer with on-demand
    /// keyboard focus so dialogs can capture text input when clicked.
    pub fn centered_popup(width: u32, height: u32) -> LayerShellSettings {
        LayerShellSettings {
            anchor: Anchor::empty(),
            layer: Layer::Overlay,
            exclusive_zone: 0,
            size: Some((width, height)),
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            start_mode: StartMode::Active,
            events_transparent: false,
        }
    }
}
