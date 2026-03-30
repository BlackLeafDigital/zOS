// === pages/audio/inputs.rs — Virtual Input mixer strips ===

use std::cell::RefCell;
use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, InputConfig, OutputConfig};

/// Build horizontal row of Virtual Input mixer strips.
pub fn build_strips(
    input_configs: &Rc<RefCell<Vec<InputConfig>>>,
    output_configs: &Rc<RefCell<Vec<OutputConfig>>>,
) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let all_sinks = pipewire::list_sinks();
    let saved_defaults = pipewire::load_app_routing_defaults();
    let streams = pipewire::list_streams();

    let configs = input_configs.borrow();
    for (idx, input_cfg) in configs.iter().enumerate() {
        container.append(&build_single_strip(
            idx,
            input_cfg,
            output_configs,
            &all_sinks,
            &saved_defaults,
            &streams,
            input_configs,
        ));
    }
    drop(configs);

    // "Add Input" button
    container.append(&build_add_button(input_configs, output_configs));

    container
}

fn build_single_strip(
    idx: usize,
    config: &InputConfig,
    output_configs: &Rc<RefCell<Vec<OutputConfig>>>,
    all_sinks: &[pipewire::AudioDevice],
    saved_defaults: &std::collections::HashMap<String, String>,
    streams: &[pipewire::AudioStream],
    input_configs: &Rc<RefCell<Vec<InputConfig>>>,
) -> gtk::Box {
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .width_request(180)
        .build();
    strip.add_css_class("mixer-strip");
    strip.add_css_class("mixer-strip-input");

    // --- Header ---
    let header = gtk::Label::builder()
        .label(&config.description)
        .build();
    header.add_css_class("mixer-strip-header");
    strip.append(&header);

    // Find the matching live PipeWire sink for this input
    let live_sink = all_sinks.iter().find(|s| s.name == config.name);
    let sink_id = live_sink.map(|s| s.id).unwrap_or(0);
    let current_vol = live_sink.and_then(|s| s.volume).unwrap_or(1.0);

    // --- Gain slider (-12 to +12 dB) ---
    let gain_group = adw::PreferencesGroup::builder().title("Gain").build();
    let gain_row = adw::ActionRow::builder().build();
    let gain_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    gain_scale.set_range(-12.0, 12.0);
    gain_scale.set_increments(0.5, 1.0);
    gain_scale.set_value(config.gain as f64);
    gain_scale.set_draw_value(true);

    let gain_configs = input_configs.clone();
    gain_scale.connect_value_changed(move |s| {
        let val = s.value() as f32;
        if let Some(cfg) = gain_configs.borrow_mut().get_mut(idx) {
            cfg.gain = val;
        }
    });
    gain_row.add_suffix(&gain_scale);
    gain_group.add(&gain_row);
    strip.append(&gain_group);

    // --- Volume slider (0-150%) ---
    let vol_group = adw::PreferencesGroup::builder().title("Volume").build();
    let vol_row = adw::ActionRow::builder().build();
    let vol_label = gtk::Label::builder()
        .label(&format!("{}%", (current_vol * 100.0).round() as i32))
        .valign(gtk::Align::Center)
        .width_chars(5)
        .build();
    let vol_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .width_request(100)
        .build();
    vol_scale.set_range(0.0, 1.5);
    vol_scale.set_increments(0.01, 0.05);
    vol_scale.set_value(current_vol as f64);

    let vol_label_clone = vol_label.clone();
    vol_scale.connect_value_changed(move |s| {
        let val = s.value() as f32;
        if sink_id > 0 {
            pipewire::set_volume(sink_id, val);
        }
        vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
    });
    vol_row.add_suffix(&vol_scale);
    vol_row.add_suffix(&vol_label);
    vol_group.add(&vol_row);
    strip.append(&vol_group);

    // --- Mute toggle ---
    let muted = live_sink.map(|s| s.muted).unwrap_or(false);
    let mute_btn = gtk::ToggleButton::builder()
        .icon_name(if muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        })
        .active(muted)
        .build();
    mute_btn.add_css_class("mixer-mute-btn");
    if muted {
        mute_btn.add_css_class("muted");
    }
    mute_btn.connect_toggled(move |btn| {
        if sink_id > 0 {
            pipewire::toggle_mute(sink_id);
        }
        let icon = if btn.is_active() {
            btn.add_css_class("muted");
            "audio-volume-muted-symbolic"
        } else {
            btn.remove_css_class("muted");
            "audio-volume-high-symbolic"
        };
        btn.set_icon_name(icon);
    });
    strip.append(&mute_btn);

    // --- Route to: checkboxes ---
    let route_group = adw::PreferencesGroup::builder().title("Route to").build();
    let outputs = output_configs.borrow();
    for output in outputs.iter() {
        let is_routed = config.outputs.contains(&output.name);
        let check = gtk::CheckButton::builder()
            .label(&output.description)
            .active(is_routed)
            .build();

        let output_name = output.name.clone();
        let route_configs = input_configs.clone();
        check.connect_toggled(move |cb| {
            if let Some(cfg) = route_configs.borrow_mut().get_mut(idx) {
                if cb.is_active() {
                    if !cfg.outputs.contains(&output_name) {
                        cfg.outputs.push(output_name.clone());
                    }
                } else {
                    cfg.outputs.retain(|n| n != &output_name);
                }
            }
        });
        route_group.add(&check);
    }
    drop(outputs);
    strip.append(&route_group);

    // --- App indicators ---
    let mut has_apps = false;
    for stream in streams {
        let is_here = saved_defaults
            .get(&stream.name)
            .map(|s| s == &config.name)
            .unwrap_or(false);
        if is_here {
            if !has_apps {
                let apps_label = gtk::Label::builder()
                    .label("Apps")
                    .halign(gtk::Align::Start)
                    .build();
                apps_label.add_css_class("mixer-section-label");
                strip.append(&apps_label);
                has_apps = true;
            }
            let app_label = gtk::Label::builder()
                .label(&stream.name)
                .halign(gtk::Align::Start)
                .build();
            app_label.add_css_class("mixer-app-indicator");
            strip.append(&app_label);
        }
    }

    strip
}

fn build_add_button(
    input_configs: &Rc<RefCell<Vec<InputConfig>>>,
    _output_configs: &Rc<RefCell<Vec<OutputConfig>>>,
) -> gtk::Box {
    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .width_request(120)
        .valign(gtk::Align::Center)
        .build();

    let btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Virtual Input")
        .build();
    btn.add_css_class("suggested-action");
    btn.add_css_class("circular");

    let configs = input_configs.clone();
    btn.connect_clicked(move |b| {
        show_add_dialog(b, &configs);
    });

    col.append(&btn);
    col
}

fn show_add_dialog(btn: &gtk::Button, configs: &Rc<RefCell<Vec<InputConfig>>>) {
    let dialog = gtk::Window::builder()
        .title("Create Virtual Input")
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
        .label("Internal name (\"zos-\" prefix added):")
        .halign(gtk::Align::Start)
        .build();
    content.append(&name_label);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("my-input")
        .hexpand(true)
        .build();
    content.append(&name_entry);

    let desc_label = gtk::Label::builder()
        .label("Display name:")
        .halign(gtk::Align::Start)
        .build();
    content.append(&desc_label);

    let desc_entry = gtk::Entry::builder()
        .placeholder_text("My Input")
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
    cancel_btn.connect_clicked(move |_| dialog_clone.close());

    let dialog_clone = dialog.clone();
    let configs_clone = configs.clone();
    let add_btn = btn.clone();
    create_btn.connect_clicked(move |_| {
        let raw_name = name_entry.text().to_string();
        let description = desc_entry.text().to_string();
        if raw_name.is_empty() || description.is_empty() {
            return;
        }
        let full_name = if raw_name.starts_with("zos-") {
            raw_name
        } else {
            format!("zos-{raw_name}")
        };

        let new_input = InputConfig {
            name: full_name,
            description,
            gain: 0.0,
            outputs: vec![],
        };
        configs_clone.borrow_mut().push(new_input);

        dialog_clone.close();
        add_btn.set_sensitive(false);
        add_btn.set_tooltip_text(Some("Created — click Apply"));
    });

    dialog.present();
}
