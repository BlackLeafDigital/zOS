// === icons.rs — Desktop file parsing and icon resolution ===

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Cached mapping from app class/ID to resolved icon path and display name.
#[derive(Debug, Clone)]
pub struct IconResolver {
    /// Maps lowercase window class -> DesktopEntry.
    entries: HashMap<String, DesktopEntry>,
}

/// Parsed fields from a .desktop file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DesktopEntry {
    /// The app ID (filename without .desktop extension).
    pub app_id: String,
    /// Human-readable name.
    pub name: String,
    /// The Icon field value (may be a name or absolute path).
    pub icon: String,
    /// StartupWMClass for matching to Hyprland window class.
    pub wm_class: String,
    /// Resolved absolute path to the icon file (SVG or PNG).
    pub icon_path: Option<PathBuf>,
}

impl IconResolver {
    /// Build the resolver by scanning system desktop file directories.
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        let search_dirs = [
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            dirs_home().join(".local/share/applications"),
            dirs_home().join(".local/share/flatpak/exports/share/applications"),
        ];

        for dir in &search_dirs {
            if let Ok(read_dir) = fs::read_dir(dir) {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                        continue;
                    }
                    if let Some(de) = parse_desktop_file(&path) {
                        // Index by lowercased WM class for fast lookup.
                        if !de.wm_class.is_empty() {
                            entries.insert(de.wm_class.to_lowercase(), de.clone());
                        }
                        // Also index by app ID.
                        entries.insert(de.app_id.to_lowercase(), de);
                    }
                }
            }
        }

        Self { entries }
    }

    /// Look up a desktop entry by Hyprland window class.
    /// Tries exact match, then case-insensitive match.
    pub fn lookup(&self, window_class: &str) -> Option<&DesktopEntry> {
        let key = window_class.to_lowercase();
        self.entries.get(&key)
    }

    /// Get the resolved icon path for a window class, if available.
    #[allow(dead_code)]
    pub fn icon_path_for(&self, window_class: &str) -> Option<&Path> {
        self.lookup(window_class)
            .and_then(|e| e.icon_path.as_deref())
    }

    /// Get the display name for a window class.
    #[allow(dead_code)]
    pub fn name_for(&self, window_class: &str) -> Option<&str> {
        self.lookup(window_class).map(|e| e.name.as_str())
    }
}

fn dirs_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
}

/// Parse a .desktop file and extract relevant fields.
fn parse_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let content = fs::read_to_string(path).ok()?;

    let app_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let mut name = String::new();
    let mut icon = String::new();
    let mut wm_class = String::new();
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_desktop_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some(val) = trimmed.strip_prefix("Name=") {
            if name.is_empty() {
                name = val.to_string();
            }
        } else if let Some(val) = trimmed.strip_prefix("Icon=") {
            icon = val.to_string();
        } else if let Some(val) = trimmed.strip_prefix("StartupWMClass=") {
            wm_class = val.to_string();
        }
    }

    if name.is_empty() && icon.is_empty() {
        return None;
    }

    // If no WM class specified, fall back to the app ID.
    if wm_class.is_empty() {
        wm_class = app_id.clone();
    }

    let icon_path = resolve_icon(&icon);

    Some(DesktopEntry {
        app_id,
        name,
        icon,
        wm_class,
        icon_path,
    })
}

/// Resolve an icon name or path to an absolute file path.
/// Searches icon themes in order: Papirus, Adwaita, hicolor.
fn resolve_icon(icon: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }

    // If it's already an absolute path, use it directly.
    let as_path = Path::new(icon);
    if as_path.is_absolute() && as_path.exists() {
        return Some(as_path.to_path_buf());
    }

    // Search icon themes for the icon name.
    let themes = ["Papirus", "Adwaita", "hicolor"];
    let categories = [
        "apps",
        "categories",
        "devices",
        "mimetypes",
        "places",
        "status",
    ];
    let sizes = ["scalable", "512x512", "256x256", "128x128", "64x64", "48x48"];
    let extensions = ["svg", "png"];
    let base_dirs = [
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/var/lib/flatpak/exports/share/icons"),
        dirs_home().join(".local/share/icons"),
        dirs_home().join(".local/share/flatpak/exports/share/icons"),
    ];

    for base_dir in &base_dirs {
        for theme in &themes {
            for size in &sizes {
                for category in &categories {
                    for ext in &extensions {
                        let path = base_dir
                            .join(theme)
                            .join(size)
                            .join(category)
                            .join(format!("{}.{}", icon, ext));
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    // Fallback: check pixmaps.
    for ext in &extensions {
        let pixmap = PathBuf::from(format!("/usr/share/pixmaps/{}.{}", icon, ext));
        if pixmap.exists() {
            return Some(pixmap);
        }
    }

    None
}
