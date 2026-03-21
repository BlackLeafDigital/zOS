// === pages/audio.rs — Audio management page (VoiceMeeter-inspired) ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, DeviceType};

/// Build the audio page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    page.append(&build_output_section());
    page.append(&build_input_section());
    page.append(&build_virtual_buses_section());
    page.append(&build_routing_section());
    page.append(&build_advanced_section());

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&page)
        .build();

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    wrapper.append(&scrolled);
    wrapper
}

// ---------------------------------------------------------------------------
// Output section
// ---------------------------------------------------------------------------

fn icon_for_device_type(dt: &DeviceType) -> &'static str {
    match dt {
        DeviceType::Sink => "audio-speakers-symbolic",
        DeviceType::Source => "audio-input-microphone-symbolic",
    }
}

fn build_output_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Output")
        .build();

    let sinks = pipewire::list_sinks();

    // --- Device selector ---
    let model = gtk::StringList::new(&[]);
    let mut default_idx: u32 = 0;
    for (i, sink) in sinks.iter().enumerate() {
        let label = format!("{} ({})", sink.name, if sink.device_type == DeviceType::Sink { "sink" } else { "source" });
        model.append(&label);
        if sink.is_default {
            default_idx = i as u32;
        }
    }

    // Use device_type icon for the combo row
    let device_icon = sinks.first().map(|s| icon_for_device_type(&s.device_type)).unwrap_or("audio-speakers-symbolic");
    let combo = adw::ComboRow::builder()
        .title("Device")
        .icon_name(device_icon)
        .model(&model)
        .selected(default_idx)
        .build();

    let sinks_for_combo = sinks.clone();
    combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(sink) = sinks_for_combo.get(idx) {
            pipewire::set_default(sink.id);
        }
    });
    group.add(&combo);

    // --- Volume slider ---
    // Use get_default_volume() for a live reading of the default sink volume,
    // falling back to the parsed wpctl status value.
    let default_sink = sinks.iter().find(|s| s.is_default);
    let live_vol = pipewire::get_default_volume();
    let current_vol = live_vol.unwrap_or_else(|| default_sink.and_then(|s| s.volume).unwrap_or(1.0));
    let default_id = default_sink.map(|s| s.id).unwrap_or(0);

    let volume_row = adw::ActionRow::builder()
        .title("Volume")
        .build();

    let vol_label = gtk::Label::builder()
        .label(&format!("{}%", (current_vol * 100.0).round() as i32))
        .valign(gtk::Align::Center)
        .width_chars(5)
        .build();

    let scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .width_request(200)
        .build();
    scale.set_range(0.0, 1.5);
    scale.set_increments(0.01, 0.05);
    scale.set_value(current_vol as f64);

    let vol_label_clone = vol_label.clone();
    scale.connect_value_changed(move |s| {
        let val = s.value() as f32;
        if default_id > 0 {
            pipewire::set_volume(default_id, val);
        }
        vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
    });

    volume_row.add_suffix(&scale);
    volume_row.add_suffix(&vol_label);
    group.add(&volume_row);

    // --- Mute toggle ---
    let mute_row = adw::ActionRow::builder()
        .title("Mute")
        .build();

    // Use is_default_muted() for a live reading of the default sink mute state.
    let muted = pipewire::is_default_muted();
    let mute_btn = gtk::ToggleButton::builder()
        .icon_name(if muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        })
        .valign(gtk::Align::Center)
        .active(muted)
        .build();

    mute_btn.connect_toggled(move |btn| {
        if default_id > 0 {
            pipewire::toggle_mute(default_id);
        }
        let icon = if btn.is_active() {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        btn.set_icon_name(icon);
    });

    mute_row.add_suffix(&mute_btn);
    group.add(&mute_row);

    group
}

// ---------------------------------------------------------------------------
// Input section
// ---------------------------------------------------------------------------

fn build_input_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Input")
        .build();

    let sources = pipewire::list_sources();

    // --- Device selector ---
    let model = gtk::StringList::new(&[]);
    let mut default_idx: u32 = 0;
    for (i, source) in sources.iter().enumerate() {
        let label = format!("{} ({})", source.name, if source.device_type == DeviceType::Source { "source" } else { "sink" });
        model.append(&label);
        if source.is_default {
            default_idx = i as u32;
        }
    }

    let device_icon = sources.first().map(|s| icon_for_device_type(&s.device_type)).unwrap_or("audio-input-microphone-symbolic");
    let combo = adw::ComboRow::builder()
        .title("Device")
        .icon_name(device_icon)
        .model(&model)
        .selected(default_idx)
        .build();

    let sources_for_combo = sources.clone();
    combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(source) = sources_for_combo.get(idx) {
            pipewire::set_default(source.id);
        }
    });
    group.add(&combo);

    // --- Volume slider ---
    let default_source = sources.iter().find(|s| s.is_default);
    let current_vol = default_source.and_then(|s| s.volume).unwrap_or(1.0);
    let default_id = default_source.map(|s| s.id).unwrap_or(0);

    let volume_row = adw::ActionRow::builder()
        .title("Volume")
        .build();

    let vol_label = gtk::Label::builder()
        .label(&format!("{}%", (current_vol * 100.0).round() as i32))
        .valign(gtk::Align::Center)
        .width_chars(5)
        .build();

    let scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .width_request(200)
        .build();
    scale.set_range(0.0, 1.5);
    scale.set_increments(0.01, 0.05);
    scale.set_value(current_vol as f64);

    let vol_label_clone = vol_label.clone();
    scale.connect_value_changed(move |s| {
        let val = s.value() as f32;
        if default_id > 0 {
            pipewire::set_volume(default_id, val);
        }
        vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
    });

    volume_row.add_suffix(&scale);
    volume_row.add_suffix(&vol_label);
    group.add(&volume_row);

    // --- Mute toggle ---
    let mute_row = adw::ActionRow::builder()
        .title("Mute")
        .build();

    let muted = default_source.map(|s| s.muted).unwrap_or(false);
    let mute_btn = gtk::ToggleButton::builder()
        .icon_name(if muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-input-microphone-symbolic"
        })
        .valign(gtk::Align::Center)
        .active(muted)
        .build();

    mute_btn.connect_toggled(move |btn| {
        if default_id > 0 {
            pipewire::toggle_mute(default_id);
        }
        let icon = if btn.is_active() {
            "audio-volume-muted-symbolic"
        } else {
            "audio-input-microphone-symbolic"
        };
        btn.set_icon_name(icon);
    });

    mute_row.add_suffix(&mute_btn);
    group.add(&mute_row);

    group
}

// ---------------------------------------------------------------------------
// Virtual buses section
// ---------------------------------------------------------------------------

fn build_virtual_buses_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Audio Buses")
        .description("Virtual audio buses for routing")
        .build();

    let sinks = pipewire::list_sinks();
    let virtual_sinks: Vec<_> = sinks
        .into_iter()
        .filter(|s| s.name.starts_with("zos-") || s.name.contains("zos-"))
        .collect();

    if virtual_sinks.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No virtual buses detected")
            .subtitle("zOS audio buses will appear here when PipeWire is running")
            .build();
        group.add(&empty_row);
        return group;
    }

    for sink in &virtual_sinks {
        let display_name = friendly_bus_name(&sink.name);
        let current_vol = sink.volume.unwrap_or(1.0);
        let sink_id = sink.id;

        let row = adw::ActionRow::builder()
            .title(display_name)
            .subtitle(&sink.name)
            .build();

        let vol_label = gtk::Label::builder()
            .label(&format!("{}%", (current_vol * 100.0).round() as i32))
            .valign(gtk::Align::Center)
            .width_chars(5)
            .build();

        let scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .width_request(200)
            .build();
        scale.set_range(0.0, 1.5);
        scale.set_increments(0.01, 0.05);
        scale.set_value(current_vol as f64);

        let vol_label_clone = vol_label.clone();
        scale.connect_value_changed(move |s| {
            let val = s.value() as f32;
            pipewire::set_volume(sink_id, val);
            vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
        });

        row.add_suffix(&scale);
        row.add_suffix(&vol_label);
        group.add(&row);
    }

    group
}

// ---------------------------------------------------------------------------
// Routing section
// ---------------------------------------------------------------------------

fn build_routing_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Routing")
        .description("Active PipeWire links between ports")
        .build();

    let links = pipewire::list_links();

    if links.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No active links")
            .subtitle("PipeWire port links will appear here")
            .build();
        group.add(&empty_row);
        return group;
    }

    for (output_port, input_port) in &links {
        let row = adw::ActionRow::builder()
            .title(output_port.as_str())
            .subtitle(input_port.as_str())
            .build();

        let out = output_port.clone();
        let inp = input_port.clone();
        let disconnect_btn = gtk::Button::builder()
            .icon_name("edit-delete-symbolic")
            .tooltip_text("Disconnect link")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();

        disconnect_btn.connect_clicked(move |btn| {
            if pipewire::remove_link(&out, &inp) {
                tracing::info!("Removed link: {} -> {}", out, inp);
                // Hide the row after disconnecting
                if let Some(parent) = btn.parent() {
                    parent.set_visible(false);
                }
            } else {
                tracing::error!("Failed to remove link: {} -> {}", out, inp);
            }
        });

        row.add_suffix(&disconnect_btn);
        group.add(&row);
    }

    // --- Quick-link creation with dropdown selectors ---
    let output_ports = pipewire::list_output_ports();
    let input_ports = pipewire::list_input_ports();

    let output_model = gtk::StringList::new(
        &output_ports.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    );
    let output_dropdown = gtk::DropDown::builder()
        .model(&output_model)
        .valign(gtk::Align::Center)
        .build();

    let input_model = gtk::StringList::new(
        &input_ports.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    );
    let input_dropdown = gtk::DropDown::builder()
        .model(&input_model)
        .valign(gtk::Align::Center)
        .build();

    let output_row = adw::ActionRow::builder()
        .title("Output Port")
        .subtitle("Source of audio data")
        .build();
    output_row.add_suffix(&output_dropdown);
    group.add(&output_row);

    let input_row = adw::ActionRow::builder()
        .title("Input Port")
        .subtitle("Destination for audio data")
        .build();
    input_row.add_suffix(&input_dropdown);
    group.add(&input_row);

    let connect_btn = gtk::Button::builder()
        .label("Connect")
        .tooltip_text("Create link between selected ports")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();

    let out_dd = output_dropdown.clone();
    let inp_dd = input_dropdown.clone();
    let out_ports = output_ports.clone();
    let inp_ports = input_ports.clone();
    connect_btn.connect_clicked(move |_| {
        let out_idx = out_dd.selected() as usize;
        let inp_idx = inp_dd.selected() as usize;
        if let (Some(out_port), Some(inp_port)) = (out_ports.get(out_idx), inp_ports.get(inp_idx)) {
            if pipewire::create_link(out_port, inp_port) {
                tracing::info!("Created link: {} -> {}", out_port, inp_port);
            } else {
                tracing::error!("Failed to create link: {} -> {}", out_port, inp_port);
            }
        }
    });

    let connect_row = adw::ActionRow::builder()
        .title("Create Link")
        .subtitle("Connect the selected output to the selected input")
        .build();
    connect_row.add_suffix(&connect_btn);
    connect_row.set_activatable_widget(Some(&connect_btn));
    group.add(&connect_row);

    group
}

/// Map internal sink names to user-friendly labels.
fn friendly_bus_name(name: &str) -> &str {
    match name {
        n if n.contains("zos-main") => "Main Output",
        n if n.contains("zos-music") => "Music",
        n if n.contains("zos-chat") => "Chat / Voice",
        _ => name,
    }
}

// ---------------------------------------------------------------------------
// Advanced section
// ---------------------------------------------------------------------------

fn build_advanced_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Advanced")
        .build();

    // --- qpwgraph ---
    let graph_row = adw::ActionRow::builder()
        .title("Open Audio Graph")
        .subtitle("Visual PipeWire patchbay (qpwgraph)")
        .build();
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

    // --- EasyEffects ---
    let effects_row = adw::ActionRow::builder()
        .title("Open EasyEffects")
        .subtitle("Audio effects and equalizer")
        .build();
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

    group
}

/// Launch an application in the background.
fn launch_app(command: &str) {
    match std::process::Command::new(command).spawn() {
        Ok(_) => {
            tracing::info!("Launched {}", command);
        }
        Err(e) => {
            tracing::error!("Failed to launch {}: {}", command, e);
        }
    }
}
