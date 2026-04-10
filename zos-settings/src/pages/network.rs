// === network.rs — Network settings page ===
//
// Shows IP details, active connections, available WiFi networks (with
// connect / disconnect / rescan), a WiFi radio toggle, and device listing.
// All data comes from nmcli via `crate::services::network`.

use std::collections::HashMap;
use std::process::Command;

use iced::widget::{button, column, container, row, scrollable, text, text_input, toggler, Space};
use iced::{Background, Border, Element, Length, Task};

use crate::services::network;
use crate::theme;

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Reload all network state from nmcli.
    Refresh,
    /// Rescan WiFi networks specifically.
    RescanWifi,
    /// Toggle WiFi radio on/off.
    ToggleWifi(bool),
    /// Disconnect an active connection by name.
    Disconnect(String),
    /// Begin connecting to a WiFi SSID (open networks connect immediately).
    ConnectWifi(String),
    /// Password field changed for a given SSID.
    PasswordChanged { ssid: String, value: String },
    /// Submit the password form and connect to the secured network.
    SubmitPassword(String),
}

// ---------------------------------------------------------------------------
// NetworkPage state
// ---------------------------------------------------------------------------

pub struct NetworkPage {
    /// IP address, gateway, DNS for the primary device.
    ip_details: (String, String, String),
    /// Active connections: (name, type, device).
    active_connections: Vec<(String, String, String)>,
    /// Available WiFi networks: (ssid, signal_percent, security, in_use).
    wifi_networks: Vec<(String, String, String, bool)>,
    /// Network devices: (name, type, state, connection, signal).
    devices: Vec<(String, String, String, String, Option<u32>)>,
    /// Whether WiFi radio is currently enabled.
    wifi_enabled: bool,
    /// Per-SSID password input state (for secured networks).
    password_input: HashMap<String, String>,
}

impl NetworkPage {
    pub fn new() -> Self {
        let wifi_enabled = read_wifi_enabled();

        Self {
            ip_details: network::get_ip_details(),
            active_connections: network::get_active_connections(),
            wifi_networks: network::get_wifi_networks(),
            devices: network::get_devices(),
            wifi_enabled,
            password_input: HashMap::new(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => {
                self.ip_details = network::get_ip_details();
                self.active_connections = network::get_active_connections();
                self.wifi_networks = network::get_wifi_networks();
                self.devices = network::get_devices();
                self.wifi_enabled = read_wifi_enabled();
            }
            Message::RescanWifi => {
                // Trigger a rescan then reload the list.
                let _ = Command::new("nmcli")
                    .args(["device", "wifi", "rescan"])
                    .status();
                self.wifi_networks = network::get_wifi_networks();
            }
            Message::ToggleWifi(on) => {
                let arg = if on { "on" } else { "off" };
                let _ = Command::new("nmcli").args(["radio", "wifi", arg]).status();
                self.wifi_enabled = on;
                // Refresh the network list after toggling.
                self.wifi_networks = network::get_wifi_networks();
                self.active_connections = network::get_active_connections();
                self.ip_details = network::get_ip_details();
            }
            Message::Disconnect(name) => {
                let _ = Command::new("nmcli")
                    .args(["connection", "down", &name])
                    .status();
                // Refresh after disconnect.
                self.active_connections = network::get_active_connections();
                self.ip_details = network::get_ip_details();
                self.wifi_networks = network::get_wifi_networks();
            }
            Message::ConnectWifi(ssid) => {
                // Check if the network requires a password by looking up its security.
                let needs_password = self
                    .wifi_networks
                    .iter()
                    .any(|(s, _, sec, _)| s == &ssid && !sec.is_empty() && sec != "--");

                if needs_password {
                    // Open password entry -- insert empty string if not already present.
                    self.password_input.entry(ssid).or_default();
                } else {
                    // Open network -- connect directly.
                    network::connect_wifi(&ssid, None);
                    self.active_connections = network::get_active_connections();
                    self.ip_details = network::get_ip_details();
                    self.wifi_networks = network::get_wifi_networks();
                }
            }
            Message::PasswordChanged { ssid, value } => {
                self.password_input.insert(ssid, value);
            }
            Message::SubmitPassword(ssid) => {
                let password = self.password_input.remove(&ssid).unwrap_or_default();
                let pw = if password.is_empty() {
                    None
                } else {
                    Some(password.as_str())
                };
                network::connect_wifi(&ssid, pw);
                self.active_connections = network::get_active_connections();
                self.ip_details = network::get_ip_details();
                self.wifi_networks = network::get_wifi_networks();
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // -- Header --
        let title = text("Network").size(28).color(theme::TEXT);

        let refresh_btn = accent_button("Refresh", Message::Refresh);

        let header = row![title, Space::new().width(Length::Fill), refresh_btn]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // -- Sections --
        let ip_card = self.view_ip_details();
        let active_section = self.view_active_connections();
        let wifi_section = self.view_wifi_networks();
        let devices_section = self.view_devices();

        let content = column![
            header,
            ip_card,
            active_section,
            wifi_section,
            devices_section,
        ]
        .spacing(16)
        .padding(4);

        scrollable(content)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    }

    // -- IP Details card ------------------------------------------------------

    fn view_ip_details(&self) -> Element<'_, Message> {
        let (ip, gateway, dns) = &self.ip_details;

        let heading = text("IP Details").size(18).color(theme::TEXT);

        let ip_row = detail_row("IP Address", ip);
        let gw_row = detail_row("Gateway", gateway);
        let dns_row = detail_row("DNS", dns);

        let content = column![heading, ip_row, gw_row, dns_row].spacing(6);

        card(content)
    }

    // -- Active connections ---------------------------------------------------

    fn view_active_connections(&self) -> Element<'_, Message> {
        let heading = text("Active Connections").size(18).color(theme::TEXT);

        if self.active_connections.is_empty() {
            let empty = text("No active connections")
                .size(13)
                .color(theme::OVERLAY0);
            return card(column![heading, empty].spacing(8));
        }

        let mut rows = column![].spacing(6);

        for (name, conn_type, device) in &self.active_connections {
            let info = text(format!("{name}  ({conn_type} on {device})"))
                .size(13)
                .color(theme::TEXT);

            let disconnect_btn = small_button("Disconnect", theme::RED, {
                let name = name.clone();
                Message::Disconnect(name)
            });

            let r = row![info, Space::new().width(Length::Fill), disconnect_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center);

            rows = rows.push(r);
        }

        card(column![heading, rows].spacing(8))
    }

    // -- WiFi networks --------------------------------------------------------

    fn view_wifi_networks(&self) -> Element<'_, Message> {
        let heading = text("WiFi Networks").size(18).color(theme::TEXT);

        // WiFi toggle
        let wifi_toggle = toggler(self.wifi_enabled)
            .on_toggle(Message::ToggleWifi)
            .size(20.0);

        let toggle_label = text(if self.wifi_enabled {
            "WiFi On"
        } else {
            "WiFi Off"
        })
        .size(13)
        .color(theme::SUBTEXT0);

        let rescan_btn = accent_button("Rescan", Message::RescanWifi);

        let controls = row![
            toggle_label,
            wifi_toggle,
            Space::new().width(Length::Fill),
            rescan_btn,
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        if !self.wifi_enabled || self.wifi_networks.is_empty() {
            let empty_text = if !self.wifi_enabled {
                "WiFi radio is disabled"
            } else {
                "No WiFi networks found"
            };
            let empty = text(empty_text).size(13).color(theme::OVERLAY0);
            return card(column![heading, controls, empty].spacing(8));
        }

        let mut network_rows = column![].spacing(6);

        for (ssid, signal, security, in_use) in &self.wifi_networks {
            // Skip empty SSIDs (hidden networks).
            if ssid.is_empty() {
                continue;
            }

            let signal_icon = signal_indicator(signal);

            let ssid_label = text(ssid).size(13).color(theme::TEXT);

            let sec_label = text(if security.is_empty() || security == "--" {
                "Open"
            } else {
                security.as_str()
            })
            .size(11)
            .color(theme::SUBTEXT0);

            let connected_badge: Element<'_, Message> = if *in_use {
                text("Connected").size(11).color(theme::GREEN).into()
            } else {
                Space::new().into()
            };

            let action: Element<'_, Message> = if *in_use {
                // Already connected -- no connect button needed.
                Space::new().into()
            } else {
                let ssid_clone = ssid.clone();
                small_button("Connect", theme::BLUE, Message::ConnectWifi(ssid_clone))
            };

            let main_row = row![
                signal_icon,
                ssid_label,
                sec_label,
                connected_badge,
                Space::new().width(Length::Fill),
                action,
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center);

            network_rows = network_rows.push(main_row);

            // Password input row (if user clicked Connect on a secured network).
            if let Some(password) = self.password_input.get(ssid) {
                let ssid_for_input = ssid.clone();
                let ssid_for_submit = ssid.clone();

                let pw_input = text_input("Password", password)
                    .on_input(move |s| Message::PasswordChanged {
                        ssid: ssid_for_input.clone(),
                        value: s,
                    })
                    .on_submit(Message::SubmitPassword(ssid_for_submit.clone()))
                    .secure(true)
                    .size(13.0)
                    .width(Length::Fixed(220.0));

                let submit_btn = small_button(
                    "Join",
                    theme::GREEN,
                    Message::SubmitPassword(ssid_for_submit),
                );

                let pw_row = row![Space::new().width(28), pw_input, submit_btn,]
                    .spacing(8)
                    .align_y(iced::Alignment::Center);

                network_rows = network_rows.push(pw_row);
            }
        }

        card(column![heading, controls, network_rows].spacing(8))
    }

    // -- Devices --------------------------------------------------------------

    fn view_devices(&self) -> Element<'_, Message> {
        let heading = text("Devices").size(18).color(theme::TEXT);

        if self.devices.is_empty() {
            let empty = text("No network devices found")
                .size(13)
                .color(theme::OVERLAY0);
            return card(column![heading, empty].spacing(8));
        }

        let mut rows = column![].spacing(6);

        for (name, dev_type, state, connection, signal) in &self.devices {
            let state_color = if state.contains("connected") {
                theme::GREEN
            } else {
                theme::OVERLAY0
            };

            let name_label = text(name).size(13).color(theme::TEXT);
            let type_label = text(dev_type).size(11).color(theme::SUBTEXT0);
            let state_label = text(state).size(11).color(state_color);

            let conn_label = if connection.is_empty() || connection == "--" {
                text("").size(11).color(theme::OVERLAY0)
            } else {
                text(connection).size(11).color(theme::SUBTEXT0)
            };

            let signal_text: Element<'_, Message> = match signal {
                Some(s) => text(format!("{s}%")).size(11).color(theme::SUBTEXT0).into(),
                None => Space::new().into(),
            };

            let r = row![
                name_label,
                type_label,
                state_label,
                conn_label,
                Space::new().width(Length::Fill),
                signal_text,
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center);

            rows = rows.push(r);
        }

        card(column![heading, rows].spacing(8))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read whether WiFi radio is currently enabled.
fn read_wifi_enabled() -> bool {
    Command::new("nmcli")
        .args(["radio", "wifi"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("enabled"))
        .unwrap_or(false)
}

/// A Catppuccin-styled card container.
fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(16)
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

/// A label: value detail row.
fn detail_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    let l = text(label).size(13).color(theme::SUBTEXT0);
    let v = text(value).size(13).color(theme::TEXT);

    row![l, Space::new().width(12), v]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .into()
}

/// Small pill-shaped action button with a custom background color.
fn small_button(label: &str, color: iced::Color, msg: Message) -> Element<'_, Message> {
    let bg_color = color;
    button(text(label).size(12).color(theme::BASE))
        .on_press(msg)
        .padding([4, 12])
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => {
                    let mut c = bg_color;
                    c.a = 0.8;
                    c
                }
                _ => bg_color,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color: theme::BASE,
                border: Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .into()
}

/// Accent-colored action button (blue background).
fn accent_button(label: &str, msg: Message) -> Element<'_, Message> {
    button(text(label).size(13).color(theme::BASE))
        .on_press(msg)
        .padding([6, 16])
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => theme::LAVENDER,
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
        })
        .into()
}

/// Render a signal strength indicator as colored text.
fn signal_indicator(signal_str: &str) -> Element<'_, Message> {
    let strength: u32 = signal_str.parse().unwrap_or(0);

    let (icon, color) = if strength >= 75 {
        ("||||", theme::GREEN)
    } else if strength >= 50 {
        ("|||.", theme::YELLOW)
    } else if strength >= 25 {
        ("||..", theme::PEACH)
    } else {
        ("|...", theme::RED)
    };

    text(icon).size(12).color(color).into()
}
