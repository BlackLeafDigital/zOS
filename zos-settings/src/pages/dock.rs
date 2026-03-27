// === pages/dock.rs — Dock configuration page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use std::fs;
use std::path::Path;

/// Read dock config from disk.
fn read_dock_config() -> serde_json::Value {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let path = Path::new(&home).join(".config/zos/dock.json");
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_else(|_| default_config())
    } else {
        default_config()
    }
}

fn default_config() -> serde_json::Value {
    serde_json::json!({
        "pinned": ["org.wezfurlong.wezterm", "org.mozilla.firefox", "org.kde.dolphin"],
        "icon_size": 48,
        "magnification": 1.6,
        "auto_hide": false
    })
}

fn save_dock_config(config: &serde_json::Value) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let path = Path::new(&home).join(".config/zos/dock.json");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = fs::write(&path, json);
    }
}

pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let config = read_dock_config();

    // --- Behavior ---
    let behavior_group = adw::PreferencesGroup::builder().title("Behavior").build();

    let auto_hide = config
        .get("auto_hide")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let auto_hide_row = adw::SwitchRow::builder()
        .title("Auto-hide")
        .subtitle("Hide the dock when not in use")
        .active(auto_hide)
        .build();
    behavior_group.add(&auto_hide_row);
    page.append(&behavior_group);

    // --- Appearance ---
    let appearance_group = adw::PreferencesGroup::builder().title("Appearance").build();

    let icon_size = config
        .get("icon_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(48) as f64;
    let icon_size_row = adw::ActionRow::builder()
        .title("Icon Size")
        .subtitle("Base icon size in pixels")
        .build();
    let icon_size_spin = gtk::SpinButton::with_range(32.0, 64.0, 4.0);
    icon_size_spin.set_value(icon_size);
    icon_size_spin.set_valign(gtk::Align::Center);
    icon_size_row.add_suffix(&icon_size_spin);
    appearance_group.add(&icon_size_row);

    let magnification = config
        .get("magnification")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.6);
    let mag_row = adw::ActionRow::builder()
        .title("Magnification")
        .subtitle("Hover magnification factor (1.0 = none)")
        .build();
    let mag_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 2.0, 0.1);
    mag_scale.set_value(magnification);
    mag_scale.set_width_request(200);
    mag_scale.set_valign(gtk::Align::Center);
    mag_scale.set_draw_value(true);
    mag_row.add_suffix(&mag_scale);
    appearance_group.add(&mag_row);

    page.append(&appearance_group);

    // --- Pinned Apps (read-only display) ---
    let pinned_group = adw::PreferencesGroup::builder()
        .title("Pinned Apps")
        .description("Right-click items in the dock to pin/unpin")
        .build();

    if let Some(pinned) = config.get("pinned").and_then(|v| v.as_array()) {
        for app_id in pinned {
            if let Some(id) = app_id.as_str() {
                let display_name = id.rsplit('.').next().unwrap_or(id);
                let mut chars = display_name.chars();
                let name = match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => id.to_string(),
                };
                let row = adw::ActionRow::builder().title(&name).subtitle(id).build();
                pinned_group.add(&row);
            }
        }
    }

    page.append(&pinned_group);

    // --- Apply Button ---
    let apply_btn = gtk::Button::builder()
        .label("Apply")
        .halign(gtk::Align::Center)
        .css_classes(["suggested-action", "pill"])
        .build();

    let auto_hide_row_clone = auto_hide_row.clone();
    let icon_size_spin_clone = icon_size_spin.clone();
    let mag_scale_clone = mag_scale.clone();

    apply_btn.connect_clicked(move |btn| {
        let mut config = read_dock_config();
        config["auto_hide"] = serde_json::Value::Bool(auto_hide_row_clone.is_active());
        config["icon_size"] = serde_json::Value::Number(serde_json::Number::from(
            icon_size_spin_clone.value() as u64,
        ));
        // Round magnification to 1 decimal place
        let mag_val = (mag_scale_clone.value() * 10.0).round() / 10.0;
        config["magnification"] = serde_json::Value::from(mag_val);
        save_dock_config(&config);
        btn.set_label("Applied");
    });

    page.append(&apply_btn);

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
