//! Hyprland Compositor impl — shells out to `hyprctl -j`.
//!
//! Mirrors the pattern in `zos-dock/src/hypr.rs` but trait-implements
//! the abstract `Compositor` interface. We do NOT depend on or modify
//! zos-dock; this is a parallel implementation that shell apps consume.

use super::{Compositor, MonitorInfo, MonitorMode, WindowInfo, WorkspaceInfo};
use serde::Deserialize;
use std::error::Error;
use std::process::Command;

// --- JSON dtos that mirror hyprctl output. We collapse only the fields we care about. ---
//
// Be liberal with `Option<>` / `#[serde(default)]` because Hyprland's JSON
// shape drifts between minor releases — missing fields should not panic the
// shell.

#[derive(Debug, Deserialize)]
struct HyprWorkspace {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    monitor: String,
    #[serde(default)]
    windows: usize,
    #[serde(rename = "hasfullscreen", default)]
    _has_fullscreen: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HyprWorkspaceRef {
    id: i64,
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct HyprClient {
    address: String,
    workspace: HyprWorkspaceRef,
    /// Monitor field can be either an integer (numeric monitor id) or a
    /// string name across Hyprland versions; keep it as a `Value` and
    /// resolve at use-site.
    #[serde(default)]
    monitor: serde_json::Value,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    #[allow(dead_code)]
    pinned: bool,
}

#[derive(Debug, Deserialize)]
struct HyprMonitor {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(rename = "refreshRate", default)]
    refresh_rate: f64,
    #[serde(default = "default_scale")]
    scale: f64,
    #[serde(default)]
    focused: bool,
    #[serde(rename = "activeWorkspace", default)]
    active_workspace: Option<HyprWorkspaceRef>,
    /// Hyprland emits this as an array of strings like
    /// `"2560x1440@144.00Hz"`. Older versions sometimes omit it or
    /// return an empty array, which `#[serde(default)]` handles.
    #[serde(default, rename = "availableModes")]
    available_modes_strs: Vec<String>,
}

fn default_scale() -> f64 {
    1.0
}

/// Parse a Hyprland mode string of the form `"WxH@RR.RRHz"` into
/// a [`MonitorMode`]. Tolerates trailing whitespace and a missing
/// `Hz` suffix. Returns `None` on any structural mismatch.
fn parse_mode_string(s: &str) -> Option<MonitorMode> {
    // Examples we accept:
    //   "1920x1080@60.00Hz"
    //   "2560x1440@144.000000Hz"
    //   "1920x1080@60"           (no Hz suffix)
    //   "  1920x1080@60.00Hz \n" (surrounding whitespace)
    let s = s.trim();
    let s = s.strip_suffix("Hz").unwrap_or(s);
    let s = s.trim_end();
    let (resolution, refresh) = s.split_once('@')?;
    let (w, h) = resolution.trim().split_once('x')?;
    Some(MonitorMode {
        width: w.trim().parse().ok()?,
        height: h.trim().parse().ok()?,
        refresh_hz: refresh.trim().parse().ok()?,
    })
}

// --- Compositor impl ---

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

    /// Fetch the address of the currently focused window, if any.
    /// Returns `None` when nothing is focused, the call fails, or parse fails.
    fn active_address(&self) -> Option<String> {
        let raw = self.hyprctl(&["activewindow"]).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        v.get("address")?.as_str().map(String::from)
    }

    /// Resolve the monitor name from the polymorphic `monitor` field of a
    /// client. Falls back to `""` on unknown shapes.
    fn resolve_monitor_name(
        monitor_field: &serde_json::Value,
        id_to_name: &std::collections::HashMap<i64, String>,
    ) -> String {
        match monitor_field {
            serde_json::Value::Number(n) => n
                .as_i64()
                .and_then(|id| id_to_name.get(&id).cloned())
                .unwrap_or_default(),
            serde_json::Value::String(s) => s.clone(),
            _ => String::new(),
        }
    }
}

impl Compositor for Hyprland {
    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>, Box<dyn Error + Send + Sync>> {
        let raw = self.hyprctl(&["workspaces"])?;
        let workspaces: Vec<HyprWorkspace> = serde_json::from_str(&raw)
            .map_err(|e| format!("failed to parse hyprctl workspaces: {e}"))?;

        // Determine active workspaces — hyprctl monitors gives us per-monitor active.
        let monitors_raw = self.hyprctl(&["monitors"])?;
        let monitors: Vec<HyprMonitor> = serde_json::from_str(&monitors_raw)
            .map_err(|e| format!("failed to parse hyprctl monitors: {e}"))?;
        let active_ids: std::collections::HashSet<i64> = monitors
            .iter()
            .filter_map(|m| m.active_workspace.as_ref().map(|aw| aw.id))
            .collect();

        Ok(workspaces
            .into_iter()
            .map(|w| WorkspaceInfo {
                active: active_ids.contains(&w.id),
                id: w.id,
                name: w.name,
                monitor: w.monitor,
                windows: w.windows,
            })
            .collect())
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, Box<dyn Error + Send + Sync>> {
        let raw = self.hyprctl(&["clients"])?;
        let clients: Vec<HyprClient> = serde_json::from_str(&raw)
            .map_err(|e| format!("failed to parse hyprctl clients: {e}"))?;

        // Build a monitor-id → name map so we can resolve the polymorphic
        // `monitor` field. If monitors call fails or parses bad, fall back
        // to an empty map (string-form monitor fields still work).
        let id_to_name: std::collections::HashMap<i64, String> = self
            .hyprctl(&["monitors"])
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<HyprMonitor>>(&raw).ok())
            .map(|mons| mons.into_iter().map(|m| (m.id, m.name)).collect())
            .unwrap_or_default();

        let active_addr = self.active_address();

        Ok(clients
            .into_iter()
            .map(|c| {
                let monitor_name = Self::resolve_monitor_name(&c.monitor, &id_to_name);
                let focused = active_addr.as_deref() == Some(c.address.as_str());
                WindowInfo {
                    address: c.address,
                    workspace_id: c.workspace.id,
                    monitor: monitor_name,
                    class: c.class,
                    title: c.title,
                    focused,
                }
            })
            .collect())
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>, Box<dyn Error + Send + Sync>> {
        let raw = self.hyprctl(&["monitors"])?;
        let monitors: Vec<HyprMonitor> = serde_json::from_str(&raw)
            .map_err(|e| format!("failed to parse hyprctl monitors: {e}"))?;

        Ok(monitors
            .into_iter()
            .map(|m| MonitorInfo {
                active_workspace_id: m.active_workspace.as_ref().map(|aw| aw.id),
                id: m.id,
                name: m.name,
                width: m.width,
                height: m.height,
                refresh_rate: m.refresh_rate,
                scale: m.scale,
                focused: m.focused,
                available_modes: m
                    .available_modes_strs
                    .iter()
                    .filter_map(|s| parse_mode_string(s))
                    .collect(),
            })
            .collect())
    }

    fn active_window(&self) -> Result<Option<WindowInfo>, Box<dyn Error + Send + Sync>> {
        let raw = self.hyprctl(&["activewindow"])?;
        // hyprctl activewindow -j returns `{}` when no active window.
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "{}" {
            return Ok(None);
        }
        let c: HyprClient = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            // If the active-window shape diverges, treat as "nothing
            // focused" rather than killing the caller. The other queries
            // already cover the common case.
            Err(_) => return Ok(None),
        };

        // Resolve the monitor name. If this lookup fails for any reason
        // (e.g. hyprctl monitors transient error), fall back to empty.
        let id_to_name: std::collections::HashMap<i64, String> = self
            .hyprctl(&["monitors"])
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<HyprMonitor>>(&raw).ok())
            .map(|mons| mons.into_iter().map(|m| (m.id, m.name)).collect())
            .unwrap_or_default();
        let monitor_name = Self::resolve_monitor_name(&c.monitor, &id_to_name);

        Ok(Some(WindowInfo {
            address: c.address,
            workspace_id: c.workspace.id,
            monitor: monitor_name,
            class: c.class,
            title: c.title,
            focused: true,
        }))
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

// ---------------------------------------------------------------------------
// Tests — pure JSON parsing, no hyprctl invocation.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspaces_json() {
        let raw = r#"[
            {
                "id": 1,
                "name": "1",
                "monitor": "DP-1",
                "monitorID": 0,
                "windows": 3,
                "hasfullscreen": false,
                "lastwindow": "0xabc",
                "lastwindowtitle": "term"
            },
            {
                "id": 2,
                "name": "2",
                "monitor": "DP-2",
                "monitorID": 1,
                "windows": 0,
                "hasfullscreen": false,
                "lastwindow": "0x0",
                "lastwindowtitle": ""
            }
        ]"#;
        let parsed: Vec<HyprWorkspace> = serde_json::from_str(raw).expect("workspaces parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, 1);
        assert_eq!(parsed[0].name, "1");
        assert_eq!(parsed[0].monitor, "DP-1");
        assert_eq!(parsed[0].windows, 3);
        assert_eq!(parsed[1].id, 2);
        assert_eq!(parsed[1].monitor, "DP-2");
    }

    #[test]
    fn parses_clients_json_with_numeric_monitor() {
        let raw = r#"[
            {
                "address": "0xabc",
                "mapped": true,
                "hidden": false,
                "at": [100, 200],
                "size": [800, 600],
                "workspace": { "id": 1, "name": "1" },
                "monitor": 0,
                "class": "wezterm",
                "title": "Terminal",
                "initialClass": "wezterm",
                "initialTitle": "Terminal",
                "pid": 12345,
                "xwayland": false,
                "pinned": false,
                "fullscreen": 0,
                "fullscreenClient": 0,
                "grouped": [],
                "tags": [],
                "swallowing": "0x0",
                "focusHistoryID": 0,
                "inhibitingIdle": false,
                "xdgTag": null,
                "xdgDescription": null
            }
        ]"#;
        let parsed: Vec<HyprClient> = serde_json::from_str(raw).expect("clients parse");
        assert_eq!(parsed.len(), 1);
        let c = &parsed[0];
        assert_eq!(c.address, "0xabc");
        assert_eq!(c.workspace.id, 1);
        assert_eq!(c.class, "wezterm");
        assert_eq!(c.title, "Terminal");
        assert!(c.monitor.is_number());
        assert_eq!(c.monitor.as_i64(), Some(0));
    }

    #[test]
    fn parses_clients_json_with_string_monitor() {
        // Some Hyprland versions emit the monitor field as a string name.
        let raw = r#"[
            {
                "address": "0xdef",
                "workspace": { "id": 2, "name": "2" },
                "monitor": "DP-1",
                "class": "firefox",
                "title": "Mozilla",
                "pinned": false
            }
        ]"#;
        let parsed: Vec<HyprClient> = serde_json::from_str(raw).expect("clients parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].monitor.as_str(), Some("DP-1"));
    }

    #[test]
    fn parses_monitors_json() {
        let raw = r#"[
            {
                "id": 0,
                "name": "DP-1",
                "description": "Acer 27in",
                "make": "Acer",
                "model": "XB271HU",
                "serial": "ABC123",
                "width": 2560,
                "height": 1440,
                "refreshRate": 144.0,
                "x": 0,
                "y": 0,
                "activeWorkspace": { "id": 1, "name": "1" },
                "specialWorkspace": { "id": 0, "name": "" },
                "reserved": [0,0,0,0],
                "scale": 1.0,
                "transform": 0,
                "focused": true,
                "dpmsStatus": true,
                "vrr": false,
                "activelyTearing": false,
                "disabled": false,
                "currentFormat": "XRGB8888",
                "availableModes": []
            }
        ]"#;
        let parsed: Vec<HyprMonitor> = serde_json::from_str(raw).expect("monitors parse");
        assert_eq!(parsed.len(), 1);
        let m = &parsed[0];
        assert_eq!(m.id, 0);
        assert_eq!(m.name, "DP-1");
        assert_eq!(m.width, 2560);
        assert_eq!(m.height, 1440);
        assert!((m.refresh_rate - 144.0).abs() < f64::EPSILON);
        assert!((m.scale - 1.0).abs() < f64::EPSILON);
        assert!(m.focused);
        assert_eq!(m.active_workspace.as_ref().map(|aw| aw.id), Some(1));
    }

    #[test]
    fn resolves_monitor_name_numeric() {
        let mut id_to_name = std::collections::HashMap::new();
        id_to_name.insert(0, "DP-1".to_string());
        id_to_name.insert(1, "HDMI-A-1".to_string());

        let n = serde_json::Value::Number(serde_json::Number::from(1i64));
        assert_eq!(Hyprland::resolve_monitor_name(&n, &id_to_name), "HDMI-A-1");

        let s = serde_json::Value::String("DP-2".to_string());
        assert_eq!(Hyprland::resolve_monitor_name(&s, &id_to_name), "DP-2");

        let null = serde_json::Value::Null;
        assert_eq!(Hyprland::resolve_monitor_name(&null, &id_to_name), "");
    }

    #[test]
    fn handles_missing_optional_fields() {
        // workspace with only the required `id` — everything else defaulted.
        let raw = r#"[ { "id": 5 } ]"#;
        let parsed: Vec<HyprWorkspace> = serde_json::from_str(raw).expect("minimal workspace");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, 5);
        assert_eq!(parsed[0].name, "");
        assert_eq!(parsed[0].windows, 0);
    }

    #[test]
    fn parses_mode_string_variants() {
        // Standard form Hyprland emits.
        let m = parse_mode_string("1920x1080@60.00Hz").expect("standard");
        assert_eq!(m.width, 1920);
        assert_eq!(m.height, 1080);
        assert!((m.refresh_hz - 60.0).abs() < 1e-6);

        // Long fractional refresh (some monitors report 6-digit values).
        let m = parse_mode_string("2560x1440@144.000000Hz").expect("fractional");
        assert_eq!(m.width, 2560);
        assert_eq!(m.height, 1440);
        assert!((m.refresh_hz - 144.0).abs() < 1e-6);

        // No "Hz" suffix.
        let m = parse_mode_string("1920x1080@60").expect("no-Hz");
        assert_eq!(m.width, 1920);
        assert_eq!(m.height, 1080);
        assert!((m.refresh_hz - 60.0).abs() < 1e-6);

        // Surrounding whitespace.
        let m = parse_mode_string("  3840x2160@59.94Hz  \n").expect("whitespace");
        assert_eq!(m.width, 3840);
        assert_eq!(m.height, 2160);
        assert!((m.refresh_hz - 59.94).abs() < 1e-6);

        // Garbage input should return None, not panic.
        assert!(parse_mode_string("").is_none());
        assert!(parse_mode_string("not a mode").is_none());
        assert!(parse_mode_string("1920x@60Hz").is_none());
        assert!(parse_mode_string("1920x1080@Hz").is_none());
    }

    #[test]
    fn parses_available_modes_array() {
        // Sample availableModes payload similar to what hyprctl emits on
        // a real monitor that supports multiple modes.
        let raw = r#"[
            {
                "id": 0,
                "name": "DP-1",
                "width": 2560,
                "height": 1440,
                "refreshRate": 144.0,
                "scale": 1.0,
                "focused": true,
                "availableModes": [
                    "2560x1440@144.00Hz",
                    "2560x1440@120.00Hz",
                    "1920x1080@60.00Hz",
                    "garbage",
                    "1280x720@59.94Hz"
                ]
            }
        ]"#;
        let parsed: Vec<HyprMonitor> = serde_json::from_str(raw).expect("monitors parse");
        let m = &parsed[0];
        assert_eq!(m.available_modes_strs.len(), 5);

        let modes: Vec<MonitorMode> = m
            .available_modes_strs
            .iter()
            .filter_map(|s| parse_mode_string(s))
            .collect();
        // Garbage entry filtered out — 4 valid modes remain.
        assert_eq!(modes.len(), 4);
        assert_eq!(modes[0].width, 2560);
        assert_eq!(modes[0].height, 1440);
        assert!((modes[0].refresh_hz - 144.0).abs() < 1e-6);
        assert_eq!(modes[3].width, 1280);
        assert!((modes[3].refresh_hz - 59.94).abs() < 1e-6);
    }

    #[test]
    fn monitor_mode_displays_human_readable() {
        let m = MonitorMode {
            width: 2560,
            height: 1440,
            refresh_hz: 144.0,
        };
        assert_eq!(format!("{m}"), "2560x1440 @ 144.00 Hz");
    }
}
