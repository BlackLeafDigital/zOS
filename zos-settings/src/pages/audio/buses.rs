// === pages/audio/buses.rs — Virtual audio buses section ===

use std::cell::RefCell;
use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, BusConfig, BusTarget};

/// Build the audio buses preferences group.
pub fn build() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Audio Buses")
        .description("Virtual audio buses for routing and mixing")
        .build();

    let bus_configs = pipewire::load_bus_configs();
    let all_sinks = pipewire::list_sinks();

    // Physical sinks (not zos-* buses)
    let physical_sinks: Vec<_> = all_sinks
        .iter()
        .filter(|s| !s.name.starts_with("zos-"))
        .cloned()
        .collect();

    // Build each bus row
    for bus_cfg in &bus_configs {
        let bus_name = bus_cfg.name.clone();
        let bus_description = bus_cfg.description.clone();

        // Find matching live sink for volume data
        let live_sink = all_sinks.iter().find(|s| s.name == bus_name);
        let current_vol = live_sink.and_then(|s| s.volume).unwrap_or(1.0);
        let sink_id = live_sink.map(|s| s.id).unwrap_or(0);

        let expander = adw::ExpanderRow::builder()
            .title(&bus_description)
            .subtitle(&bus_name)
            .build();
        expander.add_css_class("mixer-bus-label");

        // --- Volume row ---
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
            if sink_id > 0 {
                pipewire::set_volume(sink_id, val);
            }
            vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
        });

        vol_row.add_suffix(&scale);
        vol_row.add_suffix(&vol_label);
        expander.add_row(&vol_row);

        // --- Route-to row ---
        // Build the target list: (None), physical sinks, other buses
        let mut route_labels: Vec<String> = Vec::new();
        let mut route_targets: Vec<Option<BusTarget>> = Vec::new();

        // (None) entry
        route_labels.push("(None)".into());
        route_targets.push(None);

        // Physical sinks
        for phys in &physical_sinks {
            route_labels.push(phys.name.clone());
            route_targets.push(Some(BusTarget::PhysicalSink(phys.name.clone())));
        }

        // Other zos-* buses (excluding self)
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

        // Determine current routing to pre-select
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
            .title("Route to")
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
                        // (None) — disconnect by routing to an empty physical sink
                        pipewire::route_bus_to_target(
                            &bus_name_for_route,
                            &BusTarget::PhysicalSink(String::new()),
                        );
                    }
                }
            }
        });
        expander.add_row(&route_combo);

        // --- Remove button row ---
        let remove_row = adw::ActionRow::builder().title("Remove Bus").build();

        let remove_btn = gtk::Button::builder()
            .label("Remove Bus")
            .valign(gtk::Align::Center)
            .css_classes(["destructive-action"])
            .build();

        let bus_name_for_remove = bus_name.clone();
        let all_configs = Rc::new(RefCell::new(bus_configs.clone()));
        let configs_for_remove = all_configs.clone();
        remove_btn.connect_clicked(move |btn| {
            pipewire::remove_virtual_sink(&bus_name_for_remove);

            let mut configs = configs_for_remove.borrow_mut();
            configs.retain(|c| c.name != bus_name_for_remove);
            pipewire::save_bus_configs(&configs);

            btn.set_label("Removed - reopen page");
            btn.set_sensitive(false);
        });

        remove_row.add_suffix(&remove_btn);
        expander.add_row(&remove_row);

        group.add(&expander);
    }

    // --- Add Bus button row ---
    let add_row = adw::ActionRow::builder().title("Create New Bus").build();

    let add_btn = gtk::Button::builder()
        .label("Add Bus")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();

    let bus_configs_for_add = bus_configs.clone();
    add_btn.connect_clicked(move |btn| {
        show_add_bus_dialog(btn, &bus_configs_for_add);
    });

    add_row.add_suffix(&add_btn);
    group.add(&add_row);

    group
}

/// Show a dialog for creating a new audio bus.
fn show_add_bus_dialog(btn: &gtk::Button, existing_configs: &[BusConfig]) {
    let dialog = gtk::Window::builder()
        .title("Create Audio Bus")
        .modal(true)
        .default_width(350)
        .build();

    if let Some(window) = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
        dialog.set_transient_for(Some(&window));
    }

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let name_label = gtk::Label::builder()
        .label("Bus name (\"zos-\" prefix added automatically):")
        .halign(gtk::Align::Start)
        .build();
    content.append(&name_label);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("my-bus")
        .hexpand(true)
        .build();
    content.append(&name_entry);

    let desc_label = gtk::Label::builder()
        .label("Description:")
        .halign(gtk::Align::Start)
        .build();
    content.append(&desc_label);

    let desc_entry = gtk::Entry::builder()
        .placeholder_text("My Audio Bus")
        .hexpand(true)
        .build();
    content.append(&desc_entry);

    let button_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();

    let cancel_btn = gtk::Button::builder().label("Cancel").build();
    let create_btn = gtk::Button::builder()
        .label("Create")
        .css_classes(["suggested-action"])
        .build();

    button_box.append(&cancel_btn);
    button_box.append(&create_btn);
    content.append(&button_box);

    dialog.set_child(Some(&content));

    let dialog_clone = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_clone.close();
    });

    let dialog_clone = dialog.clone();
    let existing = existing_configs.to_vec();
    let add_btn_ref = btn.clone();
    create_btn.connect_clicked(move |_| {
        let raw_name = name_entry.text().to_string();
        let description = desc_entry.text().to_string();

        if raw_name.is_empty() || description.is_empty() {
            return;
        }

        let full_name = if raw_name.starts_with("zos-") {
            raw_name
        } else {
            format!("zos-{}", raw_name)
        };

        pipewire::create_virtual_sink(&full_name, &description);

        let mut configs = existing.clone();
        configs.push(BusConfig {
            name: full_name,
            description,
            target: BusTarget::PhysicalSink(String::new()),
        });
        pipewire::save_bus_configs(&configs);

        dialog_clone.close();
        add_btn_ref.set_label("Created - reopen page");
        add_btn_ref.set_sensitive(false);
    });

    dialog.present();
}
