// === input.rs — Keyboard, mouse, and touchpad settings page ===
//
// Reads input settings from ~/.config/hypr/user-settings.conf, presents
// sliders/pickers/togglers for each, and applies changes live via
// `hyprctl keyword` while persisting them back to the config file.

use std::fs;

use iced::widget::{column, container, pick_list, row, scrollable, slider, text, toggler, Space};
use iced::{Background, Border, Element, Length, Task};

use crate::services::hyprctl;
use crate::theme;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const LAYOUTS: &[&str] = &["us", "gb", "de", "fr", "es", "it", "pt", "ru", "jp", "kr"];

const DEFAULT_KB_LAYOUT: &str = "us";
const DEFAULT_REPEAT_RATE: u32 = 25;
const DEFAULT_REPEAT_DELAY: u32 = 600;
const DEFAULT_SENSITIVITY: f32 = 0.0;
const DEFAULT_FLAT_ACCEL: bool = false;
const DEFAULT_NATURAL_SCROLL: bool = true;
const DEFAULT_TAP_TO_CLICK: bool = true;

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Keyboard layout changed.
    SetLayout(String),
    /// Repeat rate slider changed.
    SetRepeatRate(u32),
    /// Repeat delay slider changed.
    SetRepeatDelay(u32),
    /// Mouse sensitivity slider changed.
    SetSensitivity(f32),
    /// Flat acceleration toggler changed.
    SetFlatAccel(bool),
    /// Natural scroll toggler changed.
    SetNaturalScroll(bool),
    /// Tap-to-click toggler changed.
    SetTapToClick(bool),
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct InputPage {
    kb_layout: String,
    repeat_rate: u32,
    repeat_delay: u32,
    sensitivity: f32,
    flat_accel: bool,
    natural_scroll: bool,
    tap_to_click: bool,
}

impl InputPage {
    pub fn new() -> Self {
        let conf = read_config();
        Self {
            kb_layout: conf.kb_layout,
            repeat_rate: conf.repeat_rate,
            repeat_delay: conf.repeat_delay,
            sensitivity: conf.sensitivity,
            flat_accel: conf.flat_accel,
            natural_scroll: conf.natural_scroll,
            tap_to_click: conf.tap_to_click,
        }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SetLayout(layout) => {
                self.kb_layout = layout;
                hyprctl::keyword("input:kb_layout", &self.kb_layout);
            }
            Message::SetRepeatRate(rate) => {
                self.repeat_rate = rate;
                hyprctl::keyword("input:repeat_rate", &self.repeat_rate.to_string());
            }
            Message::SetRepeatDelay(delay) => {
                self.repeat_delay = delay;
                hyprctl::keyword("input:repeat_delay", &self.repeat_delay.to_string());
            }
            Message::SetSensitivity(sens) => {
                // Round to nearest 0.05 step.
                self.sensitivity = (sens * 20.0).round() / 20.0;
                hyprctl::keyword("input:sensitivity", &format!("{:.2}", self.sensitivity));
            }
            Message::SetFlatAccel(flat) => {
                self.flat_accel = flat;
                let profile = if flat { "flat" } else { "adaptive" };
                hyprctl::keyword("input:accel_profile", profile);
            }
            Message::SetNaturalScroll(enabled) => {
                self.natural_scroll = enabled;
                let val = if enabled { "true" } else { "false" };
                hyprctl::keyword("input:touchpad:natural_scroll", val);
            }
            Message::SetTapToClick(enabled) => {
                self.tap_to_click = enabled;
                let val = if enabled { "true" } else { "false" };
                hyprctl::keyword("input:touchpad:tap-to-click", val);
            }
        }
        write_config(self);
        Task::none()
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Input").size(28).color(theme::TEXT);
        let subtitle = text("Keyboard, mouse, and touchpad settings")
            .size(14)
            .color(theme::SUBTEXT0);

        let header = column![title, subtitle].spacing(4);

        let keyboard_section = self.view_keyboard();
        let mouse_section = self.view_mouse();
        let touchpad_section = self.view_touchpad();

        let content = column![header, keyboard_section, mouse_section, touchpad_section]
            .spacing(24)
            .padding(4)
            .width(Length::Fill);

        scrollable(content).height(Length::Fill).into()
    }

    fn view_keyboard(&self) -> Element<'_, Message> {
        // Layout picker
        let layout_label = text("Keyboard Layout").size(13).color(theme::SUBTEXT0);
        let layout_options: Vec<String> = LAYOUTS.iter().map(|s| (*s).to_string()).collect();
        let selected_layout = Some(self.kb_layout.clone());
        let layout_picker = pick_list(layout_options, selected_layout, Message::SetLayout)
            .width(Length::Fixed(200.0))
            .text_size(13.0);

        let layout_row = row![
            layout_label,
            Space::new().width(Length::Fill),
            layout_picker
        ]
        .align_y(iced::Alignment::Center);

        // Repeat rate slider
        let rate_label = text(format!("Repeat Rate: {}", self.repeat_rate))
            .size(13)
            .color(theme::SUBTEXT0);
        let rate_slider = slider(1..=100_u32, self.repeat_rate, Message::SetRepeatRate)
            .step(1u32)
            .width(Length::Fixed(280.0));

        let rate_row = row![rate_label, Space::new().width(Length::Fill), rate_slider]
            .align_y(iced::Alignment::Center);

        // Repeat delay slider
        let delay_label = text(format!("Repeat Delay: {} ms", self.repeat_delay))
            .size(13)
            .color(theme::SUBTEXT0);
        let delay_slider = slider(100..=1000_u32, self.repeat_delay, Message::SetRepeatDelay)
            .step(10u32)
            .width(Length::Fixed(280.0));

        let delay_row = row![delay_label, Space::new().width(Length::Fill), delay_slider]
            .align_y(iced::Alignment::Center);

        let section_content = column![layout_row, rate_row, delay_row].spacing(12);

        let section = column![
            text("Keyboard").size(16).color(theme::TEXT),
            Space::new().height(4),
            card(section_content),
        ]
        .spacing(8)
        .width(Length::Fill);

        section.into()
    }

    fn view_mouse(&self) -> Element<'_, Message> {
        // Sensitivity slider
        let sens_label = text(format!("Sensitivity: {:.2}", self.sensitivity))
            .size(13)
            .color(theme::SUBTEXT0);
        let sens_slider = slider(-1.0..=1.0_f32, self.sensitivity, Message::SetSensitivity)
            .step(0.05)
            .width(Length::Fixed(280.0));

        let sens_row = row![sens_label, Space::new().width(Length::Fill), sens_slider]
            .align_y(iced::Alignment::Center);

        // Flat acceleration toggler
        let accel_label = text("Flat Acceleration").size(13).color(theme::SUBTEXT0);
        let accel_toggle = toggler(self.flat_accel)
            .on_toggle(Message::SetFlatAccel)
            .size(20.0);

        let accel_row = row![accel_label, Space::new().width(Length::Fill), accel_toggle]
            .align_y(iced::Alignment::Center);

        let section_content = column![sens_row, accel_row].spacing(12);

        let section = column![
            text("Mouse").size(16).color(theme::TEXT),
            Space::new().height(4),
            card(section_content),
        ]
        .spacing(8)
        .width(Length::Fill);

        section.into()
    }

    fn view_touchpad(&self) -> Element<'_, Message> {
        // Natural scroll toggler
        let natural_label = text("Natural Scroll").size(13).color(theme::SUBTEXT0);
        let natural_toggle = toggler(self.natural_scroll)
            .on_toggle(Message::SetNaturalScroll)
            .size(20.0);

        let natural_row = row![
            natural_label,
            Space::new().width(Length::Fill),
            natural_toggle
        ]
        .align_y(iced::Alignment::Center);

        // Tap-to-click toggler
        let tap_label = text("Tap-to-Click").size(13).color(theme::SUBTEXT0);
        let tap_toggle = toggler(self.tap_to_click)
            .on_toggle(Message::SetTapToClick)
            .size(20.0);

        let tap_row = row![tap_label, Space::new().width(Length::Fill), tap_toggle]
            .align_y(iced::Alignment::Center);

        let section_content = column![natural_row, tap_row].spacing(12);

        let section = column![
            text("Touchpad").size(16).color(theme::TEXT),
            Space::new().height(4),
            card(section_content),
        ]
        .spacing(8)
        .width(Length::Fill);

        section.into()
    }
}

// ---------------------------------------------------------------------------
// Reusable helpers
// ---------------------------------------------------------------------------

/// Wraps content in a styled card container (SURFACE0 background, rounded).
fn card<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
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
}

// ---------------------------------------------------------------------------
// Config read/write
// ---------------------------------------------------------------------------

/// Parsed input configuration values.
struct InputConfig {
    kb_layout: String,
    repeat_rate: u32,
    repeat_delay: u32,
    sensitivity: f32,
    flat_accel: bool,
    natural_scroll: bool,
    tap_to_click: bool,
}

/// Returns the path to `~/.config/hypr/user-settings.conf`.
fn config_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{home}/.config/hypr/user-settings.conf"))
}

/// Read and parse `~/.config/hypr/user-settings.conf`, returning defaults
/// for any missing keys.
fn read_config() -> InputConfig {
    let mut cfg = InputConfig {
        kb_layout: DEFAULT_KB_LAYOUT.to_string(),
        repeat_rate: DEFAULT_REPEAT_RATE,
        repeat_delay: DEFAULT_REPEAT_DELAY,
        sensitivity: DEFAULT_SENSITIVITY,
        flat_accel: DEFAULT_FLAT_ACCEL,
        natural_scroll: DEFAULT_NATURAL_SCROLL,
        tap_to_click: DEFAULT_TAP_TO_CLICK,
    };

    let path = match config_path() {
        Some(p) => p,
        None => return cfg,
    };

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return cfg,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "kb_layout" => {
                    cfg.kb_layout = value.to_string();
                }
                "repeat_rate" => {
                    if let Ok(v) = value.parse::<u32>() {
                        cfg.repeat_rate = v.clamp(1, 100);
                    }
                }
                "repeat_delay" => {
                    if let Ok(v) = value.parse::<u32>() {
                        cfg.repeat_delay = v.clamp(100, 1000);
                    }
                }
                "sensitivity" => {
                    if let Ok(v) = value.parse::<f32>() {
                        cfg.sensitivity = v.clamp(-1.0, 1.0);
                    }
                }
                "accel_profile" => {
                    cfg.flat_accel = value == "flat";
                }
                "natural_scroll" => {
                    cfg.natural_scroll = value == "true";
                }
                "tap-to-click" => {
                    cfg.tap_to_click = value == "true";
                }
                _ => {}
            }
        }
    }

    cfg
}

/// Write the full `input { ... }` block to `~/.config/hypr/user-settings.conf`,
/// overwriting the file entirely.
fn write_config(page: &InputPage) {
    let path = match config_path() {
        Some(p) => p,
        None => return,
    };

    // Ensure the directory exists.
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = fs::create_dir_all(dir);
    }

    let accel_profile = if page.flat_accel { "flat" } else { "adaptive" };
    let natural_scroll = if page.natural_scroll { "true" } else { "false" };
    let tap_to_click = if page.tap_to_click { "true" } else { "false" };

    let content = format!(
        "\
input {{
    kb_layout = {kb_layout}
    repeat_rate = {repeat_rate}
    repeat_delay = {repeat_delay}
    sensitivity = {sensitivity:.2}
    accel_profile = {accel_profile}
    touchpad {{
        natural_scroll = {natural_scroll}
        tap-to-click = {tap_to_click}
    }}
}}
",
        kb_layout = page.kb_layout,
        repeat_rate = page.repeat_rate,
        repeat_delay = page.repeat_delay,
        sensitivity = page.sensitivity,
        accel_profile = accel_profile,
        natural_scroll = natural_scroll,
        tap_to_click = tap_to_click,
    );

    let _ = fs::write(&path, content);
}
