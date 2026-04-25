// === pages/power.rs — Power actions (suspend, reboot, shutdown) ===

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Length, Task};

use crate::services::power;
use crate::theme;

// ---------------------------------------------------------------------------
// Power action enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PowerAction {
    Suspend,
    Reboot,
    Shutdown,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    /// User clicked an action button — enter confirmation state.
    RequestConfirm(PowerAction),
    /// User confirmed the action.
    Confirm,
    /// User cancelled the confirmation.
    Cancel,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct PowerPage {
    confirming: Option<PowerAction>,
}

impl PowerPage {
    pub fn new() -> Self {
        Self { confirming: None }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RequestConfirm(action) => {
                self.confirming = Some(action);
                Task::none()
            }
            Message::Cancel => {
                self.confirming = None;
                Task::none()
            }
            Message::Confirm => {
                if let Some(action) = self.confirming.take() {
                    match action {
                        PowerAction::Suspend => power::suspend(),
                        PowerAction::Reboot => power::reboot(),
                        PowerAction::Shutdown => power::shutdown(),
                    }
                }
                Task::none()
            }
        }
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Power").size(28).color(theme::TEXT);

        let cards = column![
            self.view_action_card(
                PowerAction::Suspend,
                "Suspend",
                "Suspends your system to RAM",
                "Suspend",
                theme::BLUE,
            ),
            self.view_action_card(
                PowerAction::Reboot,
                "Reboot",
                "Restarts your system",
                "Reboot",
                theme::YELLOW,
            ),
            self.view_action_card(
                PowerAction::Shutdown,
                "Shut Down",
                "Powers off your system",
                "Shut Down",
                theme::RED,
            ),
        ]
        .spacing(16);

        let content = column![title, cards].spacing(24).width(Length::Fill);

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

    fn view_action_card<'a>(
        &'a self,
        action: PowerAction,
        title: &'a str,
        description: &'a str,
        button_label: &'a str,
        accent: iced::Color,
    ) -> Element<'a, Message> {
        let title_text = text(title).size(18).color(theme::TEXT);
        let desc_text = text(description).size(13).color(theme::SUBTEXT0);

        let is_confirming = self.confirming == Some(action);

        let action_area: Element<'_, Message> = if is_confirming {
            let prompt = text("Are you sure?").size(13).color(theme::YELLOW);

            let confirm_btn = button(text("Confirm").size(13).color(theme::BASE))
                .on_press(Message::Confirm)
                .padding([8, 16])
                .style(move |_theme, status| {
                    let bg = match status {
                        button::Status::Hovered => iced::Color { a: 0.85, ..accent },
                        button::Status::Pressed => iced::Color { a: 0.70, ..accent },
                        _ => accent,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: theme::BASE,
                        border: Border {
                            radius: 8.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });

            let cancel_btn = button(text("Cancel").size(13).color(theme::TEXT))
                .on_press(Message::Cancel)
                .padding([8, 16])
                .style(|_theme, status| {
                    let bg = match status {
                        button::Status::Hovered => theme::SURFACE2,
                        button::Status::Pressed => theme::OVERLAY0,
                        _ => theme::SURFACE1,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: theme::TEXT,
                        border: Border {
                            radius: 8.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });

            row![prompt, Space::new().width(12), confirm_btn, cancel_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            let btn = button(text(button_label).size(13).color(theme::BASE))
                .on_press(Message::RequestConfirm(action))
                .padding([8, 16])
                .style(move |_theme, status| {
                    let bg = match status {
                        button::Status::Hovered => iced::Color { a: 0.85, ..accent },
                        button::Status::Pressed => iced::Color { a: 0.70, ..accent },
                        _ => accent,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: theme::BASE,
                        border: Border {
                            radius: 8.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                });

            btn.into()
        };

        let info = column![title_text, desc_text]
            .spacing(4)
            .width(Length::Fill);

        let card_content = row![info, action_area]
            .spacing(16)
            .align_y(iced::Alignment::Center);

        container(card_content)
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
