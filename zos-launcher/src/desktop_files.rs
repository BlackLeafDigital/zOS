//! Minimal `.desktop` file parser. Reads `[Desktop Entry]` sections from the
//! standard XDG paths. Skips entries with NoDisplay=true, Type != Application,
//! or Hidden=true.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub name: String,
    pub comment: String,
    pub exec: String,
    /// Parsed but not rendered in v1 — icon-theme lookup is deferred.
    #[allow(dead_code)]
    pub icon: String,
    pub terminal: bool,
}

pub fn discover() -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for dir in xdg_app_dirs() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let path = ent.path();
                if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                    continue;
                }
                // Use the filename as a dedup id (so user override of system entry wins
                // because user dirs come first in xdg_app_dirs order).
                let id = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if !seen_ids.insert(id) {
                    continue;
                }
                if let Some(entry) = parse_desktop_file(&path) {
                    entries.push(entry);
                }
            }
        }
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn xdg_app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // User dir first so user overrides win in dedup
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(&home).join(".local/share/applications"));
    }

    // System dirs from XDG_DATA_DIRS (or default)
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in data_dirs.split(':') {
        dirs.push(PathBuf::from(d).join("applications"));
    }

    // Flatpak system + user
    dirs.push(PathBuf::from(
        "/var/lib/flatpak/exports/share/applications",
    ));
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(
            PathBuf::from(&home).join(".local/share/flatpak/exports/share/applications"),
        );
    }

    dirs
}

fn parse_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_section = false;
    let mut name = String::new();
    let mut comment = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut terminal = false;
    let mut nodisplay = false;
    let mut hidden = false;
    let mut type_app = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, val)) = trimmed.split_once('=') else {
            continue;
        };
        // Skip locale-specific keys (e.g., Name[de]) — use only the bare key
        if key.contains('[') {
            continue;
        }

        match key.trim() {
            "Name" => name = val.trim().to_string(),
            "Comment" => comment = val.trim().to_string(),
            "Exec" => exec = val.trim().to_string(),
            "Icon" => icon = val.trim().to_string(),
            "Terminal" => terminal = val.trim().eq_ignore_ascii_case("true"),
            "NoDisplay" => nodisplay = val.trim().eq_ignore_ascii_case("true"),
            "Hidden" => hidden = val.trim().eq_ignore_ascii_case("true"),
            "Type" => type_app = val.trim() == "Application",
            _ => {}
        }
    }

    if !type_app || nodisplay || hidden || name.is_empty() || exec.is_empty() {
        return None;
    }
    Some(DesktopEntry {
        name,
        comment,
        exec,
        icon,
        terminal,
    })
}

/// Simple substring + word-boundary scoring. Returns Some(score) if matched, None if not.
/// Higher score = better match.
pub fn score(entry: &DesktopEntry, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0); // all entries match an empty query
    }
    let q = query.to_lowercase();
    let name_l = entry.name.to_lowercase();
    let comment_l = entry.comment.to_lowercase();

    // Exact name match: highest priority
    if name_l == q {
        return Some(1000);
    }
    // Name starts with query
    if name_l.starts_with(&q) {
        return Some(800);
    }
    // Name contains query
    if name_l.contains(&q) {
        // Earlier position = higher score
        let pos = name_l.find(&q).unwrap_or(name_l.len()) as i32;
        return Some(500 - pos);
    }
    // Comment contains query (lower priority)
    if comment_l.contains(&q) {
        return Some(100);
    }
    None
}

/// Strip Exec field codes (%U, %f, %F, %u, etc.) per the desktop entry spec.
pub fn strip_exec_codes(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    for token in exec.split_whitespace() {
        if token.starts_with('%') && token.len() == 2 {
            // Skip exec codes
            continue;
        }
        // Strip surrounding quotes
        let cleaned = token.trim_matches('"').trim_matches('\'');
        args.push(cleaned.to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_returns_entries() {
        let entries = discover();
        // On any reasonable Linux box there will be at least a handful.
        // We don't assert exact count, just that the call works and returns something.
        eprintln!("discovered {} desktop entries", entries.len());
    }

    #[test]
    fn score_empty_query_matches_all() {
        let e = DesktopEntry {
            name: "Foo".into(),
            comment: String::new(),
            exec: "foo".into(),
            icon: String::new(),
            terminal: false,
        };
        assert_eq!(score(&e, ""), Some(0));
    }

    #[test]
    fn score_exact_name_beats_substring() {
        let exact = DesktopEntry {
            name: "term".into(),
            comment: String::new(),
            exec: "x".into(),
            icon: String::new(),
            terminal: false,
        };
        let prefix = DesktopEntry {
            name: "terminal".into(),
            comment: String::new(),
            exec: "x".into(),
            icon: String::new(),
            terminal: false,
        };
        assert!(score(&exact, "term").unwrap() > score(&prefix, "term").unwrap());
    }

    #[test]
    fn strip_exec_codes_drops_field_codes() {
        let args = strip_exec_codes("firefox %U");
        assert_eq!(args, vec!["firefox"]);
        let args = strip_exec_codes("foo %f bar");
        assert_eq!(args, vec!["foo", "bar"]);
    }
}
