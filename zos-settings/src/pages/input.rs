// === pages/input.rs — Keyboard & Mouse/Touchpad settings page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use std::sync::{Arc, Mutex};

use crate::services::hyprctl;

/// Shared input settings state for persistence.
struct InputState {
    layout: String,
    repeat_rate: i32,
    repeat_delay: i32,
    sensitivity: f64,
    accel_flat: bool,
    natural_scroll: bool,
    tap_to_click: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            layout: "us".to_string(),
            repeat_rate: 25,
            repeat_delay: 600,
            sensitivity: 0.0,
            accel_flat: false,
            natural_scroll: true,
            tap_to_click: true,
        }
    }
}

/// Write all input settings to ~/.config/hypr/user-settings.conf for persistence.
fn persist_input_settings(
    layout: &str,
    repeat_rate: i32,
    repeat_delay: i32,
    sensitivity: f64,
    accel_flat: bool,
    natural_scroll: bool,
    tap_to_click: bool,
) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{}/.config/hypr/user-settings.conf", home);
    let accel_profile = if accel_flat { "flat" } else { "adaptive" };
    let content = format!(
        "# zOS Input Settings — managed by zos-settings\n\
         input {{\n    \
         kb_layout = {}\n    \
         repeat_rate = {}\n    \
         repeat_delay = {}\n    \
         sensitivity = {:.1}\n    \
         accel_profile = {}\n    \
         touchpad {{\n        \
         natural_scroll = {}\n        \
         tap-to-click = {}\n    \
         }}\n\
         }}\n",
        layout, repeat_rate, repeat_delay, sensitivity, accel_profile, natural_scroll, tap_to_click
    );
    let _ = std::fs::write(&path, content);
}

/// Helper to persist the current shared state.
fn persist_state(state: &Arc<Mutex<InputState>>) {
    let s = state.lock().unwrap();
    persist_input_settings(
        &s.layout,
        s.repeat_rate,
        s.repeat_delay,
        s.sensitivity,
        s.accel_flat,
        s.natural_scroll,
        s.tap_to_click,
    );
}

/// Build the input settings page widget.
pub fn build() -> gtk::Box {
    let state = Arc::new(Mutex::new(InputState::default()));

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    page.append(&build_keyboard_section(Arc::clone(&state)));
    page.append(&build_mouse_section(Arc::clone(&state)));
    page.append(&build_touchpad_section(Arc::clone(&state)));

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
// Keyboard section
// ---------------------------------------------------------------------------

fn build_keyboard_section(state: Arc<Mutex<InputState>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Keyboard").build();

    let layout_options = ["us", "gb", "de", "fr", "es", "it", "pt", "ru", "jp", "kr"];
    let layout_model = gtk::StringList::new(&layout_options);

    let layout_combo = adw::ComboRow::builder()
        .title("Layout")
        .model(&layout_model)
        .selected(0) // default: us
        .build();

    {
        let state = Arc::clone(&state);
        layout_combo.connect_selected_notify(move |row| {
            let idx = row.selected() as usize;
            if let Some(&layout) = layout_options.get(idx) {
                hyprctl::keyword("input:kb_layout", layout);
                state.lock().unwrap().layout = layout.to_string();
                persist_state(&state);
            }
        });
    }
    group.add(&layout_combo);

    // --- Repeat rate ---
    let rate_row = adw::ActionRow::builder()
        .title("Repeat Rate")
        .subtitle("Characters per second")
        .build();

    let rate_adj = gtk::Adjustment::new(25.0, 10.0, 50.0, 1.0, 5.0, 0.0);
    let rate_spin = gtk::SpinButton::builder()
        .adjustment(&rate_adj)
        .valign(gtk::Align::Center)
        .build();

    {
        let state = Arc::clone(&state);
        rate_spin.connect_value_changed(move |spin| {
            let val = spin.value() as i32;
            hyprctl::keyword("input:repeat_rate", &val.to_string());
            state.lock().unwrap().repeat_rate = val;
            persist_state(&state);
        });
    }

    rate_row.add_suffix(&rate_spin);
    group.add(&rate_row);

    // --- Repeat delay ---
    let delay_row = adw::ActionRow::builder()
        .title("Repeat Delay")
        .subtitle("Milliseconds before repeat starts")
        .build();

    let delay_adj = gtk::Adjustment::new(600.0, 100.0, 1000.0, 50.0, 100.0, 0.0);
    let delay_spin = gtk::SpinButton::builder()
        .adjustment(&delay_adj)
        .valign(gtk::Align::Center)
        .build();

    {
        let state = Arc::clone(&state);
        delay_spin.connect_value_changed(move |spin| {
            let val = spin.value() as i32;
            hyprctl::keyword("input:repeat_delay", &val.to_string());
            state.lock().unwrap().repeat_delay = val;
            persist_state(&state);
        });
    }

    delay_row.add_suffix(&delay_spin);
    group.add(&delay_row);

    group
}

// ---------------------------------------------------------------------------
// Mouse section
// ---------------------------------------------------------------------------

fn build_mouse_section(state: Arc<Mutex<InputState>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Mouse").build();

    // --- Sensitivity slider ---
    let sensitivity_row = adw::ActionRow::builder().title("Sensitivity").build();

    let sens_adj = gtk::Adjustment::new(0.0, -1.0, 1.0, 0.1, 0.5, 0.0);
    let sens_scale = gtk::Scale::builder()
        .adjustment(&sens_adj)
        .orientation(gtk::Orientation::Horizontal)
        .draw_value(true)
        .digits(1)
        .hexpand(true)
        .width_request(200)
        .valign(gtk::Align::Center)
        .build();
    sens_scale.add_mark(-1.0, gtk::PositionType::Bottom, Some("Slow"));
    sens_scale.add_mark(0.0, gtk::PositionType::Bottom, Some("Default"));
    sens_scale.add_mark(1.0, gtk::PositionType::Bottom, Some("Fast"));

    {
        let state = Arc::clone(&state);
        sens_scale.connect_value_changed(move |scale| {
            let val = scale.value();
            hyprctl::keyword("input:sensitivity", &format!("{:.1}", val));
            state.lock().unwrap().sensitivity = val;
            persist_state(&state);
        });
    }

    sensitivity_row.add_suffix(&sens_scale);
    group.add(&sensitivity_row);

    // --- Acceleration profile ---
    let accel_row = adw::ActionRow::builder()
        .title("Flat Acceleration")
        .subtitle("Off = adaptive (accelerated), On = flat (raw)")
        .build();

    let accel_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(false)
        .build();

    {
        let state = Arc::clone(&state);
        accel_switch.connect_active_notify(move |sw| {
            let is_flat = sw.is_active();
            let profile = if is_flat { "flat" } else { "adaptive" };
            hyprctl::keyword("input:accel_profile", profile);
            state.lock().unwrap().accel_flat = is_flat;
            persist_state(&state);
        });
    }

    accel_row.add_suffix(&accel_switch);
    accel_row.set_activatable_widget(Some(&accel_switch));
    group.add(&accel_row);

    group
}

// ---------------------------------------------------------------------------
// Touchpad section
// ---------------------------------------------------------------------------

fn build_touchpad_section(state: Arc<Mutex<InputState>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Touchpad").build();

    // --- Natural scroll ---
    let natural_row = adw::ActionRow::builder()
        .title("Natural Scroll")
        .subtitle("Scroll direction follows content")
        .build();

    let natural_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(true) // default from defaults.conf
        .build();

    {
        let state = Arc::clone(&state);
        natural_switch.connect_active_notify(move |sw| {
            let active = sw.is_active();
            let val = if active { "true" } else { "false" };
            hyprctl::keyword("input:touchpad:natural_scroll", val);
            state.lock().unwrap().natural_scroll = active;
            persist_state(&state);
        });
    }

    natural_row.add_suffix(&natural_switch);
    natural_row.set_activatable_widget(Some(&natural_switch));
    group.add(&natural_row);

    // --- Tap to click ---
    let tap_row = adw::ActionRow::builder()
        .title("Tap to Click")
        .subtitle("Tap on the touchpad to click")
        .build();

    let tap_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(true) // default from defaults.conf
        .build();

    {
        let state = Arc::clone(&state);
        tap_switch.connect_active_notify(move |sw| {
            let active = sw.is_active();
            let val = if active { "true" } else { "false" };
            hyprctl::keyword("input:touchpad:tap-to-click", val);
            state.lock().unwrap().tap_to_click = active;
            persist_state(&state);
        });
    }

    tap_row.add_suffix(&tap_switch);
    tap_row.set_activatable_widget(Some(&tap_switch));
    group.add(&tap_row);

    group
}
