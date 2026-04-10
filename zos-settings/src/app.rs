// === zos-settings — main application shell ===

use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Element, Length, Subscription, Task, Theme};

use crate::pages;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Overview,
    Display,
    Audio,
    Appearance,
    Input,
    Network,
    Dock,
    Power,
    Boot,
}

impl Page {
    pub const ALL: [Page; 9] = [
        Page::Overview,
        Page::Display,
        Page::Audio,
        Page::Appearance,
        Page::Input,
        Page::Network,
        Page::Dock,
        Page::Power,
        Page::Boot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Display => "Display",
            Page::Audio => "Audio",
            Page::Appearance => "Appearance",
            Page::Input => "Input",
            Page::Network => "Network",
            Page::Dock => "Dock",
            Page::Power => "Power",
            Page::Boot => "Boot",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    SelectPage(Page),
    Audio(pages::audio::Message),
    Overview(pages::overview::Message),
    Display(pages::display::Message),
    Appearance(pages::appearance::Message),
    Input(pages::input::Message),
    Network(pages::network::Message),
    Dock(pages::dock::Message),
    Power(pages::power::Message),
    Boot(pages::boot::Message),
}

pub struct App {
    current: Page,
    audio: pages::audio::AudioPage,
    overview: pages::overview::OverviewPage,
    display: pages::display::DisplayPage,
    appearance: pages::appearance::AppearancePage,
    input: pages::input::InputPage,
    network: pages::network::NetworkPage,
    dock: pages::dock::DockPage,
    power: pages::power::PowerPage,
    boot: pages::boot::BootPage,
}

impl App {
    pub fn boot() -> Self {
        Self {
            current: Page::Overview,
            audio: pages::audio::AudioPage::new(),
            overview: pages::overview::OverviewPage::new(),
            display: pages::display::DisplayPage::new(),
            appearance: pages::appearance::AppearancePage::new(),
            input: pages::input::InputPage::new(),
            network: pages::network::NetworkPage::new(),
            dock: pages::dock::DockPage::new(),
            power: pages::power::PowerPage::new(),
            boot: pages::boot::BootPage::new(),
        }
    }

    pub fn title(&self) -> String {
        format!("zOS Settings — {}", self.current.label())
    }

    pub fn theme(&self) -> Theme {
        theme::zos_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectPage(p) => {
                self.current = p;
                Task::none()
            }
            Message::Audio(m) => self.audio.update(m).map(Message::Audio),
            Message::Overview(m) => self.overview.update(m).map(Message::Overview),
            Message::Display(m) => self.display.update(m).map(Message::Display),
            Message::Appearance(m) => self.appearance.update(m).map(Message::Appearance),
            Message::Input(m) => self.input.update(m).map(Message::Input),
            Message::Network(m) => self.network.update(m).map(Message::Network),
            Message::Dock(m) => self.dock.update(m).map(Message::Dock),
            Message::Power(m) => self.power.update(m).map(Message::Power),
            Message::Boot(m) => self.boot.update(m).map(Message::Boot),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content: Element<Message> = match self.current {
            Page::Overview => self.overview.view().map(Message::Overview),
            Page::Display => self.display.view().map(Message::Display),
            Page::Audio => self.audio.view().map(Message::Audio),
            Page::Appearance => self.appearance.view().map(Message::Appearance),
            Page::Input => self.input.view().map(Message::Input),
            Page::Network => self.network.view().map(Message::Network),
            Page::Dock => self.dock.view().map(Message::Dock),
            Page::Power => self.power.view().map(Message::Power),
            Page::Boot => self.boot.view().map(Message::Boot),
        };

        let sidebar = sidebar(self.current);

        row![
            sidebar,
            container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(24),
        ]
        .height(Length::Fill)
        .into()
    }
}

fn sidebar(current: Page) -> Element<'static, Message> {
    let mut col = column![].spacing(4).padding(16).width(Length::Fixed(200.0));
    col = col.push(text("zOS Settings").size(18).color(theme::TEXT));
    col = col.push(Space::new().height(12));
    for page in Page::ALL {
        let is_active = page == current;
        let label =
            text(page.label())
                .size(14)
                .color(if is_active { theme::BASE } else { theme::TEXT });
        let btn = button(label)
            .on_press(Message::SelectPage(page))
            .width(Length::Fill)
            .style(move |_theme, status| sidebar_button_style(is_active, status));
        col = col.push(btn);
    }
    container(col)
        .width(Length::Fixed(220.0))
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(theme::MANTLE.into()),
            ..container::Style::default()
        })
        .into()
}

fn sidebar_button_style(is_active: bool, status: button::Status) -> button::Style {
    let base = if is_active {
        Some(Background::Color(theme::BLUE))
    } else {
        match status {
            button::Status::Hovered => Some(Background::Color(theme::SURFACE0)),
            _ => None,
        }
    };
    button::Style {
        background: base,
        text_color: if is_active { theme::BASE } else { theme::TEXT },
        border: iced::Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
