// === services/network.rs — Network service helpers (nmcli wrappers) ===

use std::path::PathBuf;
use std::process::Command;

/// Parse network devices from nmcli.
/// Returns Vec<(device_name, type, state, connection_name, signal)>.
/// Filters out loopback ("lo") and "wifi-p2p" type devices.
pub fn get_devices() -> Vec<(String, String, String, String, Option<u32>)> {
    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION,SIGNAL",
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
                    let parts: Vec<&str> = line.splitn(5, ':').collect();
                    if parts.len() == 5 {
                        let device = parts[0].to_string();
                        let dev_type = parts[1].to_string();
                        if device == "lo" || dev_type == "wifi-p2p" {
                            return None;
                        }
                        let signal = parts[4].trim().parse::<u32>().ok();
                        Some((
                            device,
                            dev_type,
                            parts[2].to_string(),
                            parts[3].to_string(),
                            signal,
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

/// Parse active connections from nmcli.
pub fn get_active_connections() -> Vec<(String, String, String)> {
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
pub fn get_wifi_networks() -> Vec<(String, String, String, bool)> {
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
pub fn get_ip_details() -> (String, String, String) {
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
pub fn connect_wifi(ssid: &str, password: Option<&str>) {
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

// Network favorites persistence — retained for the planned "favorite networks"
// UI affordance (pinning SSIDs to the top of the list). Wired in but not yet
// consumed by the network page.
/// Path to the network favorites JSON file.
#[allow(dead_code)]
pub fn favorites_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::Path::new(&home).join(".config/zos/network-favorites.json")
}

/// Load favorite device names from disk.
#[allow(dead_code)]
pub fn load_favorites() -> Vec<String> {
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
#[allow(dead_code)]
pub fn save_favorites(favorites: &[String]) {
    let path = favorites_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::json!({ "favorites": favorites });
    if let Ok(contents) = serde_json::to_string_pretty(&json) {
        let _ = std::fs::write(&path, contents);
    }
}
