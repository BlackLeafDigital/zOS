// === commands/status.rs — System info and config area status ===

use crate::config;
use color_eyre::eyre::{Context, Result};
use serde::Deserialize;
use std::fs;

// --- Image info from ublue-os ---
#[derive(Debug, Deserialize, Default)]
struct ImageInfoJson {
    #[serde(default, alias = "image-name")]
    image_name: Option<String>,
    #[serde(default, alias = "fedora-version")]
    fedora_version: Option<String>,
}

// --- Public types ---

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub os_version: String,
    pub image_name: String,
    pub fedora_version: String,
    pub last_update: String,
}

#[derive(Debug, Clone)]
pub struct ConfigArea {
    pub name: String,
    pub system_version: u32,
    pub user_version: u32,
    pub up_to_date: bool,
}

// --- Functions ---

/// Gather overall system information.
pub fn get_system_info() -> SystemInfo {
    let os_version = config::read_system_version().unwrap_or_else(|_| "unknown".into());

    let (image_name, fedora_version) = read_image_info()
        .unwrap_or(("unknown".into(), "unknown".into()));

    let last_update = get_last_update().unwrap_or_else(|| "unknown".into());

    SystemInfo {
        os_version,
        image_name,
        fedora_version,
        last_update,
    }
}

/// Compare system vs user config versions for each managed area.
pub fn get_config_status() -> Vec<ConfigArea> {
    let user_state = config::read_user_state().unwrap_or_default();
    let system_versions = read_system_config_versions();

    vec![
        ConfigArea {
            name: "Hyprland".into(),
            system_version: system_versions.hypr,
            user_version: user_state.hypr,
            up_to_date: user_state.hypr >= system_versions.hypr,
        },
        ConfigArea {
            name: "Waybar".into(),
            system_version: system_versions.waybar,
            user_version: user_state.waybar,
            up_to_date: user_state.waybar >= system_versions.waybar,
        },
        ConfigArea {
            name: "wlogout".into(),
            system_version: system_versions.wlogout,
            user_version: user_state.wlogout,
            up_to_date: user_state.wlogout >= system_versions.wlogout,
        },
        ConfigArea {
            name: "zshrc".into(),
            system_version: system_versions.zshrc,
            user_version: user_state.zshrc,
            up_to_date: user_state.zshrc >= system_versions.zshrc,
        },
        ConfigArea {
            name: "gitconfig".into(),
            system_version: system_versions.gitconfig,
            user_version: user_state.gitconfig,
            up_to_date: user_state.gitconfig >= system_versions.gitconfig,
        },
    ]
}

// --- Internal helpers ---

fn read_image_info() -> Result<(String, String)> {
    let path = config::IMAGE_INFO;
    if !std::path::Path::new(path).exists() {
        return Ok(("unknown".into(), "unknown".into()));
    }
    let content = fs::read_to_string(path)
        .wrap_err("Failed to read image-info.json")?;
    let info: ImageInfoJson = serde_json::from_str(&content)
        .wrap_err("Failed to parse image-info.json")?;
    Ok((
        info.image_name.unwrap_or_else(|| "unknown".into()),
        info.fedora_version.unwrap_or_else(|| "unknown".into()),
    ))
}

fn get_last_update() -> Option<String> {
    let output = std::process::Command::new("rpm-ostree")
        .arg("status")
        .arg("--json")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Extract timestamp from the JSON — look for the first "timestamp" field
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout) {
        val.pointer("/deployments/0/timestamp")
            .and_then(|v| v.as_i64())
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| ts.to_string())
            })
    } else {
        None
    }
}

/// Read the system-shipped config versions. In a real deployment these come from
/// /usr/share/zos/config-versions.json. If not present, default to 1.
fn read_system_config_versions() -> config::ConfigState {
    let version_file = "/usr/share/zos/config-versions.json";
    if let Ok(content) = fs::read_to_string(version_file) {
        if let Ok(state) = serde_json::from_str::<config::ConfigState>(&content) {
            return state;
        }
    }
    // Defaults — version 1 means the initial shipped config
    config::ConfigState {
        hypr: 1,
        waybar: 1,
        wlogout: 1,
        zshrc: 1,
        gitconfig: 1,
    }
}
