// === dock.rs — Main dock application: state, update, view ===

use crate::config::{DockConfig, DockPosition};
use crate::hypr::{self, HyprWindow};
use crate::hypr_events::{self, HyprEvent};
use crate::icons::IconResolver;
use iced::widget::{center, column, container, mouse_area, row, svg, text, tooltip, Space};
use iced::{
    event, gradient, Background, Border, Color, ContentFit, Element, Event, Gradient, Length,
    Radians, Subscription, Task, Theme,
};
use iced_anim::Spring;
use iced_layershell::actions::ActionCallback;
use iced_layershell::actions::{IcedNewMenuSettings, MenuDirection};
use iced_layershell::reexport::Anchor;
use iced_layershell::to_layer_message;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Thickness of the layer-shell surface in pixels (short axis). Must match main.rs.
const SURFACE_THICKNESS: f32 = 100.0;

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
#[allow(dead_code)]
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
const INDICATOR_COLOR: Color = Color {
    r: 0x6C as f32 / 255.0,
    g: 0x70 as f32 / 255.0,
    b: 0x86 as f32 / 255.0,
    a: 1.0,
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
    /// Window addresses for workspace/monitor operations.
    pub window_addresses: Vec<String>,
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
    /// When the cursor entered the hidden dock trigger zone (for reveal delay).
    pub show_timer: Option<Instant>,
    /// Animated slide offset for auto-hide (0.0 = visible, SURFACE_THICKNESS = hidden).
    pub slide_offset: Spring<f32>,
    /// Last observed modification time of the config file.
    pub config_mtime: Option<std::time::SystemTime>,
    /// Currently open context menu, if any.
    pub context_menu: Option<ContextMenuState>,
    /// Surface length (long-axis monitor dimension) for coordinate calculations.
    pub surface_length: u32,
    /// Main window IDs (one per monitor), captured from view() for input region updates.
    pub known_window_ids: RefCell<Vec<iced::window::Id>>,
    /// Previous item count, used to detect when input region needs updating.
    pub prev_item_count: usize,
    /// Timer for auto-re-hiding after a minimize-triggered reveal.
    pub minimize_reveal_timer: Option<Instant>,
    /// Phase for animated focused-app indicator gradient (0.0–360.0 degrees).
    pub indicator_phase: f32,
    /// Which screen edge the dock is on.
    pub position: DockPosition,
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
    /// Move window to a workspace from context menu.
    ContextMenuMoveWorkspace(String, i32),
    /// Move window to a monitor from context menu.
    ContextMenuMoveMonitor(String, String),
    /// Real-time event from Hyprland event socket.
    HyprEvent(HyprEvent),
}

pub fn boot() -> Dock {
    let config = DockConfig::load();
    let icon_resolver = IconResolver::new();
    let windows = hypr::get_windows();
    let position = config.position;
    let surface_length = if position.is_horizontal() {
        hypr::get_monitor_widths().into_iter().max().unwrap_or(1920)
    } else {
        hypr::get_monitor_heights()
            .into_iter()
            .max()
            .unwrap_or(1080)
    };

    let mut dock = Dock {
        items: Vec::new(),
        config,
        icon_resolver,
        cursor_x: None,
        item_positions: Vec::new(),
        active_address: hypr::get_active_window_address(),
        hidden: false,
        hide_timer: None,
        show_timer: None,
        slide_offset: Spring::new(0.0),
        config_mtime: DockConfig::config_mtime(),
        context_menu: None,
        surface_length,
        known_window_ids: RefCell::new(Vec::new()),
        prev_item_count: 0,
        minimize_reveal_timer: None,
        indicator_phase: 0.0,
        position,
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
            // Advance rainbow indicator phase (~30°/sec at 30fps).
            dock.indicator_phase = (dock.indicator_phase + 1.0) % 360.0;
            // Check reveal timer — 500ms dwell at bottom edge (matches macOS)
            let mut hidden_changed = false;
            if let Some(show_start) = dock.show_timer {
                if show_start.elapsed() >= Duration::from_millis(500) {
                    dock.hidden = false;
                    dock.slide_offset.set_target(0.0);
                    dock.hide_timer = None;
                    dock.show_timer = None;
                    hidden_changed = true;
                }
            }
            // Check hide timer — 300ms after cursor leaves (skip if context menu open)
            if let Some(timer_start) = dock.hide_timer {
                if timer_start.elapsed() >= Duration::from_millis(300)
                    && dock.cursor_x.is_none()
                    && dock.context_menu.is_none()
                {
                    // Dismiss any open context menu before hiding
                    if let Some(cm) = dock.context_menu.take() {
                        return Task::done(Message::RemoveWindow(cm.id));
                    }
                    dock.hidden = true;
                    dock.slide_offset.set_target(SURFACE_THICKNESS);
                    dock.hide_timer = None;
                    hidden_changed = true;
                }
            }
            // Check minimize-reveal timer — re-hide after 2s if cursor isn't hovering
            if let Some(timer) = dock.minimize_reveal_timer {
                if timer.elapsed() >= Duration::from_secs(2)
                    && dock.cursor_x.is_none()
                    && dock.context_menu.is_none()
                {
                    if let Some(cm) = dock.context_menu.take() {
                        return Task::done(Message::RemoveWindow(cm.id));
                    }
                    dock.hidden = true;
                    dock.slide_offset.set_target(SURFACE_THICKNESS);
                    dock.minimize_reveal_timer = None;
                    hidden_changed = true;
                }
            }
            // If nothing is animating and nothing needs refresh, we can be idle.
            let _ = any_energy;
            if hidden_changed {
                dock.input_region_tasks()
            } else {
                Task::none()
            }
        }
        Message::IcedEvent(event) => {
            match event {
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    if dock.hidden && dock.config.auto_hide {
                        // Start the reveal dwell timer if not already running
                        if dock.show_timer.is_none() {
                            dock.show_timer = Some(Instant::now());
                        }
                    } else {
                        // Re-enter during hide animation: reverse and stay visible
                        if dock.hide_timer.is_some() {
                            dock.hide_timer = None;
                            dock.slide_offset.set_target(0.0);
                        }
                        dock.cursor_x = Some(if dock.position.is_horizontal() {
                            position.x
                        } else {
                            position.y
                        });
                        dock.update_magnification();
                        // Cancel minimize-reveal auto-re-hide if user is interacting
                        dock.minimize_reveal_timer = None;
                    }
                }
                Event::Mouse(iced::mouse::Event::CursorLeft) => {
                    dock.cursor_x = None;
                    dock.show_timer = None;
                    dock.reset_magnification();
                    if dock.config.auto_hide && dock.context_menu.is_none() {
                        dock.hide_timer = Some(Instant::now());
                    }
                }
                _ => {}
            }
            Task::none()
        }
        Message::MouseLeft => {
            dock.cursor_x = None;
            dock.show_timer = None;
            dock.reset_magnification();
            if dock.config.auto_hide && dock.context_menu.is_none() {
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

                // Count menu items for dynamic height
                let workspaces = hypr::get_workspaces();
                let monitors = hypr::get_monitors();
                let mut item_count: u32 = 2; // Pin/Unpin + Close/Launch
                if item.is_running() && !item.minimized {
                    item_count += workspaces.len() as u32; // Move to Workspace entries
                    if monitors.len() > 1 {
                        item_count += monitors.len() as u32; // Move to Monitor entries
                    }
                }
                if item.minimized {
                    item_count = 2; // Restore + Close
                }

                let menu_height: u32 = item_count * 36 + 16;
                let (menu_id, open_task) = Message::menu_open(IcedNewMenuSettings {
                    size: (220, menu_height),
                    direction: match dock.position {
                        DockPosition::Top | DockPosition::Left => MenuDirection::Down,
                        DockPosition::Bottom | DockPosition::Right => MenuDirection::Up,
                    },
                });

                let window_addresses: Vec<String> =
                    item.windows.iter().map(|w| w.address.clone()).collect();

                dock.context_menu = Some(ContextMenuState {
                    id: menu_id,
                    app_id: item.app_id.clone(),
                    is_running: item.is_running(),
                    is_pinned: item.pinned,
                    is_minimized: item.minimized,
                    window_addresses,
                });

                tasks.push(open_task);
                Task::batch(tasks)
            } else {
                Task::none()
            }
        }
        Message::RefreshWindows => {
            let old_count = dock.prev_item_count;
            let was_hidden = dock.hidden;
            let windows = hypr::get_windows();
            dock.active_address = hypr::get_active_window_address();
            dock.rebuild_items(&windows);
            // Update input region if item count changed or hidden state changed
            if dock.prev_item_count != old_count || dock.hidden != was_hidden {
                dock.input_region_tasks()
            } else {
                Task::none()
            }
        }
        Message::TogglePin(app_id) => {
            dock.config.toggle_pin(&app_id);
            dock.config.save();
            let windows = hypr::get_windows();
            dock.rebuild_items(&windows);
            dock.input_region_tasks()
        }
        Message::CheckConfig => {
            let new_mtime = DockConfig::config_mtime();
            if new_mtime != dock.config_mtime {
                dock.config_mtime = new_mtime;
                let new_config = DockConfig::load();
                let position_changed = new_config.position != dock.position;
                dock.config = new_config;
                dock.position = dock.config.position;
                let windows = hypr::get_windows();
                dock.rebuild_items(&windows);

                if position_changed {
                    // Recompute surface length for the new axis
                    dock.surface_length = if dock.position.is_horizontal() {
                        hypr::get_monitor_widths().into_iter().max().unwrap_or(1920)
                    } else {
                        hypr::get_monitor_heights()
                            .into_iter()
                            .max()
                            .unwrap_or(1080)
                    };

                    let new_anchor = match dock.position {
                        DockPosition::Bottom => Anchor::Bottom | Anchor::Left | Anchor::Right,
                        DockPosition::Top => Anchor::Top | Anchor::Left | Anchor::Right,
                        DockPosition::Left => Anchor::Left | Anchor::Top | Anchor::Bottom,
                        DockPosition::Right => Anchor::Right | Anchor::Top | Anchor::Bottom,
                    };
                    let new_size = if dock.position.is_horizontal() {
                        (0u32, 100u32)
                    } else {
                        (100u32, 0u32)
                    };

                    let ids = dock.known_window_ids.borrow().clone();
                    let mut tasks: Vec<Task<Message>> = ids
                        .into_iter()
                        .map(|id| {
                            Task::done(Message::AnchorSizeChange {
                                id,
                                anchor: new_anchor,
                                size: new_size,
                            })
                        })
                        .collect();
                    tasks.push(dock.input_region_tasks());
                    return Task::batch(tasks);
                }

                return dock.input_region_tasks();
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
            let region_task = dock.input_region_tasks();
            if let Some(cm) = dock.context_menu.take() {
                Task::batch([Task::done(Message::RemoveWindow(cm.id)), region_task])
            } else {
                region_task
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
        Message::ContextMenuMoveWorkspace(address, workspace_id) => {
            hypr::move_window_to_workspace(&address, workspace_id);
            let windows = hypr::get_windows();
            dock.rebuild_items(&windows);
            if let Some(cm) = dock.context_menu.take() {
                Task::batch([
                    Task::done(Message::RemoveWindow(cm.id)),
                    dock.input_region_tasks(),
                ])
            } else {
                dock.input_region_tasks()
            }
        }
        Message::ContextMenuMoveMonitor(address, monitor_name) => {
            hypr::move_window_to_monitor(&address, &monitor_name);
            let windows = hypr::get_windows();
            dock.rebuild_items(&windows);
            if let Some(cm) = dock.context_menu.take() {
                Task::batch([
                    Task::done(Message::RemoveWindow(cm.id)),
                    dock.input_region_tasks(),
                ])
            } else {
                dock.input_region_tasks()
            }
        }
        Message::DismissMenu => {
            if let Some(cm) = dock.context_menu.take() {
                Task::done(Message::RemoveWindow(cm.id))
            } else {
                Task::none()
            }
        }
        Message::HyprEvent(event) => {
            match event {
                HyprEvent::ActiveWindowChanged { address } => {
                    dock.active_address = Some(address);
                    Task::none()
                }
                HyprEvent::WindowOpened | HyprEvent::WindowClosed => {
                    let windows = hypr::get_windows();
                    dock.active_address = hypr::get_active_window_address();
                    dock.rebuild_items(&windows);
                    dock.input_region_tasks()
                }
                HyprEvent::WindowMoved { workspace } => {
                    let windows = hypr::get_windows();
                    dock.active_address = hypr::get_active_window_address();
                    dock.rebuild_items(&windows);
                    let mut tasks = vec![dock.input_region_tasks()];

                    // If a window was just minimized and dock is auto-hidden, reveal it briefly
                    if workspace.starts_with("special:minimize")
                        && dock.config.auto_hide
                        && dock.hidden
                    {
                        dock.hidden = false;
                        dock.slide_offset.set_target(0.0);
                        dock.minimize_reveal_timer = Some(Instant::now());
                        dock.hide_timer = None;
                        dock.show_timer = None;
                        tasks.push(dock.input_region_tasks());
                    }

                    Task::batch(tasks)
                }
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

    // Capture main window IDs (any window that isn't a context menu) for input region updates.
    {
        let mut ids = dock.known_window_ids.borrow_mut();
        if !ids.contains(&window_id) {
            ids.push(window_id);
        }
    }

    let slide = *dock.slide_offset.value();
    if slide > SURFACE_THICKNESS - 2.0 {
        // Fully hidden — render transparent
        return container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let is_horizontal = dock.position.is_horizontal();

    let mut item_elements: Vec<Element<'_, Message>> = Vec::new();
    let mut has_pinned = false;
    let mut started_running = false;
    let mut started_minimized = false;

    for (i, item) in dock.items.iter().enumerate() {
        // Insert separator between pinned and unpinned-running items.
        if !item.pinned && !item.minimized && !started_running && has_pinned {
            started_running = true;
            item_elements.push(separator(is_horizontal));
        }
        // Insert separator between running items and minimized items.
        if item.minimized && !started_minimized {
            started_minimized = true;
            if has_pinned || started_running {
                item_elements.push(separator(is_horizontal));
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
                    .content_fit(ContentFit::Cover)
                    .opacity(icon_opacity)
                    .into()
            } else {
                iced::widget::image(path.to_string_lossy().to_string())
                    .width(Length::Fixed(scaled_size as f32))
                    .height(Length::Fixed(scaled_size as f32))
                    .content_fit(ContentFit::Cover)
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

        // Active indicator: animated pill for focused, dot for running.
        // For horizontal docks, indicator dimensions are width x height;
        // for vertical docks, swap so the indicator stays along the short axis.
        let (dot_w, dot_h, focus_w, focus_h): (f32, f32, f32, f32) = if is_horizontal {
            (6.0, 4.0, 16.0, 4.0)
        } else {
            (4.0, 6.0, 4.0, 16.0)
        };

        let dot: Element<'_, Message> = if item.minimized {
            container(Space::new().width(dot_w).height(dot_h))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(SEPARATOR_COLOR)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else if item.is_focused(&dock.active_address) {
            let phase = dock.indicator_phase;
            container(Space::new().width(focus_w).height(focus_h))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Gradient(Gradient::Linear(
                        gradient::Linear::new(Radians(0.0))
                            .add_stop(0.0, hsl_to_color(phase, 0.8, 0.7))
                            .add_stop(0.5, hsl_to_color(phase + 120.0, 0.8, 0.7))
                            .add_stop(1.0, hsl_to_color(phase + 240.0, 0.8, 0.7)),
                    ))),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else if item.is_running() {
            container(Space::new().width(dot_w).height(dot_h))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(Background::Color(INDICATOR_COLOR)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        } else {
            Space::new().width(dot_w).height(dot_h).into()
        };

        // Arrange icon + indicator based on dock position:
        //   Bottom: column![icon, dot]   Top: column![dot, icon]
        //   Left:   row![dot, icon]      Right: row![icon, dot]
        let item_layout: Element<'_, Message> = match dock.position {
            DockPosition::Bottom => column![icon_widget, dot]
                .spacing(2)
                .align_x(iced::Alignment::Center)
                .into(),
            DockPosition::Top => column![dot, icon_widget]
                .spacing(2)
                .align_x(iced::Alignment::Center)
                .into(),
            DockPosition::Left => row![dot, icon_widget]
                .spacing(2)
                .align_y(iced::Alignment::Center)
                .into(),
            DockPosition::Right => row![icon_widget, dot]
                .spacing(2)
                .align_y(iced::Alignment::Center)
                .into(),
        };

        let item_container = if is_horizontal {
            container(item_layout)
                .width(Length::Fixed(scaled_size as f32 + 4.0))
                .padding(2)
        } else {
            container(item_layout)
                .height(Length::Fixed(scaled_size as f32 + 4.0))
                .padding(2)
        };

        let item_widget =
            mouse_area(
                item_container.style(move |_theme: &Theme| container::Style {
                    background: None,
                    ..Default::default()
                }),
            )
            .on_press(Message::ItemClicked(i))
            .on_right_press(Message::ItemRightClicked(i))
            .interaction(iced::mouse::Interaction::Pointer);

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

        let tip_pos = match dock.position {
            DockPosition::Bottom => tooltip::Position::Top,
            DockPosition::Top => tooltip::Position::Bottom,
            DockPosition::Left => tooltip::Position::Right,
            DockPosition::Right => tooltip::Position::Left,
        };
        let item_with_tooltip = tooltip(item_widget, tooltip_content, tip_pos).gap(4);

        item_elements.push(item_with_tooltip.into());
    }

    // Assemble items into a row (horizontal) or column (vertical).
    let items_container: Element<'_, Message> = if is_horizontal {
        let row_align = match dock.position {
            DockPosition::Top => iced::Alignment::Start,
            _ => iced::Alignment::End,
        };
        let mut r = row![].spacing(4).align_y(row_align);
        for item in item_elements {
            r = r.push(item);
        }
        r.into()
    } else {
        let col_align = match dock.position {
            DockPosition::Left => iced::Alignment::Start,
            _ => iced::Alignment::End,
        };
        let mut c = column![].spacing(4).align_x(col_align);
        for item in item_elements {
            c = c.push(item);
        }
        c.into()
    };

    // Wrap in the dock background container.
    // Horizontal: thin top/bottom padding, wide left/right.
    // Vertical: wide top/bottom, thin left/right.
    let pill_padding: [u16; 2] = if is_horizontal { [6, 12] } else { [12, 6] };
    let dock_bg =
        container(container(items_container).padding(pill_padding)).style(|_theme: &Theme| {
            container::Style {
                background: Some(Background::Color(BG_COLOR)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 16.0.into(),
                },
                ..Default::default()
            }
        });

    // Outer container: position the dock pill against the correct edge with slide offset.
    let slide_padding = match dock.position {
        DockPosition::Bottom => iced::Padding {
            top: slide,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
        DockPosition::Top => iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: slide,
            left: 0.0,
        },
        DockPosition::Left => iced::Padding {
            top: 0.0,
            right: slide,
            bottom: 0.0,
            left: 0.0,
        },
        DockPosition::Right => iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: slide,
        },
    };

    let dock_view: Element<'_, Message> = if is_horizontal {
        let v_align = match dock.position {
            DockPosition::Top => iced::alignment::Vertical::Top,
            _ => iced::alignment::Vertical::Bottom,
        };
        container(dock_bg)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .align_y(v_align)
            .padding(slide_padding)
            .into()
    } else {
        let h_align = match dock.position {
            DockPosition::Left => iced::alignment::Horizontal::Left,
            _ => iced::alignment::Horizontal::Right,
        };
        container(dock_bg)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .align_x(h_align)
            .padding(slide_padding)
            .into()
    };

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
        // Real-time Hyprland events via socket.
        hypr_events::hypr_events_subscription().map(Message::HyprEvent),
        // Fallback poll every 5s in case socket misses events.
        iced::time::every(Duration::from_secs(5)).map(|_| Message::RefreshWindows),
        // Forward iced events for mouse tracking.
        event::listen().map(Message::IcedEvent),
    ];

    let has_focused = !dock.hidden
        && dock
            .items
            .iter()
            .any(|item| item.is_focused(&dock.active_address));

    // When animating, tick at ~60fps for smooth spring physics.
    if any_animating
        || dock.slide_offset.has_energy()
        || dock.show_timer.is_some()
        || dock.hide_timer.is_some()
        || dock.minimize_reveal_timer.is_some()
    {
        subs.push(
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick(Instant::now())),
        );
    } else if has_focused {
        // Slower tick for rainbow indicator animation (~30fps).
        subs.push(
            iced::time::every(Duration::from_millis(33)).map(|_| Message::Tick(Instant::now())),
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

/// Create a separator element.
/// Horizontal docks get a vertical bar (2px wide, 32px tall).
/// Vertical docks get a horizontal bar (32px wide, 2px tall).
fn separator<'a>(horizontal: bool) -> Element<'a, Message> {
    let (w, h): (f32, f32) = if horizontal { (2.0, 32.0) } else { (32.0, 2.0) };
    container(Space::new().width(w).height(h))
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
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
        items = items.push(menu_button(
            "Restore",
            Message::ContextMenuRestore(app_id.clone()),
        ));
        items = items.push(menu_button(
            "Close",
            Message::ContextMenuClose(app_id.clone()),
        ));
    } else if cm.is_running {
        // Pin/Unpin
        let label = if cm.is_pinned { "Unpin" } else { "Pin" };
        items = items.push(menu_button(label, Message::ContextMenuPin(app_id.clone())));

        // Move to Workspace options
        let workspaces = hypr::get_workspaces();
        let first_addr = cm.window_addresses.first().cloned().unwrap_or_default();
        for ws in &workspaces {
            let label = format!("Move to Workspace {}", ws.name);
            items = items.push(menu_button(
                &label,
                Message::ContextMenuMoveWorkspace(first_addr.clone(), ws.id),
            ));
        }

        // Move to Monitor options (only if multiple monitors)
        let monitors = hypr::get_monitors();
        if monitors.len() > 1 {
            for mon in &monitors {
                let label = format!("Move to {}", mon.name);
                items = items.push(menu_button(
                    &label,
                    Message::ContextMenuMoveMonitor(first_addr.clone(), mon.name.clone()),
                ));
            }
        }

        // Close
        items = items.push(menu_button(
            "Close",
            Message::ContextMenuClose(app_id.clone()),
        ));
    } else {
        items = items.push(menu_button(
            "Launch",
            Message::ContextMenuLaunch(app_id.clone()),
        ));
        items = items.push(menu_button(
            "Unpin",
            Message::ContextMenuPin(app_id.clone()),
        ));
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
fn menu_button(label: impl ToString, msg: Message) -> Element<'static, Message> {
    mouse_area(
        container(text(label.to_string()).size(14).color(TEXT_COLOR))
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
    .interaction(iced::mouse::Interaction::Pointer)
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

            // Mark both app_id and wm_class as seen so running windows
            // with a different class name don't create duplicates.
            seen.insert(wm_class.clone());

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
        self.prev_item_count = self.items.len();
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

    /// Compute the total rendered width of the dock content (for centering/input region calculations).
    fn compute_dock_content_length(&self) -> f32 {
        let icon_size = self.config.icon_size as f32;
        let spacing = 4.0;
        let padding = 12.0;
        let item_width = icon_size + spacing + 4.0; // icon + spacing + container padding (2px each side)
        let separator_gap = 2.0 + spacing; // separator width + spacing

        let mut width = padding * 2.0;
        let mut has_pinned = false;
        let mut started_running = false;
        let mut started_minimized = false;

        for item in &self.items {
            if !item.pinned && !item.minimized && !started_running && has_pinned {
                started_running = true;
                width += separator_gap;
            }
            if item.minimized && !started_minimized {
                started_minimized = true;
                if has_pinned || started_running {
                    width += separator_gap;
                }
            }
            if item.pinned {
                has_pinned = true;
            }
            width += item_width;
        }

        width
    }

    /// Update magnification targets based on cursor position.
    fn update_magnification(&mut self) {
        let cursor_x = match self.cursor_x {
            Some(x) => x,
            None => return,
        };

        // Convert surface-relative cursor_x to dock-content-relative coordinates.
        // The dock content is centered within the full-width surface.
        let dock_width = self.compute_dock_content_length();
        let centering_offset = (self.surface_length as f32 - dock_width) / 2.0;
        let dock_relative_x = cursor_x - centering_offset;

        let magnification = self.config.magnification;
        let icon_size = self.config.icon_size as f32;
        let half_item = (icon_size + 4.0) / 2.0;

        for (i, item) in self.items.iter_mut().enumerate() {
            let item_x = self.item_positions.get(i).copied().unwrap_or(0.0);
            let distance = (dock_relative_x - item_x).abs();
            let target_scale = if distance <= half_item {
                magnification
            } else {
                1.0
            };
            item.scale.set_target(target_scale);
        }
    }

    /// Reset all magnification targets back to 1.0.
    fn reset_magnification(&mut self) {
        for item in &mut self.items {
            item.scale.set_target(1.0);
        }
    }

    /// Build tasks to update the input region and margin for all known main windows.
    /// When visible: input region covers only the centered dock content with edge margin.
    /// When hidden (auto-hide): 4px trigger strip at the relevant screen edge, 0px margin.
    fn input_region_tasks(&self) -> Task<Message> {
        let ids = self.known_window_ids.borrow().clone();
        if ids.is_empty() {
            return Task::none();
        }

        let dock_length = self.compute_dock_content_length();
        let sl = self.surface_length as i32;
        let st = SURFACE_THICKNESS as i32;
        let hidden = self.hidden;
        let auto_hide = self.config.auto_hide;
        let position = self.position;
        let is_horizontal = position.is_horizontal();
        let pill_size = (self.config.icon_size as f32 + 26.0)
            .min(SURFACE_THICKNESS)
            .ceil() as i32;
        let has_context_menu = self.context_menu.is_some();
        let dock_len_i = dock_length.ceil() as i32;

        let tasks: Vec<Task<Message>> = ids
            .into_iter()
            .flat_map(|id| {
                let (region_x, region_y, region_w, region_h) = if hidden && auto_hide {
                    // 4px trigger strip at the screen edge
                    match position {
                        DockPosition::Bottom => (0, st - 4, sl, 4),
                        DockPosition::Top => (0, 0, sl, 4),
                        DockPosition::Left => (0, 0, 4, sl),
                        DockPosition::Right => (st - 4, 0, 4, sl),
                    }
                } else if has_context_menu {
                    // Context menu open: full surface catches clicks to dismiss
                    if is_horizontal {
                        (0, 0, sl, st)
                    } else {
                        (0, 0, st, sl)
                    }
                } else {
                    // Only the visible dock pill area, centered on the long axis
                    match position {
                        DockPosition::Bottom => {
                            let cx = ((sl as f32 - dock_length) / 2.0).max(0.0) as i32;
                            (cx, st - pill_size, dock_len_i, pill_size)
                        }
                        DockPosition::Top => {
                            let cx = ((sl as f32 - dock_length) / 2.0).max(0.0) as i32;
                            (cx, 0, dock_len_i, pill_size)
                        }
                        DockPosition::Left => {
                            let cy = ((sl as f32 - dock_length) / 2.0).max(0.0) as i32;
                            (0, cy, pill_size, dock_len_i)
                        }
                        DockPosition::Right => {
                            let cy = ((sl as f32 - dock_length) / 2.0).max(0.0) as i32;
                            (st - pill_size, cy, pill_size, dock_len_i)
                        }
                    }
                };

                // Dynamic margin: 0 when hidden (surface touches screen edge), 8 when visible
                let margin = if hidden && auto_hide {
                    (0i32, 0i32, 0i32, 0i32)
                } else {
                    match position {
                        DockPosition::Bottom => (0, 0, 8, 0),
                        DockPosition::Top => (8, 0, 0, 0),
                        DockPosition::Left => (0, 0, 0, 8),
                        DockPosition::Right => (0, 8, 0, 0),
                    }
                };

                vec![
                    Task::done(Message::SetInputRegion {
                        id,
                        callback: ActionCallback::new(move |region| {
                            region.add(region_x, region_y, region_w, region_h);
                        }),
                    }),
                    Task::done(Message::MarginChange { id, margin }),
                ]
            })
            .collect();

        Task::batch(tasks)
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

/// Convert HSL values to an iced Color. Hue in degrees (0–360), saturation/lightness in 0.0–1.0.
fn hsl_to_color(hue: f32, saturation: f32, lightness: f32) -> Color {
    let h = ((hue % 360.0) + 360.0) % 360.0;
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = lightness - c / 2.0;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color {
        r: r + m,
        g: g + m,
        b: b + m,
        a: 1.0,
    }
}
