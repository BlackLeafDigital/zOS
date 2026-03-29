// === pages/audio/mod.rs — Audio management page (VoiceMeeter-inspired 3-column layout) ===

mod buses;
mod input;
mod output;
mod routing;

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::services::pipewire::{BusConfig, DeviceType};

/// Build the audio page widget with a 3-column layout:
/// Left = Inputs/Sources, Center = Virtual Buses, Right = Outputs
pub fn build() -> gtk::Box {
    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    // --- 3-column mixer layout ---
    let columns = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(8)
        .margin_start(16)
        .margin_end(16)
        .homogeneous(true)
        .vexpand(true)
        .build();

    // Left column: Inputs + App Streams
    let left_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();
    let left_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    left_col.append(&input::build());
    left_col.append(&routing::build());
    left_scroll.set_child(Some(&left_col));
    columns.append(&left_scroll);

    // Center column: Virtual Buses
    let center_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();
    let center_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    center_col.append(&buses::build());
    center_scroll.set_child(Some(&center_col));
    columns.append(&center_scroll);

    // Right column: Outputs
    let right_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();
    let right_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    right_col.append(&output::build());
    right_scroll.set_child(Some(&right_col));
    columns.append(&right_scroll);

    wrapper.append(&columns);

    // --- Tools row at bottom ---
    wrapper.append(&build_tools_section());

    // Wrap in a scroll for the overall page
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&wrapper)
        .build();

    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    outer.append(&scrolled);
    outer
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

// ---------------------------------------------------------------------------
// Tools section
// ---------------------------------------------------------------------------

fn build_tools_section() -> gtk::Box {
    use relm4::adw;
    use relm4::adw::prelude::*;

    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(16)
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("Tools")
        .build();

    let graph_row = adw::ActionRow::builder()
        .title("Open Audio Graph")
        .subtitle("Advanced audio routing with qpwgraph")
        .build();
    let graph_icon = gtk::Image::from_icon_name("preferences-system-symbolic");
    graph_icon.set_valign(gtk::Align::Center);
    graph_row.add_prefix(&graph_icon);
    let graph_btn = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();
    graph_btn.connect_clicked(|_| {
        launch_app("qpwgraph");
    });
    graph_row.add_suffix(&graph_btn);
    graph_row.set_activatable_widget(Some(&graph_btn));
    group.add(&graph_row);

    let effects_row = adw::ActionRow::builder()
        .title("Open EasyEffects")
        .subtitle("Audio effects and equalizer")
        .build();
    let effects_icon = gtk::Image::from_icon_name("multimedia-equalizer-symbolic");
    effects_icon.set_valign(gtk::Align::Center);
    effects_row.add_prefix(&effects_icon);
    let effects_btn = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();
    effects_btn.connect_clicked(|_| {
        launch_app("easyeffects");
    });
    effects_row.add_suffix(&effects_btn);
    effects_row.set_activatable_widget(Some(&effects_btn));
    group.add(&effects_row);

    container.append(&group);
    container
}
