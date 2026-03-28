// === pages/audio/input.rs — Input device section ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire::{self, DeviceType};

use super::icon_for_device_type;

/// Build the input devices preferences group.
pub fn build() -> adw::PreferencesGroup {
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
        .model(&model)
        .selected(default_idx)
        .build();
    let combo_icon = gtk::Image::from_icon_name(device_icon);
    combo_icon.set_valign(gtk::Align::Center);
    combo.add_prefix(&combo_icon);

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
