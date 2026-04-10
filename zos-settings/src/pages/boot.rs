// === pages/boot.rs — Boot / GRUB configuration page ===
//
// Displays GRUB timeout settings, Windows dual-boot status, and
// provides controls for managing boot entries and rebooting to Windows.

use iced::widget::{button, column, container, row, scrollable, slider, text, Space};
use iced::{Background, Border, Element, Length, Task};

use zos_core::commands::grub::{self, GrubStatus};

use crate::services::power;
use crate::theme;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// User moved the timeout slider.
    SetTimeout(u32),
    /// User pressed "Apply Timeout".
    ApplyTimeout,
    /// Async result of applying timeout (success, status message).
    TimeoutApplied(bool, String),
    /// User pressed "Create Windows Boot Entry".
    CreateWindowsBls,
    /// Async result of creating BLS entry (success, status message).
    BlsCreated(bool, String),
    /// User pressed "Reboot to Windows" (first click).
    RebootToWindows,
    /// User confirmed reboot to Windows.
    ConfirmReboot,
    /// User cancelled reboot confirmation.
    CancelReboot,
    /// Refresh GRUB status from disk.
    Refresh,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct BootPage {
    is_root: bool,
    grub_status: GrubStatus,
    timeout_value: u32,
    status_message: Option<String>,
    confirming_reboot: bool,
}

impl BootPage {
    pub fn new() -> Self {
        let grub_status = grub::get_grub_status();
        let timeout_value = grub_status.current_timeout.unwrap_or(5);

        Self {
            is_root: grub::is_root(),
            grub_status,
            timeout_value,
            status_message: None,
            confirming_reboot: false,
        }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SetTimeout(val) => {
                self.timeout_value = val;
                Task::none()
            }
            Message::ApplyTimeout => {
                let seconds = self.timeout_value;
                self.status_message = None;
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            match grub::apply_grub_timeout(seconds) {
                                Ok(()) => (true, format!("Timeout set to {}s", seconds)),
                                Err(e) => (false, format!("Failed: {}", e)),
                            }
                        })
                        .await
                        .unwrap_or((false, "Internal error".to_string()))
                    },
                    |(ok, msg)| Message::TimeoutApplied(ok, msg),
                )
            }
            Message::TimeoutApplied(ok, msg) => {
                self.status_message = Some(msg);
                if ok {
                    self.grub_status.current_timeout = Some(self.timeout_value);
                }
                Task::none()
            }
            Message::CreateWindowsBls => {
                self.status_message = None;
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| match grub::create_windows_bls() {
                            Ok(()) => (true, "Windows boot entry created".to_string()),
                            Err(e) => (false, format!("Failed: {}", e)),
                        })
                        .await
                        .unwrap_or((false, "Internal error".to_string()))
                    },
                    |(ok, msg)| Message::BlsCreated(ok, msg),
                )
            }
            Message::BlsCreated(ok, msg) => {
                self.status_message = Some(msg);
                if ok {
                    self.grub_status.bls_entry_exists = true;
                }
                Task::none()
            }
            Message::RebootToWindows => {
                self.confirming_reboot = true;
                Task::none()
            }
            Message::ConfirmReboot => {
                self.confirming_reboot = false;
                power::reboot_to_windows();
                Task::none()
            }
            Message::CancelReboot => {
                self.confirming_reboot = false;
                Task::none()
            }
            Message::Refresh => {
                self.grub_status = grub::get_grub_status();
                self.timeout_value = self.grub_status.current_timeout.unwrap_or(5);
                self.status_message = None;
                self.confirming_reboot = false;
                Task::none()
            }
        }
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Boot").size(28).color(theme::TEXT);

        let mut content = column![title].spacing(16).width(Length::Fill);

        // Root warning banner
        if !self.is_root {
            content = content.push(self.view_root_warning());
        }

        // GRUB Timeout section
        content = content.push(self.view_timeout_section());

        // Windows Boot section (only if Windows detected)
        if self.grub_status.windows_detected {
            content = content.push(self.view_windows_section());
        }

        // Status message
        if let Some(ref msg) = self.status_message {
            let color = if msg.starts_with("Failed") {
                theme::RED
            } else {
                theme::GREEN
            };
            content = content.push(text(msg.as_str()).size(13).color(color));
        }

        scrollable(
            container(content)
                .width(Length::Fill)
                .height(Length::Shrink),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // -----------------------------------------------------------------------
    // View helpers
    // -----------------------------------------------------------------------

    fn view_root_warning(&self) -> Element<'_, Message> {
        let warning_text = text("Some boot settings require root privileges")
            .size(13)
            .color(theme::BASE);

        container(warning_text)
            .padding([10, 16])
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme::YELLOW)),
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }

    fn view_timeout_section(&self) -> Element<'_, Message> {
        let heading = text("GRUB Timeout").size(18).color(theme::TEXT);

        // Current timeout display
        let current_label = match self.grub_status.current_timeout {
            Some(t) => format!("Current timeout: {}s", t),
            None => "Current timeout: not set".to_string(),
        };
        let current_text = text(current_label).size(13).color(theme::SUBTEXT0);

        // Timeout slider
        let slider_label = text(format!("New timeout: {}s", self.timeout_value))
            .size(13)
            .color(theme::SUBTEXT0);

        let timeout_slider = slider(0..=30_u32, self.timeout_value, Message::SetTimeout)
            .step(1_u32)
            .width(Length::Fixed(280.0));

        let slider_row = row![
            slider_label,
            Space::new().width(Length::Fill),
            timeout_slider
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        // Apply button
        let apply_btn = action_button("Apply Timeout", Some(Message::ApplyTimeout), theme::BLUE);

        let card_content = column![current_text, slider_row, apply_btn].spacing(10);

        column![heading, Space::new().height(4), card(card_content)]
            .spacing(8)
            .into()
    }

    fn view_windows_section(&self) -> Element<'_, Message> {
        let heading = text("Windows Boot").size(18).color(theme::TEXT);

        let mut card_content = column![].spacing(10);

        // Windows path display
        if let Some(ref path) = self.grub_status.windows_path {
            let path_text = text(format!("Windows detected at: {}", path))
                .size(13)
                .color(theme::SUBTEXT0);
            card_content = card_content.push(path_text);
        }

        // BLS entry status
        if self.grub_status.bls_entry_exists {
            let dot = text("\u{25CF} ").size(14).color(theme::GREEN);
            let label = text("Boot entry configured").size(13).color(theme::TEXT);
            let status_row = row![dot, label].align_y(iced::Alignment::Center);
            card_content = card_content.push(status_row);
        } else {
            let create_btn = action_button(
                "Create Windows Boot Entry",
                Some(Message::CreateWindowsBls),
                theme::BLUE,
            );
            card_content = card_content.push(create_btn);
        }

        // Reboot to Windows button with confirmation
        let reboot_el: Element<'_, Message> = if self.confirming_reboot {
            let prompt = text("Are you sure?").size(13).color(theme::TEXT);
            let confirm_btn = action_button("Confirm", Some(Message::ConfirmReboot), theme::RED);
            let cancel_btn = action_button("Cancel", Some(Message::CancelReboot), theme::OVERLAY0);
            row![prompt, Space::new().width(8), confirm_btn, cancel_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            action_button(
                "Reboot to Windows",
                Some(Message::RebootToWindows),
                theme::PEACH,
            )
        };

        card_content = card_content.push(reboot_el);

        column![heading, Space::new().height(4), card(card_content)]
            .spacing(8)
            .into()
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

/// A styled action button with accent color.
fn action_button(
    label: &str,
    on_press: Option<Message>,
    accent: iced::Color,
) -> Element<'_, Message> {
    let btn_label = text(label).size(13).color(theme::BASE);
    let mut btn = button(btn_label)
        .padding([8, 16])
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color { a: 0.85, ..accent },
                button::Status::Pressed => iced::Color { a: 0.70, ..accent },
                button::Status::Disabled => theme::SURFACE1,
                _ => accent,
            };
            let text_color = match status {
                button::Status::Disabled => theme::OVERLAY0,
                _ => theme::BASE,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }

    btn.into()
}
