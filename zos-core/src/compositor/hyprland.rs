//! Hyprland Compositor impl — shells out to `hyprctl -j`.
//!
//! Mirrors the pattern in `zos-dock/src/hypr.rs` but trait-implements
//! the abstract `Compositor` interface. We do NOT depend on or modify
//! zos-dock; this is a parallel implementation that shell apps consume.

use super::{Compositor, MonitorInfo, WindowInfo, WorkspaceInfo};
use std::error::Error;
use std::process::Command;

#[derive(Debug, Default)]
pub struct Hyprland;

impl Hyprland {
    pub fn new() -> Result<Self, Box<dyn Error + Send + Sync>> {
        // Verify we're actually under Hyprland. Fail fast if not.
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
            return Err(
                "not running under Hyprland (HYPRLAND_INSTANCE_SIGNATURE unset)".into(),
            );
        }
        Ok(Self)
    }

    fn hyprctl(&self, args: &[&str]) -> Result<String, Box<dyn Error + Send + Sync>> {
        let out = Command::new("hyprctl")
            .args(args)
            .arg("-j")
            .output()
            .map_err(|e| format!("hyprctl spawn failed: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "hyprctl {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            )
            .into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

impl Compositor for Hyprland {
    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>, Box<dyn Error + Send + Sync>> {
        // Drive the shellout for now; full JSON parsing lands once we
        // wire serde_json into zos-core (follow-up).
        let _raw = self.hyprctl(&["workspaces"])?;
        // TODO(zos-core-hypr): parse JSON via serde_json once we add the dep
        Ok(Vec::new())
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, Box<dyn Error + Send + Sync>> {
        let _raw = self.hyprctl(&["clients"])?;
        Ok(Vec::new())
    }
    fn monitors(&self) -> Result<Vec<MonitorInfo>, Box<dyn Error + Send + Sync>> {
        let _raw = self.hyprctl(&["monitors"])?;
        Ok(Vec::new())
    }
    fn active_window(&self) -> Result<Option<WindowInfo>, Box<dyn Error + Send + Sync>> {
        let _raw = self.hyprctl(&["activewindow"])?;
        Ok(None)
    }
    fn focus_window(&self, address: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.hyprctl(&[
            "dispatch",
            "focuswindow",
            &format!("address:{}", address),
        ])?;
        Ok(())
    }
    fn switch_to_workspace(&self, id: i64) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.hyprctl(&["dispatch", "workspace", &id.to_string()])?;
        Ok(())
    }
}
