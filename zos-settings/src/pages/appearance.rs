// === pages/appearance.rs — Full appearance settings with nwg-look parity ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gio;
use relm4::gtk::pango;

use crate::services::appearance;
use crate::services::hyprctl;

// ---------------------------------------------------------------------------
// Hyprpaper helpers (wallpaper config is hyprpaper-specific, not gsettings)
// ---------------------------------------------------------------------------

/// Read the current wallpaper path from hyprpaper.conf.
fn read_wallpaper_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    let config_path = format!("{}/.config/hypr/hyprpaper.conf", home);

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return String::from("Not configured"),
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("wallpaper") {
            if let Some(val) = trimmed.split('=').nth(1) {
                let val = val.trim();
                if let Some(path) = val.split(',').nth(1) {
                    return path.trim().to_string();
                }
            }
        }
    }

    String::from("Not configured")
}

/// Update hyprpaper.conf with a new wallpaper path.
fn write_wallpaper_config(path: &str) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    let config_path = format!("{}/.config/hypr/hyprpaper.conf", home);

    let content = format!(
        "preload = {}\nwallpaper = ,{}\nsplash = false\n",
        path, path
    );

    if let Err(e) = std::fs::write(&config_path, &content) {
        tracing::error!("Failed to write hyprpaper.conf: {}", e);
        return;
    }

    // Restart hyprpaper to apply
    let _ = std::process::Command::new("pkill")
        .arg("hyprpaper")
        .status();
    let _ = std::process::Command::new("hyprpaper").spawn();
    tracing::info!("Updated wallpaper to {}", path);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find the index of `needle` in `options`, returning 0 if not found.
fn find_index(options: &[&str], needle: &str) -> u32 {
    options.iter().position(|&o| o == needle).unwrap_or(0) as u32
}

/// Find the index of `needle` in a Vec<String>, returning 0 if not found.
fn find_string_index(options: &[String], needle: &str) -> u32 {
    options.iter().position(|o| o == needle).unwrap_or(0) as u32
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build the appearance settings page widget.
pub fn build() -> gtk::Box {
    let page = super::page_content();

    page.append(&build_theme_section());
    page.append(&build_cursor_section());
    page.append(&build_font_section());
    page.append(&build_wallpaper_section());
    page.append(&build_export_section());
    page.append(&build_sound_section());

    super::page_wrapper(&page)
}

// ---------------------------------------------------------------------------
// Section 1: Theme
// ---------------------------------------------------------------------------

fn build_theme_section() -> adw::PreferencesGroup {
    let settings = appearance::read_current_settings();
    let group = adw::PreferencesGroup::builder().title("Theme").build();

    // --- Color Scheme ---
    let scheme_labels = ["System Default", "Prefer Dark", "Prefer Light"];
    let scheme_values = ["default", "prefer-dark", "prefer-light"];
    let scheme_model = gtk::StringList::new(&scheme_labels);

    let scheme_combo = adw::ComboRow::builder()
        .title("Color Scheme")
        .model(&scheme_model)
        .selected(find_index(&scheme_values, &settings.color_scheme))
        .build();

    let scheme_icon = gtk::Image::from_icon_name("weather-clear-night-symbolic");
    scheme_icon.set_valign(gtk::Align::Center);
    scheme_combo.add_prefix(&scheme_icon);

    scheme_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(&val) = scheme_values.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "color-scheme", val);
        }
    });
    group.add(&scheme_combo);

    // --- GTK Theme ---
    let gtk_themes = appearance::list_gtk_themes();
    let gtk_theme_strs: Vec<&str> = gtk_themes.iter().map(|s| s.as_str()).collect();
    let gtk_model = gtk::StringList::new(&gtk_theme_strs);

    let gtk_combo = adw::ComboRow::builder()
        .title("GTK Theme")
        .model(&gtk_model)
        .selected(find_string_index(&gtk_themes, &settings.gtk_theme))
        .build();

    let gtk_icon = gtk::Image::from_icon_name("applications-graphics-symbolic");
    gtk_icon.set_valign(gtk::Align::Center);
    gtk_combo.add_prefix(&gtk_icon);

    gtk_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(theme) = gtk_themes.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "gtk-theme", theme);
        }
    });
    group.add(&gtk_combo);

    // --- Icon Theme ---
    let icon_themes = appearance::list_icon_themes();
    let icon_theme_strs: Vec<&str> = icon_themes.iter().map(|s| s.as_str()).collect();
    let icon_model = gtk::StringList::new(&icon_theme_strs);

    let icon_combo = adw::ComboRow::builder()
        .title("Icon Theme")
        .model(&icon_model)
        .selected(find_string_index(&icon_themes, &settings.icon_theme))
        .build();

    let icon_icon = gtk::Image::from_icon_name("folder-symbolic");
    icon_icon.set_valign(gtk::Align::Center);
    icon_combo.add_prefix(&icon_icon);

    icon_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(theme) = icon_themes.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "icon-theme", theme);
        }
    });
    group.add(&icon_combo);

    group
}

// ---------------------------------------------------------------------------
// Section 2: Cursor
// ---------------------------------------------------------------------------

fn build_cursor_section() -> adw::PreferencesGroup {
    let settings = appearance::read_current_settings();
    let group = adw::PreferencesGroup::builder().title("Cursor").build();

    // --- Cursor Theme ---
    let cursor_themes = appearance::list_cursor_themes();
    let cursor_theme_strs: Vec<&str> = cursor_themes.iter().map(|s| s.as_str()).collect();
    let cursor_model = gtk::StringList::new(&cursor_theme_strs);

    let cursor_combo = adw::ComboRow::builder()
        .title("Cursor Theme")
        .model(&cursor_model)
        .selected(find_string_index(&cursor_themes, &settings.cursor_theme))
        .build();

    let cursor_icon = gtk::Image::from_icon_name("input-mouse-symbolic");
    cursor_icon.set_valign(gtk::Align::Center);
    cursor_combo.add_prefix(&cursor_icon);

    cursor_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(theme) = cursor_themes.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "cursor-theme", theme);
            hyprctl::keyword("env", &format!("XCURSOR_THEME,{}", theme));
            appearance::export_cursor_index(theme);
        }
    });
    group.add(&cursor_combo);

    // --- Cursor Size ---
    let adjustment = gtk::Adjustment::new(settings.cursor_size as f64, 16.0, 96.0, 8.0, 8.0, 0.0);

    let size_row = adw::SpinRow::new(Some(&adjustment), 1.0, 0);
    size_row.set_title("Cursor Size");

    let size_icon = gtk::Image::from_icon_name("zoom-in-symbolic");
    size_icon.set_valign(gtk::Align::Center);
    size_row.add_prefix(&size_icon);

    size_row.connect_value_notify(move |spin| {
        let size = spin.value() as u32;
        let size_str = size.to_string();
        appearance::gsettings_set("org.gnome.desktop.interface", "cursor-size", &size_str);
        hyprctl::keyword("env", &format!("XCURSOR_SIZE,{}", size));
        hyprctl::keyword("env", &format!("HYPRCURSOR_SIZE,{}", size));
    });
    group.add(&size_row);

    group
}

// ---------------------------------------------------------------------------
// Section 3: Fonts
// ---------------------------------------------------------------------------

fn build_font_section() -> adw::PreferencesGroup {
    let settings = appearance::read_current_settings();
    let group = adw::PreferencesGroup::builder().title("Fonts").build();

    // --- Interface Font ---
    let font_row = adw::ActionRow::builder().title("Interface Font").build();

    let font_icon = gtk::Image::from_icon_name("font-x-generic-symbolic");
    font_icon.set_valign(gtk::Align::Center);
    font_row.add_prefix(&font_icon);

    let font_dialog = gtk::FontDialog::new();
    let font_btn = gtk::FontDialogButton::new(Some(font_dialog));
    let desc = pango::FontDescription::from_string(&settings.font_name);
    font_btn.set_font_desc(&desc);
    font_btn.set_valign(gtk::Align::Center);

    font_btn.connect_font_desc_notify(|btn| {
        if let Some(desc) = btn.font_desc() {
            let font_str = desc.to_string();
            appearance::gsettings_set("org.gnome.desktop.interface", "font-name", &font_str);
        }
    });

    font_row.add_suffix(&font_btn);
    group.add(&font_row);

    // --- Text Scaling Factor ---
    let scale_adjustment =
        gtk::Adjustment::new(settings.text_scaling_factor, 0.5, 3.0, 0.05, 0.1, 0.0);

    let scale_row = adw::SpinRow::new(Some(&scale_adjustment), 0.05, 2);
    scale_row.set_title("Text Scaling Factor");

    scale_row.connect_value_notify(move |spin| {
        let val = spin.value();
        appearance::gsettings_set(
            "org.gnome.desktop.interface",
            "text-scaling-factor",
            &format!("{:.2}", val),
        );
    });
    group.add(&scale_row);

    // --- Font Hinting ---
    let hinting_labels = ["None", "Slight", "Medium", "Full"];
    let hinting_values = ["none", "slight", "medium", "full"];
    let hinting_model = gtk::StringList::new(&hinting_labels);

    let hinting_combo = adw::ComboRow::builder()
        .title("Font Hinting")
        .model(&hinting_model)
        .selected(find_index(&hinting_values, &settings.font_hinting))
        .build();

    let hinting_icon = gtk::Image::from_icon_name("format-text-direction-symbolic");
    hinting_icon.set_valign(gtk::Align::Center);
    hinting_combo.add_prefix(&hinting_icon);

    hinting_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(&val) = hinting_values.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "font-hinting", val);
        }
    });
    group.add(&hinting_combo);

    // --- Subpixel Order (created before antialiasing so we can reference it) ---
    let subpixel_labels = ["RGB", "BGR", "VRGB", "VBGR"];
    let subpixel_values = ["rgb", "bgr", "vrgb", "vbgr"];
    let subpixel_model = gtk::StringList::new(&subpixel_labels);

    let subpixel_combo = adw::ComboRow::builder()
        .title("Subpixel Order")
        .model(&subpixel_model)
        .selected(find_index(&subpixel_values, &settings.font_rgba_order))
        .sensitive(settings.font_antialiasing == "rgba")
        .build();

    subpixel_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(&val) = subpixel_values.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "font-rgba-order", val);
        }
    });

    // --- Font Antialiasing ---
    let aa_labels = ["None", "Grayscale", "Subpixel (RGBA)"];
    let aa_values = ["none", "grayscale", "rgba"];
    let aa_model = gtk::StringList::new(&aa_labels);

    let aa_combo = adw::ComboRow::builder()
        .title("Font Antialiasing")
        .model(&aa_model)
        .selected(find_index(&aa_values, &settings.font_antialiasing))
        .build();

    let subpixel_combo_ref = subpixel_combo.clone();
    aa_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(&val) = aa_values.get(idx) {
            appearance::gsettings_set("org.gnome.desktop.interface", "font-antialiasing", val);
            subpixel_combo_ref.set_sensitive(val == "rgba");
        }
    });
    group.add(&aa_combo);

    group.add(&subpixel_combo);

    group
}

// ---------------------------------------------------------------------------
// Section 4: Wallpaper
// ---------------------------------------------------------------------------

fn build_wallpaper_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Wallpaper").build();

    let current_wp = read_wallpaper_path();

    let wp_row = adw::ActionRow::builder()
        .title("Current Wallpaper")
        .subtitle(&current_wp)
        .build();

    let wp_icon = gtk::Image::from_icon_name("preferences-desktop-wallpaper-symbolic");
    wp_icon.set_valign(gtk::Align::Center);
    wp_row.add_prefix(&wp_icon);

    group.add(&wp_row);

    // --- Change wallpaper button ---
    let change_row = adw::ActionRow::builder()
        .title("Change Wallpaper")
        .subtitle("Select a new wallpaper image")
        .build();

    let change_icon = gtk::Image::from_icon_name("folder-pictures-symbolic");
    change_icon.set_valign(gtk::Align::Center);
    change_row.add_prefix(&change_icon);

    let change_btn = gtk::Button::builder()
        .label("Browse")
        .valign(gtk::Align::Center)
        .build();

    let wp_row_clone = wp_row.clone();
    change_btn.connect_clicked(move |btn| {
        let filter = gtk::FileFilter::new();
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        filter.add_mime_type("image/webp");
        filter.set_name(Some("Images"));

        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title("Select Wallpaper")
            .filters(&filters)
            .build();

        let window = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let wp_ref = wp_row_clone.clone();
        dialog.open(window.as_ref(), None::<&gio::Cancellable>, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let path_str = path.to_string_lossy().to_string();
                    write_wallpaper_config(&path_str);
                    wp_ref.set_subtitle(&path_str);
                }
            }
        });
    });

    change_row.add_suffix(&change_btn);
    change_row.set_activatable_widget(Some(&change_btn));
    group.add(&change_row);

    group
}

// ---------------------------------------------------------------------------
// Section 5: Config Export
// ---------------------------------------------------------------------------

fn build_export_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Config Export")
        .description("Export settings to config files for GTK applications")
        .build();

    // --- GTK3 switch ---
    let gtk3_switch = adw::SwitchRow::builder()
        .title("Export GTK3 settings.ini")
        .subtitle("~/.config/gtk-3.0/settings.ini")
        .active(true)
        .build();
    group.add(&gtk3_switch);

    // --- GTK4 switch ---
    let gtk4_switch = adw::SwitchRow::builder()
        .title("Export GTK4 settings.ini")
        .subtitle("~/.config/gtk-4.0/settings.ini")
        .active(true)
        .build();
    group.add(&gtk4_switch);

    // --- GTK4 Symlinks switch ---
    let symlinks_switch = adw::SwitchRow::builder()
        .title("GTK4 Theme Symlinks")
        .subtitle("Link theme CSS to ~/.config/gtk-4.0/")
        .build();
    group.add(&symlinks_switch);

    // --- Cursor index.theme switch ---
    let cursor_switch = adw::SwitchRow::builder()
        .title("Export Cursor index.theme")
        .subtitle("~/.icons/default/index.theme")
        .active(true)
        .build();
    group.add(&cursor_switch);

    // --- Apply button ---
    let apply_row = adw::ActionRow::builder()
        .title("Apply Exports")
        .subtitle("Write selected config files now")
        .build();

    let apply_btn = gtk::Button::builder()
        .label("Apply")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();

    let gtk3_ref = gtk3_switch.clone();
    let gtk4_ref = gtk4_switch.clone();
    let symlinks_ref = symlinks_switch.clone();
    let cursor_ref = cursor_switch.clone();

    apply_btn.connect_clicked(move |_| {
        let settings = appearance::read_current_settings();

        if gtk3_ref.is_active() {
            appearance::export_gtk3_settings(&settings);
        }
        if gtk4_ref.is_active() {
            appearance::export_gtk4_settings(&settings);
        }
        if symlinks_ref.is_active() {
            appearance::export_gtk4_symlinks(&settings.gtk_theme);
        }
        if cursor_ref.is_active() {
            appearance::export_cursor_index(&settings.cursor_theme);
        }

        tracing::info!("Config export applied");
    });

    apply_row.add_suffix(&apply_btn);
    apply_row.set_activatable_widget(Some(&apply_btn));
    group.add(&apply_row);

    // --- Clear GTK4 Symlinks ---
    let clear_row = adw::ActionRow::builder()
        .title("Clear GTK4 Symlinks")
        .subtitle("Remove symlinked theme CSS from ~/.config/gtk-4.0/")
        .build();

    let clear_btn = gtk::Button::builder()
        .label("Clear")
        .valign(gtk::Align::Center)
        .build();

    clear_btn.connect_clicked(move |_| {
        appearance::clear_gtk4_symlinks();
        tracing::info!("GTK4 symlinks cleared");
    });

    clear_row.add_suffix(&clear_btn);
    clear_row.set_activatable_widget(Some(&clear_btn));
    group.add(&clear_row);

    group
}

// ---------------------------------------------------------------------------
// Section 6: Sound
// ---------------------------------------------------------------------------

fn build_sound_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Sound").build();

    // --- Event Sounds ---
    let event_current =
        appearance::gsettings_get("org.gnome.desktop.sound", "event-sounds") == "true";

    let event_row = adw::SwitchRow::builder()
        .title("Event Sounds")
        .subtitle("Play sounds for desktop events")
        .active(event_current)
        .build();

    let event_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
    event_icon.set_valign(gtk::Align::Center);
    event_row.add_prefix(&event_icon);

    event_row.connect_active_notify(|row| {
        let val = if row.is_active() { "true" } else { "false" };
        appearance::gsettings_set("org.gnome.desktop.sound", "event-sounds", val);
    });
    group.add(&event_row);

    // --- Input Feedback Sounds ---
    let feedback_current =
        appearance::gsettings_get("org.gnome.desktop.sound", "input-feedback-sounds") == "true";

    let feedback_row = adw::SwitchRow::builder()
        .title("Input Feedback Sounds")
        .subtitle("Play sounds for input events")
        .active(feedback_current)
        .build();

    feedback_row.connect_active_notify(|row| {
        let val = if row.is_active() { "true" } else { "false" };
        appearance::gsettings_set("org.gnome.desktop.sound", "input-feedback-sounds", val);
    });
    group.add(&feedback_row);

    group
}
