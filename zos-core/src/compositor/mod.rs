//! Compositor IPC abstraction.
//!
//! `Compositor` is the trait every zOS shell app uses to query and
//! manipulate window-manager state (workspaces, windows, focus). It's
//! deliberately runtime-pluggable so the same UI code works on Hyprland
//! today and `zos-wm` once we swap over.
//!
//! Two impls ship:
//! - `Hyprland` — shells out to `hyprctl` (current production path).
//! - `ZosWm` — talks to `zos-wm`'s socket (Phase 8 stub for now).
//!
//! Apps select the impl via `detect()` which checks
//! `XDG_CURRENT_DESKTOP` + socket presence.

pub mod hyprland;
pub mod zos_wm;

use std::error::Error;

/// Stable workspace info — what shell apps see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub id: i64,
    pub name: String,
    pub monitor: String,
    pub windows: usize,
    pub active: bool,
}

/// Stable window info — what shell apps see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub address: String,
    pub workspace_id: i64,
    pub monitor: String,
    pub class: String,
    pub title: String,
    pub focused: bool,
}

/// Stable monitor info — what shell apps see.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub id: i64,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: f64,
    pub scale: f64,
    pub focused: bool,
    pub active_workspace_id: Option<i64>,
}

/// Compositor IPC trait. All methods may fail (network, missing
/// compositor, parse errors). Errors propagate boxed.
pub trait Compositor: Send + Sync {
    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>, Box<dyn Error + Send + Sync>>;
    fn windows(&self) -> Result<Vec<WindowInfo>, Box<dyn Error + Send + Sync>>;
    fn monitors(&self) -> Result<Vec<MonitorInfo>, Box<dyn Error + Send + Sync>>;
    fn active_window(&self) -> Result<Option<WindowInfo>, Box<dyn Error + Send + Sync>>;
    fn focus_window(&self, address: &str) -> Result<(), Box<dyn Error + Send + Sync>>;
    fn switch_to_workspace(&self, id: i64) -> Result<(), Box<dyn Error + Send + Sync>>;
}

/// Auto-detect which Compositor impl to use based on environment.
///
/// Checks `XDG_CURRENT_DESKTOP` for `zos-wm`; falls back to Hyprland.
pub fn detect() -> Result<Box<dyn Compositor>, Box<dyn Error + Send + Sync>> {
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        if desktop.contains("zos-wm") {
            return Ok(Box::new(zos_wm::ZosWm::new()?));
        }
    }
    Ok(Box::new(hyprland::Hyprland::new()?))
}
