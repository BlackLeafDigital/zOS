// === pages/audio/routing.rs — App streams section (left column) ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use crate::services::pipewire;

use super::friendly_bus_name;

/// Build the app streams preferences group for the left column.
/// Shows active audio streams with bus routing dropdowns and "remember" toggles.
pub fn build() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("App Streams")
        .description("Route app audio to a virtual bus")
        .build();

    let streams = pipewire::list_streams();
    let saved_defaults = pipewire::load_app_routing_defaults();
    let bus_configs = pipewire::load_bus_configs();
    let all_sinks = pipewire::list_sinks();

    if streams.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No active streams")
            .subtitle("Play audio in an app to see it here")
            .build();
        let empty_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        empty_icon.set_valign(gtk::Align::Center);
        empty_row.add_prefix(&empty_icon);
        group.add(&empty_row);
        return group;
    }

    // Build dropdown options: "Default", then each zos-* sink
    let mut dropdown_labels: Vec<String> = vec!["Default".into()];
    let mut dropdown_sink_names: Vec<String> = vec![String::new()];

    let zos_sinks: Vec<_> = all_sinks
        .iter()
        .filter(|s| s.name.starts_with("zos-"))
        .collect();

    for zos_sink in &zos_sinks {
        let label = friendly_bus_name(&zos_sink.name, &bus_configs);
        dropdown_labels.push(label);
        dropdown_sink_names.push(zos_sink.name.clone());
    }

    let label_strs: Vec<&str> = dropdown_labels.iter().map(|s| s.as_str()).collect();

    for stream in &streams {
        let row = adw::ActionRow::builder()
            .title(&stream.name)
            .build();

        let stream_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
        stream_icon.set_valign(gtk::Align::Center);
        row.add_prefix(&stream_icon);

        let model = gtk::StringList::new(&label_strs);
        let dropdown = gtk::DropDown::builder()
            .model(&model)
            .selected(0)
            .valign(gtk::Align::Center)
            .build();

        // Pre-select based on saved defaults
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

    group
}
