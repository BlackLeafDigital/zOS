// === pages/appearance.rs — Appearance / Theme settings page ===
//
// Allows the user to configure GTK themes, icon themes, cursor themes,
// color scheme, font rendering, and export GTK config files. All live
// changes go through gsettings; Hyprland cursor env is updated via hyprctl.

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text, Space};
use iced::{Background, Border, Element, Length, Task};

use crate::services::appearance;
use crate::services::hyprctl;
use crate::theme;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Message {
    // Theme section
    SetColorScheme(String),
    SetGtkTheme(String),
    SetIconTheme(String),

    // Cursor section
    SetCursorTheme(String),
    SetCursorSize(f64),

    // Font section
    SetTextScaling(f64),
    SetFontHinting(String),
    SetFontAntialiasing(String),
    SetFontRgbaOrder(String),

    // Config export section
    ExportGtk3,
    ExportGtk4,
    ExportGtk4Symlinks,
    ClearGtk4Symlinks,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct AppearancePage {
    settings: appearance::ThemeSettings,
    gtk_themes: Vec<String>,
    icon_themes: Vec<String>,
    cursor_themes: Vec<String>,
    status: Option<String>,
}

impl AppearancePage {
    pub fn new() -> Self {
        Self {
            settings: appearance::read_current_settings(),
            gtk_themes: appearance::list_gtk_themes(),
            icon_themes: appearance::list_icon_themes(),
            cursor_themes: appearance::list_cursor_themes(),
            status: None,
        }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    pub fn update(&mut self, message: Message) -> Task<Message> {
        const SCHEMA: &str = "org.gnome.desktop.interface";

        match message {
            // -- Theme --
            Message::SetColorScheme(val) => {
                appearance::gsettings_set(SCHEMA, "color-scheme", &val);
                self.settings.color_scheme = val;
            }
            Message::SetGtkTheme(val) => {
                appearance::gsettings_set(SCHEMA, "gtk-theme", &val);
                self.settings.gtk_theme = val;
            }
            Message::SetIconTheme(val) => {
                appearance::gsettings_set(SCHEMA, "icon-theme", &val);
                self.settings.icon_theme = val;
            }

            // -- Cursor --
            Message::SetCursorTheme(val) => {
                appearance::gsettings_set(SCHEMA, "cursor-theme", &val);
                hyprctl::keyword("env", &format!("XCURSOR_THEME,{val}"));
                appearance::export_cursor_index(&val);
                self.settings.cursor_theme = val;
            }
            Message::SetCursorSize(val) => {
                let size = val.round() as u32;
                appearance::gsettings_set(SCHEMA, "cursor-size", &size.to_string());
                hyprctl::keyword("env", &format!("XCURSOR_SIZE,{size}"));
                self.settings.cursor_size = size;
            }

            // -- Font --
            Message::SetTextScaling(val) => {
                // Round to nearest 0.05 for cleaner values.
                let rounded = (val * 20.0).round() / 20.0;
                appearance::gsettings_set(SCHEMA, "text-scaling-factor", &format!("{rounded:.2}"));
                self.settings.text_scaling_factor = rounded;
            }
            Message::SetFontHinting(val) => {
                appearance::gsettings_set(SCHEMA, "font-hinting", &val);
                self.settings.font_hinting = val;
            }
            Message::SetFontAntialiasing(val) => {
                appearance::gsettings_set(SCHEMA, "font-antialiasing", &val);
                self.settings.font_antialiasing = val;
            }
            Message::SetFontRgbaOrder(val) => {
                appearance::gsettings_set(SCHEMA, "font-rgba-order", &val);
                self.settings.font_rgba_order = val;
            }

            // -- Config export --
            Message::ExportGtk3 => {
                appearance::export_gtk3_settings(&self.settings);
                self.status = Some("Exported GTK3 settings.ini".into());
            }
            Message::ExportGtk4 => {
                appearance::export_gtk4_settings(&self.settings);
                self.status = Some("Exported GTK4 settings.ini".into());
            }
            Message::ExportGtk4Symlinks => {
                appearance::export_gtk4_symlinks(&self.settings.gtk_theme);
                self.status = Some(format!(
                    "Symlinked GTK4 assets for '{}'",
                    self.settings.gtk_theme
                ));
            }
            Message::ClearGtk4Symlinks => {
                appearance::clear_gtk4_symlinks();
                self.status = Some("Cleared GTK4 symlinks".into());
            }
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------

    pub fn view(&self) -> Element<'_, Message> {
        let title = text("Appearance").size(28).color(theme::TEXT);

        let theme_section = self.view_theme_section();
        let cursor_section = self.view_cursor_section();
        let font_section = self.view_font_section();
        let export_section = self.view_export_section();

        let content = column![
            title,
            theme_section,
            cursor_section,
            font_section,
            export_section,
        ]
        .spacing(24)
        .width(Length::Fill);

        scrollable(
            container(content)
                .width(Length::Fill)
                .height(Length::Shrink),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    // -----------------------------------------------------------------------
    // Section: Theme
    // -----------------------------------------------------------------------

    fn view_theme_section(&self) -> Element<'_, Message> {
        let heading = text("Theme").size(16).color(theme::TEXT);

        // Color scheme
        let color_schemes = vec![
            "prefer-dark".to_string(),
            "prefer-light".to_string(),
            "default".to_string(),
        ];
        let color_row = setting_row(
            "Color Scheme",
            pick_list(
                color_schemes,
                Some(self.settings.color_scheme.clone()),
                Message::SetColorScheme,
            )
            .width(Length::Fixed(280.0))
            .text_size(13.0)
            .into(),
        );

        // GTK theme
        let gtk_row = setting_row(
            "GTK Theme",
            pick_list(
                self.gtk_themes.clone(),
                Some(self.settings.gtk_theme.clone()),
                Message::SetGtkTheme,
            )
            .width(Length::Fixed(280.0))
            .text_size(13.0)
            .into(),
        );

        // Icon theme
        let icon_row = setting_row(
            "Icon Theme",
            pick_list(
                self.icon_themes.clone(),
                Some(self.settings.icon_theme.clone()),
                Message::SetIconTheme,
            )
            .width(Length::Fixed(280.0))
            .text_size(13.0)
            .into(),
        );

        let card_content = column![color_row, gtk_row, icon_row].spacing(12);

        column![heading, Space::new().height(4), card(card_content)]
            .spacing(8)
            .into()
    }

    // -----------------------------------------------------------------------
    // Section: Cursor
    // -----------------------------------------------------------------------

    fn view_cursor_section(&self) -> Element<'_, Message> {
        let heading = text("Cursor").size(16).color(theme::TEXT);

        // Cursor theme
        let cursor_row = setting_row(
            "Cursor Theme",
            pick_list(
                self.cursor_themes.clone(),
                Some(self.settings.cursor_theme.clone()),
                Message::SetCursorTheme,
            )
            .width(Length::Fixed(280.0))
            .text_size(13.0)
            .into(),
        );

        // Cursor size slider (16-48)
        let size = self.settings.cursor_size;
        let size_label = text(format!("Cursor Size: {size}"))
            .size(13)
            .color(theme::SUBTEXT0)
            .width(Length::Fixed(160.0));

        let size_slider = slider(16.0..=48.0, size as f64, Message::SetCursorSize)
            .step(2.0)
            .width(Length::Fixed(280.0));

        let size_row = row![size_label, size_slider]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        let card_content = column![cursor_row, size_row].spacing(12);

        column![heading, Space::new().height(4), card(card_content)]
            .spacing(8)
            .into()
    }

    // -----------------------------------------------------------------------
    // Section: Font
    // -----------------------------------------------------------------------

    fn view_font_section(&self) -> Element<'_, Message> {
        let heading = text("Fonts").size(16).color(theme::TEXT);

        // Current font (display only)
        let font_row = setting_row(
            "Font",
            text(&self.settings.font_name)
                .size(13)
                .color(theme::TEXT)
                .into(),
        );

        // Text scaling slider (0.5 - 2.0)
        let scale = self.settings.text_scaling_factor;
        let scale_label = text(format!("Text Scaling: {scale:.2}"))
            .size(13)
            .color(theme::SUBTEXT0)
            .width(Length::Fixed(160.0));

        let scale_slider = slider(0.5..=2.0, scale, Message::SetTextScaling)
            .step(0.05)
            .width(Length::Fixed(280.0));

        let scale_row = row![scale_label, scale_slider]
            .spacing(12)
            .align_y(iced::Alignment::Center);

        // Hinting
        let hinting_options = vec![
            "none".to_string(),
            "slight".to_string(),
            "medium".to_string(),
            "full".to_string(),
        ];
        let hinting_row = setting_row(
            "Hinting",
            pick_list(
                hinting_options,
                Some(self.settings.font_hinting.clone()),
                Message::SetFontHinting,
            )
            .width(Length::Fixed(280.0))
            .text_size(13.0)
            .into(),
        );

        // Antialiasing
        let aa_options = vec![
            "none".to_string(),
            "grayscale".to_string(),
            "subpixel".to_string(),
        ];
        let aa_row = setting_row(
            "Antialiasing",
            pick_list(
                aa_options,
                Some(self.settings.font_antialiasing.clone()),
                Message::SetFontAntialiasing,
            )
            .width(Length::Fixed(280.0))
            .text_size(13.0)
            .into(),
        );

        // Subpixel order -- only shown when antialiasing = "subpixel"
        let mut card_content = column![font_row, scale_row, hinting_row, aa_row].spacing(12);

        if self.settings.font_antialiasing == "subpixel" {
            let rgba_options = vec![
                "rgb".to_string(),
                "bgr".to_string(),
                "vrgb".to_string(),
                "vbgr".to_string(),
            ];
            let rgba_row = setting_row(
                "Subpixel Order",
                pick_list(
                    rgba_options,
                    Some(self.settings.font_rgba_order.clone()),
                    Message::SetFontRgbaOrder,
                )
                .width(Length::Fixed(280.0))
                .text_size(13.0)
                .into(),
            );
            card_content = card_content.push(rgba_row);
        }

        column![heading, Space::new().height(4), card(card_content)]
            .spacing(8)
            .into()
    }

    // -----------------------------------------------------------------------
    // Section: Config Export
    // -----------------------------------------------------------------------

    fn view_export_section(&self) -> Element<'_, Message> {
        let heading = text("Config Export").size(16).color(theme::TEXT);

        let gtk3_btn = action_button(
            "Export GTK3 Settings",
            Some(Message::ExportGtk3),
            theme::BLUE,
        );
        let gtk4_btn = action_button(
            "Export GTK4 Settings",
            Some(Message::ExportGtk4),
            theme::BLUE,
        );
        let sym_btn = action_button(
            "Export GTK4 Symlinks",
            Some(Message::ExportGtk4Symlinks),
            theme::BLUE,
        );
        let clear_btn = action_button(
            "Clear GTK4 Symlinks",
            Some(Message::ClearGtk4Symlinks),
            theme::RED,
        );

        let btn_row = row![gtk3_btn, gtk4_btn, sym_btn, clear_btn].spacing(12);

        let mut card_content = column![btn_row].spacing(12);

        if let Some(ref msg) = self.status {
            card_content = card_content.push(text(msg.as_str()).size(13).color(theme::GREEN));
        }

        column![heading, Space::new().height(4), card(card_content)]
            .spacing(8)
            .into()
    }
}

// ---------------------------------------------------------------------------
// Reusable helpers
// ---------------------------------------------------------------------------

/// A labelled row: left-aligned label (fixed width) + right-hand widget.
fn setting_row<'a>(label: &'a str, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label)
            .size(13)
            .color(theme::SUBTEXT0)
            .width(Length::Fixed(160.0)),
        widget,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

/// Wraps content in a styled card container (SURFACE0 background, rounded).
fn card<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::SURFACE0)),
            border: Border {
                radius: 12.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
}

/// A styled action button with accent color.
fn action_button(
    label: &str,
    on_press: Option<Message>,
    accent: iced::Color,
) -> Element<'_, Message> {
    let btn_label = text(label).size(13).color(theme::BASE);
    let mut btn = button(btn_label)
        .padding([8, 16])
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color { a: 0.85, ..accent },
                button::Status::Pressed => iced::Color { a: 0.70, ..accent },
                button::Status::Disabled => theme::SURFACE1,
                _ => accent,
            };
            let text_color = match status {
                button::Status::Disabled => theme::OVERLAY0,
                _ => theme::BASE,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }

    btn.into()
}
