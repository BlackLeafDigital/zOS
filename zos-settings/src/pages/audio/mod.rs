// === pages/audio/mod.rs — Unified audio bus settings page ===

use std::cell::RefCell;
use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::glib;

use crate::services::pipewire::{self, AudioBusConfig};

/// Build the audio settings page with per-bus views and a dropdown selector.
pub fn build() -> gtk::Box {
    let bus_configs = Rc::new(RefCell::new(pipewire::load_audio_bus_configs()));
    let ui_state = Rc::new(RefCell::new(pipewire::load_audio_ui_state()));

    let page = super::page_content();

    // --- Global app routing ---
    page.append(&build_global_app_routing(&bus_configs));

    // --- Bus selector row ---
    let selector_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let bus_model = gtk::StringList::new(&[]);
    {
        let configs = bus_configs.borrow();
        for cfg in configs.iter() {
            bus_model.append(&cfg.description);
        }
    }

    let bus_combo = adw::ComboRow::builder()
        .title("Active Bus")
        .model(&bus_model)
        .hexpand(true)
        .build();

    // Pre-select from saved UI state
    {
        let state = ui_state.borrow();
        let configs = bus_configs.borrow();
        if !state.last_selected_bus.is_empty() {
            if let Some(idx) = configs
                .iter()
                .position(|c| c.name == state.last_selected_bus)
            {
                bus_combo.set_selected(idx as u32);
            }
        }
    }

    let selector_group = adw::PreferencesGroup::new();
    selector_group.add(&bus_combo);
    selector_row.append(&selector_group);

    let add_btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Audio Bus")
        .valign(gtk::Align::Center)
        .build();
    add_btn.add_css_class("suggested-action");
    add_btn.add_css_class("circular");
    selector_row.append(&add_btn);

    page.append(&selector_row);

    // --- Bus view container (rebuilt on selection change) ---
    let bus_view_container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    // Build initial bus view
    {
        let selected = bus_combo.selected() as usize;
        let configs = bus_configs.borrow();
        if let Some(cfg) = configs.get(selected) {
            build_bus_view(
                &bus_view_container,
                selected,
                cfg,
                &bus_configs,
                &bus_model,
                &bus_combo,
            );
        }
    }

    page.append(&bus_view_container);

    // --- Apply button ---
    let apply_container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .margin_top(8)
        .margin_bottom(16)
        .build();

    let apply_btn = gtk::Button::builder().label("Apply").build();
    apply_btn.add_css_class("suggested-action");

    {
        let configs = bus_configs.clone();
        apply_btn.connect_clicked(move |b| {
            b.set_sensitive(false);
            b.set_label("Applying...");
            let c = configs.borrow();
            pipewire::apply_audio_bus_config(&c);
            drop(c);
            b.set_label("Applied");
            let btn = b.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
                btn.set_label("Apply");
                btn.set_sensitive(true);
            });
        });
    }

    apply_container.append(&apply_btn);
    page.append(&apply_container);

    // --- Bus selector change handler ---
    {
        let configs = bus_configs.clone();
        let container = bus_view_container.clone();
        let state = ui_state.clone();
        let model_for_handler = bus_model.clone();
        bus_combo.connect_selected_notify(move |combo| {
            let selected = combo.selected() as usize;
            let cfgs = configs.borrow();
            // Clear old bus view
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
            if let Some(cfg) = cfgs.get(selected) {
                // Save last selection
                let mut s = state.borrow_mut();
                s.last_selected_bus = cfg.name.clone();
                pipewire::save_audio_ui_state(&s);
                drop(s);
                drop(cfgs);

                let cfgs = configs.borrow();
                if let Some(cfg) = cfgs.get(selected) {
                    build_bus_view(
                        &container,
                        selected,
                        cfg,
                        &configs,
                        &model_for_handler,
                        combo,
                    );
                }
            }
        });
    }

    // --- Add Bus button handler ---
    {
        let configs = bus_configs.clone();
        let model = bus_model.clone();
        let combo = bus_combo.clone();
        add_btn.connect_clicked(move |b| {
            show_add_bus_dialog(b, &configs, &model, &combo);
        });
    }

    super::page_wrapper(&page)
}

// ---------------------------------------------------------------------------
// Bus view builder
// ---------------------------------------------------------------------------

fn build_bus_view(
    container: &gtk::Box,
    idx: usize,
    config: &AudioBusConfig,
    bus_configs: &Rc<RefCell<Vec<AudioBusConfig>>>,
    bus_model: &gtk::StringList,
    bus_combo: &adw::ComboRow,
) {
    // --- Volume section (live — changes take effect immediately) ---
    let vol_group = adw::PreferencesGroup::builder().title("Volume").build();

    let all_sinks = pipewire::list_sinks();
    let live_sink = all_sinks.iter().find(|s| s.name == config.name);
    let sink_id = live_sink.map(|s| s.id).unwrap_or(0);
    let current_vol = live_sink.and_then(|s| s.volume).unwrap_or(1.0);

    let vol_row = adw::ActionRow::builder().title("Volume").build();
    let vol_label = gtk::Label::builder()
        .label(format!("{}%", (current_vol * 100.0).round() as i32))
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

    {
        let label = vol_label.clone();
        vol_scale.connect_value_changed(move |s| {
            let val = s.value() as f32;
            if sink_id > 0 {
                pipewire::set_volume(sink_id, val);
            }
            label.set_label(&format!("{}%", (val * 100.0).round() as i32));
        });
    }
    vol_row.add_suffix(&vol_scale);
    vol_row.add_suffix(&vol_label);
    vol_group.add(&vol_row);

    // Mute toggle
    let muted = live_sink.map(|s| s.muted).unwrap_or(false);
    let mute_row = adw::ActionRow::builder().title("Mute").build();
    let mute_btn = gtk::ToggleButton::builder()
        .icon_name(if muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        })
        .active(muted)
        .valign(gtk::Align::Center)
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
    mute_row.add_suffix(&mute_btn);
    vol_group.add(&mute_row);

    container.append(&vol_group);

    // --- Processing section (deferred — requires Apply) ---
    let proc_group = adw::PreferencesGroup::builder()
        .title("Processing")
        .description("Changes take effect after Apply")
        .build();

    let gain_row = adw::ActionRow::builder().title("Gain").build();
    let gain_label = gtk::Label::builder()
        .label(format!("{:.1} dB", config.gain))
        .valign(gtk::Align::Center)
        .width_chars(8)
        .build();
    let gain_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    gain_scale.set_range(-12.0, 12.0);
    gain_scale.set_increments(0.5, 1.0);
    gain_scale.set_value(config.gain as f64);

    {
        let label = gain_label.clone();
        let configs = bus_configs.clone();
        gain_scale.connect_value_changed(move |s| {
            let val = s.value() as f32;
            label.set_label(&format!("{:.1} dB", val));
            if let Some(cfg) = configs.borrow_mut().get_mut(idx) {
                cfg.gain = val;
            }
        });
    }
    gain_row.add_suffix(&gain_scale);
    gain_row.add_suffix(&gain_label);
    proc_group.add(&gain_row);

    container.append(&proc_group);

    // --- Output Device section ---
    let device_group = adw::PreferencesGroup::builder()
        .title("Output Device")
        .build();
    let physical_sinks = pipewire::list_physical_sinks();
    let device_model = gtk::StringList::new(&[]);
    let mut device_selected: u32 = 0;
    for (i, sink) in physical_sinks.iter().enumerate() {
        device_model.append(&sink.name);
        if sink.name == config.physical_device {
            device_selected = i as u32;
        }
    }
    let device_combo = adw::ComboRow::builder()
        .title("Adapter")
        .model(&device_model)
        .selected(device_selected)
        .build();

    {
        let configs = bus_configs.clone();
        let physical_names: Vec<String> = physical_sinks.iter().map(|s| s.name.clone()).collect();
        device_combo.connect_selected_notify(move |row| {
            let sel = row.selected() as usize;
            if let Some(dev_name) = physical_names.get(sel) {
                if let Some(cfg) = configs.borrow_mut().get_mut(idx) {
                    cfg.physical_device = dev_name.clone();
                }
            }
        });
    }

    device_group.add(&device_combo);
    container.append(&device_group);

    // --- EQ section (collapsible) ---
    let eq_group = adw::PreferencesGroup::new();
    let eq_expander = adw::ExpanderRow::builder()
        .title("Equalizer")
        .show_enable_switch(true)
        .enable_expansion(config.eq_enabled)
        .build();

    let eq_icon = gtk::Image::from_icon_name("multimedia-equalizer-symbolic");
    eq_icon.set_valign(gtk::Align::Center);
    eq_expander.add_prefix(&eq_icon);

    {
        let configs = bus_configs.clone();
        eq_expander.connect_enable_expansion_notify(move |row| {
            if let Some(cfg) = configs.borrow_mut().get_mut(idx) {
                cfg.eq_enabled = row.enables_expansion();
            }
        });
    }

    build_eq_band_rows(
        &eq_expander,
        "Low",
        config.eq_low.freq,
        config.eq_low.gain,
        20.0,
        500.0,
        idx,
        bus_configs,
        EqBandTarget::Low,
    );
    build_eq_band_rows(
        &eq_expander,
        "Mid",
        config.eq_mid.freq,
        config.eq_mid.gain,
        200.0,
        8000.0,
        idx,
        bus_configs,
        EqBandTarget::Mid,
    );
    build_eq_band_rows(
        &eq_expander,
        "High",
        config.eq_high.freq,
        config.eq_high.gain,
        2000.0,
        20000.0,
        idx,
        bus_configs,
        EqBandTarget::High,
    );

    eq_group.add(&eq_expander);
    container.append(&eq_group);

    // --- Delete Bus button (non-default buses only) ---
    let is_default = config.name == "zos-main" || config.name == "zos-chat";
    if !is_default {
        let delete_container = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .halign(gtk::Align::Center)
            .margin_top(12)
            .build();
        let delete_btn = gtk::Button::builder().label("Delete Bus").build();
        delete_btn.add_css_class("destructive-action");

        let configs = bus_configs.clone();
        let bus_name = config.name.clone();
        let model = bus_model.clone();
        let combo = bus_combo.clone();
        delete_btn.connect_clicked(move |_| {
            {
                let mut cfgs = configs.borrow_mut();
                cfgs.retain(|c| c.name != bus_name);
                pipewire::save_audio_bus_configs(&cfgs);
            }
            // Rebuild dropdown model
            while model.n_items() > 0 {
                model.remove(0);
            }
            let cfgs = configs.borrow();
            for cfg in cfgs.iter() {
                model.append(&cfg.description);
            }
            drop(cfgs);
            combo.set_selected(0);
        });

        delete_container.append(&delete_btn);
        container.append(&delete_container);
    }
}

// ---------------------------------------------------------------------------
// Global app routing
// ---------------------------------------------------------------------------

fn build_global_app_routing(
    bus_configs: &Rc<RefCell<Vec<AudioBusConfig>>>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("App Routing")
        .description("Route application audio to buses")
        .build();

    let streams = pipewire::list_streams();
    let saved_defaults = pipewire::load_app_routing_defaults();
    let configs = bus_configs.borrow();

    if streams.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No active audio streams")
            .subtitle("Play audio in an app to see it here")
            .build();
        let icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        icon.set_valign(gtk::Align::Center);
        empty_row.add_prefix(&icon);
        group.add(&empty_row);
        return group;
    }

    // Build bus dropdown options: ["Unassigned", "Main", "Chat / Voice", ...]
    let mut bus_labels = vec!["Unassigned".to_string()];
    let mut bus_names = vec![String::new()];
    for cfg in configs.iter() {
        bus_labels.push(cfg.description.clone());
        bus_names.push(cfg.name.clone());
    }
    let label_strs: Vec<&str> = bus_labels.iter().map(|s| s.as_str()).collect();

    for stream in &streams {
        let row = adw::ActionRow::builder().title(&stream.name).build();
        let icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        icon.set_valign(gtk::Align::Center);
        row.add_prefix(&icon);

        let model = gtk::StringList::new(&label_strs);
        let dropdown = gtk::DropDown::builder()
            .model(&model)
            .valign(gtk::Align::Center)
            .build();

        // Pre-select current assignment
        let current_bus = saved_defaults
            .get(&stream.name)
            .cloned()
            .unwrap_or_default();
        let selected_idx = bus_names
            .iter()
            .position(|n| n == &current_bus)
            .unwrap_or(0);
        dropdown.set_selected(selected_idx as u32);

        let stream_name = stream.name.clone();
        let stream_id = stream.id;
        let bus_names_clone = bus_names.clone();
        dropdown.connect_selected_notify(move |dd| {
            let sel = dd.selected() as usize;
            if let Some(bus_name) = bus_names_clone.get(sel) {
                if bus_name.is_empty() {
                    pipewire::save_app_routing_default(&stream_name, "");
                    pipewire::set_default(stream_id);
                } else {
                    pipewire::save_app_routing_default(&stream_name, bus_name);
                    pipewire::route_stream_to_sink(stream_id, bus_name);
                }
            }
        });

        row.add_suffix(&dropdown);
        group.add(&row);
    }

    drop(configs);
    group
}

// ---------------------------------------------------------------------------
// EQ band helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum EqBandTarget {
    Low,
    Mid,
    High,
}

#[allow(clippy::too_many_arguments)]
fn build_eq_band_rows(
    expander: &adw::ExpanderRow,
    label: &str,
    freq: f32,
    gain: f32,
    freq_min: f64,
    freq_max: f64,
    idx: usize,
    bus_configs: &Rc<RefCell<Vec<AudioBusConfig>>>,
    target: EqBandTarget,
) {
    // Frequency row
    let freq_row = adw::ActionRow::builder()
        .title(format!("{label} Freq"))
        .build();
    let freq_val_label = gtk::Label::builder()
        .label(format!("{:.0} Hz", freq))
        .valign(gtk::Align::Center)
        .width_chars(8)
        .build();
    let freq_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .width_request(100)
        .build();
    freq_scale.set_range(freq_min, freq_max);
    freq_scale.set_increments(10.0, 100.0);
    freq_scale.set_value(freq as f64);

    {
        let label_ref = freq_val_label.clone();
        let configs = bus_configs.clone();
        freq_scale.connect_value_changed(move |s| {
            let val = s.value() as f32;
            label_ref.set_label(&format!("{:.0} Hz", val));
            if let Some(cfg) = configs.borrow_mut().get_mut(idx) {
                match target {
                    EqBandTarget::Low => cfg.eq_low.freq = val,
                    EqBandTarget::Mid => cfg.eq_mid.freq = val,
                    EqBandTarget::High => cfg.eq_high.freq = val,
                }
            }
        });
    }
    freq_row.add_suffix(&freq_scale);
    freq_row.add_suffix(&freq_val_label);
    expander.add_row(&freq_row);

    // Gain row
    let gain_row = adw::ActionRow::builder()
        .title(format!("{label} Gain"))
        .build();
    let gain_val_label = gtk::Label::builder()
        .label(format!("{:.1} dB", gain))
        .valign(gtk::Align::Center)
        .width_chars(8)
        .build();
    let gain_scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .width_request(100)
        .build();
    gain_scale.set_range(-12.0, 12.0);
    gain_scale.set_increments(0.5, 1.0);
    gain_scale.set_value(gain as f64);

    {
        let label_ref = gain_val_label.clone();
        let configs = bus_configs.clone();
        gain_scale.connect_value_changed(move |s| {
            let val = s.value() as f32;
            label_ref.set_label(&format!("{:.1} dB", val));
            if let Some(cfg) = configs.borrow_mut().get_mut(idx) {
                match target {
                    EqBandTarget::Low => cfg.eq_low.gain = val,
                    EqBandTarget::Mid => cfg.eq_mid.gain = val,
                    EqBandTarget::High => cfg.eq_high.gain = val,
                }
            }
        });
    }
    gain_row.add_suffix(&gain_scale);
    gain_row.add_suffix(&gain_val_label);
    expander.add_row(&gain_row);
}

// ---------------------------------------------------------------------------
// Add Bus dialog
// ---------------------------------------------------------------------------

fn show_add_bus_dialog(
    btn: &gtk::Button,
    configs: &Rc<RefCell<Vec<AudioBusConfig>>>,
    model: &gtk::StringList,
    combo: &adw::ComboRow,
) {
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
        .label("Internal name (\"zos-\" prefix added):")
        .halign(gtk::Align::Start)
        .build();
    content.append(&name_label);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("gaming")
        .hexpand(true)
        .build();
    content.append(&name_entry);

    let desc_label = gtk::Label::builder()
        .label("Display name:")
        .halign(gtk::Align::Start)
        .build();
    content.append(&desc_label);

    let desc_entry = gtk::Entry::builder()
        .placeholder_text("Gaming")
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
    let model_clone = model.clone();
    let combo_clone = combo.clone();
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

        let new_bus = AudioBusConfig {
            name: full_name,
            description: description.clone(),
            gain: 0.0,
            physical_device: String::new(),
            eq_enabled: false,
            eq_low: pipewire::EqBand {
                freq: 200.0,
                gain: 0.0,
            },
            eq_mid: pipewire::EqBand {
                freq: 1000.0,
                gain: 0.0,
            },
            eq_high: pipewire::EqBand {
                freq: 8000.0,
                gain: 0.0,
            },
        };

        let mut cfgs = configs_clone.borrow_mut();
        cfgs.push(new_bus);
        pipewire::save_audio_bus_configs(&cfgs);
        let new_idx = cfgs.len() - 1;
        drop(cfgs);

        model_clone.append(&description);
        combo_clone.set_selected(new_idx as u32);

        dialog_clone.close();
    });

    dialog.present();
}
