// === services/appearance.rs — Theme discovery, gsettings helpers, config export ===

use std::collections::BTreeSet;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// gsettings helpers
// ---------------------------------------------------------------------------

/// Read a gsettings value, trimming whitespace and surrounding single quotes.
/// Returns "unknown" on failure.
pub fn gsettings_get(schema: &str, key: &str) -> String {
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

/// Write a gsettings value. Logs success or failure via tracing.
pub fn gsettings_set(schema: &str, key: &str, value: &str) {
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

// ---------------------------------------------------------------------------
// ThemeSettings
// ---------------------------------------------------------------------------

pub struct ThemeSettings {
    pub gtk_theme: String,
    pub icon_theme: String,
    pub cursor_theme: String,
    pub cursor_size: u32,
    pub font_name: String,
    pub font_hinting: String,
    pub font_antialiasing: String,
    pub font_rgba_order: String,
    pub text_scaling_factor: f64,
    pub color_scheme: String,
}

/// Read all current appearance settings from gsettings.
pub fn read_current_settings() -> ThemeSettings {
    let schema = "org.gnome.desktop.interface";

    let cursor_size = gsettings_get(schema, "cursor-size")
        .parse::<u32>()
        .unwrap_or(24);

    let text_scaling_factor = gsettings_get(schema, "text-scaling-factor")
        .parse::<f64>()
        .unwrap_or(1.0);

    ThemeSettings {
        gtk_theme: gsettings_get(schema, "gtk-theme"),
        icon_theme: gsettings_get(schema, "icon-theme"),
        cursor_theme: gsettings_get(schema, "cursor-theme"),
        cursor_size,
        font_name: gsettings_get(schema, "font-name"),
        font_hinting: gsettings_get(schema, "font-hinting"),
        font_antialiasing: gsettings_get(schema, "font-antialiasing"),
        font_rgba_order: gsettings_get(schema, "font-rgba-order"),
        text_scaling_factor,
        color_scheme: gsettings_get(schema, "color-scheme"),
    }
}

// ---------------------------------------------------------------------------
// Theme directory scanning
// ---------------------------------------------------------------------------

/// Return the list of directories to scan for a given category ("themes" or "icons").
fn theme_search_dirs(subdir: &str) -> Vec<PathBuf> {
    let home = home_dir();
    let mut dirs = Vec::new();

    // $XDG_DATA_HOME/{subdir} (default ~/.local/share/{subdir})
    let data_home =
        std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", home));
    dirs.push(PathBuf::from(format!("{}/{}", data_home, subdir)));

    // Each directory in $XDG_DATA_DIRS + /{subdir}
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':') {
        if !dir.is_empty() {
            dirs.push(PathBuf::from(format!("{}/{}", dir, subdir)));
        }
    }

    // $HOME/.{subdir}
    dirs.push(PathBuf::from(format!("{}/.{}", home, subdir)));

    dirs
}

/// Discover installed GTK themes.
///
/// A directory is considered a valid GTK theme if it contains a `gtk-3.0/`
/// or `gtk-4.0/` subdirectory. "Default" and "Emacs" are excluded.
pub fn list_gtk_themes() -> Vec<String> {
    let mut themes = BTreeSet::new();

    for search_dir in theme_search_dirs("themes") {
        let entries = match std::fs::read_dir(&search_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let has_gtk3 = path.join("gtk-3.0").is_dir();
            let has_gtk4 = path.join("gtk-4.0").is_dir();
            if !has_gtk3 && !has_gtk4 {
                continue;
            }

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "Default" || name == "Emacs" {
                    continue;
                }
                themes.insert(name.to_string());
            }
        }
    }

    themes.into_iter().collect()
}

/// Discover installed icon themes.
///
/// A directory is valid if it contains `index.theme` with both a `Name=`
/// line and a `Directories=` line. The directory name is returned (not the
/// Name= value). "default", "hicolor", and "locolor" are excluded
/// (case-insensitive).
pub fn list_icon_themes() -> Vec<String> {
    let mut themes = BTreeSet::new();

    for search_dir in theme_search_dirs("icons") {
        let entries = match std::fs::read_dir(&search_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Exclusions (case-insensitive)
            let lower = dir_name.to_lowercase();
            if lower == "default" || lower == "hicolor" || lower == "locolor" {
                continue;
            }

            let index_path = path.join("index.theme");
            let content = match std::fs::read_to_string(&index_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let has_name = content.lines().any(|l| l.starts_with("Name="));
            let has_dirs = content.lines().any(|l| l.starts_with("Directories="));
            if has_name && has_dirs {
                themes.insert(dir_name);
            }
        }
    }

    themes.into_iter().collect()
}

/// Discover installed cursor themes.
///
/// A directory is valid if it contains a `cursors/` subdirectory.
/// Same exclusions as icon themes.
pub fn list_cursor_themes() -> Vec<String> {
    let mut themes = BTreeSet::new();

    for search_dir in theme_search_dirs("icons") {
        let entries = match std::fs::read_dir(&search_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let lower = dir_name.to_lowercase();
            if lower == "default" || lower == "hicolor" || lower == "locolor" {
                continue;
            }

            if path.join("cursors").is_dir() {
                themes.insert(dir_name);
            }
        }
    }

    themes.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Config export
// ---------------------------------------------------------------------------

/// Return `$HOME`, falling back to `/root`.
pub fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
}

/// Write `~/.config/gtk-3.0/settings.ini` with current theme settings.
pub fn export_gtk3_settings(settings: &ThemeSettings) {
    let home = home_dir();
    let dir = PathBuf::from(format!("{}/.config/gtk-3.0", home));
    let path = dir.join("settings.ini");

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::error!("Failed to create {}: {}", dir.display(), e);
        return;
    }

    let content = gtk_settings_ini(settings);
    if let Err(e) = std::fs::write(&path, &content) {
        tracing::error!("Failed to write {}: {}", path.display(), e);
    }
}

/// Write `~/.config/gtk-4.0/settings.ini` with current theme settings.
pub fn export_gtk4_settings(settings: &ThemeSettings) {
    let home = home_dir();
    let dir = PathBuf::from(format!("{}/.config/gtk-4.0", home));
    let path = dir.join("settings.ini");

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::error!("Failed to create {}: {}", dir.display(), e);
        return;
    }

    let content = gtk_settings_ini(settings);
    if let Err(e) = std::fs::write(&path, &content) {
        tracing::error!("Failed to write {}: {}", path.display(), e);
    }
}

/// Generate the GTK settings.ini content (shared by GTK3 and GTK4).
fn gtk_settings_ini(settings: &ThemeSettings) -> String {
    let hintstyle = match settings.font_hinting.as_str() {
        "full" => "hintfull",
        "slight" => "hintslight",
        "medium" => "hintmedium",
        "none" => "hintnone",
        other => other,
    };

    let prefer_dark = if settings.color_scheme.contains("dark") {
        1
    } else {
        0
    };

    format!(
        "[Settings]\n\
         gtk-theme-name={}\n\
         gtk-icon-theme-name={}\n\
         gtk-font-name={}\n\
         gtk-cursor-theme-name={}\n\
         gtk-cursor-theme-size={}\n\
         gtk-toolbar-style=GTK_TOOLBAR_BOTH_HORIZ\n\
         gtk-toolbar-icon-size=GTK_ICON_SIZE_LARGE_TOOLBAR\n\
         gtk-xft-antialias=1\n\
         gtk-xft-hinting=1\n\
         gtk-xft-hintstyle={}\n\
         gtk-xft-rgba={}\n\
         gtk-application-prefer-dark-theme={}\n",
        settings.gtk_theme,
        settings.icon_theme,
        settings.font_name,
        settings.cursor_theme,
        settings.cursor_size,
        hintstyle,
        settings.font_rgba_order,
        prefer_dark,
    )
}

/// Write `~/.icons/default/index.theme` to set the default cursor theme.
pub fn export_cursor_index(cursor_theme: &str) {
    let home = home_dir();
    let dir = PathBuf::from(format!("{}/.icons/default", home));
    let path = dir.join("index.theme");

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::error!("Failed to create {}: {}", dir.display(), e);
        return;
    }

    let content = format!(
        "[Icon Theme]\n\
         Name=Default\n\
         Comment=Default Cursor Theme\n\
         Inherits={}\n",
        cursor_theme,
    );

    if let Err(e) = std::fs::write(&path, &content) {
        tracing::error!("Failed to write {}: {}", path.display(), e);
    }
}

/// Create symlinks for the GTK4 theme assets in `~/.config/gtk-4.0/`.
///
/// Scans the same theme directories used by `list_gtk_themes()` to find the
/// theme's `gtk-4.0/` folder, then symlinks `gtk.css`, `gtk-dark.css`, and
/// `assets/` into the user's config directory.
pub fn export_gtk4_symlinks(theme_name: &str) {
    let home = home_dir();
    let target_dir = PathBuf::from(format!("{}/.config/gtk-4.0", home));

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        tracing::error!("Failed to create {}: {}", target_dir.display(), e);
        return;
    }

    // Find the theme's gtk-4.0 directory
    let mut gtk4_dir: Option<PathBuf> = None;
    for search_dir in theme_search_dirs("themes") {
        let candidate = search_dir.join(theme_name).join("gtk-4.0");
        if candidate.is_dir() {
            gtk4_dir = Some(candidate);
            break;
        }
    }

    let gtk4_dir = match gtk4_dir {
        Some(d) => d,
        None => {
            tracing::info!(
                "No gtk-4.0 directory found for theme '{}', skipping symlinks",
                theme_name
            );
            return;
        }
    };

    // Symlink gtk.css
    let css_src = gtk4_dir.join("gtk.css");
    let css_dst = target_dir.join("gtk.css");
    if css_src.exists() {
        remove_if_symlink(&css_dst);
        if let Err(e) = std::os::unix::fs::symlink(&css_src, &css_dst) {
            tracing::error!(
                "Failed to symlink {} -> {}: {}",
                css_src.display(),
                css_dst.display(),
                e
            );
        }
    }

    // Symlink gtk-dark.css (if exists)
    let dark_src = gtk4_dir.join("gtk-dark.css");
    let dark_dst = target_dir.join("gtk-dark.css");
    if dark_src.exists() {
        remove_if_symlink(&dark_dst);
        if let Err(e) = std::os::unix::fs::symlink(&dark_src, &dark_dst) {
            tracing::error!(
                "Failed to symlink {} -> {}: {}",
                dark_src.display(),
                dark_dst.display(),
                e
            );
        }
    }

    // Symlink assets/ (if exists)
    let assets_src = gtk4_dir.join("assets");
    let assets_dst = target_dir.join("assets");
    if assets_src.is_dir() {
        remove_if_symlink(&assets_dst);
        if let Err(e) = std::os::unix::fs::symlink(&assets_src, &assets_dst) {
            tracing::error!(
                "Failed to symlink {} -> {}: {}",
                assets_src.display(),
                assets_dst.display(),
                e
            );
        }
    }
}

/// Remove `~/.config/gtk-4.0/{gtk.css, gtk-dark.css, assets}` if they are symlinks.
pub fn clear_gtk4_symlinks() {
    let home = home_dir();
    let dir = PathBuf::from(format!("{}/.config/gtk-4.0", home));

    for name in &["gtk.css", "gtk-dark.css", "assets"] {
        let path = dir.join(name);
        remove_if_symlink(&path);
    }
}

/// Remove a path only if it is a symlink.
fn remove_if_symlink(path: &PathBuf) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_symlink() => {
            if let Err(e) = std::fs::remove_file(path) {
                tracing::error!("Failed to remove symlink {}: {}", path.display(), e);
            }
        }
        _ => {}
    }
}
