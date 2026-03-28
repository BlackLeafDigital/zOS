// === pages/audio/mod.rs — Audio management page (VoiceMeeter-inspired) ===

mod advanced;
mod buses;
mod input;
mod output;
mod routing;

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::services::pipewire::{BusConfig, DeviceType};

/// Build the audio page widget.
pub fn build() -> gtk::Box {
    let page = super::page_content();

    page.append(&output::build());
    page.append(&input::build());
    page.append(&buses::build());
    page.append(&routing::build());
    page.append(&advanced::build());

    super::page_wrapper(&page)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn icon_for_device_type(dt: &DeviceType) -> &'static str {
    match dt {
        DeviceType::Sink => "audio-speakers-symbolic",
        DeviceType::Source => "audio-input-microphone-symbolic",
    }
}

/// Map internal sink names to user-friendly labels using bus configs.
/// Falls back to pattern matching if no config is found.
pub(crate) fn friendly_bus_name(name: &str, bus_configs: &[BusConfig]) -> String {
    if let Some(cfg) = bus_configs.iter().find(|c| c.name == name) {
        return cfg.description.clone();
    }
    match name {
        n if n.contains("zos-main") => "Main Output".into(),
        n if n.contains("zos-music") => "Music".into(),
        n if n.contains("zos-chat") => "Chat / Voice".into(),
        _ => name.to_string(),
    }
}

/// Launch an application in the background.
pub(crate) fn launch_app(command: &str) {
    match std::process::Command::new(command).spawn() {
        Ok(_) => {
            tracing::info!("Launched {}", command);
        }
        Err(e) => {
            tracing::error!("Failed to launch {}: {}", command, e);
        }
    }
}
