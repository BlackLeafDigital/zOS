// === pipewire.rs — PipeWire / WirePlumber audio service layer ===
//
// Wraps wpctl and pw-link commands for audio device management.
// All functions are synchronous and shell out to the CLI tools,
// returning sensible defaults on failure.

use std::process::Command;

/// Describes an audio device reported by WirePlumber.
#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
    pub device_type: DeviceType,
    pub is_default: bool,
    pub volume: Option<f32>,
    pub muted: bool,
}

/// Whether a device is a sink (output) or source (input).
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Sink,
    Source,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Run a command and return its stdout as a String, or None on failure.
fn run_cmd(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Parse the "Sinks:" or "Sources:" section from `wpctl status` output.
///
/// Lines inside a section look like:
/// ```text
///  *   46. HyperX QuadCast S Analog Stereo      [vol: 0.50]
///      47. USB Audio Speakers                     [vol: 1.00 MUTED]
/// ```
fn parse_device_section(status: &str, section: &str, device_type: DeviceType) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    let mut in_section = false;

    for line in status.lines() {
        // Strip tree-drawing characters first so all checks work uniformly.
        let cleaned: String = line
            .replace('│', " ")
            .replace('├', " ")
            .replace('└', " ")
            .replace('─', " ");
        let cleaned = cleaned.trim();

        if cleaned.is_empty() {
            continue;
        }

        // Detect section header — e.g. "Sinks:" or "Sources:"
        if cleaned.ends_with(':') {
            if cleaned == section {
                in_section = true;
                continue;
            } else if in_section {
                // We've reached the next section
                break;
            }
            continue;
        }

        if !in_section {
            continue;
        }

        // Detect default marker
        let is_default = cleaned.starts_with('*');
        let cleaned = cleaned.trim_start_matches('*').trim();

        // Expect: <id>. <name> [vol: <vol>]  or  <id>. <name> [vol: <vol> MUTED]
        let dot_pos = match cleaned.find('.') {
            Some(p) => p,
            None => continue,
        };

        let id: u32 = match cleaned[..dot_pos].trim().parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        let rest = cleaned[dot_pos + 1..].trim();

        // Extract volume info from brackets
        let (name, volume, muted) = if let Some(bracket_start) = rest.rfind('[') {
            let name = rest[..bracket_start].trim().to_string();
            let bracket_content = rest[bracket_start..]
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim();

            let muted = bracket_content.contains("MUTED");

            let volume = bracket_content
                .strip_prefix("vol:")
                .and_then(|s| s.replace("MUTED", "").trim().parse::<f32>().ok());

            (name, volume, muted)
        } else {
            (rest.to_string(), None, false)
        };

        devices.push(AudioDevice {
            id,
            name,
            device_type: device_type.clone(),
            is_default,
            volume,
            muted,
        });
    }

    devices
}

/// Grab the full `wpctl status` text.
fn wpctl_status() -> Option<String> {
    run_cmd("wpctl", &["status"])
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// List all audio sinks (outputs).
pub fn list_sinks() -> Vec<AudioDevice> {
    wpctl_status()
        .map(|s| parse_device_section(&s, "Sinks:", DeviceType::Sink))
        .unwrap_or_default()
}

/// List all audio sources (inputs).
pub fn list_sources() -> Vec<AudioDevice> {
    wpctl_status()
        .map(|s| parse_device_section(&s, "Sources:", DeviceType::Source))
        .unwrap_or_default()
}

/// Get volume of the default audio sink (0.0 – 1.5+).
///
/// Parses the output of `wpctl get-volume @DEFAULT_AUDIO_SINK@`
/// which looks like: `Volume: 0.50` or `Volume: 0.50 [MUTED]`.
pub fn get_default_volume() -> Option<f32> {
    let output = run_cmd("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
    // "Volume: 0.50" or "Volume: 0.50 [MUTED]"
    output
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<f32>().ok())
}

/// Check whether the default audio sink is muted.
pub fn is_default_muted() -> bool {
    run_cmd("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .map(|s| s.contains("[MUTED]"))
        .unwrap_or(false)
}

/// Set absolute volume for a device (0.0 – 1.5).
pub fn set_volume(device_id: u32, volume: f32) {
    let vol = volume.clamp(0.0, 1.5);
    let _ = Command::new("wpctl")
        .args(["set-volume", &device_id.to_string(), &format!("{vol:.2}")])
        .status();
}

/// Toggle mute for a device.
pub fn toggle_mute(device_id: u32) {
    let _ = Command::new("wpctl")
        .args(["set-mute", &device_id.to_string(), "toggle"])
        .status();
}

/// Set a device as the default for its type.
pub fn set_default(device_id: u32) {
    let _ = Command::new("wpctl")
        .args(["set-default", &device_id.to_string()])
        .status();
}

/// An active audio playback stream (application currently producing audio).
#[derive(Debug, Clone)]
pub struct AudioStream {
    pub id: u32,
    pub name: String,
}

/// List currently-active playback streams by parsing the `Streams:` section of
/// `wpctl status`.  Each stream line looks like:
///
/// ```text
///  └─ Streams:
///         71. Floorp
///             46. → HyperX QuadCast Analog Stereo
/// ```
///
/// We capture the top-level numbered entries (the app) and ignore the
/// indented sub-entries (the sink they are connected to).
pub fn list_streams() -> Vec<AudioStream> {
    let status = match wpctl_status() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut streams = Vec::new();
    let mut in_streams = false;

    for line in status.lines() {
        let cleaned: String = line
            .replace('\u{2502}', " ") // │
            .replace('\u{251c}', " ") // ├
            .replace('\u{2514}', " ") // └
            .replace('\u{2500}', " "); // ─
        let trimmed = cleaned.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Detect section headers
        if trimmed.ends_with(':') {
            if trimmed == "Streams:" {
                in_streams = true;
                continue;
            } else if in_streams {
                break;
            }
            continue;
        }

        if !in_streams {
            continue;
        }

        // Skip sub-entries that start with an arrow (→) after the id — these
        // are the target sink, not the app itself.
        if trimmed.contains("\u{2192}") {
            continue;
        }

        // Strip default marker
        let cleaned_line = trimmed.trim_start_matches('*').trim();

        let dot_pos = match cleaned_line.find('.') {
            Some(p) => p,
            None => continue,
        };

        let id: u32 = match cleaned_line[..dot_pos].trim().parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        let rest = cleaned_line[dot_pos + 1..].trim();
        // Strip any trailing bracket info like [vol: 0.50]
        let name = if let Some(bracket) = rest.rfind('[') {
            rest[..bracket].trim().to_string()
        } else {
            rest.to_string()
        };

        if name.is_empty() {
            continue;
        }

        streams.push(AudioStream { id, name });
    }

    streams
}

/// Route a stream to a virtual sink by disconnecting its current output links
/// and creating new ones to the target sink.
///
/// `stream_id` is the PipeWire node id of the playback stream.
/// `sink_name` is the node name of the target sink (e.g. "zos-music").
///
/// Uses `wpctl set-default` on the sink first, then `pw-link` to wire things up.
pub fn route_stream_to_sink(stream_id: u32, sink_name: &str) {
    // Use pw-link to find the stream's output ports and the sink's input ports,
    // then connect them.  First, disconnect existing links from this stream.
    let stream_id_str = stream_id.to_string();

    // Get output ports for this stream node
    let stream_ports: Vec<String> = run_cmd("pw-link", &["--output", "--id"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let l = l.trim();
                    // Lines look like: "  42 node_name:port_name"
                    // We want ports belonging to the stream node.
                    // pw-link --output --id shows: <id> <port>
                    let parts: Vec<&str> = l.splitn(2, char::is_whitespace).collect();
                    if parts.len() == 2 {
                        let port = parts[1].trim();
                        // Check if this port belongs to our stream node
                        // Port names start with the node name or we can check node ids
                        Some(port.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Find which output ports belong to this stream's node by querying pw-cli
    let node_ports: Vec<String> = run_cmd("pw-link", &["--output"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let l = l.trim();
                    if l.is_empty() {
                        return None;
                    }
                    Some(l.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    // Get the stream's node name from pw-cli info
    let stream_node_name = run_cmd("pw-cli", &["info", &stream_id_str]).and_then(|info| {
        for line in info.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("node.name") {
                // node.name = "Firefox"
                if let Some(val) = trimmed.split('=').nth(1) {
                    return Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
                }
            }
        }
        None
    });

    let stream_prefix = match stream_node_name {
        Some(ref n) => n.clone(),
        None => return, // Can't identify stream ports
    };

    // Find output ports belonging to this stream
    let my_outputs: Vec<String> = node_ports
        .iter()
        .filter(|p| p.starts_with(&stream_prefix))
        .cloned()
        .collect();

    if my_outputs.is_empty() {
        // Fallback: try moving via wpctl
        let _ = Command::new("wpctl")
            .args(["set-default", &stream_id_str])
            .status();
        return;
    }

    // Find input ports belonging to the target sink
    let sink_inputs: Vec<String> = run_cmd("pw-link", &["--input"])
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let l = l.trim();
                    if l.starts_with(sink_name) {
                        Some(l.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    if sink_inputs.is_empty() {
        return;
    }

    // Disconnect existing links from our stream's output ports
    if let Some(links_output) = run_cmd("pw-link", &["--links"]) {
        let mut current_output: Option<String> = None;
        for line in links_output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
                if let Some(ref out) = current_output {
                    if my_outputs.iter().any(|p| p == out) {
                        let input = trimmed
                            .trim_start_matches("|->")
                            .trim_start_matches("\\->")
                            .trim();
                        remove_link(out, input);
                    }
                }
            } else {
                current_output = Some(trimmed.to_string());
            }
        }
    }

    // Create new links: pair FL->FL, FR->FR by matching channel suffixes
    for out_port in &my_outputs {
        // Get the channel suffix (e.g. ":output_FL")
        let out_channel = out_port.rsplit(':').next().unwrap_or("");
        let channel_suffix = out_channel.trim_start_matches("output_");

        // Find matching input port
        for inp_port in &sink_inputs {
            let inp_channel = inp_port.rsplit(':').next().unwrap_or("");
            let inp_suffix = inp_channel.trim_start_matches("input_");
            if channel_suffix == inp_suffix {
                create_link(out_port, inp_port);
                break;
            }
        }
    }

    // Also disconnect from any unused stream_ports
    drop(stream_ports);
}

/// List all available output ports (sources of audio data).
#[allow(dead_code)]
pub fn list_output_ports() -> Vec<String> {
    run_cmd("pw-link", &["--output"])
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// List all available input ports (destinations for audio data).
#[allow(dead_code)]
pub fn list_input_ports() -> Vec<String> {
    run_cmd("pw-link", &["--input"])
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// List all PipeWire links as `(output_port, input_port)` pairs.
///
/// Parses the output of `pw-link --links`, which lists connected
/// port pairs one per line as `output -> input`.
#[allow(dead_code)]
pub fn list_links() -> Vec<(String, String)> {
    let output = match run_cmd("pw-link", &["--links"]) {
        Some(o) => o,
        None => return Vec::new(),
    };

    let mut links = Vec::new();
    let mut current_output: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Lines starting with a pipe/arrow indicate an input linked to the
        // previous output.  The exact format of `pw-link --links` is:
        //   output_port
        //      |-> input_port
        if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
            if let Some(ref out) = current_output {
                let input = trimmed
                    .trim_start_matches("|->")
                    .trim_start_matches("\\->")
                    .trim();
                if !input.is_empty() {
                    links.push((out.clone(), input.to_string()));
                }
            }
        } else {
            // This is an output port name.
            current_output = Some(trimmed.to_string());
        }
    }

    links
}

/// Create a link between two PipeWire ports.  Returns `true` on success.
pub fn create_link(output_port: &str, input_port: &str) -> bool {
    Command::new("pw-link")
        .args([output_port, input_port])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Remove a link between two PipeWire ports.  Returns `true` on success.
pub fn remove_link(output_port: &str, input_port: &str) -> bool {
    Command::new("pw-link")
        .args(["-d", output_port, input_port])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Dynamic audio bus management
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for a virtual audio bus.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BusConfig {
    pub name: String,
    pub description: String,
    pub target: BusTarget,
}

/// Where a bus routes its audio.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BusTarget {
    PhysicalSink(String),
    Bus(String),
}

// ---------------------------------------------------------------------------
// New Voicemeeter-style config types
// ---------------------------------------------------------------------------

/// A single parametric EQ band.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EqBand {
    pub freq: f32,
    pub gain: f32,
}

/// Virtual Input strip configuration.
/// A null audio sink that applications route their audio to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputConfig {
    pub name: String,
    pub description: String,
    pub gain: f32,
    pub outputs: Vec<String>,
}

/// Virtual Output strip configuration.
/// Routes to a single physical audio adapter, optionally with EQ effects.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutputConfig {
    pub name: String,
    pub description: String,
    pub physical_device: String,
    pub eq_enabled: bool,
    pub eq_low: EqBand,
    pub eq_mid: EqBand,
    pub eq_high: EqBand,
}

/// Path to the zOS audio bus configuration file.
pub fn bus_configs_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/root"));
    PathBuf::from(home).join(".config/zos/audio-buses.json")
}

/// Load bus configurations from disk, returning sensible defaults if the file
/// doesn't exist or can't be parsed.
pub fn load_bus_configs() -> Vec<BusConfig> {
    let path = bus_configs_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(configs) = serde_json::from_str::<Vec<BusConfig>>(&data) {
            return configs;
        }
    }
    vec![
        BusConfig {
            name: "zos-main".into(),
            description: "Main Output".into(),
            target: BusTarget::PhysicalSink(String::new()),
        },
        BusConfig {
            name: "zos-music".into(),
            description: "Music".into(),
            target: BusTarget::PhysicalSink(String::new()),
        },
        BusConfig {
            name: "zos-chat".into(),
            description: "Chat / Voice".into(),
            target: BusTarget::PhysicalSink(String::new()),
        },
    ]
}

/// Persist bus configurations to disk as JSON.
#[allow(dead_code)]
pub fn save_bus_configs(buses: &[BusConfig]) {
    let path = bus_configs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(buses) {
        let _ = std::fs::write(&path, json);
    }
}

// ---------------------------------------------------------------------------
// Virtual Input / Output config persistence
// ---------------------------------------------------------------------------

fn zos_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/root"));
    PathBuf::from(home).join(".config/zos")
}

fn pipewire_conf_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/root"));
    PathBuf::from(home).join(".config/pipewire/pipewire.conf.d")
}

pub fn input_configs_path() -> PathBuf {
    zos_config_dir().join("audio-inputs.json")
}

pub fn output_configs_path() -> PathBuf {
    zos_config_dir().join("audio-outputs.json")
}

pub fn pipewire_input_config_path(name: &str) -> PathBuf {
    pipewire_conf_dir().join(format!("10-zos-input-{name}.conf"))
}

pub fn pipewire_output_config_path(name: &str) -> PathBuf {
    pipewire_conf_dir().join(format!("10-zos-output-{name}.conf"))
}

/// Load Virtual Input configs. Migrates from old audio-buses.json if needed.
pub fn load_input_configs() -> Vec<InputConfig> {
    let path = input_configs_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(configs) = serde_json::from_str::<Vec<InputConfig>>(&data) {
            return configs;
        }
    }
    // Migration: convert old BusConfig format
    let old = load_bus_configs();
    if !old.is_empty() {
        return old
            .iter()
            .map(|b| InputConfig {
                name: b.name.clone(),
                description: b.description.clone(),
                gain: 0.0,
                outputs: vec!["zos-out-a1".into()],
            })
            .collect();
    }
    default_input_configs()
}

fn default_input_configs() -> Vec<InputConfig> {
    vec![
        InputConfig {
            name: "zos-main".into(),
            description: "Main".into(),
            gain: 0.0,
            outputs: vec!["zos-out-a1".into()],
        },
        InputConfig {
            name: "zos-music".into(),
            description: "Music".into(),
            gain: 0.0,
            outputs: vec!["zos-out-a1".into()],
        },
        InputConfig {
            name: "zos-chat".into(),
            description: "Chat / Voice".into(),
            gain: 0.0,
            outputs: vec!["zos-out-a1".into()],
        },
    ]
}

pub fn save_input_configs(configs: &[InputConfig]) {
    let path = input_configs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(configs) {
        let _ = std::fs::write(&path, json);
    }
}

/// Load Virtual Output configs with sensible defaults.
pub fn load_output_configs() -> Vec<OutputConfig> {
    let path = output_configs_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(configs) = serde_json::from_str::<Vec<OutputConfig>>(&data) {
            return configs;
        }
    }
    default_output_configs()
}

fn default_output_configs() -> Vec<OutputConfig> {
    let sinks = list_sinks();
    let physical: Vec<_> = sinks.iter().filter(|s| !s.name.starts_with("zos-")).collect();
    let first = physical.first().map(|s| s.name.clone()).unwrap_or_default();
    let second = physical.get(1).map(|s| s.name.clone()).unwrap_or_default();

    let mut outputs = vec![OutputConfig {
        name: "zos-out-a1".into(),
        description: "A1".into(),
        physical_device: first,
        eq_enabled: false,
        eq_low: EqBand { freq: 200.0, gain: 0.0 },
        eq_mid: EqBand { freq: 1000.0, gain: 0.0 },
        eq_high: EqBand { freq: 8000.0, gain: 0.0 },
    }];
    if !second.is_empty() {
        outputs.push(OutputConfig {
            name: "zos-out-a2".into(),
            description: "A2".into(),
            physical_device: second,
            eq_enabled: false,
            eq_low: EqBand { freq: 200.0, gain: 0.0 },
            eq_mid: EqBand { freq: 1000.0, gain: 0.0 },
            eq_high: EqBand { freq: 8000.0, gain: 0.0 },
        });
    }
    outputs
}

pub fn save_output_configs(configs: &[OutputConfig]) {
    let path = output_configs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(configs) {
        let _ = std::fs::write(&path, json);
    }
}

/// List physical (non-zos) sinks only.
pub fn list_physical_sinks() -> Vec<AudioDevice> {
    list_sinks()
        .into_iter()
        .filter(|s| !s.name.starts_with("zos-"))
        .collect()
}

/// Path to the PipeWire config fragment for a virtual bus.
#[allow(dead_code)]
pub fn pipewire_bus_config_path(bus_name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/root"));
    PathBuf::from(home).join(format!(
        ".config/pipewire/pipewire.conf.d/10-zos-bus-{bus_name}.conf"
    ))
}

/// Create a virtual null audio sink via a PipeWire config fragment and restart
/// PipeWire so it picks up the change.
#[allow(dead_code)]
pub fn create_virtual_sink(name: &str, description: &str) {
    let path = pipewire_bus_config_path(name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let config = format!(
        r#"context.objects = [
    {{ factory = adapter
      args = {{
          factory.name   = support.null-audio-sink
          node.name       = "{name}"
          node.description = "{description}"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }}
    }}
]
"#
    );

    let _ = std::fs::write(&path, config);
    let _ = Command::new("systemctl")
        .args(["--user", "restart", "pipewire"])
        .status();
}

/// Remove a virtual sink's config fragment and restart PipeWire.
#[allow(dead_code)]
pub fn remove_virtual_sink(name: &str) {
    let path = pipewire_bus_config_path(name);
    let _ = std::fs::remove_file(&path);
    let _ = Command::new("systemctl")
        .args(["--user", "restart", "pipewire"])
        .status();
}

/// Query `pw-link --links` to find what a bus's monitor ports are connected to.
///
/// Returns the target node name (the part before `:playback_` or `:input_`).
#[allow(dead_code)]
pub fn get_bus_routing(bus_name: &str) -> Option<String> {
    let output = run_cmd("pw-link", &["--links"])?;
    let monitor_prefix = format!("{bus_name}:monitor_FL");

    let mut found_our_port = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
            if found_our_port {
                let target = trimmed
                    .trim_start_matches("|->")
                    .trim_start_matches("\\->")
                    .trim();
                // Extract node name before `:playback_` or `:input_`
                if let Some(colon) = target.find(":playback_").or_else(|| target.find(":input_")) {
                    return Some(target[..colon].to_string());
                }
                return Some(target.to_string());
            }
        } else {
            found_our_port = trimmed == monitor_prefix;
        }
    }

    None
}

/// Disconnect any existing monitor links from a bus and connect it to the
/// given target (physical sink or another bus).
#[allow(dead_code)]
pub fn route_bus_to_target(bus_name: &str, target: &BusTarget) {
    // Disconnect existing monitor links from this bus
    if let Some(links_output) = run_cmd("pw-link", &["--links"]) {
        let monitor_fl = format!("{bus_name}:monitor_FL");
        let monitor_fr = format!("{bus_name}:monitor_FR");

        let mut current_output: Option<String> = None;
        for line in links_output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
                if let Some(ref out) = current_output {
                    if *out == monitor_fl || *out == monitor_fr {
                        let input = trimmed
                            .trim_start_matches("|->")
                            .trim_start_matches("\\->")
                            .trim();
                        remove_link(out, input);
                    }
                }
            } else {
                current_output = Some(trimmed.to_string());
            }
        }
    }

    // Create new links based on target type
    match target {
        BusTarget::PhysicalSink(sink_name) => {
            if !sink_name.is_empty() {
                create_link(
                    &format!("{bus_name}:monitor_FL"),
                    &format!("{sink_name}:playback_FL"),
                );
                create_link(
                    &format!("{bus_name}:monitor_FR"),
                    &format!("{sink_name}:playback_FR"),
                );
            }
        }
        BusTarget::Bus(other_bus) => {
            if !other_bus.is_empty() {
                create_link(
                    &format!("{bus_name}:monitor_FL"),
                    &format!("{other_bus}:input_FL"),
                );
                create_link(
                    &format!("{bus_name}:monitor_FR"),
                    &format!("{other_bus}:input_FR"),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// New Input/Output PipeWire config generation + routing
// ---------------------------------------------------------------------------

/// Write a PipeWire null-audio-sink config fragment for a Virtual Input.
pub fn create_input_sink(config: &InputConfig) {
    let path = pipewire_input_config_path(&config.name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let name = &config.name;
    let desc = &config.description;
    let content = format!(
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
    let _ = std::fs::write(&path, content);
}

/// Remove a Virtual Input's PipeWire config fragment.
pub fn remove_input_sink(name: &str) {
    let _ = std::fs::remove_file(pipewire_input_config_path(name));
}

/// Write a PipeWire config fragment for a Virtual Output.
/// When EQ is enabled, creates a filter-chain with 3-band parametric EQ.
/// When disabled, creates a simple null-audio-sink.
pub fn create_output_node(config: &OutputConfig) {
    let path = pipewire_output_config_path(&config.name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let name = &config.name;
    let desc = &config.description;

    if config.eq_enabled && !config.physical_device.is_empty() {
        let target = &config.physical_device;
        let content = format!(
            r#"context.modules = [
    {{ name = libpipewire-module-filter-chain
      args = {{
          node.description = "{desc}"
          filter.graph = {{
              nodes = [
                  {{ type = builtin label = bq_lowshelf name = eq_low
                    control = {{ "Freq" = {low_freq} "Q" = 0.707 "Gain" = {low_gain} }} }}
                  {{ type = builtin label = bq_peaking name = eq_mid
                    control = {{ "Freq" = {mid_freq} "Q" = 1.0 "Gain" = {mid_gain} }} }}
                  {{ type = builtin label = bq_highshelf name = eq_high
                    control = {{ "Freq" = {high_freq} "Q" = 0.707 "Gain" = {high_gain} }} }}
              ]
              links = [
                  {{ output = "eq_low:Out" input = "eq_mid:In" }}
                  {{ output = "eq_mid:Out" input = "eq_high:In" }}
              ]
          }}
          capture.props = {{
              node.name    = "{name}"
              media.class  = "Audio/Sink"
              audio.position = [ FL FR ]
          }}
          playback.props = {{
              node.name    = "{name}-play"
              node.target  = "{target}"
          }}
      }}
    }}
]
"#,
            low_freq = config.eq_low.freq,
            low_gain = config.eq_low.gain,
            mid_freq = config.eq_mid.freq,
            mid_gain = config.eq_mid.gain,
            high_freq = config.eq_high.freq,
            high_gain = config.eq_high.gain,
        );
        let _ = std::fs::write(&path, content);
    } else {
        let content = format!(
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
        let _ = std::fs::write(&path, content);
    }
}

/// Remove a Virtual Output's PipeWire config fragment.
pub fn remove_output_node(name: &str) {
    let _ = std::fs::remove_file(pipewire_output_config_path(name));
}

/// Disconnect all monitor_FL/FR links from a given node.
pub fn disconnect_monitor_links(node_name: &str) {
    if let Some(links_output) = run_cmd("pw-link", &["--links"]) {
        let monitor_fl = format!("{node_name}:monitor_FL");
        let monitor_fr = format!("{node_name}:monitor_FR");

        let mut current_output: Option<String> = None;
        for line in links_output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("|->") || trimmed.starts_with("\\->") {
                if let Some(ref out) = current_output {
                    if *out == monitor_fl || *out == monitor_fr {
                        let input = trimmed
                            .trim_start_matches("|->")
                            .trim_start_matches("\\->")
                            .trim();
                        remove_link(out, input);
                    }
                }
            } else {
                current_output = Some(trimmed.to_string());
            }
        }
    }
}

/// Route a Virtual Input's monitor ports to multiple Virtual Outputs.
pub fn route_input_to_outputs(input_name: &str, output_names: &[String]) {
    disconnect_monitor_links(input_name);
    for output_name in output_names {
        if output_name.is_empty() {
            continue;
        }
        create_link(
            &format!("{input_name}:monitor_FL"),
            &format!("{output_name}:input_FL"),
        );
        create_link(
            &format!("{input_name}:monitor_FR"),
            &format!("{output_name}:input_FR"),
        );
    }
}

/// Route a Virtual Output to its physical device (non-EQ path only).
pub fn route_output_to_device(output_name: &str, device_name: &str) {
    if device_name.is_empty() {
        return;
    }
    disconnect_monitor_links(output_name);
    create_link(
        &format!("{output_name}:monitor_FL"),
        &format!("{device_name}:playback_FL"),
    );
    create_link(
        &format!("{output_name}:monitor_FR"),
        &format!("{device_name}:playback_FR"),
    );
}

/// Clean up old-format bus config fragments (10-zos-bus-*).
fn cleanup_old_bus_configs() {
    let dir = pipewire_conf_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("10-zos-bus-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Write all PipeWire config fragments, restart PipeWire, and establish links.
pub fn apply_full_config(inputs: &[InputConfig], outputs: &[OutputConfig]) {
    cleanup_old_bus_configs();

    // Also clean existing input/output fragments to handle removals
    let dir = pipewire_conf_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("10-zos-input-") || name.starts_with("10-zos-output-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    for input in inputs {
        create_input_sink(input);
    }
    for output in outputs {
        create_output_node(output);
    }

    save_input_configs(inputs);
    save_output_configs(outputs);

    let _ = Command::new("systemctl")
        .args(["--user", "restart", "pipewire"])
        .status();

    // Wait for PipeWire to come back up
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Establish pw-link connections
    for input in inputs {
        route_input_to_outputs(&input.name, &input.outputs);
    }
    for output in outputs {
        if !output.eq_enabled {
            route_output_to_device(&output.name, &output.physical_device);
        }
    }
}

/// Path to the per-app routing defaults file.
pub fn app_routing_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/root"));
    PathBuf::from(home).join(".config/zos/app-routing.json")
}

/// Load the mapping of application names to their default bus.
pub fn load_app_routing_defaults() -> HashMap<String, String> {
    let path = app_routing_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&data) {
            return map;
        }
    }
    HashMap::new()
}

/// Insert or update a per-app routing default.  If `bus_name` is empty, the
/// entry is removed instead.
pub fn save_app_routing_default(app_name: &str, bus_name: &str) {
    let mut map = load_app_routing_defaults();

    if bus_name.is_empty() {
        map.remove(app_name);
    } else {
        map.insert(app_name.to_string(), bus_name.to_string());
    }

    let path = app_routing_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(&path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sinks_section() {
        let status = r#"
PipeWire 'pipewire-0' [1.2.0, user@host, cookie:12345]
 └─ Clients:

Audio
 ├─ Devices:
 │
 ├─ Sinks:
 │      *   46. HyperX QuadCast S Analog Stereo              [vol: 0.50]
 │          47. USB Audio Speakers                             [vol: 1.00 MUTED]
 │
 ├─ Sources:
 │      *   48. HyperX QuadCast S Mono                        [vol: 0.75]
 │
 └─ Streams:
"#;

        let sinks = parse_device_section(status, "Sinks:", DeviceType::Sink);
        assert_eq!(sinks.len(), 2);

        assert_eq!(sinks[0].id, 46);
        assert!(sinks[0].is_default);
        assert_eq!(sinks[0].volume, Some(0.50));
        assert!(!sinks[0].muted);

        assert_eq!(sinks[1].id, 47);
        assert!(!sinks[1].is_default);
        assert_eq!(sinks[1].volume, Some(1.00));
        assert!(sinks[1].muted);
    }

    #[test]
    fn parse_sources_section() {
        let status = r#"
Audio
 ├─ Sinks:
 │      *   46. Speaker                    [vol: 0.50]
 │
 ├─ Sources:
 │      *   48. Built-in Mic               [vol: 0.75]
 │          49. USB Mic                     [vol: 1.00]
 │
 └─ Streams:
"#;

        let sources = parse_device_section(status, "Sources:", DeviceType::Source);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].id, 48);
        assert!(sources[0].is_default);
        assert_eq!(sources[1].id, 49);
        assert!(!sources[1].is_default);
    }
}
