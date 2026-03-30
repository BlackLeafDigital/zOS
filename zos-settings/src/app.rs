// === app.rs — Root application component with sidebar navigation ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::pages;

// --- Page enum ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    fn label(self) -> &'static str {
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

    fn icon(self) -> &'static str {
        match self {
            Page::Overview => "user-home-symbolic",
            Page::Display => "video-display-symbolic",
            Page::Audio => "audio-volume-high-symbolic",
            Page::Appearance => "applications-graphics-symbolic",
            Page::Input => "input-keyboard-symbolic",
            Page::Network => "network-wireless-signal-excellent-symbolic",
            Page::Dock => "go-down-symbolic",
            Page::Power => "system-shutdown-symbolic",
            Page::Boot => "drive-harddisk-symbolic",
        }
    }

    fn stack_name(self) -> &'static str {
        match self {
            Page::Overview => "overview",
            Page::Display => "display",
            Page::Audio => "audio",
            Page::Appearance => "appearance",
            Page::Input => "input",
            Page::Network => "network",
            Page::Dock => "dock",
            Page::Power => "power",
            Page::Boot => "boot",
        }
    }
}

const ALL_PAGES: &[Page] = &[
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

// --- App model ---

pub struct App {
    current_page: Page,
}

#[derive(Debug)]
pub enum AppMsg {
    SelectPage(Page),
}

pub struct AppWidgets {
    stack: gtk::Stack,
}

impl SimpleComponent for App {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type Root = adw::ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .title("zOS Settings")
            .default_width(1100)
            .default_height(600)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Force dark color scheme
        let style_manager = adw::StyleManager::default();
        style_manager.set_color_scheme(adw::ColorScheme::ForceDark);

        let model = App {
            current_page: Page::Overview,
        };

        // --- Build sidebar ---
        let sidebar_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        sidebar_list.add_css_class("navigation-sidebar");

        for page in ALL_PAGES {
            let row_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(12)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(12)
                .margin_end(12)
                .build();

            let icon = gtk::Image::from_icon_name(page.icon());
            icon.set_pixel_size(24);
            let label = gtk::Label::new(Some(page.label()));
            row_box.append(&icon);
            row_box.append(&label);

            let list_row = gtk::ListBoxRow::builder().child(&row_box).build();
            sidebar_list.append(&list_row);
        }

        // Select the first row
        if let Some(first_row) = sidebar_list.row_at_index(0) {
            sidebar_list.select_row(Some(&first_row));
        }

        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .width_request(220)
            .child(&sidebar_list)
            .build();

        // --- Build content stack ---
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .transition_duration(200)
            .hexpand(true)
            .vexpand(true)
            .build();

        let overview_page = pages::overview::build();
        stack.add_named(&overview_page, Some(Page::Overview.stack_name()));

        let display_page = pages::display::build();
        stack.add_named(&display_page, Some(Page::Display.stack_name()));

        let audio_page = pages::audio::build();
        stack.add_named(&audio_page, Some(Page::Audio.stack_name()));

        let appearance_page = pages::appearance::build();
        stack.add_named(&appearance_page, Some(Page::Appearance.stack_name()));

        let input_page = pages::input::build();
        stack.add_named(&input_page, Some(Page::Input.stack_name()));

        let network_page = pages::network::build();
        stack.add_named(&network_page, Some(Page::Network.stack_name()));

        let dock_page = pages::dock::build();
        stack.add_named(&dock_page, Some(Page::Dock.stack_name()));

        let power_page = pages::power::build();
        stack.add_named(&power_page, Some(Page::Power.stack_name()));

        let boot_page = pages::boot::build();
        stack.add_named(&boot_page, Some(Page::Boot.stack_name()));

        stack.set_visible_child_name(Page::Overview.stack_name());

        // --- Connect sidebar selection ---
        let sender_clone = sender.clone();
        sidebar_list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let idx = row.index() as usize;
                if let Some(&page) = ALL_PAGES.get(idx) {
                    sender_clone.input(AppMsg::SelectPage(page));
                }
            }
        });

        // --- Layout ---
        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();

        // Add separator between sidebar and content
        let separator = gtk::Separator::new(gtk::Orientation::Vertical);

        content_box.append(&sidebar_scroll);
        content_box.append(&separator);
        content_box.append(&stack);

        // Wrap in a vertical box with header bar
        let main_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();

        let header = adw::HeaderBar::new();
        main_box.append(&header);
        main_box.append(&content_box);

        root.set_content(Some(&main_box));

        let widgets = AppWidgets {
            stack: stack.clone(),
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppMsg::SelectPage(page) => {
                self.current_page = page;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets
            .stack
            .set_visible_child_name(self.current_page.stack_name());
    }
}
