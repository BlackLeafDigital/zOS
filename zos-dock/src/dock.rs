// === dock.rs — Main dock application: state, update, view ===

use crate::config::DockConfig;
use crate::hypr::{self, HyprWindow};
use crate::icons::IconResolver;
use iced::widget::{center, column, container, mouse_area, row, svg, text, Space};
use iced::{event, Background, Border, Color, Element, Event, Length, Subscription, Task, Theme};
use iced_anim::Spring;
use iced_layershell::to_layer_message;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// --- Catppuccin Mocha colors ---
const BG_COLOR: Color = Color {
    r: 0x18 as f32 / 255.0,
    g: 0x18 as f32 / 255.0,
    b: 0x25 as f32 / 255.0,
    a: 0.85,
};
const SURFACE_COLOR: Color = Color {
    r: 0x31 as f32 / 255.0,
    g: 0x31 as f32 / 255.0,
    b: 0x44 as f32 / 255.0,
    a: 1.0,
};
const ACCENT_BLUE: Color = Color {
    r: 0x89 as f32 / 255.0,
    g: 0xB4 as f32 / 255.0,
    b: 0xFA as f32 / 255.0,
    a: 1.0,
};
const TEXT_COLOR: Color = Color {
    r: 0xCD as f32 / 255.0,
    g: 0xD6 as f32 / 255.0,
    b: 0xF4 as f32 / 255.0,
    a: 1.0,
};
const SEPARATOR_COLOR: Color = Color {
    r: 0x58 as f32 / 255.0,
    g: 0x5B as f32 / 255.0,
    b: 0x70 as f32 / 255.0,
    a: 0.6,
};

/// A single item displayed in the dock.
#[derive(Debug, Clone)]
pub struct DockItem {
    /// Desktop app ID or Hyprland window class.
    pub app_id: String,
    /// Human-readable name.
    pub name: String,
    /// Resolved icon path (SVG or PNG).
    pub icon_path: Option<PathBuf>,
    /// Whether this item is pinned in the config.
    pub pinned: bool,
    /// Hyprland windows belonging to this app.
    pub windows: Vec<HyprWindow>,
    /// Current visual scale (animated via spring).
    pub scale: Spring<f32>,
}

impl DockItem {
    fn is_running(&self) -> bool {
        !self.windows.is_empty()
    }

    fn is_focused(&self, active_address: &Option<String>) -> bool {
        match active_address {
            Some(addr) => self.windows.iter().any(|w| w.address == *addr),
            None => false,
        }
    }
}

/// Main dock application state.
pub struct Dock {
    pub items: Vec<DockItem>,
    pub config: DockConfig,
    pub icon_resolver: IconResolver,
    /// Mouse cursor X position relative to the dock, if hovering.
    pub cursor_x: Option<f32>,
    /// Positions of each item center (computed during view, used for magnification).
    pub item_positions: Vec<f32>,
    /// Address of the currently focused window (from hyprctl activewindow -j).
    pub active_address: Option<String>,
}

/// Messages handled by the dock.
#[to_layer_message]
#[derive(Debug, Clone)]
pub enum Message {
    /// Animation/physics tick.
    Tick(Instant),
    /// Global iced event (mouse move, etc.).
    IcedEvent(Event),
    /// Mouse left the dock area.
    MouseLeft,
    /// User clicked a dock item.
    ItemClicked(usize),
    /// User right-clicked a dock item.
    ItemRightClicked(usize),
    /// Refresh the Hyprland window list.
    RefreshWindows,
    /// Toggle pin state for an app.
    TogglePin(String),
}

pub fn boot() -> Dock {
    let config = DockConfig::load();
    let icon_resolver = IconResolver::new();
    let windows = hypr::get_windows();

    let mut dock = Dock {
        items: Vec::new(),
        config,
        icon_resolver,
        cursor_x: None,
        item_positions: Vec::new(),
        active_address: hypr::get_active_window_address(),
    };
    dock.rebuild_items(&windows);
    dock
}

pub fn namespace() -> String {
    "zos-dock".to_string()
}

pub fn update(dock: &mut Dock, message: Message) -> Task<Message> {
    match message {
        Message::Tick(now) => {
            let mut any_energy = false;
            for item in &mut dock.items {
                if item.scale.has_energy() {
                    item.scale.update(iced_anim::Event::Tick(now));
                    any_energy = true;
                }
            }
            // If nothing is animating and nothing needs refresh, we can be idle.
            let _ = any_energy;
            Task::none()
        }
        Message::IcedEvent(event) => {
            match event {
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    dock.cursor_x = Some(position.x);
                    dock.update_magnification();
                }
                Event::Mouse(iced::mouse::Event::CursorLeft) => {
                    dock.cursor_x = None;
                    dock.reset_magnification();
                }
                _ => {}
            }
            Task::none()
        }
        Message::MouseLeft => {
            dock.cursor_x = None;
            dock.reset_magnification();
            Task::none()
        }
        Message::ItemClicked(index) => {
            tracing::info!("Dock: ItemClicked index={}", index);
            if let Some(item) = dock.items.get(index) {
                if item.is_running() {
                    // Cycle through windows: focus the most recently unfocused one,
                    // or if already focused and there are multiple, focus the next.
                    if let Some(window) = item
                        .windows
                        .iter()
                        .find(|w| w.focus_history_id != 0)
                        .or(item.windows.first())
                    {
                        hypr::focus_window(&window.address);
                    }
                } else {
                    // Launch the application.
                    hypr::launch_app(&item.app_id);
                }
            }
            Task::none()
        }
        Message::ItemRightClicked(index) => {
            if let Some(item) = dock.items.get(index) {
                let app_id = item.app_id.clone();
                if item.is_running() {
                    // Close the first window.
                    if let Some(window) = item.windows.first() {
                        tracing::info!("Right-click: closing window '{}' of {}", window.title, app_id);
                        hypr::close_window(&window.address);
                    }
                } else if item.pinned {
                    // Unpin the item.
                    tracing::info!("Right-click: unpinning {}", app_id);
                    dock.config.toggle_pin(&app_id);
                    dock.config.save();
                    let windows = hypr::get_windows();
                    dock.rebuild_items(&windows);
                }
            }
            Task::none()
        }
        Message::RefreshWindows => {
            let windows = hypr::get_windows();
            dock.active_address = hypr::get_active_window_address();
            dock.rebuild_items(&windows);
            Task::none()
        }
        Message::TogglePin(app_id) => {
            dock.config.toggle_pin(&app_id);
            dock.config.save();
            let windows = hypr::get_windows();
            dock.rebuild_items(&windows);
            Task::none()
        }
        // Layer shell action messages are handled by the framework.
        _ => Task::none(),
    }
}

pub fn view(dock: &Dock) -> Element<'_, Message> {
    let icon_size = dock.config.icon_size;

    let mut items_row = row![].spacing(4).align_y(iced::Alignment::End);

    let mut has_pinned = false;
    let mut started_running = false;

    for (i, item) in dock.items.iter().enumerate() {
        // Insert separator between pinned and unpinned-running items.
        if !item.pinned && !started_running && has_pinned {
            started_running = true;
            items_row = items_row.push(separator());
        }
        if item.pinned {
            has_pinned = true;
        }

        let scale = *item.scale.value();
        let scaled_size = (icon_size as f32 * scale) as u16;

        // Build the icon widget.
        let icon_widget: Element<'_, Message> = if let Some(path) = &item.icon_path {
            if path.extension().and_then(|e| e.to_str()) == Some("svg") {
                svg(svg::Handle::from_path(path))
                    .width(Length::Fixed(scaled_size as f32))
                    .height(Length::Fixed(scaled_size as f32))
                    .into()
            } else {
                iced::widget::image(path.to_string_lossy().to_string())
                    .width(Length::Fixed(scaled_size as f32))
                    .height(Length::Fixed(scaled_size as f32))
                    .into()
            }
        } else {
            // Fallback: show the first letter of the app name.
            let label = item
                .name
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            container(
                center(text(label).size(scaled_size as f32 * 0.5).color(TEXT_COLOR))
                    .width(Length::Fixed(scaled_size as f32))
                    .height(Length::Fixed(scaled_size as f32)),
            )
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(SURFACE_COLOR)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .into()
        };

        // Active indicator dot below the icon.
        let dot: Element<'_, Message> = if item.is_running() {
            let dot_color = if item.is_focused(&dock.active_address) {
                ACCENT_BLUE
            } else {
                SURFACE_COLOR
            };
            container(Space::new().width(6).height(6))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(dot_color)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else {
            Space::new().width(6).height(6).into()
        };

        let item_column = column![icon_widget, dot]
            .spacing(2)
            .align_x(iced::Alignment::Center);

        let item_widget = mouse_area(container(item_column).padding(2).style(
            move |_theme: &Theme| container::Style {
                background: None,
                ..Default::default()
            },
        ))
        .on_press(Message::ItemClicked(i))
        .on_right_press(Message::ItemRightClicked(i));

        items_row = items_row.push(item_widget);
    }

    // Wrap in the dock background container.
    let dock_bg = container(items_row.padding([6, 12]))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(BG_COLOR)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 16.0.into(),
            },
            ..Default::default()
        })
        .center_x(Length::Shrink);

    // Outer container to center the dock horizontally.
    container(dock_bg)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom)
        .into()
}

pub fn subscription(dock: &Dock) -> Subscription<Message> {
    let any_animating = dock.items.iter().any(|item| item.scale.has_energy());

    let mut subs = vec![
        // Poll Hyprland for window changes every 500ms.
        iced::time::every(Duration::from_millis(500)).map(|_| Message::RefreshWindows),
        // Forward iced events for mouse tracking.
        event::listen().map(Message::IcedEvent),
    ];

    // When animating, tick at ~60fps for smooth spring physics.
    if any_animating {
        subs.push(
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick(Instant::now())),
        );
    }

    Subscription::batch(subs)
}

pub fn style(_dock: &Dock, _theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: TEXT_COLOR,
    }
}

/// Create a vertical separator element.
fn separator<'a>() -> Element<'a, Message> {
    container(Space::new().width(2))
        .height(Length::Fixed(32.0))
        .width(Length::Fixed(2.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(SEPARATOR_COLOR)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 1.0.into(),
            },
            ..Default::default()
        })
        .into()
}

impl Dock {
    /// Rebuild the dock items list from config + current windows.
    fn rebuild_items(&mut self, windows: &[HyprWindow]) {
        // Group windows by class (lowercased).
        let mut window_groups: HashMap<String, Vec<HyprWindow>> = HashMap::new();
        for w in windows {
            window_groups
                .entry(w.class.to_lowercase())
                .or_default()
                .push(w.clone());
        }

        // Preserve existing scale springs where possible.
        let old_scales: HashMap<String, Spring<f32>> = self
            .items
            .drain(..)
            .map(|item| (item.app_id.to_lowercase(), item.scale))
            .collect();

        let mut seen = std::collections::HashSet::new();

        // First: add pinned items in config order.
        for app_id in &self.config.pinned {
            let key = app_id.to_lowercase();
            seen.insert(key.clone());

            let entry = self.icon_resolver.lookup(app_id);
            let name = entry
                .map(|e| e.name.clone())
                .unwrap_or_else(|| app_id_to_name(app_id));
            let icon_path = entry.and_then(|e| e.icon_path.clone());

            let wm_class = entry
                .map(|e| e.wm_class.to_lowercase())
                .unwrap_or_else(|| key.clone());

            let item_windows = window_groups.remove(&wm_class).unwrap_or_default();

            let scale = old_scales
                .get(&key)
                .cloned()
                .unwrap_or_else(|| Spring::new(1.0));

            self.items.push(DockItem {
                app_id: app_id.clone(),
                name,
                icon_path,
                pinned: true,
                windows: item_windows,
                scale,
            });
        }

        // Second: add running but unpinned windows.
        let mut running_items: Vec<DockItem> = Vec::new();
        for (class_lower, group_windows) in window_groups {
            if seen.contains(&class_lower) || group_windows.is_empty() {
                continue;
            }
            seen.insert(class_lower.clone());

            let representative_class = &group_windows[0].class;
            let entry = self.icon_resolver.lookup(representative_class);
            let name = entry
                .map(|e| e.name.clone())
                .unwrap_or_else(|| representative_class.clone());
            let icon_path = entry.and_then(|e| e.icon_path.clone());
            let app_id = entry
                .map(|e| e.app_id.clone())
                .unwrap_or_else(|| representative_class.clone());

            let scale = old_scales
                .get(&class_lower)
                .cloned()
                .unwrap_or_else(|| Spring::new(1.0));

            running_items.push(DockItem {
                app_id,
                name,
                icon_path,
                pinned: false,
                windows: group_windows,
                scale,
            });
        }

        // Sort running items by name for stable ordering.
        running_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.items.extend(running_items);

        // Recompute approximate item positions.
        self.recompute_positions();
    }

    /// Recompute the center X positions of each item (for magnification calculations).
    fn recompute_positions(&mut self) {
        let icon_size = self.config.icon_size as f32;
        let spacing = 4.0;
        let padding = 12.0;
        let item_width = icon_size + spacing;

        self.item_positions.clear();
        let mut x = padding + icon_size / 2.0;

        let mut has_pinned = false;
        let mut started_running = false;

        for item in &self.items {
            if !item.pinned && !started_running && has_pinned {
                started_running = true;
                x += 6.0; // separator width + spacing
            }
            if item.pinned {
                has_pinned = true;
            }

            self.item_positions.push(x);
            x += item_width;
        }
    }

    /// Update magnification targets based on cursor position.
    fn update_magnification(&mut self) {
        let cursor_x = match self.cursor_x {
            Some(x) => x,
            None => return,
        };

        let magnification = self.config.magnification;
        let icon_size = self.config.icon_size as f32;
        let spread = icon_size * 2.5;

        for (i, item) in self.items.iter_mut().enumerate() {
            let item_x = self.item_positions.get(i).copied().unwrap_or(0.0);
            let distance = (cursor_x - item_x).abs();
            let gaussian = (-(distance * distance) / (2.0 * spread * spread)).exp();
            let target_scale = 1.0 + (magnification - 1.0) * gaussian;
            item.scale.set_target(target_scale);
        }
    }

    /// Reset all magnification targets back to 1.0.
    fn reset_magnification(&mut self) {
        for item in &mut self.items {
            item.scale.set_target(1.0);
        }
    }
}

/// Convert an app ID like "org.wezfurlong.wezterm" to a display name "Wezterm".
fn app_id_to_name(app_id: &str) -> String {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut chars = last.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        None => app_id.to_string(),
    }
}
