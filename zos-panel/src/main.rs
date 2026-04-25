//! zos-panel — top-bar replacing HyprPanel.
//!
//! v1 modules: Clock, Workspaces, Active Window, Audio, Network, Battery.
//! Future modules: Tray, Bluetooth, Power.
//!
//! Layer-shell anchored Top/Left/Right with `BAR_HEIGHT` exclusive zone.
//! State refreshes every second via the `subscription` tick — workspaces
//! and active window come from `zos_core::compositor`. Audio/network/battery
//! are cheap synchronous shellouts (`pactl`, `nmcli`) and `/sys` reads on
//! each tick; if a tool is missing the module degrades to a `❓` rather
//! than crashing. The clock is naive UTC (`SystemTime::now()` formatted
//! `%H:%M`); time-zone math is a deliberate follow-up.

use iced::{
    Background, Border, Element, Length, Padding, Subscription, Task, Theme,
    alignment::Vertical,
    border::Radius,
    widget::{Row, Space, button, container, row, text},
};
use iced_layershell::settings::Settings;
use iced_layershell::to_layer_message;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use zos_core::compositor::{self, Compositor, WorkspaceInfo};
use zos_ui::theme;

const BAR_HEIGHT: u32 = 32;

#[derive(Default)]
struct Panel {
    time: String,
    workspaces: Vec<WorkspaceInfo>,
    active_title: String,
    audio: AudioState,
    network: NetworkState,
    battery: Option<BatteryState>,
}

impl Panel {
    fn new() -> Self {
        Self {
            time: current_hms(),
            workspaces: Vec::new(),
            active_title: String::new(),
            audio: AudioState::default(),
            network: NetworkState::default(),
            battery: None,
        }
    }
}

#[derive(Default, Clone, Debug)]
struct AudioState {
    volume_pct: Option<u32>,
    muted: bool,
}

#[derive(Default, Clone, Debug)]
enum NetworkState {
    Wifi(Option<String>),
    Wired,
    #[default]
    Disconnected,
}

#[derive(Clone, Debug)]
struct BatteryState {
    capacity_pct: u32,
    charging: bool,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Msg {
    Tick,
    SwitchToWorkspace(i64),
}

/// Naive `HH:MM` formatter from `SystemTime` — assumes the seconds-since-epoch
/// reading is the time we want to display. Time-zone awareness (DST,
/// non-UTC system clocks) is a follow-up; chrono is intentionally not pulled
/// in for v1.
fn current_hms() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    format!("{:02}:{:02}", h, m)
}

/// Pull current workspaces + focused window title from the compositor.
/// Returns `(Vec::new(), String::new())` shape values on partial failure
/// so the panel still renders cleanly.
fn fetch_state(comp: &dyn Compositor) -> (Vec<WorkspaceInfo>, String) {
    let workspaces = comp.workspaces().unwrap_or_default();
    let active = comp.active_window().ok().flatten();
    let title = active.map(|w| w.title).unwrap_or_default();
    (workspaces, title)
}

fn boot() -> (Panel, Task<Msg>) {
    (Panel::new(), Task::none())
}

fn namespace() -> String {
    "zos-panel".into()
}

fn theme_fn(_state: &Panel) -> Theme {
    theme::zos_theme()
}

fn update(state: &mut Panel, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Tick => {
            state.time = current_hms();
            // Refresh compositor data on each tick (cheap shellout). If
            // we're not running under a known compositor (e.g. the panel
            // is launched outside of Hyprland for testing), the clock
            // continues to tick — just no workspaces / title.
            if let Ok(comp) = compositor::detect() {
                let (workspaces, title) = fetch_state(&*comp);
                state.workspaces = workspaces;
                state.active_title = title;
            }
            // System modules — synchronous shellouts/reads. Each fetcher
            // tolerates missing tools (pactl/nmcli not installed) by
            // returning an unknown / disconnected / None state.
            state.audio = fetch_audio();
            state.network = fetch_network();
            state.battery = fetch_battery();
        }
        Msg::SwitchToWorkspace(id) => {
            if let Ok(comp) = compositor::detect() {
                if let Err(e) = comp.switch_to_workspace(id) {
                    tracing::warn!(workspace = id, error = ?e, "switch_to_workspace failed");
                }
            }
        }
        // Layer-shell control messages (anchor/size/margin/etc.) injected
        // by `#[to_layer_message]`. zos-panel doesn't reconfigure its
        // surface at runtime, so these are no-ops.
        _ => {}
    }
    Task::none()
}

fn view(state: &Panel) -> Element<'_, Msg> {
    let workspaces_row: Row<'_, Msg> = state.workspaces.iter().fold(
        row![].spacing(theme::space::X1),
        |acc, ws| {
            let label = format!("{}", ws.id);
            let id = ws.id;
            let active = ws.active;
            acc.push(
                button(text(label).size(theme::font_size::SM))
                    .on_press(Msg::SwitchToWorkspace(id))
                    .style(move |_, status| ws_button_style(active, status))
                    .padding(Padding::from([2.0_f32, 8.0_f32])),
            )
        },
    );

    let clock = container(
        text(state.time.clone())
            .size(theme::font_size::BASE)
            .color(theme::TEXT),
    )
    .padding(Padding::from([0.0_f32, 12.0_f32]));

    // Truncate long titles so they don't push layout around. We byte-slice
    // the first 60 chars after asserting it's a char boundary; UTF-8
    // titles fall through to the unchanged path.
    let title_text = if state.active_title.is_empty() {
        "—".to_string()
    } else if state.active_title.len() > 60 {
        let cutoff = nearest_char_boundary(&state.active_title, 60);
        format!("{}…", &state.active_title[..cutoff])
    } else {
        state.active_title.clone()
    };
    let title = text(title_text)
        .size(theme::font_size::SM)
        .color(theme::SUBTEXT0);

    let audio_module = audio_view(&state.audio);
    let network_module = network_view(&state.network);
    let battery_module = battery_view(state.battery.as_ref());

    container(
        row![
            workspaces_row,
            Space::new().width(Length::Fill),
            title,
            Space::new().width(Length::Fill),
            audio_module,
            network_module,
            battery_module,
            clock,
        ]
        .spacing(theme::space::X3)
        .align_y(Vertical::Center)
        .padding(Padding::from([0.0_f32, 12.0_f32])),
    )
    .height(Length::Fill)
    .width(Length::Fill)
    .style(panel_bg_style)
    .into()
}

fn subscription(_state: &Panel) -> Subscription<Msg> {
    iced::time::every(Duration::from_secs(1)).map(|_| Msg::Tick)
}

/// Find the largest char boundary in `s` that is `<= max`. Returns
/// `s.len()` if `max >= s.len()`. Required because `&s[..max]` panics
/// when `max` falls inside a multi-byte codepoint (most non-ASCII titles).
fn nearest_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut idx = max;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

// --- System modules: fetch -----------------------------------------------

/// Query PulseAudio/PipeWire (via `pactl`) for the default sink's volume +
/// mute state. Returns `AudioState::default()` (no volume, not muted) on
/// any failure — `pactl` not installed, no default sink, parse failure.
fn fetch_audio() -> AudioState {
    use std::process::Command;
    let muted = Command::new("pactl")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains("yes"))
        .unwrap_or(false);
    let volume_pct = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            // Output looks like:
            //   "Volume: front-left: 65536 / 100% / 0.00 dB,
            //    front-right: 65536 / 100% / 0.00 dB"
            // Grab the digits immediately preceding the first '%'.
            let pct_idx = s.find('%')?;
            let pre = &s[..pct_idx];
            let num_start = pre.rfind(|c: char| !c.is_ascii_digit())? + 1;
            pre[num_start..].parse::<u32>().ok()
        });
    AudioState { volume_pct, muted }
}

/// Determine the active network connection via `nmcli`. Walks the device
/// list looking for the first connected device; if it's wifi, makes a
/// second `nmcli` call to grab the SSID. Returns `Disconnected` on any
/// failure (nmcli missing, NetworkManager not running, no connected
/// device).
fn fetch_network() -> NetworkState {
    use std::process::Command;
    let out = Command::new("nmcli")
        .args(["-t", "-f", "device,type,state", "device"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    // each line: "wlan0:wifi:connected" or "eth0:ethernet:connected" etc
    for line in out.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        if parts[2] != "connected" {
            continue;
        }
        match parts[1] {
            "wifi" => {
                let ssid = Command::new("nmcli")
                    .args(["-t", "-f", "active,ssid", "dev", "wifi"])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .and_then(|s| {
                        s.lines().find_map(|l| {
                            let parts: Vec<&str> = l.splitn(2, ':').collect();
                            if parts.first() == Some(&"yes") {
                                Some(parts.get(1)?.to_string())
                            } else {
                                None
                            }
                        })
                    });
                return NetworkState::Wifi(ssid);
            }
            "ethernet" => return NetworkState::Wired,
            _ => continue,
        }
    }
    NetworkState::Disconnected
}

/// Find the first `BAT*` entry in `/sys/class/power_supply` and read its
/// `capacity` + `status`. Returns `None` on desktop machines (no battery
/// directory) or if the kernel sysfs layout is unexpected — the view
/// renders zero-width Space in that case so the layout doesn't shift.
fn fetch_battery() -> Option<BatteryState> {
    use std::fs;
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for e in entries.flatten() {
        let name = e.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("BAT") {
            let path = e.path();
            let capacity: u32 = fs::read_to_string(path.join("capacity"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let status = fs::read_to_string(path.join("status"))
                .ok()?
                .trim()
                .to_string();
            return Some(BatteryState {
                capacity_pct: capacity,
                charging: status.eq_ignore_ascii_case("charging"),
            });
        }
    }
    None
}

// --- System modules: view -------------------------------------------------

fn audio_view<'a>(state: &AudioState) -> Element<'a, Msg> {
    let label = if state.muted {
        "🔇".to_string()
    } else if let Some(pct) = state.volume_pct {
        format!("🔊{}%", pct)
    } else {
        "❓".to_string()
    };
    container(
        text(label)
            .size(theme::font_size::SM)
            .color(theme::TEXT),
    )
    .padding(Padding::from([0.0_f32, 6.0_f32]))
    .into()
}

fn network_view<'a>(state: &NetworkState) -> Element<'a, Msg> {
    let label = match state {
        NetworkState::Wifi(Some(ssid)) => format!("📶{}", ssid),
        NetworkState::Wifi(None) => "📶?".to_string(),
        NetworkState::Wired => "🔌".to_string(),
        NetworkState::Disconnected => "❌".to_string(),
    };
    // Truncate long SSIDs at the char-boundary level so multi-byte
    // unicode (emoji ssids etc) doesn't panic on slicing.
    let display = if label.chars().count() > 16 {
        let truncated: String = label.chars().take(15).collect();
        format!("{}…", truncated)
    } else {
        label
    };
    container(
        text(display)
            .size(theme::font_size::SM)
            .color(theme::TEXT),
    )
    .padding(Padding::from([0.0_f32, 6.0_f32]))
    .into()
}

fn battery_view<'a>(state: Option<&BatteryState>) -> Element<'a, Msg> {
    if let Some(b) = state {
        let icon = if b.charging { "⚡" } else { "🔋" };
        let color = if b.capacity_pct < 20 {
            theme::RED
        } else if b.capacity_pct < 40 {
            theme::YELLOW
        } else {
            theme::TEXT
        };
        container(
            text(format!("{}{}%", icon, b.capacity_pct))
                .size(theme::font_size::SM)
                .color(color),
        )
        .padding(Padding::from([0.0_f32, 6.0_f32]))
        .into()
    } else {
        // No battery — desktop machine. Render zero-width Space so the
        // row layout doesn't shift between machines that have a battery
        // and ones that don't.
        Space::new().width(0.0_f32).height(0.0_f32).into()
    }
}

// --- Styles ---------------------------------------------------------------

fn panel_bg_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::CRUST)),
        border: Border {
            color: theme::SURFACE0,
            width: 1.0,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    }
}

fn ws_button_style(active: bool, status: button::Status) -> button::Style {
    let bg = if active {
        theme::BLUE
    } else {
        match status {
            button::Status::Hovered => theme::SURFACE1,
            _ => theme::SURFACE0,
        }
    };
    let fg = if active { theme::CRUST } else { theme::TEXT };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            color: bg,
            width: 0.0,
            radius: Radius::from(theme::radius::SM),
        },
        ..Default::default()
    }
}

// --- Entrypoint -----------------------------------------------------------

fn main() -> iced_layershell::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-panel starting");

    let layer_settings = zos_ui::layer::layer_shell::top_bar(BAR_HEIGHT);

    iced_layershell::application(boot, namespace, update, view)
        .theme(theme_fn)
        .subscription(subscription)
        .settings(Settings {
            layer_settings,
            antialiasing: true,
            ..Default::default()
        })
        .run()
}
