// === hypr.rs — Hyprland IPC: window list, focus, close ===

use std::process::Command;

/// A window tracked by Hyprland.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HyprWindow {
    /// Hyprland address (hex string like "0x55a1b2c3d4e5").
    pub address: String,
    /// Window class (matches .desktop StartupWMClass / app_id).
    pub class: String,
    /// Window title.
    pub title: String,
    /// Workspace name the window lives on.
    pub workspace_name: String,
    /// Focus history ID: 0 = currently focused, higher = older.
    pub focus_history_id: i32,
}

/// Query Hyprland for the current window list via `hyprctl clients -j`.
pub fn get_windows() -> Vec<HyprWindow> {
    let output = match Command::new("hyprctl").args(["clients", "-j"]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let json_str = match std::str::from_utf8(&output.stdout) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let clients: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    clients
        .into_iter()
        .filter_map(|c| {
            let address = c.get("address")?.as_str()?.to_string();
            let class = c
                .get("class")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = c
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let workspace_name = c
                .get("workspace")
                .and_then(|w| w.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let focus_history_id = c
                .get("focusHistoryID")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1) as i32;

            // Skip windows with empty class (usually internal Hyprland windows).
            if class.is_empty() {
                return None;
            }

            Some(HyprWindow {
                address,
                class,
                title,
                workspace_name,
                focus_history_id,
            })
        })
        .filter(|w| !w.workspace_name.starts_with("special:minimize"))
        .collect()
}

/// Get the address of the currently focused window.
pub fn get_active_window_address() -> Option<String> {
    let output = std::process::Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    json.get("address")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Focus a window by its Hyprland address.
pub fn focus_window(address: &str) {
    let _ = Command::new("hyprctl")
        .args(["dispatch", "focuswindow", &format!("address:{}", address)])
        .status();
}

/// Close a window by its Hyprland address.
#[allow(dead_code)]
pub fn close_window(address: &str) {
    let _ = Command::new("hyprctl")
        .args(["dispatch", "closewindow", &format!("address:{}", address)])
        .status();
}

/// Query Hyprland for minimized windows (on the special:minimize workspace).
pub fn get_minimized_windows() -> Vec<HyprWindow> {
    let output = match Command::new("hyprctl").args(["clients", "-j"]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    if !output.status.success() {
        return Vec::new();
    }
    let json_str = match std::str::from_utf8(&output.stdout) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let clients: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    clients
        .into_iter()
        .filter_map(|c| {
            let address = c.get("address")?.as_str()?.to_string();
            let class = c
                .get("class")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = c
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let workspace_name = c
                .get("workspace")
                .and_then(|w| w.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let focus_history_id = c
                .get("focusHistoryID")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1) as i32;
            if class.is_empty() || !workspace_name.starts_with("special:minimize") {
                return None;
            }
            Some(HyprWindow {
                address,
                class,
                title,
                workspace_name,
                focus_history_id,
            })
        })
        .collect()
}

/// Unminimize a window by moving it to the current workspace and focusing it.
pub fn unminimize_window(address: &str) {
    let _ = Command::new("hyprctl")
        .args([
            "dispatch",
            "movetoworkspacesilent",
            &format!("e+0,address:{}", address),
        ])
        .status();
    focus_window(address);
}

/// Launch an application by its desktop file app ID (e.g. "org.wezfurlong.wezterm").
/// Attempts to find and exec the desktop file via `gtk-launch`, falling back to
/// looking up the `Exec` line manually.
pub fn launch_app(app_id: &str) {
    // Try desktop file names in order of likelihood.
    let candidates = [
        format!("{}.desktop", app_id),
        format!("{}.desktop", app_id.to_lowercase()),
    ];

    for candidate in &candidates {
        let status = Command::new("gtk-launch").arg(candidate).status();
        if let Ok(s) = status {
            if s.success() {
                return;
            }
        }
    }

    // Fallback: try running the last segment of the app ID as a command.
    // e.g. "org.wezfurlong.wezterm" -> "wezterm"
    if let Some(cmd) = app_id.rsplit('.').next() {
        let _ = Command::new(cmd).spawn();
    }
}
