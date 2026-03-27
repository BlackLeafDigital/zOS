// === pages/panel.rs — HyprPanel customization page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gio;

const THEMES_DIR: &str = "/usr/share/hyprpanel/themes";

/// Discover available theme files from the themes directory.
fn list_themes() -> Vec<(String, String)> {
    let mut themes = Vec::new();
    let entries = match std::fs::read_dir(THEMES_DIR) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::error!("Failed to read themes directory {}: {}", THEMES_DIR, e);
            return themes;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let display_name = stem
                    .replace('_', " ")
                    .split_whitespace()
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(first) => {
                                let upper: String = first.to_uppercase().collect();
                                format!("{}{}", upper, chars.collect::<String>())
                            }
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let full_path = path.to_string_lossy().to_string();
                themes.push((display_name, full_path));
            }
        }
    }

    themes.sort_by(|a, b| a.0.cmp(&b.0));
    themes
}

/// Read bar layout from ~/.config/hyprpanel/config.json.
fn read_bar_layouts() -> (String, String, String) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    let config_path = format!("{}/.config/hyprpanel/config.json", home);

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return ("N/A".into(), "N/A".into(), "N/A".into()),
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return ("N/A".into(), "N/A".into(), "N/A".into()),
    };

    let extract_modules = |position: &str| -> String {
        // Try bar.layouts.0.left/middle/right first, then bar.layouts.*.left etc.
        let layouts = &json["bar"]["layouts"];

        // Try "0" key first (common in HyprPanel config)
        let layout = if layouts["0"].is_object() {
            &layouts["0"]
        } else if layouts["*"].is_object() {
            &layouts["*"]
        } else {
            return "N/A".into();
        };

        match &layout[position] {
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            _ => "N/A".into(),
        }
    };

    (
        extract_modules("left"),
        extract_modules("middle"),
        extract_modules("right"),
    )
}

/// Run a hyprpanel CLI command.
fn hyprpanel_run(args: &[&str]) {
    match std::process::Command::new("hyprpanel").args(args).status() {
        Ok(status) if status.success() => {
            tracing::info!("hyprpanel {} succeeded", args.join(" "));
        }
        Ok(status) => {
            tracing::error!("hyprpanel {} exited with {}", args.join(" "), status);
        }
        Err(e) => {
            tracing::error!("Failed to run hyprpanel {}: {}", args.join(" "), e);
        }
    }
}

/// Build the panel settings page widget.
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
    page.append(&build_bar_layout_section());
    page.append(&build_quick_actions_section());

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

    let themes = list_themes();

    if themes.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No themes found")
            .subtitle(format!("Check that {} exists", THEMES_DIR))
            .build();
        group.add(&empty_row);
        return group;
    }

    let model = gtk::StringList::new(&[]);
    for (display_name, _) in &themes {
        model.append(display_name);
    }

    let combo = adw::ComboRow::builder()
        .title("Panel Theme")
        .model(&model)
        .selected(gtk::INVALID_LIST_POSITION)
        .build();

    let icon = gtk::Image::from_icon_name("applications-graphics-symbolic");
    icon.set_valign(gtk::Align::Center);
    combo.add_prefix(&icon);

    let themes_for_combo = themes.clone();
    combo.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some((_, ref path)) = themes_for_combo.get(idx) {
            hyprpanel_run(&["useTheme", path]);
        }
    });

    group.add(&combo);

    group
}

// ---------------------------------------------------------------------------
// Bar layout section
// ---------------------------------------------------------------------------

fn build_bar_layout_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Bar Layout").build();

    let (left, center, right) = read_bar_layouts();

    let left_row = adw::ActionRow::builder()
        .title("Left")
        .subtitle(&left)
        .build();
    group.add(&left_row);

    let center_row = adw::ActionRow::builder()
        .title("Center")
        .subtitle(&center)
        .build();
    group.add(&center_row);

    let right_row = adw::ActionRow::builder()
        .title("Right")
        .subtitle(&right)
        .build();
    group.add(&right_row);

    let edit_row = adw::ActionRow::builder()
        .title("Edit Layout")
        .subtitle("Open HyprPanel settings to modify bar layout")
        .build();

    let edit_btn = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();

    edit_btn.connect_clicked(|_| {
        hyprpanel_run(&["-t", "settingsDialog"]);
    });

    edit_row.add_suffix(&edit_btn);
    edit_row.set_activatable_widget(Some(&edit_btn));
    group.add(&edit_row);

    group
}

// ---------------------------------------------------------------------------
// Quick actions section
// ---------------------------------------------------------------------------

fn build_quick_actions_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Quick Actions")
        .build();

    // --- Open Panel Settings ---
    let settings_row = adw::ActionRow::builder()
        .title("Open Panel Settings")
        .subtitle("Open the full HyprPanel settings dialog")
        .build();

    let settings_btn = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();

    settings_btn.connect_clicked(|_| {
        hyprpanel_run(&["-t", "settingsDialog"]);
    });

    settings_row.add_suffix(&settings_btn);
    settings_row.set_activatable_widget(Some(&settings_btn));
    group.add(&settings_row);

    // --- Restart Panel ---
    let restart_row = adw::ActionRow::builder()
        .title("Restart Panel")
        .subtitle("Restart HyprPanel to apply changes")
        .build();

    let restart_btn = gtk::Button::builder()
        .label("Restart")
        .valign(gtk::Align::Center)
        .build();

    restart_btn.connect_clicked(|_| {
        hyprpanel_run(&["restart"]);
    });

    restart_row.add_suffix(&restart_btn);
    restart_row.set_activatable_widget(Some(&restart_btn));
    group.add(&restart_row);

    // --- Set Wallpaper ---
    let wallpaper_row = adw::ActionRow::builder()
        .title("Set Wallpaper")
        .subtitle("Choose an image to set as the panel wallpaper")
        .build();

    let wallpaper_btn = gtk::Button::builder()
        .label("Browse")
        .valign(gtk::Align::Center)
        .build();

    wallpaper_btn.connect_clicked(move |btn| {
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

        dialog.open(window.as_ref(), None::<&gio::Cancellable>, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let path_str = path.to_string_lossy().to_string();
                    hyprpanel_run(&["setWallpaper", &path_str]);
                }
            }
        });
    });

    wallpaper_row.add_suffix(&wallpaper_btn);
    wallpaper_row.set_activatable_widget(Some(&wallpaper_btn));
    group.add(&wallpaper_row);

    group
}
