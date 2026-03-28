// === widgets.rs — Reusable UI widget helpers ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

/// Build a volume slider row with a label showing the current percentage.
///
/// The scale ranges from 0.0 to 1.5 (150%). When the user drags the slider,
/// `on_change` is called with the new value and the percentage label updates.
pub fn volume_row(
    title: &str,
    current_vol: f32,
    on_change: impl Fn(f32) + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();

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
        on_change(val);
        vol_label_clone.set_label(&format!("{}%", (val * 100.0).round() as i32));
    });

    row.add_suffix(&scale);
    row.add_suffix(&vol_label);
    row
}

/// Build an `ActionRow` with a leading icon, title, and subtitle.
pub fn icon_action_row(title: &str, subtitle: &str, icon_name: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_valign(gtk::Align::Center);
    row.add_prefix(&icon);
    row
}

/// Build a stat card: a vertical box with a large value label and a description.
pub fn stat_card(value: &str, value_class: &str, label_text: &str) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Fill)
        .spacing(4)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();
    card.add_css_class("dashboard-stat-card");

    let value_label = gtk::Label::builder().label(value).build();
    value_label.add_css_class(value_class);

    let desc_label = gtk::Label::builder().label(label_text).build();
    desc_label.add_css_class("dashboard-stat-label");

    card.append(&value_label);
    card.append(&desc_label);
    card
}
