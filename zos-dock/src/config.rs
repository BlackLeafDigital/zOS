// === config.rs — Dock configuration: pinned apps, icon size, magnification ===

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Dock configuration persisted to ~/.config/zos/dock.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockConfig {
    /// Desktop app IDs for pinned applications (e.g. "org.wezfurlong.wezterm").
    pub pinned: Vec<String>,
    /// Base icon size in pixels before magnification.
    pub icon_size: u32,
    /// Peak magnification factor (1.0 = no magnification, 2.0 = double size).
    pub magnification: f32,
    /// Whether the dock should auto-hide when no windows are focused on it.
    pub auto_hide: bool,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            pinned: vec![
                "org.wezfurlong.wezterm".to_string(),
                "org.mozilla.firefox".to_string(),
                "org.kde.dolphin".to_string(),
            ],
            icon_size: 48,
            magnification: 1.6,
            auto_hide: false,
        }
    }
}

impl DockConfig {
    /// Returns the path to the dock config file.
    fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        Path::new(&home).join(".config/zos/dock.json")
    }

    /// Load config from disk, falling back to defaults if the file doesn't exist
    /// or can't be parsed.
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Save the current config to disk, creating parent directories as needed.
    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }

    /// Get the modification time of the config file, if it exists.
    pub fn config_mtime() -> Option<SystemTime> {
        let path = Self::config_path();
        std::fs::metadata(&path).ok()?.modified().ok()
    }

    /// Check whether a given app ID is pinned.
    #[allow(dead_code)]
    pub fn is_pinned(&self, app_id: &str) -> bool {
        self.pinned.iter().any(|p| p == app_id)
    }

    /// Toggle the pin state of an app. Returns true if the app is now pinned.
    pub fn toggle_pin(&mut self, app_id: &str) -> bool {
        if let Some(pos) = self.pinned.iter().position(|p| p == app_id) {
            self.pinned.remove(pos);
            false
        } else {
            self.pinned.push(app_id.to_string());
            true
        }
    }
}
