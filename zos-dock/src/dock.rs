// === dock.rs — Main dock application: state, update, view ===

use crate::config::DockConfig;
use crate::hypr::{self, HyprWindow};
use crate::icons::IconResolver;
use iced::widget::{center, column, container, mouse_area, row, svg, text, tooltip, Space};
use iced::{event, Background, Border, Color, Element, Event, Length, Subscription, Task, Theme};
use iced_anim::Spring;
use iced_layershell::actions::{IcedNewMenuSettings, MenuDirection};
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
    /// Whether this item represents a minimized window.
    pub minimized: bool,
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

/// State for an open right-click context menu popup.
#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub id: iced::window::Id,
    pub app_id: String,
    pub is_running: bool,
    pub is_pinned: bool,
    pub is_minimized: bool,
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
    /// Whether the dock is currently hidden (auto-hide mode).
    pub hidden: bool,
    /// When the cursor last left the dock (for auto-hide delay).
    pub hide_timer: Option<Instant>,
    /// Animated slide offset for auto-hide (0.0 = visible, 68.0 = hidden).
    pub slide_offset: Spring<f32>,
    /// Last observed modification time of the config file.
    pub config_mtime: Option<std::time::SystemTime>,
    /// Currently open context menu, if any.
    pub context_menu: Option<ContextMenuState>,
}

/// Messages handled by the dock.
#[to_layer_message(multi)]
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
    /// Check if config file changed on disk.
    CheckConfig,
    /// Close a window from context menu.
    ContextMenuClose(String),
    /// Pin/unpin from context menu.
    ContextMenuPin(String),
    /// Unminimize from context menu.
    ContextMenuRestore(String),
    /// Launch app from context menu.
    ContextMenuLaunch(String),
    /// Dismiss the context menu.
    DismissMenu,
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
        hidden: false,
        hide_timer: None,
        slide_offset: Spring::new(0.0),
        config_mtime: DockConfig::config_mtime(),
        context_menu: None,
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
            if dock.slide_offset.has_energy() {
                dock.slide_offset.update(iced_anim::Event::Tick(now));
                any_energy = true;
            }
            // If nothing is animating and nothing needs refresh, we can be idle.
            let _ = any_energy;
            Task::none()
        }
        Message::IcedEvent(event) => {
            match event {
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    if dock.hidden && dock.config.auto_hide {
                        dock.hidden = false;
                        dock.slide_offset.set_target(0.0);
                        dock.hide_timer = None;
                    }
                    dock.cursor_x = Some(position.x);
                    dock.update_magnification();
                }
                Event::Mouse(iced::mouse::Event::CursorLeft) => {
                    dock.cursor_x = None;
                    dock.reset_magnification();
                    if dock.config.auto_hide {
                        dock.hide_timer = Some(Instant::now());
                    }
                }
                _ => {}
            }
            Task::none()
        }
        Message::MouseLeft => {
            dock.cursor_x = None;
            dock.reset_magnification();
            if dock.config.auto_hide {
                dock.hide_timer = Some(Instant::now());
            }
            Task::none()
        }
        Message::ItemClicked(index) => {
            // Dismiss context menu if open
            if dock.context_menu.is_some() {
                let cm = dock.context_menu.take().unwrap();
                return Task::done(Message::RemoveWindow(cm.id));
            }
            tracing::info!("Dock: ItemClicked index={}", index);
            if let Some(item) = dock.items.get(index) {
                if item.minimized {
                    // Unminimize the window: move it back to the current workspace and focus.
                    if let Some(window) = item.windows.first() {
                        hypr::unminimize_window(&window.address);
                    }
                } else if item.is_running() {
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
                // Close any existing context menu first
                let mut tasks: Vec<Task<Message>> = Vec::new();
                if let Some(old_cm) = dock.context_menu.take() {
                    tasks.push(Task::done(Message::RemoveWindow(old_cm.id)));
                }

                let menu_height: u32 = 2 * 36 + 16;
                let (menu_id, open_task) = Message::menu_open(IcedNewMenuSettings {
                    size: (180, menu_height),
                    direction: MenuDirection::Up,
                });

                dock.context_menu = Some(ContextMenuState {
                    id: menu_id,
                    app_id: item.app_id.clone(),
                    is_running: item.is_running(),
                    is_pinned: item.pinned,
                    is_minimized: item.minimized,
                });

                tasks.push(open_task);
                Task::batch(tasks)
            } else {
                Task::none()
            }
        }
        Message::RefreshWindows => {
            let windows = hypr::get_windows();
            dock.active_address = hypr::get_active_window_address();
            dock.rebuild_items(&windows);
            // Auto-hide check
            if dock.config.auto_hide {
                if let Some(timer_start) = dock.hide_timer {
                    if timer_start.elapsed() >= Duration::from_millis(800)
                        && dock.cursor_x.is_none()
                    {
                        dock.hidden = true;
                        dock.slide_offset.set_target(68.0);
                        dock.hide_timer = None;
                    }
                }
            }
            Task::none()
        }
        Message::TogglePin(app_id) => {
            dock.config.toggle_pin(&app_id);
            dock.config.save();
            let windows = hypr::get_windows();
            dock.rebuild_items(&windows);
            Task::none()
        }
        Message::CheckConfig => {
            let new_mtime = DockConfig::config_mtime();
            if new_mtime != dock.config_mtime {
                dock.config_mtime = new_mtime;
                dock.config = DockConfig::load();
                let windows = hypr::get_windows();
                dock.rebuild_items(&windows);
            }
            Task::none()
        }
        Message::ContextMenuClose(app_id) => {
            if let Some(item) = dock.items.iter().find(|i| i.app_id == app_id) {
                if let Some(w) = item.windows.first() {
                    hypr::close_window(&w.address);
                }
            }
            if let Some(cm) = dock.context_menu.take() {
                Task::done(Message::RemoveWindow(cm.id))
            } else {
                Task::none()
            }
        }
        Message::ContextMenuPin(app_id) => {
            dock.config.toggle_pin(&app_id);
            dock.config.save();
            let windows = hypr::get_windows();
            dock.rebuild_items(&windows);
            if let Some(cm) = dock.context_menu.take() {
                Task::done(Message::RemoveWindow(cm.id))
            } else {
                Task::none()
            }
        }
        Message::ContextMenuRestore(app_id) => {
            let minimized = hypr::get_minimized_windows();
            if let Some(w) = minimized
                .iter()
                .find(|w| w.class.to_lowercase() == app_id.to_lowercase())
            {
                hypr::unminimize_window(&w.address);
            }
            if let Some(cm) = dock.context_menu.take() {
                Task::done(Message::RemoveWindow(cm.id))
            } else {
                Task::none()
            }
        }
        Message::ContextMenuLaunch(app_id) => {
            hypr::launch_app(&app_id);
            if let Some(cm) = dock.context_menu.take() {
                Task::done(Message::RemoveWindow(cm.id))
            } else {
                Task::none()
            }
        }
        Message::DismissMenu => {
            if let Some(cm) = dock.context_menu.take() {
                Task::done(Message::RemoveWindow(cm.id))
            } else {
                Task::none()
            }
        }
        // Layer shell action messages are handled by the framework.
        _ => Task::none(),
    }
}

pub fn view(dock: &Dock, window_id: iced::window::Id) -> Element<'_, Message> {
    let icon_size = dock.config.icon_size;

    // If this is a context menu popup, render the menu
    if let Some(ref cm) = dock.context_menu {
        if cm.id == window_id {
            return render_context_menu(cm);
        }
    }

    let slide = *dock.slide_offset.value();
    if slide > 66.0 {
        // Fully hidden — render transparent
        return container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let mut items_row = row![].spacing(4).align_y(iced::Alignment::End);

    let mut has_pinned = false;
    let mut started_running = false;
    let mut started_minimized = false;

    for (i, item) in dock.items.iter().enumerate() {
        // Insert separator between pinned and unpinned-running items.
        if !item.pinned && !item.minimized && !started_running && has_pinned {
            started_running = true;
            items_row = items_row.push(separator());
        }
        // Insert separator between running items and minimized items.
        if item.minimized && !started_minimized {
            started_minimized = true;
            if has_pinned || started_running {
                items_row = items_row.push(separator());
            }
        }
        if item.pinned {
            has_pinned = true;
        }

        let scale = *item.scale.value();
        let scaled_size = (icon_size as f32 * scale) as u16;
        let icon_opacity: f32 = if item.minimized { 0.5 } else { 1.0 };

        // Build the icon widget.
        let icon_widget: Element<'_, Message> = if let Some(path) = &item.icon_path {
            if path.extension().and_then(|e| e.to_str()) == Some("svg") {
                svg(svg::Handle::from_path(path))
                    .width(Length::Fixed(scaled_size as f32))
                    .height(Length::Fixed(scaled_size as f32))
                    .opacity(icon_opacity)
                    .into()
            } else {
                iced::widget::image(path.to_string_lossy().to_string())
                    .width(Length::Fixed(scaled_size as f32))
                    .height(Length::Fixed(scaled_size as f32))
                    .opacity(icon_opacity)
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
            let fallback_text_color = Color {
                a: icon_opacity,
                ..TEXT_COLOR
            };
            let fallback_bg_color = Color {
                a: icon_opacity,
                ..SURFACE_COLOR
            };
            container(
                center(
                    text(label)
                        .size(scaled_size as f32 * 0.5)
                        .color(fallback_text_color),
                )
                .width(Length::Fixed(scaled_size as f32))
                .height(Length::Fixed(scaled_size as f32)),
            )
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(fallback_bg_color)),
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
        let dot: Element<'_, Message> = if item.minimized {
            // Minimized items get a dimmed/muted dot.
            container(Space::new().width(6).height(6))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(SEPARATOR_COLOR)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else if item.is_running() {
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

        let tooltip_content = container(text(&item.name).size(12).color(TEXT_COLOR))
            .padding([4, 8])
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(BG_COLOR)),
                border: Border {
                    color: SURFACE_COLOR,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        let item_with_tooltip =
            tooltip(item_widget, tooltip_content, tooltip::Position::Top).gap(4);

        items_row = items_row.push(item_with_tooltip);
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
    let dock_view: Element<'_, Message> = container(dock_bg)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            top: slide,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        })
        .into();

    // If a context menu is open, clicking the dock dismisses it
    if dock.context_menu.is_some() {
        mouse_area(dock_view).on_press(Message::DismissMenu).into()
    } else {
        dock_view
    }
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
    if any_animating || dock.cursor_x.is_some() || dock.slide_offset.has_energy() {
        subs.push(
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick(Instant::now())),
        );
    }

    // Poll for config file changes every second.
    subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::CheckConfig));

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

/// Render the right-click context menu popup.
fn render_context_menu(cm: &ContextMenuState) -> Element<'_, Message> {
    let app_id = cm.app_id.clone();
    let mut items = column![].spacing(2).padding(8);

    if cm.is_minimized {
        let id = app_id.clone();
        items = items.push(menu_button("Restore", Message::ContextMenuRestore(id)));
        let id = app_id.clone();
        items = items.push(menu_button("Close", Message::ContextMenuClose(id)));
    } else if cm.is_running {
        let id = app_id.clone();
        items = items.push(menu_button("Close", Message::ContextMenuClose(id)));
        let id = app_id.clone();
        let label = if cm.is_pinned { "Unpin" } else { "Pin" };
        items = items.push(menu_button(label, Message::ContextMenuPin(id)));
    } else {
        let id = app_id.clone();
        items = items.push(menu_button("Launch", Message::ContextMenuLaunch(id)));
        let id = app_id.clone();
        items = items.push(menu_button("Unpin", Message::ContextMenuPin(id)));
    }

    container(items)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(BG_COLOR)),
            border: Border {
                color: SURFACE_COLOR,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Create a styled menu button.
fn menu_button(label: &str, msg: Message) -> Element<'_, Message> {
    mouse_area(
        container(text(label).size(14).color(TEXT_COLOR))
            .padding([6, 12])
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: None,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }),
    )
    .on_press(msg)
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
                minimized: false,
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
                minimized: false,
                scale,
            });
        }

        // Sort running items by name for stable ordering.
        running_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.items.extend(running_items);

        // Third: add minimized windows.
        let minimized_windows = hypr::get_minimized_windows();
        let mut minimized_items: Vec<DockItem> = Vec::new();
        for w in &minimized_windows {
            let entry = self.icon_resolver.lookup(&w.class);
            let name = entry
                .map(|e| e.name.clone())
                .unwrap_or_else(|| w.class.clone());
            let icon_path = entry.and_then(|e| e.icon_path.clone());
            let app_id = entry
                .map(|e| e.app_id.clone())
                .unwrap_or_else(|| w.class.clone());
            let key = w.class.to_lowercase();

            let scale = old_scales
                .get(&key)
                .cloned()
                .unwrap_or_else(|| Spring::new(1.0));

            minimized_items.push(DockItem {
                app_id,
                name,
                icon_path,
                pinned: false,
                windows: vec![w.clone()],
                minimized: true,
                scale,
            });
        }
        minimized_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.items.extend(minimized_items);

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
        let mut started_minimized = false;

        for item in &self.items {
            if !item.pinned && !item.minimized && !started_running && has_pinned {
                started_running = true;
                x += 6.0; // separator width + spacing
            }
            if item.minimized && !started_minimized {
                started_minimized = true;
                if has_pinned || started_running {
                    x += 6.0; // separator width + spacing
                }
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
