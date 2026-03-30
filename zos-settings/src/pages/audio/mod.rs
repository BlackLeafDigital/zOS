// === pages/audio/mod.rs — Voicemeeter-style mixer layout ===

mod inputs;
mod outputs;

use std::cell::RefCell;
use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, InputConfig};

/// Build the audio page with a Voicemeeter Potato-style mixer.
///
/// Layout:
///   Top:    App routing row (compact stream → input assignments)
///   Middle: [Virtual Input strips] | divider | [Virtual Output strips]
///   Bottom: Apply button
pub fn build() -> gtk::Box {
    let input_configs = Rc::new(RefCell::new(pipewire::load_input_configs()));
    let output_configs = Rc::new(RefCell::new(pipewire::load_output_configs()));

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    // --- Top: App routing row ---
    wrapper.append(&build_app_routing_row(&input_configs));

    // --- Middle: Mixer strips ---
    let mixer_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .vexpand(true)
        .build();

    // Left section: Virtual Input strips
    let inputs_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();
    inputs_scroll.set_child(Some(&inputs::build_strips(&input_configs, &output_configs)));
    mixer_box.append(&inputs_scroll);

    // Vertical divider
    let divider = gtk::Separator::new(gtk::Orientation::Vertical);
    divider.add_css_class("mixer-divider");
    mixer_box.append(&divider);

    // Right section: Virtual Output strips
    let outputs_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();
    outputs_scroll.set_child(Some(&outputs::build_strips(&output_configs)));
    mixer_box.append(&outputs_scroll);

    wrapper.append(&mixer_box);

    // --- Bottom: Apply button ---
    wrapper.append(&build_apply_button(&input_configs, &output_configs));

    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    outer.append(&wrapper);
    outer
}

// ---------------------------------------------------------------------------
// App routing row
// ---------------------------------------------------------------------------

fn build_app_routing_row(input_configs: &Rc<RefCell<Vec<InputConfig>>>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("App Routing")
        .description("Assign active audio streams to Virtual Inputs")
        .build();

    let streams = pipewire::list_streams();
    let saved_defaults = pipewire::load_app_routing_defaults();
    let configs = input_configs.borrow();

    if streams.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No active streams")
            .subtitle("Play audio in an app to see it here")
            .build();
        let icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        icon.set_valign(gtk::Align::Center);
        empty_row.add_prefix(&icon);
        group.add(&empty_row);
    } else {
        // Build dropdown labels from input configs
        let mut dropdown_labels: Vec<String> = vec!["Default".into()];
        let mut dropdown_sink_names: Vec<String> = vec![String::new()];
        for cfg in configs.iter() {
            dropdown_labels.push(cfg.description.clone());
            dropdown_sink_names.push(cfg.name.clone());
        }
        let label_strs: Vec<&str> = dropdown_labels.iter().map(|s| s.as_str()).collect();

        for stream in &streams {
            let row = adw::ActionRow::builder().title(&stream.name).build();
            let icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
            icon.set_valign(gtk::Align::Center);
            row.add_prefix(&icon);

            let model = gtk::StringList::new(&label_strs);
            let dropdown = gtk::DropDown::builder()
                .model(&model)
                .selected(0)
                .valign(gtk::Align::Center)
                .build();

            // Pre-select from saved defaults
            if let Some(saved_bus) = saved_defaults.get(&stream.name) {
                for (i, sink_name) in dropdown_sink_names.iter().enumerate() {
                    if sink_name == saved_bus {
                        dropdown.set_selected(i as u32);
                        if !saved_bus.is_empty() {
                            pipewire::route_stream_to_sink(stream.id, saved_bus);
                        }
                        break;
                    }
                }
            }

            let stream_id = stream.id;
            let sinks_clone = dropdown_sink_names.clone();
            dropdown.connect_selected_notify(move |dd| {
                let sel = dd.selected() as usize;
                if let Some(sink_name) = sinks_clone.get(sel) {
                    if sink_name.is_empty() {
                        pipewire::set_default(stream_id);
                    } else {
                        pipewire::route_stream_to_sink(stream_id, sink_name);
                    }
                }
            });

            // "Remember" toggle
            let stream_name = stream.name.clone();
            let is_saved = saved_defaults.contains_key(&stream.name);
            let remember_btn = gtk::ToggleButton::builder()
                .icon_name(if is_saved {
                    "starred-symbolic"
                } else {
                    "non-starred-symbolic"
                })
                .valign(gtk::Align::Center)
                .active(is_saved)
                .build();
            remember_btn.add_css_class("flat");

            let dropdown_ref = dropdown.clone();
            let sink_names_for_remember = dropdown_sink_names.clone();
            remember_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    let sel = dropdown_ref.selected() as usize;
                    let bus = sink_names_for_remember
                        .get(sel)
                        .cloned()
                        .unwrap_or_default();
                    pipewire::save_app_routing_default(&stream_name, &bus);
                    btn.set_icon_name("starred-symbolic");
                } else {
                    pipewire::save_app_routing_default(&stream_name, "");
                    btn.set_icon_name("non-starred-symbolic");
                }
            });

            row.add_suffix(&remember_btn);
            row.add_suffix(&dropdown);
            group.add(&row);
        }
    }

    drop(configs);
    container.append(&group);
    container
}

// ---------------------------------------------------------------------------
// Apply button
// ---------------------------------------------------------------------------

fn build_apply_button(
    input_configs: &Rc<RefCell<Vec<InputConfig>>>,
    output_configs: &Rc<RefCell<Vec<pipewire::OutputConfig>>>,
) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .margin_top(8)
        .margin_bottom(16)
        .build();

    let btn = gtk::Button::builder()
        .label("Apply")
        .build();
    btn.add_css_class("suggested-action");

    let inputs = input_configs.clone();
    let outputs = output_configs.clone();
    btn.connect_clicked(move |b| {
        b.set_sensitive(false);
        b.set_label("Applying...");

        let i = inputs.borrow();
        let o = outputs.borrow();
        pipewire::apply_full_config(&i, &o);

        b.set_label("Applied");
    });

    container.append(&btn);
    container
}
