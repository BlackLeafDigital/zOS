// === pages/overview.rs — Dashboard page for zOS Settings ===

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Length, Task};

use zos_core::commands::{doctor, setup, status, update};
use zos_core::config;

use crate::theme;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// Doctor checks completed (pass, fail, warn).
    HealthLoaded(usize, usize, usize),
    /// Update check completed.
    UpdateLoaded(bool, Option<String>),
    /// User pressed "Run Health Check".
    RunHealthCheck,
    /// User pressed "Check for Updates".
    CheckUpdates,
    /// Update apply finished; bool = success.
    UpdateApplied(bool, String),
    /// User pressed "Apply Update".
    ApplyUpdate,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct OverviewPage {
    info: status::SystemInfo,
    health_pass: usize,
    health_fail: usize,
    health_warn: usize,
    setup_installed: usize,
    setup_total: usize,
    update_pending: bool,
    update_details: Option<String>,
    loading_health: bool,
    loading_update: bool,
    applying_update: bool,
    update_result: Option<String>,
}

impl OverviewPage {
    pub fn new() -> Self {
        let info = status::get_system_info();

        let checks = doctor::run_doctor_checks();
        let (pass, fail, warn) = doctor::summarize(&checks);

        let steps = setup::get_setup_steps();
        let setup_installed = steps.iter().filter(|s| s.installed).count();
        let setup_total = steps.len();

        let (update_pending, update_details) = match update::check_for_updates() {
            Ok(us) => (us.pending, us.pending_details),
            Err(_) => (false, None),
        };

        Self {
            info,
            health_pass: pass,
            health_fail: fail,
            health_warn: warn,
            setup_installed,
            setup_total,
            update_pending,
            update_details,
            loading_health: false,
            loading_update: false,
            applying_update: false,
            update_result: None,
        }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::HealthLoaded(pass, fail, warn) => {
                self.health_pass = pass;
                self.health_fail = fail;
                self.health_warn = warn;
                self.loading_health = false;
                Task::none()
            }
            Message::UpdateLoaded(pending, details) => {
                self.update_pending = pending;
                self.update_details = details;
                self.loading_update = false;
                Task::none()
            }
            Message::RunHealthCheck => {
                self.loading_health = true;
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            let checks = doctor::run_doctor_checks();
                            doctor::summarize(&checks)
                        })
                        .await
                        .unwrap_or((0, 0, 0))
                    },
                    |(pass, fail, warn)| Message::HealthLoaded(pass, fail, warn),
                )
            }
            Message::CheckUpdates => {
                self.loading_update = true;
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| match update::check_for_updates() {
                            Ok(us) => (us.pending, us.pending_details),
                            Err(_) => (false, None),
                        })
                        .await
                        .unwrap_or((false, None))
                    },
                    |(pending, details)| Message::UpdateLoaded(pending, details),
                )
            }
            Message::ApplyUpdate => {
                self.applying_update = true;
                self.update_result = None;
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| match update::apply_update() {
                            Ok(output) => {
                                if output.status.success() {
                                    (true, update::reboot_message().to_string())
                                } else {
                                    let stderr =
                                        String::from_utf8_lossy(&output.stderr).to_string();
                                    (false, format!("Update failed: {}", stderr))
                                }
                            }
                            Err(e) => (false, format!("Update error: {}", e)),
                        })
                        .await
                        .unwrap_or((false, "Internal error running update".to_string()))
                    },
                    |(ok, msg)| Message::UpdateApplied(ok, msg),
                )
            }
            Message::UpdateApplied(ok, msg) => {
                self.applying_update = false;
                self.update_result = Some(msg);
                if ok {
                    self.update_pending = false;
                }
                Task::none()
            }
        }
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let welcome = self.view_welcome();
        let status_cards = self.view_status_cards();
        let sys_info = self.view_system_info();
        let actions = self.view_quick_actions();

        let content = column![welcome, status_cards, sys_info, actions,]
            .spacing(24)
            .width(Length::Fill);

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

    fn view_welcome(&self) -> Element<'_, Message> {
        let title = text("Welcome to zOS").size(28).color(theme::TEXT);
        let subtitle = text(format!(
            "Version {} on Fedora {}",
            self.info.os_version, self.info.fedora_version
        ))
        .size(14)
        .color(theme::SUBTEXT0);

        column![title, subtitle].spacing(4).into()
    }

    fn view_status_cards(&self) -> Element<'_, Message> {
        let health_card = self.view_health_card();
        let update_card = self.view_update_card();
        let setup_card = self.view_setup_card();

        row![health_card, update_card, setup_card]
            .spacing(16)
            .width(Length::Fill)
            .into()
    }

    fn view_health_card(&self) -> Element<'_, Message> {
        let total = self.health_pass + self.health_fail + self.health_warn;
        let (indicator_color, label) = if self.loading_health {
            (theme::OVERLAY0, "Checking...".to_string())
        } else if self.health_fail > 0 {
            (
                theme::RED,
                format!("{}/{} checks passed", self.health_pass, total),
            )
        } else if self.health_warn > 0 {
            (
                theme::YELLOW,
                format!("{}/{} checks passed", self.health_pass, total),
            )
        } else {
            (theme::GREEN, format!("All {} checks passed", total))
        };

        let dot = text("\u{25CF} ").size(16).color(indicator_color);
        let status_text = text(label).size(14).color(theme::TEXT);
        let status_row = row![dot, status_text].align_y(iced::Alignment::Center);

        let card_content = column![
            text("Health").size(12).color(theme::SUBTEXT0),
            Space::new().height(4),
            status_row,
        ]
        .width(Length::Fill);

        card(card_content).into()
    }

    fn view_update_card(&self) -> Element<'_, Message> {
        let (indicator_color, label) = if self.loading_update {
            (theme::OVERLAY0, "Checking...".to_string())
        } else if self.update_pending {
            (theme::PEACH, "Update available".to_string())
        } else {
            (theme::GREEN, "Up to date".to_string())
        };

        let dot = text("\u{25CF} ").size(16).color(indicator_color);
        let status_text = text(label).size(14).color(theme::TEXT);
        let status_row = row![dot, status_text].align_y(iced::Alignment::Center);

        let card_content = column![
            text("System").size(12).color(theme::SUBTEXT0),
            Space::new().height(4),
            status_row,
        ]
        .width(Length::Fill);

        card(card_content).into()
    }

    fn view_setup_card(&self) -> Element<'_, Message> {
        let all_done = self.setup_installed == self.setup_total && config::is_setup_done();

        let (indicator_color, label) = if all_done {
            (theme::GREEN, "All set".to_string())
        } else {
            (
                theme::YELLOW,
                format!("{}/{} tools ready", self.setup_installed, self.setup_total),
            )
        };

        let dot = text("\u{25CF} ").size(16).color(indicator_color);
        let status_text = text(label).size(14).color(theme::TEXT);
        let status_row = row![dot, status_text].align_y(iced::Alignment::Center);

        let card_content = column![
            text("Setup").size(12).color(theme::SUBTEXT0),
            Space::new().height(4),
            status_row,
        ]
        .width(Length::Fill);

        card(card_content).into()
    }

    fn view_system_info(&self) -> Element<'_, Message> {
        let rows = [
            ("OS Version", self.info.os_version.as_str()),
            ("Image", self.info.image_name.as_str()),
            ("Fedora Version", self.info.fedora_version.as_str()),
            ("Last Update", self.info.last_update.as_str()),
        ];

        let mut info_col = column![].spacing(8);

        for (label, value) in rows {
            let kv_row = row![
                text(label)
                    .size(13)
                    .color(theme::SUBTEXT0)
                    .width(Length::Fixed(140.0)),
                text(value).size(13).color(theme::TEXT),
            ]
            .align_y(iced::Alignment::Center);
            info_col = info_col.push(kv_row);
        }

        let section = column![
            text("System Information").size(16).color(theme::TEXT),
            Space::new().height(4),
            card(info_col),
        ]
        .spacing(8)
        .width(Length::Fill);

        section.into()
    }

    fn view_quick_actions(&self) -> Element<'_, Message> {
        let health_btn = action_button(
            if self.loading_health {
                "Checking..."
            } else {
                "Run Health Check"
            },
            if self.loading_health {
                None
            } else {
                Some(Message::RunHealthCheck)
            },
            theme::BLUE,
        );

        let update_btn = if self.update_pending {
            action_button(
                if self.applying_update {
                    "Applying..."
                } else {
                    "Apply Update"
                },
                if self.applying_update {
                    None
                } else {
                    Some(Message::ApplyUpdate)
                },
                theme::PEACH,
            )
        } else {
            action_button(
                if self.loading_update {
                    "Checking..."
                } else {
                    "Check for Updates"
                },
                if self.loading_update {
                    None
                } else {
                    Some(Message::CheckUpdates)
                },
                theme::BLUE,
            )
        };

        let mut actions_row = row![health_btn, update_btn].spacing(12);

        // Show update result message if present.
        if let Some(ref msg) = self.update_result {
            let msg_color = if msg.starts_with("Update failed") || msg.starts_with("Update error") {
                theme::RED
            } else {
                theme::GREEN
            };
            actions_row = actions_row.push(
                container(text(msg.as_str()).size(13).color(msg_color))
                    .align_y(iced::alignment::Vertical::Center),
            );
        }

        let section = column![
            text("Quick Actions").size(16).color(theme::TEXT),
            Space::new().height(4),
            actions_row,
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
