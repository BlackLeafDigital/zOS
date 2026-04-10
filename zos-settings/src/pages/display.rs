// === display.rs — Monitor configuration page ===
//
// Reads monitor info from `hyprctl monitors -j`, lets the user change
// resolution, scale, rotation, and position per monitor, then writes
// to ~/.config/hypr/monitors.conf and reloads Hyprland.

use std::fmt;
use std::fs;
use std::process::Command;

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text, Space};
use iced::{Background, Border, Element, Length, Task};
use serde::Deserialize;

use crate::services::hyprctl;
use crate::theme;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A single mode reported by Hyprland (e.g. "2560x1440@143.97").
#[derive(Debug, Clone, PartialEq)]
struct Mode {
    width: u32,
    height: u32,
    refresh: f32,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}@{:.2}", self.width, self.height, self.refresh)
    }
}

/// Rotation/transform variants matching Hyprland's transform values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl Transform {
    const ALL: [Transform; 8] = [
        Transform::Normal,
        Transform::Rotate90,
        Transform::Rotate180,
        Transform::Rotate270,
        Transform::Flipped,
        Transform::Flipped90,
        Transform::Flipped180,
        Transform::Flipped270,
    ];

    fn from_hyprland(val: u32) -> Self {
        match val {
            1 => Transform::Rotate90,
            2 => Transform::Rotate180,
            3 => Transform::Rotate270,
            4 => Transform::Flipped,
            5 => Transform::Flipped90,
            6 => Transform::Flipped180,
            7 => Transform::Flipped270,
            _ => Transform::Normal,
        }
    }

    fn to_hyprland(self) -> u32 {
        match self {
            Transform::Normal => 0,
            Transform::Rotate90 => 1,
            Transform::Rotate180 => 2,
            Transform::Rotate270 => 3,
            Transform::Flipped => 4,
            Transform::Flipped90 => 5,
            Transform::Flipped180 => 6,
            Transform::Flipped270 => 7,
        }
    }
}

impl fmt::Display for Transform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Transform::Normal => write!(f, "Normal"),
            Transform::Rotate90 => write!(f, "90\u{b0}"),
            Transform::Rotate180 => write!(f, "180\u{b0}"),
            Transform::Rotate270 => write!(f, "270\u{b0}"),
            Transform::Flipped => write!(f, "Flipped"),
            Transform::Flipped90 => write!(f, "Flipped 90\u{b0}"),
            Transform::Flipped180 => write!(f, "Flipped 180\u{b0}"),
            Transform::Flipped270 => write!(f, "Flipped 270\u{b0}"),
        }
    }
}

/// State for a single monitor as edited by the user.
#[derive(Debug, Clone)]
struct MonitorState {
    name: String,
    description: String,
    available_modes: Vec<Mode>,
    selected_mode: Option<Mode>,
    scale: f32,
    transform: Transform,
    x: i32,
    y: i32,
}

// ---------------------------------------------------------------------------
// JSON structs for hyprctl monitors -j
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct HyprMonitor {
    name: String,
    description: String,
    width: u32,
    height: u32,
    #[serde(alias = "refreshRate")]
    refresh_rate: f32,
    x: i32,
    y: i32,
    scale: f32,
    transform: u32,
    #[serde(alias = "availableModes")]
    available_modes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Re-read monitor state from Hyprland.
    Refresh,
    /// User selected a resolution mode for a monitor.
    SelectMode { index: usize, mode: String },
    /// User changed the scale slider for a monitor.
    SetScale { index: usize, scale: f32 },
    /// User selected a transform/rotation for a monitor.
    SetTransform { index: usize, transform: String },
    /// User changed position X for a monitor.
    SetPositionX { index: usize, value: String },
    /// User changed position Y for a monitor.
    SetPositionY { index: usize, value: String },
    /// Write monitors.conf and reload Hyprland.
    Apply,
}

// ---------------------------------------------------------------------------
// DisplayPage
// ---------------------------------------------------------------------------

pub struct DisplayPage {
    monitors: Vec<MonitorState>,
    /// Transient text input buffers for position fields so the user can type
    /// freely without the input snapping to parsed integers mid-keystroke.
    position_x_buf: Vec<String>,
    position_y_buf: Vec<String>,
    /// Status message shown after apply (success or error).
    status: Option<String>,
}

impl DisplayPage {
    pub fn new() -> Self {
        let monitors = read_monitors();
        let x_bufs: Vec<String> = monitors.iter().map(|m| m.x.to_string()).collect();
        let y_bufs: Vec<String> = monitors.iter().map(|m| m.y.to_string()).collect();
        Self {
            monitors,
            position_x_buf: x_bufs,
            position_y_buf: y_bufs,
            status: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => {
                self.monitors = read_monitors();
                self.position_x_buf = self.monitors.iter().map(|m| m.x.to_string()).collect();
                self.position_y_buf = self.monitors.iter().map(|m| m.y.to_string()).collect();
                self.status = None;
            }
            Message::SelectMode { index, mode } => {
                if let Some(mon) = self.monitors.get_mut(index) {
                    if let Some(parsed) = parse_mode(&mode) {
                        mon.selected_mode = Some(parsed);
                    }
                }
            }
            Message::SetScale { index, scale } => {
                if let Some(mon) = self.monitors.get_mut(index) {
                    // Round to nearest 0.25 increment.
                    mon.scale = (scale * 4.0).round() / 4.0;
                }
            }
            Message::SetTransform { index, transform } => {
                if let Some(mon) = self.monitors.get_mut(index) {
                    for t in Transform::ALL {
                        if t.to_string() == transform {
                            mon.transform = t;
                            break;
                        }
                    }
                }
            }
            Message::SetPositionX { index, value } => {
                if let Some(buf) = self.position_x_buf.get_mut(index) {
                    *buf = value.clone();
                }
                if let Some(mon) = self.monitors.get_mut(index) {
                    if let Ok(v) = value.parse::<i32>() {
                        mon.x = v;
                    }
                }
            }
            Message::SetPositionY { index, value } => {
                if let Some(buf) = self.position_y_buf.get_mut(index) {
                    *buf = value.clone();
                }
                if let Some(mon) = self.monitors.get_mut(index) {
                    if let Ok(v) = value.parse::<i32>() {
                        mon.y = v;
                    }
                }
            }
            Message::Apply => {
                self.status = Some(apply_monitors(&self.monitors));
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // -- Header --
        let title = text("Display").size(28).color(theme::TEXT);

        let refresh_btn = button(text("Refresh").size(13).color(theme::BASE))
            .on_press(Message::Refresh)
            .padding([8, 16])
            .style(|_theme, _status| button::Style {
                background: Some(Background::Color(theme::BLUE)),
                text_color: theme::BASE,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

        let header = row![title, Space::new().width(Length::Fill), refresh_btn]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // -- Monitor cards --
        let mut cards = column![].spacing(16);

        if self.monitors.is_empty() {
            cards = cards.push(
                text("No monitors detected. Is Hyprland running?")
                    .size(14)
                    .color(theme::OVERLAY0),
            );
        }

        for (i, mon) in self.monitors.iter().enumerate() {
            cards = cards.push(self.view_monitor_card(i, mon));
        }

        // -- Apply button --
        let apply_btn = button(text("Apply").size(15).color(theme::BASE))
            .on_press(Message::Apply)
            .padding([10, 32])
            .style(|_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme::GREEN,
                    _ => theme::BLUE,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: theme::BASE,
                    border: Border {
                        radius: 10.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            });

        // -- Status text --
        let status_el: Element<'_, Message> = match &self.status {
            Some(msg) => {
                let color = if msg.starts_with("Error") {
                    theme::RED
                } else {
                    theme::GREEN
                };
                text(msg).size(13).color(color).into()
            }
            None => Space::new().into(),
        };

        let bottom_row =
            row![apply_btn, Space::new().width(12), status_el].align_y(iced::Alignment::Center);

        let content = column![header, cards, bottom_row]
            .spacing(16)
            .padding(4)
            .width(Length::Fill);

        scrollable(content).height(Length::Fill).into()
    }

    fn view_monitor_card<'a>(
        &'a self,
        index: usize,
        mon: &'a MonitorState,
    ) -> Element<'a, Message> {
        // -- Name + description --
        let name_label = text(&mon.name).size(18).color(theme::TEXT);
        let desc_label = text(&mon.description).size(12).color(theme::SUBTEXT0);

        // -- Resolution picker --
        let mode_strings: Vec<String> = mon.available_modes.iter().map(|m| m.to_string()).collect();
        let selected_mode = mon.selected_mode.as_ref().map(|m| m.to_string());

        let res_label = text("Resolution").size(13).color(theme::SUBTEXT0);
        let res_picker = pick_list(mode_strings, selected_mode, move |mode| {
            Message::SelectMode { index, mode }
        })
        .width(Length::Fixed(280.0))
        .text_size(13.0);

        // -- Scale slider --
        let scale_label = text(format!("Scale: {:.2}", mon.scale))
            .size(13)
            .color(theme::SUBTEXT0);

        let scale_slider = slider(0.5..=3.0_f32, mon.scale, move |s| Message::SetScale {
            index,
            scale: s,
        })
        .step(0.25)
        .width(Length::Fixed(280.0));

        // -- Transform picker --
        let transform_strings: Vec<String> = Transform::ALL.iter().map(|t| t.to_string()).collect();
        let selected_transform = mon.transform.to_string();

        let transform_label = text("Rotation").size(13).color(theme::SUBTEXT0);
        let transform_picker = pick_list(
            transform_strings,
            Some(selected_transform),
            move |transform| Message::SetTransform { index, transform },
        )
        .width(Length::Fixed(280.0))
        .text_size(13.0);

        // -- Position --
        let pos_label = text("Position").size(13).color(theme::SUBTEXT0);

        let x_buf = self
            .position_x_buf
            .get(index)
            .cloned()
            .unwrap_or_else(|| mon.x.to_string());
        let y_buf = self
            .position_y_buf
            .get(index)
            .cloned()
            .unwrap_or_else(|| mon.y.to_string());

        let x_label = text("X:").size(13).color(theme::SUBTEXT0);
        let x_input = iced::widget::text_input("0", &x_buf)
            .on_input(move |value| Message::SetPositionX { index, value })
            .width(Length::Fixed(100.0))
            .size(13.0);

        let y_label = text("Y:").size(13).color(theme::SUBTEXT0);
        let y_input = iced::widget::text_input("0", &y_buf)
            .on_input(move |value| Message::SetPositionY { index, value })
            .width(Length::Fixed(100.0))
            .size(13.0);

        let pos_row = row![x_label, x_input, Space::new().width(12), y_label, y_input]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        // -- Card assembly --
        let card = column![
            row![name_label, Space::new().width(12), desc_label].align_y(iced::Alignment::Center),
            Space::new().height(8),
            res_label,
            res_picker,
            Space::new().height(4),
            scale_label,
            scale_slider,
            Space::new().height(4),
            transform_label,
            transform_picker,
            Space::new().height(4),
            pos_label,
            pos_row,
        ]
        .spacing(4);

        container(card)
            .padding(16)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme::SURFACE0)),
                border: Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read monitor info from Hyprland via `hyprctl monitors -j`.
fn read_monitors() -> Vec<MonitorState> {
    let output = match Command::new("hyprctl").args(["monitors", "-j"]).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };

    let hypr_monitors: Vec<HyprMonitor> = match serde_json::from_slice(&output) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    hypr_monitors
        .into_iter()
        .map(|hm| {
            let available_modes: Vec<Mode> = hm
                .available_modes
                .iter()
                .filter_map(|s| parse_mode(s))
                .collect();

            // Find the mode matching the current resolution + refresh rate.
            let current_mode = available_modes
                .iter()
                .find(|m| {
                    m.width == hm.width
                        && m.height == hm.height
                        && (m.refresh - hm.refresh_rate).abs() < 1.0
                })
                .cloned();

            MonitorState {
                name: hm.name,
                description: hm.description,
                available_modes,
                selected_mode: current_mode,
                scale: hm.scale,
                transform: Transform::from_hyprland(hm.transform),
                x: hm.x,
                y: hm.y,
            }
        })
        .collect()
}

/// Parse a mode string like "2560x1440@143.97Hz" or "2560x1440@143.97" into a Mode.
fn parse_mode(s: &str) -> Option<Mode> {
    let s = s.trim_end_matches("Hz");
    let (res, rate_str) = s.split_once('@')?;
    let (w, h) = res.split_once('x')?;
    Some(Mode {
        width: w.parse().ok()?,
        height: h.parse().ok()?,
        refresh: rate_str.parse().ok()?,
    })
}

/// Write monitors.conf and reload Hyprland. Returns a status message.
fn apply_monitors(monitors: &[MonitorState]) -> String {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return "Error: HOME not set".to_string(),
    };

    let conf_dir = format!("{home}/.config/hypr");
    if let Err(e) = fs::create_dir_all(&conf_dir) {
        return format!("Error: could not create {conf_dir}: {e}");
    }

    let conf_path = format!("{conf_dir}/monitors.conf");

    let mut lines = Vec::new();
    for mon in monitors {
        let mode = match &mon.selected_mode {
            Some(m) => m.to_string(),
            None => "preferred".to_string(),
        };
        let transform = mon.transform.to_hyprland();
        // monitor=<name>,<mode>,<pos>,<scale>,transform,<val>
        let mut line = format!(
            "monitor={},{},{}x{},{:.2}",
            mon.name, mode, mon.x, mon.y, mon.scale,
        );
        if transform != 0 {
            line.push_str(&format!(",transform,{transform}"));
        }
        lines.push(line);
    }

    let content = lines.join("\n") + "\n";

    if let Err(e) = fs::write(&conf_path, &content) {
        return format!("Error: could not write {conf_path}: {e}");
    }

    hyprctl::reload();

    format!("Applied to {conf_path}")
}
