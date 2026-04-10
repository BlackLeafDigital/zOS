// === audio.rs — Voicemeeter-style audio mixer page ===
//
// Presents a virtual mixing console with input strips (per active audio stream)
// on the left and output bus cards on the right. Stream routing, bus volumes,
// mute toggles, physical device selection, and gain are all managed here.

use std::collections::HashMap;

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text, Space};
use iced::{Background, Border, Element, Length, Task};

use crate::services::pipewire::{self, AudioBusConfig, AudioDevice, EqBand};
use crate::services::pipewire_native::{self, AppStream};
use crate::theme;

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Re-read active streams from PipeWire.
    RefreshStreams,
    /// Route an app stream to a specific bus (radio-style single-select).
    SetAppRoute { app_name: String, bus_name: String },
    /// Set the master volume for a bus (0.0 to 1.5).
    SetBusVolume { index: usize, volume: f32 },
    /// Toggle mute on a bus.
    ToggleBusMute(usize),
    /// Change the physical output device for a bus.
    SetBusDevice { index: usize, device: String },
    /// Change the gain for a bus (-20 to +20 dB).
    SetBusGain { index: usize, gain: f32 },
    /// Append a new default bus.
    AddBus,
    /// Delete a bus by index.
    DeleteBus(usize),
    /// Commit all bus configs and routing to PipeWire.
    Apply,
}

// ---------------------------------------------------------------------------
// AudioPage state
// ---------------------------------------------------------------------------

pub struct AudioPage {
    /// Persisted bus configurations.
    buses: Vec<AudioBusConfig>,
    /// Per-bus master volume (0.0 to 1.5, 1.0 = unity).
    bus_volumes: Vec<f32>,
    /// Per-bus mute state.
    bus_muted: Vec<bool>,
    /// Currently active application audio streams.
    streams: Vec<AppStream>,
    /// Per-app routing: app_name -> bus node name (e.g. "zos-main").
    app_routes: HashMap<String, String>,
    /// Available physical output sinks.
    physical_sinks: Vec<AudioDevice>,
}

impl AudioPage {
    pub fn new() -> Self {
        let buses = pipewire::load_audio_bus_configs();
        let bus_count = buses.len();
        let streams = pipewire_native::list_app_streams();
        let app_routes = pipewire::load_app_routing_defaults();
        let physical_sinks = pipewire::list_physical_sinks();

        Self {
            buses,
            bus_volumes: vec![1.0; bus_count],
            bus_muted: vec![false; bus_count],
            streams,
            app_routes,
            physical_sinks,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RefreshStreams => {
                self.streams = pipewire_native::list_app_streams();
                self.physical_sinks = pipewire::list_physical_sinks();
            }
            Message::SetAppRoute { app_name, bus_name } => {
                // Radio-style: if already assigned to this bus, unassign.
                if self.app_routes.get(&app_name) == Some(&bus_name) {
                    self.app_routes.remove(&app_name);
                    pipewire::save_app_routing_default(&app_name, "");
                } else {
                    self.app_routes.insert(app_name.clone(), bus_name.clone());
                    pipewire::save_app_routing_default(&app_name, &bus_name);
                }
            }
            Message::SetBusVolume { index, volume } => {
                if let Some(v) = self.bus_volumes.get_mut(index) {
                    *v = volume;
                }
            }
            Message::ToggleBusMute(index) => {
                if let Some(m) = self.bus_muted.get_mut(index) {
                    *m = !*m;
                }
            }
            Message::SetBusDevice { index, device } => {
                if let Some(bus) = self.buses.get_mut(index) {
                    bus.physical_device = device;
                }
            }
            Message::SetBusGain { index, gain } => {
                if let Some(bus) = self.buses.get_mut(index) {
                    bus.gain = gain;
                }
            }
            Message::AddBus => {
                let n = self.buses.len() + 1;
                let default_device = self
                    .physical_sinks
                    .first()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                self.buses.push(AudioBusConfig {
                    name: format!("zos-bus-{n}"),
                    description: format!("Bus {n}"),
                    gain: 0.0,
                    physical_device: default_device,
                    eq_enabled: false,
                    eq_low: EqBand {
                        freq: 200.0,
                        gain: 0.0,
                    },
                    eq_mid: EqBand {
                        freq: 1000.0,
                        gain: 0.0,
                    },
                    eq_high: EqBand {
                        freq: 8000.0,
                        gain: 0.0,
                    },
                });
                self.bus_volumes.push(1.0);
                self.bus_muted.push(false);
            }
            Message::DeleteBus(index) => {
                if index < self.buses.len() && self.buses.len() > 1 {
                    let removed_name = self.buses[index].name.clone();
                    self.buses.remove(index);
                    self.bus_volumes.remove(index);
                    self.bus_muted.remove(index);
                    // Remove any routes pointing to the deleted bus.
                    self.app_routes.retain(|_, v| *v != removed_name);
                }
            }
            Message::Apply => {
                // Apply volume / mute via the native PipeWire client if available.
                if let Some(client) = pipewire_native::global_client() {
                    for (i, bus) in self.buses.iter().enumerate() {
                        let vol = self.bus_volumes.get(i).copied().unwrap_or(1.0);
                        let muted = self.bus_muted.get(i).copied().unwrap_or(false);
                        // Best-effort: ignore errors from volume setting.
                        let _ = client.set_node_volume(0, vol, muted);
                        // The node id 0 above is a placeholder -- we need to
                        // look up the real node id. For now, use the CLI path.
                        let _ = bus;
                    }
                }
                // Persist and apply bus configs through the CLI path.
                pipewire::save_audio_bus_configs(&self.buses);
                pipewire::apply_audio_bus_config(&self.buses);
                // Route each stream to its assigned bus.
                for stream in &self.streams {
                    if let Some(bus_name) = self.app_routes.get(&stream.app_name) {
                        pipewire::route_stream_to_sink(stream.id, bus_name);
                    }
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // -- Title --
        let title = text("Audio Mixer").size(28).color(theme::TEXT);

        // -- Refresh button --
        let refresh_btn = button(text("Refresh Streams").size(13).color(theme::BASE))
            .on_press(Message::RefreshStreams)
            .style(|_theme, _status| button::Style {
                background: Some(Background::Color(theme::BLUE)),
                text_color: theme::BASE,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

        let header = row![title, Space::new().width(Length::Fill), refresh_btn]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // -- Input strips (left) --
        let input_section = self.view_input_strips();

        // -- Output buses (right) --
        let output_section = self.view_output_buses();

        let main_area = row![
            container(input_section).width(Length::FillPortion(3)),
            Space::new().width(16),
            container(output_section).width(Length::FillPortion(2)),
        ]
        .height(Length::Fill);

        // -- Apply button --
        let apply_btn = button(text("Apply").size(15).color(theme::BASE))
            .on_press(Message::Apply)
            .padding([10, 32])
            .style(|_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme::GREEN,
                    _ => theme::BLUE,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: theme::BASE,
                    border: Border {
                        radius: 10.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });

        column![header, main_area, apply_btn]
            .spacing(16)
            .padding(4)
            .height(Length::Fill)
            .into()
    }

    // -- Input strips --------------------------------------------------------

    fn view_input_strips(&self) -> Element<'_, Message> {
        let label = text("Input Strips").size(18).color(theme::TEXT);

        if self.streams.is_empty() {
            let empty_msg = container(
                text("No audio streams \u{2014} play something to route it here")
                    .size(14)
                    .color(theme::OVERLAY0),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

            return column![label, empty_msg]
                .spacing(12)
                .height(Length::Fill)
                .into();
        }

        let mut strips_row = row![].spacing(12);

        for stream in &self.streams {
            strips_row = strips_row.push(self.view_stream_strip(stream));
        }

        let scrollable_strips = scrollable(strips_row)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .width(Length::Fill);

        column![label, scrollable_strips]
            .spacing(12)
            .height(Length::Fill)
            .into()
    }

    fn view_stream_strip<'a>(&'a self, stream: &'a AppStream) -> Element<'a, Message> {
        // Icon / app name
        let icon_label = text(stream.icon_name.as_deref().unwrap_or("audio-volume-high"))
            .size(11)
            .color(theme::OVERLAY0);

        let app_name_label = text(&stream.app_name).size(14).color(theme::TEXT);

        // Media subtitle
        let media_label: Element<'_, Message> = match &stream.media_name {
            Some(name) => text(truncate_str(name, 24))
                .size(11)
                .color(theme::SUBTEXT0)
                .into(),
            None => Space::new().into(),
        };

        // Volume slider (horizontal, 0..=1.5)
        // Streams don't have their own volume in our model; they route to buses.
        // Show a placeholder label instead.
        let vol_label = text("Route to bus:").size(11).color(theme::SUBTEXT0);

        // Bus routing buttons (radio-style)
        let current_route = self.app_routes.get(&stream.app_name);
        let mut bus_buttons = row![].spacing(4);

        for bus in &self.buses {
            let is_active = current_route == Some(&bus.name);
            let bus_name = bus.name.clone();
            let app_name = stream.app_name.clone();

            let btn_label = text(&bus.description).size(11).color(if is_active {
                theme::BASE
            } else {
                theme::TEXT
            });

            let btn = button(btn_label)
                .on_press(Message::SetAppRoute { app_name, bus_name })
                .padding([4, 8])
                .style(move |_theme, status| {
                    let bg = if is_active {
                        theme::BLUE
                    } else {
                        match status {
                            button::Status::Hovered => theme::SURFACE2,
                            _ => theme::SURFACE1,
                        }
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: if is_active { theme::BASE } else { theme::TEXT },
                        border: Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });

            bus_buttons = bus_buttons.push(btn);
        }

        // "Unassigned" indicator
        let route_status: Element<'_, Message> = if current_route.is_none() {
            text("Unassigned").size(10).color(theme::YELLOW).into()
        } else {
            Space::new().into()
        };

        let card_content = column![
            icon_label,
            app_name_label,
            media_label,
            Space::new().height(8),
            vol_label,
            bus_buttons,
            route_status,
        ]
        .spacing(4)
        .width(Length::Fixed(160.0));

        container(card_content)
            .padding(12)
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme::SURFACE0)),
                border: Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }

    // -- Output buses --------------------------------------------------------

    fn view_output_buses(&self) -> Element<'_, Message> {
        let label = text("Output Buses").size(18).color(theme::TEXT);

        let mut bus_cards = column![].spacing(12);

        for (i, bus) in self.buses.iter().enumerate() {
            bus_cards = bus_cards.push(self.view_bus_card(i, bus));
        }

        // Add Bus button
        let add_btn = button(text("+ Add Bus").size(13).color(theme::BASE))
            .on_press(Message::AddBus)
            .padding([8, 16])
            .style(|_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme::MAUVE,
                    _ => theme::BLUE,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: theme::BASE,
                    border: Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });

        let all_content = column![label, bus_cards, add_btn].spacing(12);

        scrollable(all_content).height(Length::Fill).into()
    }

    fn view_bus_card<'a>(&'a self, index: usize, bus: &'a AudioBusConfig) -> Element<'a, Message> {
        let volume = self.bus_volumes.get(index).copied().unwrap_or(1.0);
        let muted = self.bus_muted.get(index).copied().unwrap_or(false);

        // Bus name
        let name_label = text(&bus.description).size(16).color(theme::TEXT);

        // Volume slider
        let vol_pct = (volume * 100.0) as u32;
        let vol_text = text(format!("Volume: {vol_pct}%"))
            .size(12)
            .color(theme::SUBTEXT0);

        let vol_slider = slider(0.0..=1.5_f32, volume, move |v| Message::SetBusVolume {
            index,
            volume: v,
        })
        .width(Length::Fill);

        // Mute toggle
        let mute_label = if muted { "Unmute" } else { "Mute" };
        let mute_color = if muted { theme::RED } else { theme::SURFACE1 };
        let mute_text_color = if muted { theme::BASE } else { theme::TEXT };
        let mute_btn = button(text(mute_label).size(12).color(mute_text_color))
            .on_press(Message::ToggleBusMute(index))
            .padding([4, 12])
            .style(move |_theme, status| {
                let bg = if muted {
                    match status {
                        button::Status::Hovered => theme::PEACH,
                        _ => mute_color,
                    }
                } else {
                    match status {
                        button::Status::Hovered => theme::SURFACE2,
                        _ => mute_color,
                    }
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: mute_text_color,
                    border: Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });

        // Physical device picker
        let device_names: Vec<String> =
            self.physical_sinks.iter().map(|s| s.name.clone()).collect();

        let selected_device = if bus.physical_device.is_empty() {
            None
        } else {
            Some(bus.physical_device.clone())
        };

        let device_label = text("Output device:").size(12).color(theme::SUBTEXT0);

        let device_picker = pick_list(device_names, selected_device, move |dev| {
            Message::SetBusDevice { index, device: dev }
        })
        .width(Length::Fill)
        .text_size(12.0);

        // Gain slider (-20 to +20 dB)
        let gain_text = text(format!("Gain: {:.1} dB", bus.gain))
            .size(12)
            .color(theme::SUBTEXT0);

        let gain_slider = slider(-20.0..=20.0_f32, bus.gain, move |g| Message::SetBusGain {
            index,
            gain: g,
        })
        .width(Length::Fill);

        // Delete button (only if more than one bus)
        let delete_section: Element<'_, Message> = if self.buses.len() > 1 {
            button(text("Delete Bus").size(12).color(theme::RED))
                .on_press(Message::DeleteBus(index))
                .padding([4, 12])
                .style(|_theme, status| {
                    let bg = match status {
                        button::Status::Hovered => {
                            let mut c = theme::RED;
                            c.a = 0.2;
                            c
                        }
                        _ => theme::SURFACE1,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: theme::RED,
                        border: Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
                .into()
        } else {
            Space::new().into()
        };

        let card_content = column![
            name_label,
            vol_text,
            vol_slider,
            row![mute_btn, Space::new().width(Length::Fill)].spacing(8),
            Space::new().height(4),
            device_label,
            device_picker,
            Space::new().height(4),
            gain_text,
            gain_slider,
            Space::new().height(4),
            delete_section,
        ]
        .spacing(4);

        container(card_content)
            .padding(12)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme::SURFACE0)),
                border: Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a valid char boundary at or before max_len.
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
