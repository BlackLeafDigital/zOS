// === zos-settings — iced edition ===

mod app;
mod pages;
mod services;
mod theme;

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application(app::App::boot, app::App::update, app::App::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .theme(app::App::theme)
        .window_size(iced::Size::new(920.0, 640.0))
        .run()
}
