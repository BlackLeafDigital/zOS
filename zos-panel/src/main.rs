//! zos-panel — top-bar replacing HyprPanel.
//!
//! v1 modules: Clock, Workspaces, Active Window.
//! Future modules: Tray, Audio, Network, Bluetooth, Power.
//!
//! Layer-shell anchored Top/Left/Right with `BAR_HEIGHT` exclusive zone.
//! State refreshes every second via the `subscription` tick — workspaces
//! and active window come from `zos_core::compositor`. The clock is naive
//! UTC (`SystemTime::now()` formatted `%H:%M`); time-zone math is a
//! deliberate follow-up.

use iced::{
    Background, Border, Element, Length, Padding, Subscription, Task, Theme,
    alignment::Vertical,
    border::Radius,
    widget::{Row, Space, button, container, row, text},
};
use iced_layershell::settings::Settings;
use iced_layershell::to_layer_message;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use zos_core::compositor::{self, Compositor, WorkspaceInfo};
use zos_ui::theme;

const BAR_HEIGHT: u32 = 32;

#[derive(Default)]
struct Panel {
    time: String,
    workspaces: Vec<WorkspaceInfo>,
    active_title: String,
}

impl Panel {
    fn new() -> Self {
        Self {
            time: current_hms(),
            workspaces: Vec::new(),
            active_title: String::new(),
        }
    }
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Msg {
    Tick,
    SwitchToWorkspace(i64),
}

/// Naive `HH:MM` formatter from `SystemTime` — assumes the seconds-since-epoch
/// reading is the time we want to display. Time-zone awareness (DST,
/// non-UTC system clocks) is a follow-up; chrono is intentionally not pulled
/// in for v1.
fn current_hms() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    format!("{:02}:{:02}", h, m)
}

/// Pull current workspaces + focused window title from the compositor.
/// Returns `(Vec::new(), String::new())` shape values on partial failure
/// so the panel still renders cleanly.
fn fetch_state(comp: &dyn Compositor) -> (Vec<WorkspaceInfo>, String) {
    let workspaces = comp.workspaces().unwrap_or_default();
    let active = comp.active_window().ok().flatten();
    let title = active.map(|w| w.title).unwrap_or_default();
    (workspaces, title)
}

fn boot() -> (Panel, Task<Msg>) {
    (Panel::new(), Task::none())
}

fn namespace() -> String {
    "zos-panel".into()
}

fn theme_fn(_state: &Panel) -> Theme {
    theme::zos_theme()
}

fn update(state: &mut Panel, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Tick => {
            state.time = current_hms();
            // Refresh compositor data on each tick (cheap shellout). If
            // we're not running under a known compositor (e.g. the panel
            // is launched outside of Hyprland for testing), the clock
            // continues to tick — just no workspaces / title.
            if let Ok(comp) = compositor::detect() {
                let (workspaces, title) = fetch_state(&*comp);
                state.workspaces = workspaces;
                state.active_title = title;
            }
        }
        Msg::SwitchToWorkspace(id) => {
            if let Ok(comp) = compositor::detect() {
                if let Err(e) = comp.switch_to_workspace(id) {
                    tracing::warn!(workspace = id, error = ?e, "switch_to_workspace failed");
                }
            }
        }
        // Layer-shell control messages (anchor/size/margin/etc.) injected
        // by `#[to_layer_message]`. zos-panel doesn't reconfigure its
        // surface at runtime, so these are no-ops.
        _ => {}
    }
    Task::none()
}

fn view(state: &Panel) -> Element<'_, Msg> {
    let workspaces_row: Row<'_, Msg> = state.workspaces.iter().fold(
        row![].spacing(theme::space::X1),
        |acc, ws| {
            let label = format!("{}", ws.id);
            let id = ws.id;
            let active = ws.active;
            acc.push(
                button(text(label).size(theme::font_size::SM))
                    .on_press(Msg::SwitchToWorkspace(id))
                    .style(move |_, status| ws_button_style(active, status))
                    .padding(Padding::from([2.0_f32, 8.0_f32])),
            )
        },
    );

    let clock = container(
        text(state.time.clone())
            .size(theme::font_size::BASE)
            .color(theme::TEXT),
    )
    .padding(Padding::from([0.0_f32, 12.0_f32]));

    // Truncate long titles so they don't push layout around. We byte-slice
    // the first 60 chars after asserting it's a char boundary; UTF-8
    // titles fall through to the unchanged path.
    let title_text = if state.active_title.is_empty() {
        "—".to_string()
    } else if state.active_title.len() > 60 {
        let cutoff = nearest_char_boundary(&state.active_title, 60);
        format!("{}…", &state.active_title[..cutoff])
    } else {
        state.active_title.clone()
    };
    let title = text(title_text)
        .size(theme::font_size::SM)
        .color(theme::SUBTEXT0);

    container(
        row![
            workspaces_row,
            Space::new().width(Length::Fill),
            title,
            Space::new().width(Length::Fill),
            clock,
        ]
        .align_y(Vertical::Center)
        .padding(Padding::from([0.0_f32, 12.0_f32])),
    )
    .height(Length::Fill)
    .width(Length::Fill)
    .style(panel_bg_style)
    .into()
}

fn subscription(_state: &Panel) -> Subscription<Msg> {
    iced::time::every(Duration::from_secs(1)).map(|_| Msg::Tick)
}

/// Find the largest char boundary in `s` that is `<= max`. Returns
/// `s.len()` if `max >= s.len()`. Required because `&s[..max]` panics
/// when `max` falls inside a multi-byte codepoint (most non-ASCII titles).
fn nearest_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut idx = max;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

// --- Styles ---------------------------------------------------------------

fn panel_bg_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::CRUST)),
        border: Border {
            color: theme::SURFACE0,
            width: 1.0,
            radius: Radius::from(0.0),
        },
        ..Default::default()
    }
}

fn ws_button_style(active: bool, status: button::Status) -> button::Style {
    let bg = if active {
        theme::BLUE
    } else {
        match status {
            button::Status::Hovered => theme::SURFACE1,
            _ => theme::SURFACE0,
        }
    };
    let fg = if active { theme::CRUST } else { theme::TEXT };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            color: bg,
            width: 0.0,
            radius: Radius::from(theme::radius::SM),
        },
        ..Default::default()
    }
}

// --- Entrypoint -----------------------------------------------------------

fn main() -> iced_layershell::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-panel starting");

    let layer_settings = zos_ui::layer::layer_shell::top_bar(BAR_HEIGHT);

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
