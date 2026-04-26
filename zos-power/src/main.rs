//! zos-power — centered-popup power menu replacing wlogout.
//!
//! A layer-shell overlay (Catppuccin Mocha) presenting four primary
//! actions — Lock, Logout, Reboot, Shutdown — plus a Reboot dropdown
//! with "Reboot to Windows" (delegating to
//! [`zos_core::commands::grub::reboot_to_windows_elevated`]).
//!
//! Esc dismisses without acting. Any action runs the corresponding
//! command then exits the process.

use iced::{
    Background, Border, Color, Element, Event, Length, Padding, Subscription, Task, Theme,
    alignment::Horizontal,
    border::Radius,
    event,
    widget::{Column, Space, button, column, container, row, text},
};
use iced_layershell::settings::Settings;
use iced_layershell::to_layer_message;
use std::process::Command as StdCommand;
use tracing_subscriber::EnvFilter;
use zos_ui::theme;

#[derive(Default)]
struct PowerMenu {
    show_reboot_menu: bool,
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Msg {
    Lock,
    Logout,
    RebootToggle,
    Reboot,
    RebootToWindows,
    RebootToWindowsPersistent,
    Shutdown,
    Cancel,
    Event(Event),
}

fn boot() -> (PowerMenu, Task<Msg>) {
    (PowerMenu::default(), Task::none())
}

fn namespace() -> String {
    "zos-power".into()
}

fn theme_fn(_state: &PowerMenu) -> Theme {
    theme::zos_theme()
}

fn update(state: &mut PowerMenu, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Lock => {
            spawn_detached(&["loginctl", "lock-session"]);
            std::process::exit(0);
        }
        Msg::Logout => {
            spawn_detached(&["loginctl", "terminate-user", &whoami()]);
            std::process::exit(0);
        }
        Msg::RebootToggle => {
            state.show_reboot_menu = !state.show_reboot_menu;
        }
        Msg::Reboot => {
            spawn_detached(&["systemctl", "reboot"]);
            std::process::exit(0);
        }
        Msg::RebootToWindows => {
            if let Err(e) = zos_core::commands::grub::reboot_to_windows_elevated() {
                tracing::error!(error = ?e, "reboot_to_windows_elevated failed; falling back to plain reboot");
                spawn_detached(&["systemctl", "reboot"]);
            }
            std::process::exit(0);
        }
        Msg::RebootToWindowsPersistent => {
            match zos_core::commands::grub::set_persistent_boot_target_elevated(
                zos_core::commands::grub::BootTarget::Windows,
            ) {
                Ok(_new_order) => {
                    spawn_detached(&["systemctl", "reboot"]);
                }
                Err(e) => {
                    tracing::error!(error = ?e, "set_persistent_boot_target_elevated(Windows) failed; falling back to plain reboot");
                    spawn_detached(&["systemctl", "reboot"]);
                }
            }
            std::process::exit(0);
        }
        Msg::Shutdown => {
            spawn_detached(&["systemctl", "poweroff"]);
            std::process::exit(0);
        }
        Msg::Cancel => std::process::exit(0),
        Msg::Event(ev) => {
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = ev
                && let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) = key
            {
                std::process::exit(0);
            }
        }
        // Layer-shell control messages (anchor/size/margin/etc.) injected by
        // `#[to_layer_message]`. zos-power doesn't reconfigure its surface
        // at runtime — the centered popup is fully described by the initial
        // `LayerShellSettings` — so these are no-ops.
        _ => {}
    }
    Task::none()
}

fn view(state: &PowerMenu) -> Element<'_, Msg> {
    let lock_btn = action_button("\u{f033e}", "Lock", Msg::Lock); // mdi lock
    let logout_btn = action_button("\u{f0343}", "Logout", Msg::Logout); // mdi logout
    let shutdown_btn = action_button("\u{f0425}", "Shutdown", Msg::Shutdown); // mdi power

    let reboot_main: Element<'_, Msg> = button(
        column![
            text("\u{f0709}").size(theme::font_size::X3L), // mdi restart
            text("Reboot").size(theme::font_size::BASE),
        ]
        .align_x(Horizontal::Center)
        .spacing(theme::space::X2),
    )
    .on_press(Msg::Reboot)
    .style(action_btn_style)
    .padding(theme::space::X4)
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(120.0))
    .into();

    let chevron_glyph = if state.show_reboot_menu {
        "\u{25B2}" // up triangle
    } else {
        "\u{25BC}" // down triangle
    };

    let reboot_chevron: Element<'_, Msg> = button(text(chevron_glyph).size(theme::font_size::SM))
        .on_press(Msg::RebootToggle)
        .style(chevron_style)
        .padding(Padding::from([4.0_f32, 8.0_f32]))
        .into();

    let reboot_block: Element<'_, Msg> = column![
        row![reboot_main, reboot_chevron]
            .spacing(theme::space::X1)
            .align_y(iced::alignment::Vertical::Top),
    ]
    .into();

    let reboot_block: Element<'_, Msg> = if state.show_reboot_menu {
        column![
            reboot_block,
            button(text("Reboot to Windows").size(theme::font_size::SM))
                .on_press(Msg::RebootToWindows)
                .style(submenu_style)
                .width(Length::Fixed(220.0))
                .padding(Padding::from(8.0_f32)),
            button(text("Windows (Persistent)").size(theme::font_size::SM))
                .on_press(Msg::RebootToWindowsPersistent)
                .style(submenu_style)
                .width(Length::Fixed(220.0))
                .padding(Padding::from(8.0_f32)),
        ]
        .spacing(theme::space::X2)
        .align_x(Horizontal::Center)
        .into()
    } else {
        reboot_block
    };

    let grid: Column<'_, Msg> = column![
        row![lock_btn, logout_btn].spacing(theme::space::X3),
        row![reboot_block, shutdown_btn]
            .spacing(theme::space::X3)
            .align_y(iced::alignment::Vertical::Top),
    ]
    .spacing(theme::space::X3)
    .align_x(Horizontal::Center);

    let title = text("Power")
        .size(theme::font_size::X2L)
        .color(theme::TEXT)
        .align_x(Horizontal::Center);

    let cancel_btn = button(text("Cancel (Esc)").size(theme::font_size::SM))
        .on_press(Msg::Cancel)
        .style(cancel_style)
        .padding(Padding::from([6.0_f32, 16.0_f32]));

    let body: Column<'_, Msg> = column![
        title,
        grid,
        Space::new().height(theme::space::X4),
        cancel_btn,
    ]
    .spacing(theme::space::X4)
    .align_x(Horizontal::Center);

    container(body)
        .padding(theme::space::X6)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(window_bg_style)
        .into()
}

fn subscription(_state: &PowerMenu) -> Subscription<Msg> {
    event::listen().map(Msg::Event)
}

fn action_button<'a>(icon: &'a str, label: &'a str, msg: Msg) -> Element<'a, Msg> {
    button(
        column![
            text(icon).size(theme::font_size::X3L),
            text(label).size(theme::font_size::BASE),
        ]
        .align_x(Horizontal::Center)
        .spacing(theme::space::X2),
    )
    .on_press(msg)
    .style(action_btn_style)
    .padding(theme::space::X4)
    .width(Length::Fixed(120.0))
    .height(Length::Fixed(120.0))
    .into()
}

// --- Styles ---------------------------------------------------------------

fn window_bg_style(_: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(Background::Color(theme::BASE)),
        text_color: Some(theme::TEXT),
        border: Border {
            color: theme::SURFACE1,
            width: 1.0,
            radius: Radius::from(theme::radius::LG),
        },
        ..Default::default()
    }
}

fn action_btn_style(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => theme::SURFACE1,
        button::Status::Pressed => theme::SURFACE2,
        _ => theme::SURFACE0,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: theme::TEXT,
        border: Border {
            color: theme::SURFACE2,
            width: 1.0,
            radius: Radius::from(theme::radius::MD),
        },
        ..Default::default()
    }
}

fn chevron_style(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => theme::SURFACE2,
        button::Status::Pressed => theme::OVERLAY0,
        _ => theme::SURFACE1,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: theme::TEXT,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::from(theme::radius::SM),
        },
        ..Default::default()
    }
}

fn submenu_style(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => theme::SURFACE1,
        button::Status::Pressed => theme::SURFACE2,
        _ => theme::SURFACE0,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: theme::SUBTEXT0,
        border: Border {
            color: theme::SURFACE2,
            width: 1.0,
            radius: Radius::from(theme::radius::SM),
        },
        ..Default::default()
    }
}

fn cancel_style(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => theme::SURFACE0,
        button::Status::Pressed => theme::SURFACE1,
        _ => theme::CRUST,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: theme::SUBTEXT0,
        border: Border {
            color: theme::SURFACE0,
            width: 1.0,
            radius: Radius::from(theme::radius::FULL),
        },
        ..Default::default()
    }
}

// --- Process helpers ------------------------------------------------------

fn spawn_detached(cmd: &[&str]) {
    if cmd.is_empty() {
        return;
    }
    let _ = StdCommand::new(cmd[0]).args(&cmd[1..]).spawn();
}

fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| {
        std::process::Command::new("id")
            .arg("-un")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "user".to_string())
    })
}

// --- Entrypoint -----------------------------------------------------------

fn main() -> iced_layershell::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-power starting");

    let layer_settings = zos_ui::layer::layer_shell::centered_popup(360, 480);

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
