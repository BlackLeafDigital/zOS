//! `cargo run -p zos-ui --example clock`
//!
//! Minimal demo of the zos-ui theme + widget set: a digital clock that
//! ticks every second. Uses iced 0.14's functional `iced::application`
//! builder rather than the legacy `Application` trait — pass plain
//! `boot` / `update` / `view` functions, then chain `.theme(...)` /
//! `.subscription(...)` / `.title(...)` / `.run()`.
//!
//! No layer-shell here — it's a regular floating iced window so the
//! example can be `cargo build`-verified without a Wayland compositor
//! configured for layer surfaces.
//!
//! NOTE: this example does not yet integrate with `zos_ui::signal` — the
//! signal/iced bridge is a follow-up task. For now the clock state is
//! stored in plain iced application state and updated via a `Tick`
//! message driven by `iced::time::every`.

use std::time::Duration;

use iced::{
    Element, Length, Subscription, Task, Theme,
    widget::{column, container, text},
};
use zos_ui::theme;

#[derive(Default)]
struct App {
    now: String,
}

#[derive(Debug, Clone)]
enum Msg {
    Tick,
}

fn current_time() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn boot() -> (App, Task<Msg>) {
    (
        App {
            now: current_time(),
        },
        Task::none(),
    )
}

fn update(state: &mut App, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Tick => state.now = current_time(),
    }
    Task::none()
}

fn view(state: &App) -> Element<'_, Msg> {
    container(
        column![
            text("zOS Clock")
                .size(theme::font_size::X2L)
                .color(theme::TEXT),
            text(state.now.clone())
                .size(theme::font_size::X3L)
                .color(theme::BLUE),
        ]
        .spacing(theme::space::X4)
        .align_x(iced::alignment::Horizontal::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(|_theme: &Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(theme::BASE)),
        ..Default::default()
    })
    .into()
}

fn subscription(_state: &App) -> Subscription<Msg> {
    iced::time::every(Duration::from_secs(1)).map(|_| Msg::Tick)
}

fn theme(_state: &App) -> Theme {
    theme::zos_theme()
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("zOS Clock")
        .theme(theme)
        .subscription(subscription)
        .run()
}
