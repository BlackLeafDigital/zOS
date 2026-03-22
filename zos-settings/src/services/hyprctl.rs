use std::process::Command;

/// Apply a Hyprland keyword setting immediately.
pub fn keyword(key: &str, value: &str) {
    let _ = Command::new("hyprctl")
        .args(["keyword", key, value])
        .status();
}

/// Reload Hyprland configuration.
#[allow(dead_code)]
pub fn reload() {
    let _ = Command::new("hyprctl").args(["reload"]).status();
}
