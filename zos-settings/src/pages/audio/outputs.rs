// === pages/audio/outputs.rs — Virtual Output mixer strips ===

use std::cell::RefCell;
use std::rc::Rc;

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, OutputConfig};

/// Build horizontal row of Virtual Output mixer strips.
pub fn build_strips(output_configs: &Rc<RefCell<Vec<OutputConfig>>>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let physical_sinks = pipewire::list_physical_sinks();
    let all_sinks = pipewire::list_sinks();

    let configs = output_configs.borrow();
    for (idx, output_cfg) in configs.iter().enumerate() {
        container.append(&build_single_strip(
            idx,
            output_cfg,
            &physical_sinks,
            &all_sinks,
            output_configs,
        ));
    }
    drop(configs);

    container.append(&build_add_button(output_configs));

    container
}

fn build_single_strip(
    idx: usize,
    config: &OutputConfig,
    physical_sinks: &[pipewire::AudioDevice],
    all_sinks: &[pipewire::AudioDevice],
    output_configs: &Rc<RefCell<Vec<OutputConfig>>>,
) -> gtk::Box {
    let strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .width_request(200)
        .build();
    strip.add_css_class("mixer-strip");
    strip.add_css_class("mixer-strip-output");

    // --- Header ---
    let header = gtk::Label::builder()
        .label(&config.description)
        .build();
    header.add_css_class("mixer-strip-header");
    strip.append(&header);

    // --- Physical device dropdown ---
    let device_group = adw::PreferencesGroup::builder().title("Device").build();
    let model = gtk::StringList::new(&[]);
    let mut selected_idx: u32 = 0;
    for (i, sink) in physical_sinks.iter().enumerate() {
        model.append(&sink.name);
        if sink.name == config.physical_device {
            selected_idx = i as u32;
        }
    }
    let device_combo = adw::ComboRow::builder()
        .title("Adapter")
        .model(&model)
        .selected(selected_idx)
        .build();

    let device_configs = output_configs.clone();
    let physical_names: Vec<String> = physical_sinks.iter().map(|s| s.name.clone()).collect();
    device_combo.connect_selected_notify(move |row| {
        let sel = row.selected() as usize;
        if let Some(dev_name) = physical_names.get(sel) {
            if let Some(cfg) = device_configs.borrow_mut().get_mut(idx) {
                cfg.physical_device = dev_name.clone();
            }
        }
    });
    device_group.add(&device_combo);
    strip.append(&device_group);

    // --- Volume slider ---
    let live_sink = all_sinks.iter().find(|s| s.name == config.name);
    let sink_id = live_sink.map(|s| s.id).unwrap_or(0);
    let current_vol = live_sink.and_then(|s| s.volume).unwrap_or(1.0);

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

    // --- EQ Section (expandable) ---
    let eq_group = adw::PreferencesGroup::builder().title("Equalizer").build();

    // EQ enable switch
    let eq_switch_row = adw::SwitchRow::builder()
        .title("Enable EQ")
        .active(config.eq_enabled)
        .build();
    let eq_configs = output_configs.clone();
    eq_switch_row.connect_active_notify(move |row| {
        if let Some(cfg) = eq_configs.borrow_mut().get_mut(idx) {
            cfg.eq_enabled = row.is_active();
        }
    });
    eq_group.add(&eq_switch_row);

    // Low shelf band
    build_eq_band(&eq_group, "Low", config.eq_low.freq, config.eq_low.gain,
        20.0, 500.0, idx, output_configs, EqBandTarget::Low);

    // Mid peak band
    build_eq_band(&eq_group, "Mid", config.eq_mid.freq, config.eq_mid.gain,
        200.0, 8000.0, idx, output_configs, EqBandTarget::Mid);

    // High shelf band
    build_eq_band(&eq_group, "High", config.eq_high.freq, config.eq_high.gain,
        2000.0, 20000.0, idx, output_configs, EqBandTarget::High);

    strip.append(&eq_group);

    strip
}

#[derive(Clone, Copy)]
enum EqBandTarget {
    Low,
    Mid,
    High,
}

fn build_eq_band(
    group: &adw::PreferencesGroup,
    label: &str,
    freq: f32,
    gain: f32,
    freq_min: f64,
    freq_max: f64,
    idx: usize,
    output_configs: &Rc<RefCell<Vec<OutputConfig>>>,
    target: EqBandTarget,
) {
    // Frequency row
    let freq_row = adw::ActionRow::builder()
        .title(&format!("{label} Freq"))
        .build();
    let freq_val_label = gtk::Label::builder()
        .label(&format!("{:.0} Hz", freq))
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

    let freq_label_clone = freq_val_label.clone();
    let freq_configs = output_configs.clone();
    freq_scale.connect_value_changed(move |s| {
        let val = s.value() as f32;
        freq_label_clone.set_label(&format!("{:.0} Hz", val));
        if let Some(cfg) = freq_configs.borrow_mut().get_mut(idx) {
            match target {
                EqBandTarget::Low => cfg.eq_low.freq = val,
                EqBandTarget::Mid => cfg.eq_mid.freq = val,
                EqBandTarget::High => cfg.eq_high.freq = val,
            }
        }
    });
    freq_row.add_suffix(&freq_scale);
    freq_row.add_suffix(&freq_val_label);
    group.add(&freq_row);

    // Gain row
    let gain_row = adw::ActionRow::builder()
        .title(&format!("{label} Gain"))
        .build();
    let gain_val_label = gtk::Label::builder()
        .label(&format!("{:.1} dB", gain))
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

    let gain_label_clone = gain_val_label.clone();
    let gain_configs = output_configs.clone();
    gain_scale.connect_value_changed(move |s| {
        let val = s.value() as f32;
        gain_label_clone.set_label(&format!("{:.1} dB", val));
        if let Some(cfg) = gain_configs.borrow_mut().get_mut(idx) {
            match target {
                EqBandTarget::Low => cfg.eq_low.gain = val,
                EqBandTarget::Mid => cfg.eq_mid.gain = val,
                EqBandTarget::High => cfg.eq_high.gain = val,
            }
        }
    });
    gain_row.add_suffix(&gain_scale);
    gain_row.add_suffix(&gain_val_label);
    group.add(&gain_row);
}

fn build_add_button(output_configs: &Rc<RefCell<Vec<OutputConfig>>>) -> gtk::Box {
    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .width_request(120)
        .valign(gtk::Align::Center)
        .build();

    let btn = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Virtual Output")
        .build();
    btn.add_css_class("suggested-action");
    btn.add_css_class("circular");

    let configs = output_configs.clone();
    btn.connect_clicked(move |b| {
        show_add_dialog(b, &configs);
    });

    col.append(&btn);
    col
}

fn show_add_dialog(btn: &gtk::Button, configs: &Rc<RefCell<Vec<OutputConfig>>>) {
    let dialog = gtk::Window::builder()
        .title("Create Virtual Output")
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
        .label("Internal name (\"zos-out-\" prefix added):")
        .halign(gtk::Align::Start)
        .build();
    content.append(&name_label);

    let name_entry = gtk::Entry::builder()
        .placeholder_text("headphones")
        .hexpand(true)
        .build();
    content.append(&name_entry);

    let desc_label = gtk::Label::builder()
        .label("Display name:")
        .halign(gtk::Align::Start)
        .build();
    content.append(&desc_label);

    let desc_entry = gtk::Entry::builder()
        .placeholder_text("A3")
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
        let full_name = if raw_name.starts_with("zos-out-") {
            raw_name
        } else {
            format!("zos-out-{raw_name}")
        };

        let new_output = OutputConfig {
            name: full_name,
            description,
            physical_device: String::new(),
            eq_enabled: false,
            eq_low: pipewire::EqBand { freq: 200.0, gain: 0.0 },
            eq_mid: pipewire::EqBand { freq: 1000.0, gain: 0.0 },
            eq_high: pipewire::EqBand { freq: 8000.0, gain: 0.0 },
        };
        configs_clone.borrow_mut().push(new_output);

        dialog_clone.close();
        add_btn.set_sensitive(false);
        add_btn.set_tooltip_text(Some("Created — click Apply"));
    });

    dialog.present();
}
