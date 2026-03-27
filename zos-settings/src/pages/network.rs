// === pages/network.rs — Network configuration page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

/// Build the network configuration page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    page.append(&build_devices_section());
    page.append(&build_connection_section());
    page.append(&build_wifi_section());
    page.append(&build_details_section());
    page.append(&build_actions_section());

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

// --- Devices Section ---

fn build_devices_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Devices").build();

    let mut devices = get_devices();
    let favorites = Rc::new(RefCell::new(load_favorites()));

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
        for (dev_name, dev_type, state, connection) in &devices {
            let row = adw::ActionRow::builder()
                .title(dev_name.as_str())
                .subtitle(&format!("{} \u{2014} {}", dev_type, state))
                .build();

            // Prefix icon based on device type
            let icon_name = if dev_type == "wifi" {
                "network-wireless-symbolic"
            } else {
                "network-wired-symbolic"
            };
            let icon = gtk::Image::from_icon_name(icon_name);
            icon.set_valign(gtk::Align::Center);
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
                save_favorites(&favs);
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

    let active_connections = get_active_connections();
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
            let icon = gtk::Image::from_icon_name("network-wireless-symbolic");
            icon.set_valign(gtk::Align::Center);
            row.add_prefix(&icon);
            conn_group.add(&row);
        }
    }

    conn_group
}

// --- WiFi Networks Section ---

fn build_wifi_section() -> adw::PreferencesGroup {
    let wifi_group = adw::PreferencesGroup::builder().title("WiFi").build();

    let networks = get_wifi_networks();
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
            let row = adw::ActionRow::builder()
                .title(ssid.as_str())
                .subtitle(&format!("Signal: {}%  Security: {}", signal, security))
                .build();

            if *in_use {
                let check = gtk::Image::from_icon_name("emblem-ok-symbolic");
                check.set_valign(gtk::Align::Center);
                row.add_suffix(&check);
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
                        connect_wifi(&ssid_clone, None);
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

    let (ip, gateway, dns) = get_ip_details();

    let ip_row = adw::ActionRow::builder()
        .title("IP Address")
        .subtitle(&ip)
        .build();
    details_group.add(&ip_row);

    let gw_row = adw::ActionRow::builder()
        .title("Gateway")
        .subtitle(&gateway)
        .build();
    details_group.add(&gw_row);

    let dns_row = adw::ActionRow::builder()
        .title("DNS")
        .subtitle(&dns)
        .build();
    details_group.add(&dns_row);

    details_group
}

// --- Actions Section ---

fn build_actions_section() -> adw::PreferencesGroup {
    let actions_group = adw::PreferencesGroup::builder().build();

    let nm_row = adw::ActionRow::builder()
        .title("Network Connections")
        .subtitle("Advanced network configuration")
        .build();
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

/// Parse network devices from nmcli.
/// Returns Vec<(device_name, type, state, connection_name)>.
/// Filters out loopback ("lo") and "wifi-p2p" type devices.
fn get_devices() -> Vec<(String, String, String, String)> {
    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION",
            "device",
            "status",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(4, ':').collect();
                    if parts.len() == 4 {
                        let device = parts[0].to_string();
                        let dev_type = parts[1].to_string();
                        if device == "lo" || dev_type == "wifi-p2p" {
                            return None;
                        }
                        Some((device, dev_type, parts[2].to_string(), parts[3].to_string()))
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Path to the network favorites JSON file.
fn favorites_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::Path::new(&home).join(".config/zos/network-favorites.json")
}

/// Load favorite device names from disk.
fn load_favorites() -> Vec<String> {
    let path = favorites_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    parsed
        .get("favorites")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Save favorite device names to disk.
fn save_favorites(favorites: &[String]) {
    let path = favorites_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::json!({ "favorites": favorites });
    if let Ok(contents) = serde_json::to_string_pretty(&json) {
        let _ = std::fs::write(&path, contents);
    }
}

/// Parse active connections from nmcli.
fn get_active_connections() -> Vec<(String, String, String)> {
    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "NAME,TYPE,DEVICE",
            "connection",
            "show",
            "--active",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(3, ':').collect();
                    if parts.len() == 3 {
                        Some((
                            parts[0].to_string(),
                            parts[1].to_string(),
                            parts[2].to_string(),
                        ))
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Parse available WiFi networks from nmcli.
fn get_wifi_networks() -> Vec<(String, String, String, bool)> {
    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "SSID,SIGNAL,SECURITY,IN-USE",
            "device",
            "wifi",
            "list",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(4, ':').collect();
                    if parts.len() == 4 {
                        Some((
                            parts[0].to_string(),
                            parts[1].to_string(),
                            parts[2].to_string(),
                            parts[3].trim() == "*",
                        ))
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Get IP details for the first active device.
fn get_ip_details() -> (String, String, String) {
    // Find the first non-loopback connected device
    let device = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"])
        .output()
        .ok()
        .and_then(|o| {
            if !o.status.success() {
                return None;
            }
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            stdout
                .lines()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.split(':').collect();
                    if parts.len() >= 3
                        && parts[2].contains("connected")
                        && parts[1] != "loopback"
                        && parts[1] != "wifi-p2p"
                    {
                        Some(parts[0].to_string())
                    } else {
                        None
                    }
                })
                .next()
        })
        .unwrap_or_default();

    if device.is_empty() {
        return ("N/A".into(), "N/A".into(), "N/A".into());
    }

    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "IP4.ADDRESS,IP4.GATEWAY,IP4.DNS",
            "device",
            "show",
            &device,
        ])
        .output();

    let mut ip = String::from("N/A");
    let mut gateway = String::from("N/A");
    let mut dns = String::from("N/A");

    if let Ok(o) = output {
        if o.status.success() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                if let Some(val) = line.strip_prefix("IP4.ADDRESS[1]:") {
                    ip = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("IP4.GATEWAY:") {
                    gateway = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("IP4.DNS[1]:") {
                    dns = val.trim().to_string();
                }
            }
        }
    }

    (ip, gateway, dns)
}

/// Connect to an open or secured WiFi network.
fn connect_wifi(ssid: &str, password: Option<&str>) {
    let mut cmd = Command::new("nmcli");
    cmd.args(["device", "wifi", "connect", ssid]);
    if let Some(pass) = password {
        cmd.args(["password", pass]);
    }

    match cmd.status() {
        Ok(status) if status.success() => {
            tracing::info!("Connected to WiFi: {}", ssid);
        }
        Ok(status) => {
            tracing::error!("Failed to connect to {}: exit {}", ssid, status);
        }
        Err(e) => {
            tracing::error!("Failed to run nmcli: {}", e);
        }
    }
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
            connect_wifi(&ssid_owned, Some(password_str));
        }
        dialog_clone.close();
    });

    dialog.present();
}
