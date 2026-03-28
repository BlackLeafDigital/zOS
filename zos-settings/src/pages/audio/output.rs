// === pages/audio/output.rs — Output device section ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, DeviceType};

use super::icon_for_device_type;

/// Build the output devices preferences group.
pub fn build() -> adw::PreferencesGroup {
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

    let device_icon = sinks
        .first()
        .map(|s| icon_for_device_type(&s.device_type))
        .unwrap_or("audio-speakers-symbolic");
    let combo = adw::ComboRow::builder()
        .title("Device")
        .model(&model)
        .selected(default_idx)
        .build();
    let combo_icon = gtk::Image::from_icon_name(device_icon);
    combo_icon.set_valign(gtk::Align::Center);
    combo.add_prefix(&combo_icon);

    let sinks_for_combo = sinks.clone();
    combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(sink) = sinks_for_combo.get(idx) {
            pipewire::set_default(sink.id);
        }
    });
    group.add(&combo);

    // --- Volume slider ---
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
