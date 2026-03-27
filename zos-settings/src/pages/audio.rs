// === pages/audio.rs — Audio management page (VoiceMeeter-inspired) ===

use std::path::PathBuf;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, DeviceType};

const VIRTUAL_DEVICES_CONFIG: &str = r#"context.objects = [
    { factory = adapter
      args = {
          factory.name   = support.null-audio-sink
          node.name       = "zos-main"
          node.description = "Main Output"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }
    }
    { factory = adapter
      args = {
          factory.name   = support.null-audio-sink
          node.name       = "zos-music"
          node.description = "Music"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }
    }
    { factory = adapter
      args = {
          factory.name   = support.null-audio-sink
          node.name       = "zos-chat"
          node.description = "Chat / Voice"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }
    }
]
"#;

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
    page.append(&build_app_routing_section());
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
    let group = adw::PreferencesGroup::builder().title("Output").build();

    let sinks = pipewire::list_sinks();

    // --- Device selector ---
    let model = gtk::StringList::new(&[]);
    let mut default_idx: u32 = 0;
    for (i, sink) in sinks.iter().enumerate() {
        let label = format!(
            "{} ({})",
            sink.name,
            if sink.device_type == DeviceType::Sink {
                "sink"
            } else {
                "source"
            }
        );
        model.append(&label);
        if sink.is_default {
            default_idx = i as u32;
        }
    }

    // Use device_type icon for the combo row
    let device_icon = sinks
        .first()
        .map(|s| icon_for_device_type(&s.device_type))
        .unwrap_or("audio-speakers-symbolic");
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
    let current_vol =
        live_vol.unwrap_or_else(|| default_sink.and_then(|s| s.volume).unwrap_or(1.0));
    let default_id = default_sink.map(|s| s.id).unwrap_or(0);

    let volume_row = adw::ActionRow::builder().title("Volume").build();

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
    let mute_row = adw::ActionRow::builder().title("Mute").build();

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
    let group = adw::PreferencesGroup::builder().title("Input").build();

    let sources = pipewire::list_sources();

    // --- Device selector ---
    let model = gtk::StringList::new(&[]);
    let mut default_idx: u32 = 0;
    for (i, source) in sources.iter().enumerate() {
        let label = format!(
            "{} ({})",
            source.name,
            if source.device_type == DeviceType::Source {
                "source"
            } else {
                "sink"
            }
        );
        model.append(&label);
        if source.is_default {
            default_idx = i as u32;
        }
    }

    let device_icon = sources
        .first()
        .map(|s| icon_for_device_type(&s.device_type))
        .unwrap_or("audio-input-microphone-symbolic");
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

    let volume_row = adw::ActionRow::builder().title("Volume").build();

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
    let mute_row = adw::ActionRow::builder().title("Mute").build();

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
        let config_path = virtual_devices_config_path();
        if config_path.exists() {
            let empty_row = adw::ActionRow::builder()
                .title("No virtual buses detected")
                .subtitle("Config exists but PipeWire hasn't loaded it — try restarting PipeWire")
                .build();
            group.add(&empty_row);
        } else {
            let setup_row = adw::ActionRow::builder()
                .title("Virtual audio buses aren't set up yet")
                .subtitle("Create the PipeWire config and restart the audio server")
                .build();

            let setup_btn = gtk::Button::builder()
                .label("Set Up Now")
                .valign(gtk::Align::Center)
                .css_classes(["suggested-action"])
                .build();

            setup_btn.connect_clicked(move |btn| {
                let path = virtual_devices_config_path();
                if let Some(parent) = path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        tracing::error!("Failed to create directory {:?}: {}", parent, e);
                        btn.set_label("Error");
                        btn.set_sensitive(false);
                        return;
                    }
                }
                if let Err(e) = std::fs::write(&path, VIRTUAL_DEVICES_CONFIG) {
                    tracing::error!("Failed to write config {:?}: {}", path, e);
                    btn.set_label("Error");
                    btn.set_sensitive(false);
                    return;
                }
                tracing::info!("Wrote virtual devices config to {:?}", path);

                // Restart PipeWire asynchronously to avoid blocking the UI
                match std::process::Command::new("systemctl")
                    .args(["--user", "restart", "pipewire"])
                    .spawn()
                {
                    Ok(_) => tracing::info!("PipeWire restart initiated"),
                    Err(e) => tracing::error!("Failed to restart PipeWire: {}", e),
                }

                btn.set_label("Done — reopen Audio page");
                btn.set_sensitive(false);
            });

            setup_row.add_suffix(&setup_btn);
            setup_row.set_activatable_widget(Some(&setup_btn));
            group.add(&setup_row);
        }
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
// App routing section
// ---------------------------------------------------------------------------

/// The virtual bus targets a stream can be routed to.
const BUS_OPTIONS: &[(&str, &str)] = &[
    ("Default", ""),
    ("Main Output", "zos-main"),
    ("Music", "zos-music"),
    ("Chat / Voice", "zos-chat"),
];

fn build_app_routing_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("App Routing")
        .description("Route application audio to a virtual bus")
        .build();

    let streams = pipewire::list_streams();

    if streams.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No active audio streams")
            .subtitle("Start playing audio in an app and it will appear here")
            .build();
        group.add(&empty_row);
        return group;
    }

    let bus_labels: Vec<&str> = BUS_OPTIONS.iter().map(|(label, _)| *label).collect();
    let bus_sink_names: Vec<String> = BUS_OPTIONS
        .iter()
        .map(|(_, sink)| sink.to_string())
        .collect();

    for stream in &streams {
        let row = adw::ActionRow::builder()
            .title(&stream.name)
            .subtitle(&format!("ID {}", stream.id))
            .build();

        let model = gtk::StringList::new(&bus_labels);
        let dropdown = gtk::DropDown::builder()
            .model(&model)
            .selected(0) // Default
            .valign(gtk::Align::Center)
            .build();

        let stream_id = stream.id;
        let sinks = bus_sink_names.clone();
        dropdown.connect_selected_notify(move |dd| {
            let sel = dd.selected() as usize;
            if let Some(sink_name) = sinks.get(sel) {
                if sink_name.is_empty() {
                    // "Default" — move stream back to the default sink
                    pipewire::set_default(stream_id);
                } else {
                    pipewire::route_stream_to_sink(stream_id, sink_name);
                }
            }
        });

        row.add_suffix(&dropdown);
        group.add(&row);
    }

    group
}

/// Return the path to the user's PipeWire virtual-devices config file.
fn virtual_devices_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/root"));
    PathBuf::from(home).join(".config/pipewire/pipewire.conf.d/10-zos-virtual-devices.conf")
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
// Advanced section (Inputs, Outputs, Bus Routing, Advanced tools)
// ---------------------------------------------------------------------------

fn build_advanced_section() -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .build();

    // -----------------------------------------------------------------------
    // Section 1: Inputs
    // -----------------------------------------------------------------------
    let inputs_group = adw::PreferencesGroup::builder().title("Inputs").build();

    let sources = pipewire::list_sources();

    for source in &sources {
        let row = adw::ActionRow::builder()
            .title(&source.name)
            .icon_name("audio-input-microphone-symbolic")
            .build();

        let switch = gtk::Switch::builder()
            .valign(gtk::Align::Center)
            .active(!source.muted)
            .build();

        let source_id = source.id;
        switch.connect_state_set(move |_sw, active| {
            // When the switch is toggled, mute/unmute the source.
            // The switch is "active" when the source is enabled (not muted).
            // We need to toggle mute if the current state doesn't match.
            let currently_muted = !active;
            // We always toggle — the switch state tracks enabled vs. muted.
            // Since GTK calls this when the state changes, just toggle.
            let _ = std::process::Command::new("wpctl")
                .args(["set-mute", &source_id.to_string(), if currently_muted { "1" } else { "0" }])
                .status();
            gtk::glib::Propagation::Proceed
        });

        row.add_suffix(&switch);
        inputs_group.add(&row);
    }

    // Default Input combo
    let input_model = gtk::StringList::new(&[]);
    let mut input_default_idx: u32 = 0;
    for (i, source) in sources.iter().enumerate() {
        input_model.append(&source.name);
        if source.is_default {
            input_default_idx = i as u32;
        }
    }

    let input_combo = adw::ComboRow::builder()
        .title("Default Input")
        .icon_name("audio-input-microphone-symbolic")
        .model(&input_model)
        .selected(input_default_idx)
        .build();

    let sources_for_combo = sources.clone();
    input_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(source) = sources_for_combo.get(idx) {
            pipewire::set_default(source.id);
        }
    });
    inputs_group.add(&input_combo);

    container.append(&inputs_group);

    // -----------------------------------------------------------------------
    // Section 2: Outputs (physical sinks only, exclude zos-* virtual buses)
    // -----------------------------------------------------------------------
    let outputs_group = adw::PreferencesGroup::builder().title("Outputs").build();

    let all_sinks = pipewire::list_sinks();
    let physical_sinks: Vec<_> = all_sinks
        .iter()
        .filter(|s| !s.name.contains("zos-"))
        .cloned()
        .collect();

    for sink in &physical_sinks {
        let row = adw::ActionRow::builder()
            .title(&sink.name)
            .icon_name("audio-speakers-symbolic")
            .build();
        outputs_group.add(&row);
    }

    // Default Output combo
    let output_model = gtk::StringList::new(&[]);
    let mut output_default_idx: u32 = 0;
    for (i, sink) in physical_sinks.iter().enumerate() {
        output_model.append(&sink.name);
        if sink.is_default {
            output_default_idx = i as u32;
        }
    }

    let output_combo = adw::ComboRow::builder()
        .title("Default Output")
        .icon_name("audio-speakers-symbolic")
        .model(&output_model)
        .selected(output_default_idx)
        .build();

    let physical_sinks_for_combo = physical_sinks.clone();
    output_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(sink) = physical_sinks_for_combo.get(idx) {
            pipewire::set_default(sink.id);
        }
    });
    outputs_group.add(&output_combo);

    container.append(&outputs_group);

    // -----------------------------------------------------------------------
    // Section 3: Bus Routing (virtual sinks with zos-* names)
    // -----------------------------------------------------------------------
    let bus_group = adw::PreferencesGroup::builder()
        .title("Bus Routing")
        .description("Route virtual buses to physical outputs")
        .build();

    let virtual_sinks: Vec<_> = all_sinks
        .iter()
        .filter(|s| s.name.contains("zos-"))
        .cloned()
        .collect();

    if virtual_sinks.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No virtual buses configured")
            .subtitle("Set up virtual audio buses in the Audio Buses section above")
            .build();
        bus_group.add(&empty_row);
    } else {
        // Build the list of physical sink names/ids for the output device combos
        let phys_names: Vec<String> = physical_sinks.iter().map(|s| s.name.clone()).collect();
        let phys_ids: Vec<u32> = physical_sinks.iter().map(|s| s.id).collect();

        for bus in &virtual_sinks {
            let display_name = friendly_bus_name(&bus.name);
            let bus_id = bus.id;
            let current_vol = bus.volume.unwrap_or(1.0);

            let expander = adw::ExpanderRow::builder()
                .title(display_name)
                .subtitle(&bus.name)
                .icon_name("audio-speakers-symbolic")
                .build();

            // Output Device combo — lists physical sinks
            let route_model = gtk::StringList::new(&[]);
            for name in &phys_names {
                route_model.append(name);
            }

            let route_combo = adw::ComboRow::builder()
                .title("Output Device")
                .model(&route_model)
                .selected(0)
                .build();

            let bus_name_for_route = bus.name.clone();
            let phys_ids_clone = phys_ids.clone();
            let phys_names_clone = phys_names.clone();
            route_combo.connect_selected_notify(move |row| {
                let idx = row.selected() as usize;
                if let Some(_sink_id) = phys_ids_clone.get(idx) {
                    // Route the virtual bus monitor output to the selected physical sink.
                    // The monitor output of a null-audio-sink has ports like:
                    //   <bus_name>:monitor_FL, <bus_name>:monitor_FR
                    // The physical sink has input ports like:
                    //   <sink_name>:playback_FL, <sink_name>:playback_FR
                    if let Some(target_name) = phys_names_clone.get(idx) {
                        // Disconnect existing monitor links from this bus
                        if let Some(links_output) = std::process::Command::new("pw-link")
                            .args(["--links"])
                            .output()
                            .ok()
                            .filter(|o| o.status.success())
                            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        {
                            let mut current_output: Option<String> = None;
                            for line in links_output.lines() {
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }
                                if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
                                    if let Some(ref out) = current_output {
                                        if out.starts_with(&bus_name_for_route) && out.contains(":monitor_") {
                                            let input = trimmed
                                                .trim_start_matches("|->")
                                                .trim_start_matches("\\->")
                                                .trim();
                                            pipewire::remove_link(out, input);
                                        }
                                    }
                                } else {
                                    current_output = Some(trimmed.to_string());
                                }
                            }
                        }

                        // Create new links: monitor_FL -> playback_FL, monitor_FR -> playback_FR
                        for channel in &["FL", "FR"] {
                            let out_port = format!("{}:monitor_{}", bus_name_for_route, channel);
                            let in_port = format!("{}:playback_{}", target_name, channel);
                            pipewire::create_link(&out_port, &in_port);
                        }
                    }
                }
            });
            expander.add_row(&route_combo);

            // Volume slider (0-150%)
            let vol_row = adw::ActionRow::builder().title("Volume").build();

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
                pipewire::set_volume(bus_id, val);
                vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
            });

            vol_row.add_suffix(&scale);
            vol_row.add_suffix(&vol_label);
            expander.add_row(&vol_row);

            bus_group.add(&expander);
        }
    }

    container.append(&bus_group);

    // -----------------------------------------------------------------------
    // Section 4: Advanced tools
    // -----------------------------------------------------------------------
    let advanced_group = adw::PreferencesGroup::builder().title("Advanced").build();

    // --- qpwgraph ---
    let graph_row = adw::ActionRow::builder()
        .title("Open Audio Graph")
        .subtitle("Advanced audio routing")
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
    advanced_group.add(&graph_row);

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
    advanced_group.add(&effects_row);

    container.append(&advanced_group);

    container
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
