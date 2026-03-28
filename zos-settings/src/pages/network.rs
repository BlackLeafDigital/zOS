// === pages/network.rs — Network configuration page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use crate::services::network;

/// Build the network configuration page widget.
pub fn build() -> gtk::Box {
    let page = super::page_content();

    page.append(&build_devices_section());
    page.append(&build_connection_section());
    page.append(&build_wifi_section());
    page.append(&build_details_section());
    page.append(&build_actions_section());

    super::page_wrapper(&page)
}

/// Return the appropriate WiFi signal icon name and CSS class for a given signal strength.
fn wifi_signal_icon_and_class(signal: Option<u32>) -> (&'static str, &'static str) {
    match signal {
        Some(s) if s >= 75 => (
            "network-wireless-signal-excellent-symbolic",
            "signal-excellent",
        ),
        Some(s) if s >= 50 => ("network-wireless-signal-good-symbolic", "signal-good"),
        Some(s) if s >= 25 => ("network-wireless-signal-ok-symbolic", "signal-fair"),
        _ => ("network-wireless-signal-weak-symbolic", "signal-weak"),
    }
}

// --- Devices Section ---

fn build_devices_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Devices").build();

    let mut devices = network::get_devices();
    let favorites = Rc::new(RefCell::new(network::load_favorites()));

    // Sort: favorites first, then alphabetical by device name
    {
        let favs = favorites.borrow();
        devices.sort_by(|a, b| {
            let a_fav = favs.contains(&a.0);
            let b_fav = favs.contains(&b.0);
            match (a_fav, b_fav) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.cmp(&b.0),
            }
        });
    }

    if devices.is_empty() {
        let row = adw::ActionRow::builder().title("No devices found").build();
        group.add(&row);
    } else {
        for (dev_name, dev_type, state, connection, signal) in &devices {
            let row = adw::ActionRow::builder()
                .title(dev_name.as_str())
                .subtitle(&format!("{} \u{2014} {}", dev_type, state))
                .build();

            // Prefix icon based on device type and signal strength
            let (icon_name, icon_class) = if dev_type == "wifi" {
                if state == "disconnected" {
                    ("network-wireless-offline-symbolic", "signal-weak")
                } else {
                    wifi_signal_icon_and_class(*signal)
                }
            } else {
                ("network-wired-symbolic", "")
            };
            let icon = gtk::Image::from_icon_name(icon_name);
            icon.set_valign(gtk::Align::Center);
            if !icon_class.is_empty() {
                icon.add_css_class(icon_class);
            }
            row.add_prefix(&icon);

            // Show connection name if connected
            if !connection.is_empty() && state == "connected" {
                let conn_label = gtk::Label::builder()
                    .label(connection.as_str())
                    .valign(gtk::Align::Center)
                    .css_classes(["dim-label"])
                    .build();
                row.add_suffix(&conn_label);
            }

            // Status dot
            let status_class = if state == "connected" {
                "status-badge-green"
            } else if state.contains("connecting") {
                "status-badge-yellow"
            } else {
                "status-badge-red"
            };
            let status_dot = gtk::Label::builder()
                .label("\u{25CF}")
                .valign(gtk::Align::Center)
                .css_classes([status_class])
                .build();
            row.add_suffix(&status_dot);

            // Star toggle button for favorites
            let is_fav = favorites.borrow().contains(dev_name);
            let star_icon = if is_fav {
                "starred-symbolic"
            } else {
                "non-starred-symbolic"
            };
            let star_btn = gtk::ToggleButton::builder()
                .icon_name(star_icon)
                .valign(gtk::Align::Center)
                .active(is_fav)
                .css_classes(["flat"])
                .build();

            let dev_name_clone = dev_name.clone();
            let favorites_clone = Rc::clone(&favorites);
            star_btn.connect_toggled(move |btn| {
                let active = btn.is_active();
                let icon = if active {
                    "starred-symbolic"
                } else {
                    "non-starred-symbolic"
                };
                btn.set_icon_name(icon);

                let mut favs = favorites_clone.borrow_mut();
                if active {
                    if !favs.contains(&dev_name_clone) {
                        favs.push(dev_name_clone.clone());
                    }
                } else {
                    favs.retain(|f| f != &dev_name_clone);
                }
                network::save_favorites(&favs);
            });
            row.add_suffix(&star_btn);

            group.add(&row);
        }
    }

    group
}

// --- Connection Status Section ---

fn build_connection_section() -> adw::PreferencesGroup {
    let conn_group = adw::PreferencesGroup::builder().title("Connection").build();

    let active_connections = network::get_active_connections();
    if active_connections.is_empty() {
        let row = adw::ActionRow::builder()
            .title("No active connections")
            .build();
        conn_group.add(&row);
    } else {
        for (name, conn_type, device) in &active_connections {
            let row = adw::ActionRow::builder()
                .title(name.as_str())
                .subtitle(&format!("{} \u{2014} {}", conn_type, device))
                .build();

            let conn_type_lower = conn_type.to_lowercase();
            let icon_name =
                if conn_type_lower.contains("wireless") || conn_type_lower.contains("wifi") {
                    "network-wireless-symbolic"
                } else if conn_type_lower.contains("vpn") {
                    "network-vpn-symbolic"
                } else {
                    "network-wired-symbolic"
                };
            let icon = gtk::Image::from_icon_name(icon_name);
            icon.set_valign(gtk::Align::Center);
            row.add_prefix(&icon);

            let conn_name_clone = name.clone();
            let disconnect_btn = gtk::Button::builder()
                .label("Disconnect")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            disconnect_btn.connect_clicked(move |_| {
                let _ = Command::new("nmcli")
                    .args(["connection", "down", &conn_name_clone])
                    .status();
            });
            row.add_suffix(&disconnect_btn);

            conn_group.add(&row);
        }
    }

    conn_group
}

// --- WiFi Networks Section ---

fn build_wifi_section() -> adw::PreferencesGroup {
    let wifi_group = adw::PreferencesGroup::builder().title("WiFi").build();

    // Refresh button at top of group
    let scan_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Rescan WiFi networks")
        .css_classes(["flat"])
        .build();
    scan_btn.connect_clicked(|_| {
        let _ = Command::new("nmcli")
            .args(["device", "wifi", "rescan"])
            .spawn();
    });
    wifi_group.set_header_suffix(Some(&scan_btn));

    let mut networks = network::get_wifi_networks();

    // Sort: connected first, then by signal strength descending
    networks.sort_by(|a, b| {
        // in_use first
        match (b.3, a.3) {
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            _ => {}
        }
        // Then by signal descending
        let a_signal = a.1.parse::<u32>().unwrap_or(0);
        let b_signal = b.1.parse::<u32>().unwrap_or(0);
        b_signal.cmp(&a_signal)
    });

    if networks.is_empty() {
        let row = adw::ActionRow::builder()
            .title("No WiFi networks found")
            .subtitle("WiFi may be disabled or unavailable")
            .build();
        wifi_group.add(&row);
    } else {
        for (ssid, signal, security, in_use) in &networks {
            if ssid.is_empty() {
                continue;
            }

            let signal_val = signal.parse::<u32>().ok();
            let (signal_icon_name, signal_class) = wifi_signal_icon_and_class(signal_val);

            let row = adw::ActionRow::builder()
                .title(ssid.as_str())
                .subtitle(&format!("Security: {}", security))
                .build();

            // Signal strength icon as prefix
            let signal_icon = gtk::Image::from_icon_name(signal_icon_name);
            signal_icon.set_valign(gtk::Align::Center);
            signal_icon.add_css_class(signal_class);
            row.add_prefix(&signal_icon);

            if *in_use {
                let check = gtk::Image::from_icon_name("emblem-default-symbolic");
                check.set_valign(gtk::Align::Center);
                check.add_css_class("signal-excellent");
                row.add_suffix(&check);

                let connected_label = gtk::Label::builder()
                    .label("Connected")
                    .valign(gtk::Align::Center)
                    .css_classes(["dim-label"])
                    .build();
                row.add_suffix(&connected_label);
            } else {
                let ssid_clone = ssid.clone();
                let secured = !security.is_empty() && security != "--";
                let connect_btn = gtk::Button::builder()
                    .label("Connect")
                    .valign(gtk::Align::Center)
                    .build();
                connect_btn.connect_clicked(move |btn| {
                    if secured {
                        show_password_dialog(btn, &ssid_clone);
                    } else {
                        network::connect_wifi(&ssid_clone, None);
                    }
                });
                row.add_suffix(&connect_btn);
            }

            wifi_group.add(&row);
        }
    }

    wifi_group
}

// --- IP Details Section ---

fn build_details_section() -> adw::PreferencesGroup {
    let details_group = adw::PreferencesGroup::builder().title("Details").build();

    let (ip, gateway, dns) = network::get_ip_details();

    let ip_row = adw::ActionRow::builder()
        .title("IP Address")
        .subtitle(&ip)
        .build();
    let ip_icon = gtk::Image::from_icon_name("network-workgroup-symbolic");
    ip_icon.set_valign(gtk::Align::Center);
    ip_row.add_prefix(&ip_icon);
    details_group.add(&ip_row);

    let gw_row = adw::ActionRow::builder()
        .title("Gateway")
        .subtitle(&gateway)
        .build();
    let gw_icon = gtk::Image::from_icon_name("network-server-symbolic");
    gw_icon.set_valign(gtk::Align::Center);
    gw_row.add_prefix(&gw_icon);
    details_group.add(&gw_row);

    let dns_row = adw::ActionRow::builder()
        .title("DNS")
        .subtitle(&dns)
        .build();
    let dns_icon = gtk::Image::from_icon_name("preferences-system-network-symbolic");
    dns_icon.set_valign(gtk::Align::Center);
    dns_row.add_prefix(&dns_icon);
    details_group.add(&dns_row);

    details_group
}

// --- Actions Section ---

fn build_actions_section() -> adw::PreferencesGroup {
    let actions_group = adw::PreferencesGroup::builder().build();

    // WiFi enable/disable toggle
    let wifi_row = adw::ActionRow::builder()
        .title("WiFi")
        .subtitle("Enable or disable WiFi radio")
        .build();
    let wifi_icon = gtk::Image::from_icon_name("network-wireless-symbolic");
    wifi_icon.set_valign(gtk::Align::Center);
    wifi_row.add_prefix(&wifi_icon);

    // Check current wifi status
    let wifi_enabled = Command::new("nmcli")
        .args(["radio", "wifi"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string() == "enabled")
        .unwrap_or(true);

    let wifi_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(wifi_enabled)
        .build();
    wifi_switch.connect_state_set(move |_sw, active| {
        let state = if active { "on" } else { "off" };
        let _ = Command::new("nmcli")
            .args(["radio", "wifi", state])
            .status();
        gtk::glib::Propagation::Proceed
    });
    wifi_row.add_suffix(&wifi_switch);
    actions_group.add(&wifi_row);

    // Advanced network configuration (nmtui)
    let nm_row = adw::ActionRow::builder()
        .title("Network Connections")
        .subtitle("Advanced network configuration")
        .build();
    let nm_icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
    nm_icon.set_valign(gtk::Align::Center);
    nm_row.add_prefix(&nm_icon);
    let nm_btn = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();
    nm_btn.connect_clicked(|_| {
        let _ = Command::new("wezterm").args(["-e", "nmtui"]).spawn();
    });
    nm_row.add_suffix(&nm_btn);
    nm_row.set_activatable_widget(Some(&nm_btn));
    actions_group.add(&nm_row);

    actions_group
}

/// Show a password dialog for secured WiFi networks.
fn show_password_dialog(btn: &gtk::Button, ssid: &str) {
    let ssid_owned = ssid.to_string();

    let dialog = gtk::Window::builder()
        .title(&format!("Connect to {}", ssid))
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

    let label = gtk::Label::new(Some("Enter the WiFi password:"));
    content.append(&label);

    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .hexpand(true)
        .build();
    content.append(&entry);

    let button_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();

    let cancel_btn = gtk::Button::builder().label("Cancel").build();
    let connect_btn = gtk::Button::builder()
        .label("Connect")
        .css_classes(["suggested-action"])
        .build();

    button_box.append(&cancel_btn);
    button_box.append(&connect_btn);
    content.append(&button_box);

    dialog.set_child(Some(&content));

    let dialog_clone = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_clone.close();
    });

    let dialog_clone = dialog.clone();
    let entry_clone = entry.clone();
    connect_btn.connect_clicked(move |_| {
        let password = entry_clone.text();
        let password_str = password.as_str();
        if !password_str.is_empty() {
            network::connect_wifi(&ssid_owned, Some(password_str));
        }
        dialog_clone.close();
    });

    dialog.present();
}
