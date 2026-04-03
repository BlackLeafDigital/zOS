// === zos-dock — macOS-style dock for zOS (Hyprland + Wayland layer shell) ===

mod config;
mod dock;
mod hypr;
mod hypr_events;
mod icons;

use config::{DockConfig, DockPosition};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};

fn main() -> iced_layershell::Result {
    let cfg = DockConfig::load();
    let (anchor, margin, size) = match cfg.position {
        DockPosition::Bottom => (
            Anchor::Bottom | Anchor::Left | Anchor::Right,
            (0, 0, 8, 0),
            (0, 100),
        ),
        DockPosition::Top => (
            Anchor::Top | Anchor::Left | Anchor::Right,
            (8, 0, 0, 0),
            (0, 100),
        ),
        DockPosition::Left => (
            Anchor::Left | Anchor::Top | Anchor::Bottom,
            (0, 0, 0, 8),
            (100, 0),
        ),
        DockPosition::Right => (
            Anchor::Right | Anchor::Top | Anchor::Bottom,
            (0, 8, 0, 0),
            (100, 0),
        ),
    };

    iced_layershell::daemon(dock::boot, dock::namespace, dock::update, dock::view)
        .subscription(dock::subscription)
        .style(dock::style)
        .settings(Settings {
            layer_settings: LayerShellSettings {
                anchor,
                layer: Layer::Top,
                exclusive_zone: 0,
                size: Some(size),
                margin,
                keyboard_interactivity: KeyboardInteractivity::None,
                events_transparent: false,
                start_mode: StartMode::AllScreens,
            },
            antialiasing: true,
            ..Default::default()
        })
        .run()
}
