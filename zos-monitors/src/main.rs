//! zos-monitors — monitor configuration UI replacing nwg-displays.
//!
//! Phase 6 v1: a regular floating iced window (NOT layer-shell) that
//! lists detected monitors via [`zos_core::compositor::Compositor::monitors`]
//! and writes a Hyprland-format `monitors.conf` to
//! `~/.config/hypr/monitors.conf` on Apply.
//!
//! Visual drag-to-arrange is deferred — v1 is a vertical card list with
//! per-monitor name / resolution / refresh / scale / focused indicator.

use iced::{
    Background, Border, Element, Length, Padding, Task, Theme,
    alignment::Vertical,
    border::Radius,
    widget::{Space, button, column, container, pick_list, row, scrollable, text},
};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
use zos_core::compositor::{self, MonitorInfo, MonitorMode};
use zos_ui::theme;

#[derive(Default)]
struct Monitors {
    monitors: Vec<MonitorInfo>,
    apply_status: Option<String>,
}

impl Monitors {
    fn fresh() -> Self {
        let monitors = compositor::detect()
            .ok()
            .and_then(|c| c.monitors().ok())
            .unwrap_or_default();
        Self {
            monitors,
            apply_status: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    Refresh,
    Apply,
    SetMode(usize, MonitorMode),
}

fn boot() -> (Monitors, Task<Msg>) {
    (Monitors::fresh(), Task::none())
}

fn update(state: &mut Monitors, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Refresh => {
            if let Ok(c) = compositor::detect() {
                state.monitors = c.monitors().unwrap_or_default();
            } else {
                state.monitors.clear();
            }
            state.apply_status = None;
        }
        Msg::Apply => match write_monitors_conf(&state.monitors) {
            Ok(path) => {
                let msg = format!("\u{2713} wrote {}", path.display());
                tracing::info!(?path, "wrote monitors.conf");
                state.apply_status = Some(msg);
            }
            Err(e) => {
                tracing::error!(error = ?e, "failed to write monitors.conf");
                state.apply_status = Some(format!("\u{2717} {e}"));
            }
        },
        Msg::SetMode(idx, mode) => {
            if let Some(m) = state.monitors.get_mut(idx) {
                m.width = mode.width;
                m.height = mode.height;
                m.refresh_rate = mode.refresh_hz;
            }
            // User picked a new mode — clear any previous apply status
            // so the bottom row stops showing stale "wrote..." messaging.
            state.apply_status = None;
        }
    }
    Task::none()
}

fn view(state: &Monitors) -> Element<'_, Msg> {
    let header = container(
        row![
            text("Displays")
                .size(theme::font_size::X2L)
                .color(theme::TEXT),
            Space::new().width(Length::Fill),
            button(text("Refresh").size(theme::font_size::SM))
                .on_press(Msg::Refresh)
                .padding(Padding::from([4.0_f32, 12.0_f32]))
                .style(secondary_btn_style),
        ]
        .align_y(Vertical::Center)
        .spacing(theme::space::X3),
    )
    .padding(
        Padding::default()
            .top(theme::space::X4)
            .right(theme::space::X4)
            .bottom(theme::space::X3)
            .left(theme::space::X4),
    );

    let cards: Element<'_, Msg> = if state.monitors.is_empty() {
        container(
            text(
                "No monitors detected. Make sure you're running under a \
                 supported compositor (Hyprland or zos-wm).",
            )
            .size(theme::font_size::SM)
            .color(theme::SUBTEXT0),
        )
        .padding(theme::space::X4)
        .into()
    } else {
        let mut col = column![].spacing(theme::space::X3);
        for (idx, m) in state.monitors.iter().enumerate() {
            col = col.push(monitor_card(idx, m));
        }
        scrollable(col).height(Length::Fill).into()
    };

    let status_text: Element<'_, Msg> = if let Some(status) = &state.apply_status {
        text(status.clone())
            .size(theme::font_size::SM)
            .color(if status.starts_with('\u{2713}') {
                theme::GREEN
            } else {
                theme::RED
            })
            .into()
    } else {
        text("Configure displays, then apply.")
            .size(theme::font_size::SM)
            .color(theme::SUBTEXT0)
            .into()
    };

    let apply_disabled = state.monitors.is_empty();
    let apply_btn = {
        let mut b = button(text("Apply").size(theme::font_size::BASE))
            .padding(Padding::from([6.0_f32, 16.0_f32]))
            .style(primary_btn_style);
        if !apply_disabled {
            b = b.on_press(Msg::Apply);
        }
        b
    };

    let apply_row = row![
        status_text,
        Space::new().width(Length::Fill),
        apply_btn,
    ]
    .align_y(Vertical::Center)
    .spacing(theme::space::X3);

    let body = column![
        header,
        container(cards)
            .padding(Padding::from([0.0_f32, theme::space::X4]))
            .height(Length::Fill),
        container(apply_row).padding(theme::space::X4),
    ];

    container(body)
        .style(window_bg_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn monitor_card<'a>(idx: usize, m: &'a MonitorInfo) -> Element<'a, Msg> {
    let focused_badge: Element<'a, Msg> = if m.focused {
        container(
            text("Focused")
                .size(theme::font_size::XS)
                .color(theme::CRUST),
        )
        .padding(Padding::from([2.0_f32, 8.0_f32]))
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(theme::BLUE)),
            border: Border {
                color: theme::BLUE,
                width: 0.0,
                radius: Radius::from(theme::radius::FULL),
            },
            ..Default::default()
        })
        .into()
    } else {
        // Empty placeholder so the title row still aligns.
        Space::new().into()
    };

    let title = row![
        text(m.name.clone())
            .size(theme::font_size::LG)
            .color(theme::TEXT),
        Space::new().width(Length::Fill),
        focused_badge,
    ]
    .align_y(Vertical::Center)
    .spacing(theme::space::X2);

    let stats = row![
        stat_field("Resolution", &format!("{}\u{00d7}{}", m.width, m.height)),
        stat_field("Refresh", &format!("{:.1} Hz", m.refresh_rate)),
        stat_field("Scale", &format!("{:.2}\u{00d7}", m.scale)),
    ]
    .spacing(theme::space::X4);

    // Mode picker — shows a dropdown of all (resolution, refresh) combos
    // the compositor reports. We build a synthetic "current" mode from the
    // monitor's live width/height/refresh; if it doesn't appear in
    // `available_modes` (some Hyprland versions emit fractional refresh
    // values that don't quite match), the picker still renders with the
    // selection blank, which is fine.
    let current_mode = MonitorMode {
        width: m.width,
        height: m.height,
        refresh_hz: m.refresh_rate,
    };

    let mode_picker: Element<'a, Msg> = if m.available_modes.is_empty() {
        text("No modes reported by compositor")
            .size(theme::font_size::SM)
            .color(theme::SUBTEXT0)
            .into()
    } else {
        pick_list(
            m.available_modes.clone(),
            Some(current_mode),
            move |selected| Msg::SetMode(idx, selected),
        )
        .placeholder("Select mode")
        .text_size(theme::font_size::SM)
        .width(Length::Fixed(280.0))
        .into()
    };

    let mode_section = column![
        text("Mode")
            .size(theme::font_size::XS)
            .color(theme::SUBTEXT0),
        mode_picker,
    ]
    .spacing(2.0);

    container(column![title, stats, mode_section].spacing(theme::space::X3))
        .padding(theme::space::X4)
        .style(card_bg_style)
        .width(Length::Fill)
        .into()
}

fn stat_field<'a>(label: &str, value: &str) -> Element<'a, Msg> {
    column![
        text(label.to_string())
            .size(theme::font_size::XS)
            .color(theme::SUBTEXT0),
        text(value.to_string())
            .size(theme::font_size::BASE)
            .color(theme::TEXT),
    ]
    .spacing(2.0)
    .into()
}

// --- Styles ---------------------------------------------------------------

fn window_bg_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::BASE)),
        text_color: Some(theme::TEXT),
        ..Default::default()
    }
}

fn card_bg_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::SURFACE0)),
        border: Border {
            color: theme::SURFACE1,
            width: 1.0,
            radius: Radius::from(theme::radius::MD),
        },
        ..Default::default()
    }
}

fn primary_btn_style(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => theme::LAVENDER,
        button::Status::Pressed => theme::MAUVE,
        button::Status::Disabled => theme::SURFACE1,
        _ => theme::BLUE,
    };
    let text_color = match status {
        button::Status::Disabled => theme::OVERLAY0,
        _ => theme::CRUST,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: bg,
            width: 0.0,
            radius: Radius::from(theme::radius::SM),
        },
        ..Default::default()
    }
}

fn secondary_btn_style(_: &Theme, status: button::Status) -> button::Style {
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
            radius: Radius::from(theme::radius::SM),
        },
        ..Default::default()
    }
}

// --- Config writeback -----------------------------------------------------

/// Write `~/.config/hypr/monitors.conf` in Hyprland format.
///
/// Format: `monitor=NAME,WIDTHxHEIGHT@RR,POS,SCALE`
///
/// We don't yet expose position editing, so we write `auto` for position
/// (Hyprland's auto-layout). When zos-wm lands its own monitors config
/// in Phase 8, this writer will switch paths.
fn write_monitors_conf(monitors: &[MonitorInfo]) -> std::io::Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("HOME not set: {e}"),
        )
    })?;
    let path = PathBuf::from(home).join(".config/hypr/monitors.conf");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = String::from("# Generated by zos-monitors\n\n");
    for m in monitors {
        content.push_str(&format!(
            "monitor={},{}x{}@{:.0},auto,{:.2}\n",
            m.name, m.width, m.height, m.refresh_rate, m.scale
        ));
    }
    std::fs::write(&path, content)?;
    Ok(path)
}

// --- Entrypoint -----------------------------------------------------------

fn theme_fn(_state: &Monitors) -> Theme {
    theme::zos_theme()
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-monitors starting");

    iced::application(boot, update, view)
        .title("zos-monitors")
        .theme(theme_fn)
        .run()
}
