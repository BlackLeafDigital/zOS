//! zos-launcher — centered-popup app launcher replacing wofi.

use iced::{
    Background, Border, Element, Event, Length, Padding, Subscription, Task, Theme,
    border::Radius,
    keyboard::{Key, key::Named},
    widget::{self, button, column, container, scrollable, text, text_input},
};
use iced_layershell::to_layer_message;
use std::process::Command as StdCommand;
use tracing_subscriber::EnvFilter;

mod desktop_files;
use desktop_files::DesktopEntry;

const POPUP_W: u32 = 600;
const POPUP_H: u32 = 500;
const SEARCH_INPUT_ID: &str = "search-input";
const MAX_RESULTS: usize = 50;

#[derive(Default)]
struct Launcher {
    query: String,
    entries: Vec<DesktopEntry>,
    selected: usize,
}

impl Launcher {
    fn new() -> Self {
        Self {
            entries: desktop_files::discover(),
            query: String::new(),
            selected: 0,
        }
    }

    fn matched(&self) -> Vec<&DesktopEntry> {
        let mut scored: Vec<(i32, &DesktopEntry)> = self
            .entries
            .iter()
            .filter_map(|e| desktop_files::score(e, &self.query).map(|s| (s, e)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, e)| e)
            .collect()
    }
}

#[to_layer_message]
#[derive(Debug, Clone)]
enum Msg {
    Query(String),
    Up,
    Down,
    Enter,
    Cancel,
    Pick(usize),
}

fn boot() -> (Launcher, Task<Msg>) {
    (
        Launcher::new(),
        widget::operation::focus(widget::Id::new(SEARCH_INPUT_ID)),
    )
}

fn namespace() -> String {
    "zos-launcher".into()
}

fn theme_fn(_: &Launcher) -> Theme {
    zos_ui::theme::zos_theme()
}

fn update(state: &mut Launcher, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Query(q) => {
            state.query = q;
            state.selected = 0;
        }
        Msg::Up => {
            if state.selected > 0 {
                state.selected -= 1;
            }
        }
        Msg::Down => {
            let len = state.matched().len();
            if state.selected + 1 < len {
                state.selected += 1;
            }
        }
        Msg::Enter => {
            let matched = state.matched();
            if let Some(entry) = matched.get(state.selected) {
                launch(entry);
                std::process::exit(0);
            }
        }
        Msg::Pick(idx) => {
            let matched = state.matched();
            if let Some(entry) = matched.get(idx) {
                launch(entry);
                std::process::exit(0);
            }
        }
        Msg::Cancel => std::process::exit(0),
        _ => {}
    }
    Task::none()
}

fn view(state: &Launcher) -> Element<'_, Msg> {
    use zos_ui::theme;

    let search = text_input("Type to search...", &state.query)
        .id(widget::Id::new(SEARCH_INPUT_ID))
        .on_input(Msg::Query)
        .on_submit(Msg::Enter)
        .padding(Padding::from([8.0_f32, 12.0]))
        .size(theme::font_size::LG);

    let matched = state.matched();
    let mut results = column![].spacing(2.0_f32);
    for (idx, entry) in matched.iter().enumerate() {
        let selected = idx == state.selected;
        let row = column![
            text(entry.name.clone())
                .size(theme::font_size::BASE)
                .color(theme::TEXT),
            text(entry.comment.clone())
                .size(theme::font_size::SM)
                .color(theme::SUBTEXT0),
        ];
        results = results.push(
            button(row)
                .on_press(Msg::Pick(idx))
                .style(move |_, status| result_btn_style(selected, status))
                .padding(Padding::from([6.0_f32, 12.0_f32]))
                .width(Length::Fill),
        );
    }

    let body = column![
        container(search).padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 8.0,
            left: 0.0,
        }),
        scrollable(results).height(Length::Fill),
    ];

    container(body)
        .padding(theme::space::X4)
        .style(window_bg_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn subscription(_: &Launcher) -> Subscription<Msg> {
    iced::event::listen_with(|event, _, _| match event {
        Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => match key {
            Key::Named(Named::Escape) => Some(Msg::Cancel),
            Key::Named(Named::ArrowUp) => Some(Msg::Up),
            Key::Named(Named::ArrowDown) => Some(Msg::Down),
            // Enter is handled by text_input::on_submit
            _ => None,
        },
        _ => None,
    })
}

fn result_btn_style(selected: bool, status: button::Status) -> button::Style {
    use zos_ui::theme;
    let bg = if selected {
        theme::SURFACE1
    } else {
        match status {
            button::Status::Hovered => theme::SURFACE0,
            _ => theme::BASE,
        }
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: theme::TEXT,
        border: Border {
            color: bg,
            width: 0.0,
            radius: Radius::from(zos_ui::theme::radius::SM),
        },
        ..Default::default()
    }
}

fn window_bg_style(_: &Theme) -> container::Style {
    use zos_ui::theme;
    container::Style {
        background: Some(Background::Color(theme::BASE)),
        border: Border {
            color: theme::SURFACE1,
            width: 1.0,
            radius: Radius::from(theme::radius::LG),
        },
        ..Default::default()
    }
}

fn launch(entry: &DesktopEntry) {
    let args = desktop_files::strip_exec_codes(&entry.exec);
    if args.is_empty() {
        tracing::warn!(name = %entry.name, "empty Exec line");
        return;
    }
    let result = if entry.terminal {
        // Terminal=true: wrap in a terminal emulator
        StdCommand::new("wezterm")
            .arg("start")
            .arg("--")
            .args(&args)
            .spawn()
    } else {
        StdCommand::new(&args[0]).args(&args[1..]).spawn()
    };
    if let Err(e) = result {
        tracing::error!(?e, name = %entry.name, "failed to spawn");
    }
}

fn main() -> Result<(), iced_layershell::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let layer_settings = zos_ui::layer::layer_shell::centered_popup(POPUP_W, POPUP_H);

    iced_layershell::application(boot, namespace, update, view)
        .theme(theme_fn)
        .subscription(subscription)
        .settings(iced_layershell::settings::Settings {
            layer_settings,
            antialiasing: true,
            ..Default::default()
        })
        .run()
}
