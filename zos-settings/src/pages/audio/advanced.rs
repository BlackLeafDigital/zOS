// === pages/audio/advanced.rs — Advanced audio section ===

use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, BusTarget};

use super::launch_app;

/// Build the advanced audio section.
pub fn build() -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .build();

    // -----------------------------------------------------------------------
    // Inputs
    // -----------------------------------------------------------------------
    let inputs_group = adw::PreferencesGroup::builder().title("Inputs").build();

    let sources = pipewire::list_sources();

    for source in &sources {
        let row = adw::ActionRow::builder().title(&source.name).build();
        let row_icon = gtk::Image::from_icon_name("audio-input-microphone-symbolic");
        row_icon.set_valign(gtk::Align::Center);
        row.add_prefix(&row_icon);

        let switch = gtk::Switch::builder()
            .valign(gtk::Align::Center)
            .active(!source.muted)
            .build();

        let source_id = source.id;
        switch.connect_state_set(move |_sw, active| {
            let currently_muted = !active;
            let _ = std::process::Command::new("wpctl")
                .args([
                    "set-mute",
                    &source_id.to_string(),
                    if currently_muted { "1" } else { "0" },
                ])
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
        .model(&input_model)
        .selected(input_default_idx)
        .build();
    let input_combo_icon = gtk::Image::from_icon_name("audio-input-microphone-symbolic");
    input_combo_icon.set_valign(gtk::Align::Center);
    input_combo.add_prefix(&input_combo_icon);

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
    // Outputs (physical sinks only, exclude zos-* virtual buses)
    // -----------------------------------------------------------------------
    let outputs_group = adw::PreferencesGroup::builder().title("Outputs").build();

    let all_sinks = pipewire::list_sinks();
    let physical_sinks: Vec<_> = all_sinks
        .iter()
        .filter(|s| !s.name.starts_with("zos-"))
        .cloned()
        .collect();

    for sink in &physical_sinks {
        let row = adw::ActionRow::builder().title(&sink.name).build();
        let sink_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        sink_icon.set_valign(gtk::Align::Center);
        row.add_prefix(&sink_icon);
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
        .model(&output_model)
        .selected(output_default_idx)
        .build();
    let output_combo_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
    output_combo_icon.set_valign(gtk::Align::Center);
    output_combo.add_prefix(&output_combo_icon);

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
    // Bus Routing (using dynamic bus configs)
    // -----------------------------------------------------------------------
    let bus_group = adw::PreferencesGroup::builder()
        .title("Bus Routing")
        .description("Route virtual buses to physical outputs")
        .build();

    let bus_configs = pipewire::load_bus_configs();

    if bus_configs.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No virtual buses configured")
            .subtitle("Set up virtual audio buses in the Audio Buses section above")
            .build();
        bus_group.add(&empty_row);
    } else {
        for bus_cfg in &bus_configs {
            let bus_name = bus_cfg.name.clone();
            let bus_description = bus_cfg.description.clone();

            // Find matching live sink for volume data
            let live_sink = all_sinks.iter().find(|s| s.name == bus_name);
            let current_vol = live_sink.and_then(|s| s.volume).unwrap_or(1.0);
            let bus_id = live_sink.map(|s| s.id).unwrap_or(0);

            let expander = adw::ExpanderRow::builder()
                .title(&bus_description)
                .subtitle(&bus_name)
                .build();
            let expander_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
            expander_icon.set_valign(gtk::Align::Center);
            expander.add_prefix(&expander_icon);

            // Route-to combo: (None), physical sinks, other buses
            let mut route_labels: Vec<String> = Vec::new();
            let mut route_targets: Vec<Option<BusTarget>> = Vec::new();

            route_labels.push("(None)".into());
            route_targets.push(None);

            for phys in &physical_sinks {
                route_labels.push(phys.name.clone());
                route_targets.push(Some(BusTarget::PhysicalSink(phys.name.clone())));
            }

            for other_cfg in &bus_configs {
                if other_cfg.name == bus_name {
                    continue;
                }
                route_labels.push(format!("Bus: {}", other_cfg.description));
                route_targets.push(Some(BusTarget::Bus(other_cfg.name.clone())));
            }

            let route_model = gtk::StringList::new(&[]);
            for label in &route_labels {
                route_model.append(label);
            }

            // Pre-select current routing
            let current_routing = pipewire::get_bus_routing(&bus_name);
            let mut route_selected: u32 = 0;
            if let Some(ref target_name) = current_routing {
                for (i, rt) in route_targets.iter().enumerate() {
                    match rt {
                        Some(BusTarget::PhysicalSink(name)) if name == target_name => {
                            route_selected = i as u32;
                            break;
                        }
                        Some(BusTarget::Bus(name)) if name == target_name => {
                            route_selected = i as u32;
                            break;
                        }
                        _ => {}
                    }
                }
            }

            let route_combo = adw::ComboRow::builder()
                .title("Output Device")
                .model(&route_model)
                .selected(route_selected)
                .build();

            let bus_name_for_route = bus_name.clone();
            let route_targets = Rc::new(route_targets);
            let route_targets_clone = route_targets.clone();
            route_combo.connect_selected_notify(move |row| {
                let idx = row.selected() as usize;
                if let Some(target_opt) = route_targets_clone.get(idx) {
                    match target_opt {
                        Some(target) => {
                            pipewire::route_bus_to_target(&bus_name_for_route, target);
                        }
                        None => {
                            pipewire::route_bus_to_target(
                                &bus_name_for_route,
                                &BusTarget::PhysicalSink(String::new()),
                            );
                        }
                    }
                }
            });
            expander.add_row(&route_combo);

            // Volume slider
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
                if bus_id > 0 {
                    pipewire::set_volume(bus_id, val);
                }
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
    // Advanced tools
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
