// === pages/dock.rs — Dock configuration page ===
//
// Reads and writes ~/.config/zos/dock.json to configure the zos-dock:
// auto-hide, position, icon size, magnification, and pinned apps.

use std::fs;
use std::path::PathBuf;

use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, text, toggler, Space,
};
use iced::{Background, Border, Element, Length, Task};

use crate::theme;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Toggle auto-hide on/off.
    ToggleAutoHide(bool),
    /// User selected a dock position.
    SetPosition(String),
    /// User changed the icon size slider.
    SetIconSize(u32),
    /// User changed the magnification slider.
    SetMagnification(f32),
    /// Write current settings to dock.json.
    Apply,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct DockPage {
    auto_hide: bool,
    position: String,
    icon_size: u32,
    magnification: f32,
    pinned: Vec<String>,
    config_path: PathBuf,
    status: Option<String>,
}

const POSITIONS: [&str; 4] = ["bottom", "top", "left", "right"];

impl DockPage {
    pub fn new() -> Self {
        let config_path = config_path();
        let (auto_hide, position, icon_size, magnification, pinned) = read_config(&config_path);

        Self {
            auto_hide,
            position,
            icon_size,
            magnification,
            pinned,
            config_path,
            status: None,
        }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleAutoHide(val) => {
                self.auto_hide = val;
                Task::none()
            }
            Message::SetPosition(pos) => {
                self.position = pos;
                Task::none()
            }
            Message::SetIconSize(size) => {
                self.icon_size = size;
                Task::none()
            }
            Message::SetMagnification(mag) => {
                // Round to nearest 0.1 increment.
                self.magnification = (mag * 10.0).round() / 10.0;
                Task::none()
            }
            Message::Apply => {
                let result = write_config(
                    &self.config_path,
                    self.auto_hide,
                    &self.position,
                    self.icon_size,
                    self.magnification,
                    &self.pinned,
                );
                self.status = Some(result);
                Task::none()
            }
        }
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Dock").size(28).color(theme::TEXT);

        let behavior = self.view_behavior();
        let appearance = self.view_appearance();
        let pinned = self.view_pinned();
        let bottom = self.view_bottom();

        let content = column![title, behavior, appearance, pinned, bottom]
            .spacing(16)
            .padding(4)
            .width(Length::Fill);

        scrollable(content).height(Length::Fill).into()
    }

    // -- Behavior card -------------------------------------------------------

    fn view_behavior(&self) -> Element<'_, Message> {
        let heading = text("Behavior").size(18).color(theme::TEXT);

        // Auto-hide toggler
        let hide_label = text("Auto-hide").size(13).color(theme::SUBTEXT0);
        let hide_toggle = toggler(self.auto_hide)
            .on_toggle(Message::ToggleAutoHide)
            .size(20.0);
        let hide_row = row![hide_label, Space::new().width(Length::Fill), hide_toggle]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // Position pick list
        let pos_label = text("Position").size(13).color(theme::SUBTEXT0);
        let position_options: Vec<String> = POSITIONS.iter().map(|s| s.to_string()).collect();
        let pos_picker = pick_list(
            position_options,
            Some(self.position.clone()),
            Message::SetPosition,
        )
        .width(Length::Fixed(200.0))
        .text_size(13.0);
        let pos_row = row![pos_label, Space::new().width(Length::Fill), pos_picker]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        card(column![heading, hide_row, pos_row].spacing(10))
    }

    // -- Appearance card -----------------------------------------------------

    fn view_appearance(&self) -> Element<'_, Message> {
        let heading = text("Appearance").size(18).color(theme::TEXT);

        // Icon size slider
        let size_label = text(format!("Icon size: {}px", self.icon_size))
            .size(13)
            .color(theme::SUBTEXT0);
        let size_slider = slider(32..=64_u32, self.icon_size, Message::SetIconSize)
            .step(1_u32)
            .width(Length::Fixed(280.0));
        let size_row = row![size_label, Space::new().width(Length::Fill), size_slider]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // Magnification slider
        let mag_label = text(format!("Magnification: {:.1}x", self.magnification))
            .size(13)
            .color(theme::SUBTEXT0);
        let mag_slider = slider(1.0..=2.0_f32, self.magnification, Message::SetMagnification)
            .step(0.1_f32)
            .width(Length::Fixed(280.0));
        let mag_row = row![mag_label, Space::new().width(Length::Fill), mag_slider]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        card(column![heading, size_row, mag_row].spacing(10))
    }

    // -- Pinned Apps card ----------------------------------------------------

    fn view_pinned(&self) -> Element<'_, Message> {
        let heading = text("Pinned Apps").size(18).color(theme::TEXT);

        if self.pinned.is_empty() {
            let empty = text("No pinned apps configured")
                .size(13)
                .color(theme::OVERLAY0);
            return card(column![heading, empty].spacing(8));
        }

        let mut items = column![].spacing(4);
        for entry in &self.pinned {
            let label = text(entry).size(13).color(theme::TEXT);
            items = items.push(label);
        }

        card(column![heading, items].spacing(8))
    }

    // -- Bottom row (Apply + status) -----------------------------------------

    fn view_bottom(&self) -> Element<'_, Message> {
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

        row![apply_btn, Space::new().width(12), status_el]
            .align_y(iced::Alignment::Center)
            .into()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the dock config path: ~/.config/zos/dock.json
fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".config/zos/dock.json")
}

/// Read dock settings from JSON. Returns sensible defaults when the file is
/// missing or malformed.
fn read_config(path: &PathBuf) -> (bool, String, u32, f32, Vec<String>) {
    let defaults = (true, "bottom".to_string(), 48, 1.5, Vec::new());

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return defaults,
    };

    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return defaults,
    };

    let auto_hide = val
        .get("auto_hide")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let position = val
        .get("position")
        .and_then(|v| v.as_str())
        .unwrap_or("bottom")
        .to_string();

    let icon_size = val
        .get("icon_size")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(32, 64) as u32)
        .unwrap_or(48);

    let magnification = val
        .get("magnification")
        .and_then(|v| v.as_f64())
        .map(|v| v.clamp(1.0, 2.0) as f32)
        .unwrap_or(1.5);

    let pinned = val
        .get("pinned")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    (auto_hide, position, icon_size, magnification, pinned)
}

/// Serialize the current settings to JSON and write them to disk.
/// Returns a human-readable status message.
fn write_config(
    path: &PathBuf,
    auto_hide: bool,
    position: &str,
    icon_size: u32,
    magnification: f32,
    pinned: &[String],
) -> String {
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return format!("Error: could not create config directory: {e}");
        }
    }

    let obj = serde_json::json!({
        "auto_hide": auto_hide,
        "position": position,
        "icon_size": icon_size,
        "magnification": magnification,
        "pinned": pinned,
    });

    let content = match serde_json::to_string_pretty(&obj) {
        Ok(c) => c,
        Err(e) => return format!("Error: could not serialize config: {e}"),
    };

    match fs::write(path, &content) {
        Ok(()) => format!("Saved to {}", path.display()),
        Err(e) => format!("Error: could not write {}: {e}", path.display()),
    }
}

/// A Catppuccin-styled card container.
fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
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
