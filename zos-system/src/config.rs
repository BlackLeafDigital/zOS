// === config.rs — Path constants, state management, and utility functions ===

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// --- System Paths ---
pub const SYSTEM_VERSION: &str = "/usr/share/zos/version";
#[allow(dead_code)]
pub const SYSTEM_HYPR_DIR: &str = "/usr/share/zos/hypr/";
pub const SKEL_HYPR_DIR: &str = "/etc/skel/.config/hypr/";
pub const IMAGE_INFO: &str = "/usr/share/ublue-os/image-info.json";

// --- Relative to $HOME ---
pub const USER_STATE_REL: &str = ".config/zos/state.json";
pub const BACKUP_DIR_REL: &str = ".config/zos/backups/";
pub const SETUP_DONE_REL: &str = ".config/zos-setup-done";

// --- Config version state ---
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigState {
    #[serde(default)]
    pub hypr: u32,
    #[serde(default)]
    pub waybar: u32,
    #[serde(default)]
    pub wlogout: u32,
    #[serde(default)]
    pub zshrc: u32,
    #[serde(default)]
    pub gitconfig: u32,
}

/// Expand ~ to the user's home directory.
pub fn expand_home(rel: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    Path::new(&home).join(rel)
}

/// Read the system-level zOS version string.
pub fn read_system_version() -> Result<String> {
    let path = Path::new(SYSTEM_VERSION);
    if path.exists() {
        let content = fs::read_to_string(path)
            .wrap_err("Failed to read system version")?;
        Ok(content.trim().to_string())
    } else {
        Ok("unknown".to_string())
    }
}

/// Read the user's config state from ~/.config/zos/state.json.
pub fn read_user_state() -> Result<ConfigState> {
    let path = expand_home(USER_STATE_REL);
    if path.exists() {
        let content = fs::read_to_string(&path)
            .wrap_err("Failed to read user state")?;
        let state: ConfigState = serde_json::from_str(&content)
            .wrap_err("Failed to parse user state JSON")?;
        Ok(state)
    } else {
        Ok(ConfigState::default())
    }
}

/// Write the user's config state to ~/.config/zos/state.json.
pub fn write_user_state(state: &ConfigState) -> Result<()> {
    let path = expand_home(USER_STATE_REL);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .wrap_err("Failed to create state directory")?;
    }
    let json = serde_json::to_string_pretty(state)
        .wrap_err("Failed to serialize state")?;
    fs::write(&path, json)
        .wrap_err("Failed to write user state")?;
    Ok(())
}

/// Ensure the backup directory exists and return its path.
pub fn ensure_backup_dir() -> Result<PathBuf> {
    let path = expand_home(BACKUP_DIR_REL);
    fs::create_dir_all(&path)
        .wrap_err("Failed to create backup directory")?;
    Ok(path)
}

/// Check if first-login setup has been completed.
pub fn is_setup_done() -> bool {
    expand_home(SETUP_DONE_REL).exists()
}
