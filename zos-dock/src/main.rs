// === zos-dock — macOS-style dock for zOS (Hyprland + Wayland layer shell) ===

mod config;
mod dock;
mod hypr;
mod icons;

use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings};

fn main() -> iced_layershell::Result {
    iced_layershell::application(dock::boot, dock::namespace, dock::update, dock::view)
        .subscription(dock::subscription)
        .style(dock::style)
        .settings(Settings {
            layer_settings: LayerShellSettings {
                anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
                layer: Layer::Top,
                exclusive_zone: 0,
                size: Some((0, 68)),
                margin: (0, 0, 8, 0),
                keyboard_interactivity: KeyboardInteractivity::None,
                events_transparent: false,
                ..Default::default()
            },
            antialiasing: true,
            ..Default::default()
        })
        .run()
}
