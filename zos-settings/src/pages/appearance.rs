// === pages/appearance.rs — Theme, cursor, font, and wallpaper settings ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gio;

use crate::services::hyprctl;

/// Run a gsettings command.
fn gsettings_set(schema: &str, key: &str, value: &str) {
    let result = std::process::Command::new("gsettings")
        .args(["set", schema, key, value])
        .status();

    match result {
        Ok(status) if status.success() => {
            tracing::info!("gsettings set {} {} {}", schema, key, value);
        }
        Ok(status) => {
            tracing::error!(
                "gsettings set {} {} {} failed: {}",
                schema,
                key,
                value,
                status
            );
        }
        Err(e) => {
            tracing::error!("Failed to run gsettings: {}", e);
        }
    }
}

/// Read a gsettings value.
fn gsettings_get(schema: &str, key: &str) -> String {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .replace('\'', "")
            .to_string(),
        _ => String::from("unknown"),
    }
}

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

/// Build the appearance settings page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    page.append(&build_theme_section());
    page.append(&build_cursor_section());
    page.append(&build_font_section());
    page.append(&build_wallpaper_section());

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&page)
        .build();

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    wrapper.append(&scrolled);
    wrapper
}

// ---------------------------------------------------------------------------
// Theme section
// ---------------------------------------------------------------------------

fn build_theme_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Theme").build();

    // --- Dark/Light mode ---
    let current_scheme = gsettings_get("org.gnome.desktop.interface", "color-scheme");
    let is_dark = current_scheme.contains("dark");

    let dark_row = adw::ActionRow::builder()
        .title("Dark Mode")
        .subtitle("Use dark color scheme for applications")
        .build();

    let dark_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(is_dark)
        .build();

    dark_switch.connect_active_notify(|sw: &gtk::Switch| {
        let scheme = if sw.is_active() {
            "prefer-dark"
        } else {
            "prefer-light"
        };
        gsettings_set("org.gnome.desktop.interface", "color-scheme", scheme);
    });

    dark_row.add_suffix(&dark_switch);
    dark_row.set_activatable_widget(Some(&dark_switch));
    group.add(&dark_row);

    // --- GTK Theme (read-only) ---
    let gtk_theme = gsettings_get("org.gnome.desktop.interface", "gtk-theme");
    let theme_row = adw::ActionRow::builder()
        .title("GTK Theme")
        .subtitle(&gtk_theme)
        .build();
    group.add(&theme_row);

    group
}

// ---------------------------------------------------------------------------
// Cursor section
// ---------------------------------------------------------------------------

fn build_cursor_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Cursor").build();

    let size_options = ["16", "24", "32", "48"];
    let size_model = gtk::StringList::new(&size_options);

    // Default from defaults.conf is 24
    let size_combo = adw::ComboRow::builder()
        .title("Size")
        .model(&size_model)
        .selected(1) // 24
        .build();

    size_combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(&size) = size_options.get(idx) {
            hyprctl::keyword("env", &format!("XCURSOR_SIZE,{}", size));
            hyprctl::keyword("env", &format!("HYPRCURSOR_SIZE,{}", size));
            gsettings_set("org.gnome.desktop.interface", "cursor-size", size);
        }
    });
    group.add(&size_combo);

    group
}

// ---------------------------------------------------------------------------
// Font section
// ---------------------------------------------------------------------------

fn build_font_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Font").build();

    let current_font = gsettings_get("org.gnome.desktop.interface", "font-name");
    let font_row = adw::ActionRow::builder()
        .title("Interface Font")
        .subtitle(&current_font)
        .build();
    group.add(&font_row);

    group
}

// ---------------------------------------------------------------------------
// Wallpaper section
// ---------------------------------------------------------------------------

fn build_wallpaper_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Wallpaper").build();

    let current_wp = read_wallpaper_path();

    let wp_row = adw::ActionRow::builder()
        .title("Current Wallpaper")
        .subtitle(&current_wp)
        .build();
    group.add(&wp_row);

    // --- Change wallpaper button ---
    let change_row = adw::ActionRow::builder()
        .title("Change Wallpaper")
        .subtitle("Select a new wallpaper image")
        .build();

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
