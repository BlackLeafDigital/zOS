// === audio.rs — PipeWire audio config enforcement ===
//
// Reads the saved audio bus config (audio-buses-v2.json) and ensures
// PipeWire config fragments exist and pw-link connections are established.
// Runs a periodic health check to re-establish links after PipeWire restarts.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Types (duplicated from zos-settings — JSON file is the shared contract)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EqBand {
    pub freq: f32,
    pub gain: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioBusConfig {
    pub name: String,
    pub description: String,
    pub gain: f32,
    pub physical_device: String,
    pub eq_enabled: bool,
    pub eq_low: EqBand,
    pub eq_mid: EqBand,
    pub eq_high: EqBand,
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/root".into())
}

fn config_path() -> PathBuf {
    Path::new(&home_dir()).join(".config/zos/audio-buses-v2.json")
}

fn pipewire_conf_dir() -> PathBuf {
    Path::new(&home_dir()).join(".config/pipewire/pipewire.conf.d")
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

fn load_bus_configs() -> Vec<AudioBusConfig> {
    let path = config_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(configs) = serde_json::from_str::<Vec<AudioBusConfig>>(&data) {
            if !configs.is_empty() {
                return configs;
            }
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// PipeWire config fragment generation
// ---------------------------------------------------------------------------

fn bus_input_path(name: &str) -> PathBuf {
    pipewire_conf_dir().join(format!("10-zos-bus-{name}-input.conf"))
}

fn bus_output_path(name: &str) -> PathBuf {
    pipewire_conf_dir().join(format!("10-zos-bus-{name}-output.conf"))
}

fn write_bus_fragments(bus: &AudioBusConfig) {
    let dir = pipewire_conf_dir();
    let _ = std::fs::create_dir_all(&dir);

    let name = &bus.name;
    let desc = &bus.description;

    // Input null-audio-sink
    let input = format!(
        r#"context.objects = [
    {{ factory = adapter
      args = {{
          factory.name   = support.null-audio-sink
          node.name       = "{name}"
          node.description = "{desc}"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }}
    }}
]
"#
    );
    let _ = std::fs::write(bus_input_path(name), input);

    // Output node
    let out_name = format!("{name}-out");
    if bus.eq_enabled && !bus.physical_device.is_empty() {
        let target = &bus.physical_device;
        let output = format!(
            r#"context.modules = [
    {{ name = libpipewire-module-filter-chain
      args = {{
          node.description = "{desc} Output"
          filter.graph = {{
              nodes = [
                  {{ type = builtin label = bq_lowshelf name = eq_low
                    control = {{ "Freq" = {low_f} "Q" = 0.707 "Gain" = {low_g} }} }}
                  {{ type = builtin label = bq_peaking name = eq_mid
                    control = {{ "Freq" = {mid_f} "Q" = 1.0 "Gain" = {mid_g} }} }}
                  {{ type = builtin label = bq_highshelf name = eq_high
                    control = {{ "Freq" = {high_f} "Q" = 0.707 "Gain" = {high_g} }} }}
              ]
              links = [
                  {{ output = "eq_low:Out" input = "eq_mid:In" }}
                  {{ output = "eq_mid:Out" input = "eq_high:In" }}
              ]
          }}
          capture.props = {{
              node.name    = "{out_name}"
              media.class  = "Audio/Sink"
              audio.position = [ FL FR ]
          }}
          playback.props = {{
              node.name    = "{out_name}-play"
              node.target  = "{target}"
          }}
      }}
    }}
]
"#,
            low_f = bus.eq_low.freq,
            low_g = bus.eq_low.gain,
            mid_f = bus.eq_mid.freq,
            mid_g = bus.eq_mid.gain,
            high_f = bus.eq_high.freq,
            high_g = bus.eq_high.gain,
        );
        let _ = std::fs::write(bus_output_path(name), output);
    } else {
        let output = format!(
            r#"context.objects = [
    {{ factory = adapter
      args = {{
          factory.name   = support.null-audio-sink
          node.name       = "{out_name}"
          node.description = "{desc} Output"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }}
    }}
]
"#
        );
        let _ = std::fs::write(bus_output_path(name), output);
    }
}

// ---------------------------------------------------------------------------
// pw-link helpers
// ---------------------------------------------------------------------------

fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn link_exists(output_port: &str, input_port: &str) -> bool {
    let links = match run_cmd("pw-link", &["--links"]) {
        Some(s) => s,
        None => return false,
    };
    let mut current_output: Option<&str> = None;
    for line in links.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
            if let Some(out) = current_output {
                if out == output_port {
                    let inp = trimmed
                        .trim_start_matches("|->")
                        .trim_start_matches("\\->")
                        .trim();
                    if inp == input_port {
                        return true;
                    }
                }
            }
        } else {
            current_output = Some(trimmed);
        }
    }
    false
}

fn create_link(output_port: &str, input_port: &str) {
    let _ = Command::new("pw-link")
        .args([output_port, input_port])
        .status();
}

// ---------------------------------------------------------------------------
// Enforcement logic
// ---------------------------------------------------------------------------

/// Ensure pw-link connections exist for all buses. Does NOT restart PipeWire.
fn enforce_links(buses: &[AudioBusConfig]) {
    for bus in buses {
        let out_name = format!("{}-out", bus.name);

        if bus.eq_enabled {
            // Input monitor → EQ filter-chain capture
            let out_fl = format!("{}:monitor_FL", bus.name);
            let in_fl = format!("{out_name}:input_FL");
            if !link_exists(&out_fl, &in_fl) {
                tracing::info!("Re-establishing link: {out_fl} → {in_fl}");
                create_link(&out_fl, &in_fl);
            }
            let out_fr = format!("{}:monitor_FR", bus.name);
            let in_fr = format!("{out_name}:input_FR");
            if !link_exists(&out_fr, &in_fr) {
                tracing::info!("Re-establishing link: {out_fr} → {in_fr}");
                create_link(&out_fr, &in_fr);
            }
        } else if !bus.physical_device.is_empty() {
            // Input monitor → physical device playback
            let out_fl = format!("{}:monitor_FL", bus.name);
            let in_fl = format!("{}:playback_FL", bus.physical_device);
            if !link_exists(&out_fl, &in_fl) {
                tracing::info!("Re-establishing link: {out_fl} → {in_fl}");
                create_link(&out_fl, &in_fl);
            }
            let out_fr = format!("{}:monitor_FR", bus.name);
            let in_fr = format!("{}:playback_FR", bus.physical_device);
            if !link_exists(&out_fr, &in_fr) {
                tracing::info!("Re-establishing link: {out_fr} → {in_fr}");
                create_link(&out_fr, &in_fr);
            }
        } else {
            // No physical device — link through the output null-sink
            let out_fl = format!("{}:monitor_FL", bus.name);
            let in_fl = format!("{out_name}:input_FL");
            if !link_exists(&out_fl, &in_fl) {
                create_link(&out_fl, &in_fl);
            }
            let out_fr = format!("{}:monitor_FR", bus.name);
            let in_fr = format!("{out_name}:input_FR");
            if !link_exists(&out_fr, &in_fr) {
                create_link(&out_fr, &in_fr);
            }
        }
    }
}

/// Full apply: write config fragments, restart PipeWire, establish links.
fn full_apply(buses: &[AudioBusConfig]) {
    // Clean old fragments
    let dir = pipewire_conf_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("10-zos-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    for bus in buses {
        write_bus_fragments(bus);
    }

    let _ = Command::new("systemctl")
        .args(["--user", "restart", "pipewire"])
        .status();

    std::thread::sleep(std::time::Duration::from_millis(500));

    enforce_links(buses);
}

// ---------------------------------------------------------------------------
// Public API — called from main.rs via GLib timer
// ---------------------------------------------------------------------------

/// State tracked between health check ticks.
pub struct AudioEnforcer {
    last_mtime: Option<SystemTime>,
    buses: Vec<AudioBusConfig>,
    initial_applied: bool,
}

impl AudioEnforcer {
    pub fn new() -> Self {
        Self {
            last_mtime: None,
            buses: Vec::new(),
            initial_applied: false,
        }
    }

    /// Called every ~5 seconds from the GLib timer.
    pub fn tick(&mut self) {
        let current_mtime = std::fs::metadata(config_path())
            .ok()
            .and_then(|m| m.modified().ok());

        if !self.initial_applied {
            // First tick: load config and do initial apply
            self.buses = load_bus_configs();
            self.last_mtime = current_mtime;
            self.initial_applied = true;

            if !self.buses.is_empty() {
                tracing::info!("Initial audio config apply: {} buses", self.buses.len());
                // Don't do full_apply on first tick — PipeWire configs may
                // already exist from a previous session. Just ensure links.
                for bus in &self.buses {
                    if !bus_input_path(&bus.name).exists() {
                        // Config fragments missing — do a full apply
                        tracing::info!("Config fragments missing, doing full apply");
                        full_apply(&self.buses);
                        return;
                    }
                }
                // Fragments exist, just ensure links
                enforce_links(&self.buses);
            }
            return;
        }

        // Check if config file changed (user clicked Apply in settings)
        if current_mtime != self.last_mtime {
            self.last_mtime = current_mtime;
            let new_buses = load_bus_configs();
            if !new_buses.is_empty() {
                tracing::info!("Audio config changed on disk, reloading");
                self.buses = new_buses;
                // Settings already restarted PipeWire — just re-establish links
                // after a brief delay for PipeWire to settle
                std::thread::sleep(std::time::Duration::from_millis(300));
                enforce_links(&self.buses);
            }
            return;
        }

        // Periodic health check: ensure links still exist
        if !self.buses.is_empty() {
            enforce_links(&self.buses);
        }
    }
}
