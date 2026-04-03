// === commands/update.rs — OS update management via bootc ===

use color_eyre::eyre::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct UpdateStatus {
    pub current_image: String,
    pub pending: bool,
    pub pending_details: Option<String>,
}

/// Check if OS updates are available.
pub fn check_for_updates() -> Result<UpdateStatus> {
    let current_image = get_current_image();

    // bootc upgrade --check exits 0 if update available, prints info
    let output = Command::new("bootc")
        .args(["upgrade", "--check"])
        .output()
        .wrap_err("Failed to run bootc upgrade --check")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{}\n{}", stdout, stderr);

    // bootc prints "Update available" or similar when an update exists
    // "No update available" or exit code indicates no update
    let pending = output.status.success()
        && !combined.contains("No update available")
        && !combined.contains("already present");

    let details = if pending {
        Some(combined.trim().to_string())
    } else {
        None
    };

    Ok(UpdateStatus {
        current_image,
        pending,
        pending_details: details,
    })
}

/// Apply the pending OS update.
pub fn apply_update() -> Result<std::process::Output> {
    let output = Command::new("bootc")
        .arg("upgrade")
        .output()
        .wrap_err("Failed to run bootc upgrade")?;

    Ok(output)
}

/// Rollback to the previous deployment.
pub fn rollback() -> Result<std::process::Output> {
    let output = Command::new("bootc")
        .arg("rollback")
        .output()
        .wrap_err("Failed to run bootc rollback")?;

    Ok(output)
}

/// Get a reboot suggestion message.
pub fn reboot_message() -> &'static str {
    "Update applied. Please reboot to boot into the new deployment:\n  systemctl reboot"
}

// ---------------------------------------------------------------------------
// Flatpak updates
// ---------------------------------------------------------------------------

/// Check for pending flatpak updates. Returns list of app IDs with updates available.
pub fn check_flatpak_updates() -> Result<Vec<String>> {
    let output = Command::new("flatpak")
        .args(["remote-ls", "--updates", "--columns=application"])
        .output()
        .wrap_err("Failed to run flatpak remote-ls --updates")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let updates: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(updates)
}

/// Apply all pending flatpak updates.
pub fn apply_flatpak_updates() -> Result<std::process::Output> {
    let output = Command::new("flatpak")
        .args(["update", "-y"])
        .output()
        .wrap_err("Failed to run flatpak update")?;

    Ok(output)
}

// ---------------------------------------------------------------------------
// Custom package updates (GitHub releases)
// ---------------------------------------------------------------------------

/// Check for updates to custom (GitHub-distributed) packages.
/// Returns names of packages that have newer versions available.
pub fn check_custom_updates() -> Result<Vec<String>> {
    use super::install::{load_custom_packages, resolve_github_release};

    let packages = load_custom_packages();
    let mut updatable = Vec::new();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());

    // Get installed flatpak list for flatpak-type packages
    let installed_flatpaks = Command::new("flatpak")
        .args(["list", "--user", "--columns=application,version"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    for pkg in &packages {
        let is_installed = match pkg.install_type.as_str() {
            "github-flatpak" => {
                let pkg_lower = pkg.name.to_lowercase();
                installed_flatpaks
                    .lines()
                    .any(|l| l.to_lowercase().contains(&pkg_lower))
            }
            "github-appimage" => {
                let slug = pkg
                    .name
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '-' })
                    .collect::<String>()
                    .split('-')
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
                std::path::Path::new(&format!("{home}/.local/bin/{slug}")).exists()
            }
            _ => false,
        };

        if !is_installed {
            continue;
        }

        if let Ok((tag, _url)) = resolve_github_release(&pkg.github_repo, &pkg.asset_pattern) {
            updatable.push(format!("{} ({})", pkg.name, tag));
        }
    }

    Ok(updatable)
}

/// Apply updates to custom packages by re-installing from latest GitHub release.
pub fn apply_custom_updates() -> Result<String> {
    use super::install::{install_custom_package, load_custom_packages};

    let packages = load_custom_packages();
    let mut updated = Vec::new();

    for pkg in &packages {
        if pkg.install_type != "github-flatpak" {
            continue;
        }
        match install_custom_package(pkg) {
            Ok(()) => updated.push(pkg.name.clone()),
            Err(e) => {
                eprintln!("Failed to update {}: {}", pkg.name, e);
            }
        }
    }

    if updated.is_empty() {
        Ok("No custom packages to update.".into())
    } else {
        Ok(format!("Updated: {}", updated.join(", ")))
    }
}

/// Ensure flatpak/appimage overrides from custom-packages.json are applied
/// to already-installed packages. Idempotent — safe to call on every update.
pub fn ensure_custom_overrides() -> Result<()> {
    use super::install::{load_custom_packages, slugify};

    let packages = load_custom_packages();
    let installed_flatpaks = Command::new("flatpak")
        .args(["list", "--user", "--columns=application"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());

    for pkg in &packages {
        match pkg.install_type.as_str() {
            "github-flatpak" => {
                if let Some(overrides) = &pkg.flatpak_overrides {
                    if installed_flatpaks
                        .lines()
                        .any(|l| l.trim() == overrides.app_id)
                    {
                        for (key, value) in &overrides.env {
                            let env_arg = format!("--env={}={}", key, value);
                            let _ = Command::new("flatpak")
                                .args(["override", "--user", &env_arg, &overrides.app_id])
                                .status();
                        }
                    }
                }
            }
            "github-appimage" => {
                if let Some(env_vars) = &pkg.env {
                    let slug = slugify(&pkg.name);
                    let bin_path = format!("{home}/.local/bin/{slug}");
                    if std::path::Path::new(&bin_path).exists() {
                        let env_prefix: String = env_vars
                            .iter()
                            .map(|(k, v)| format!("{}={}", k, v))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let exec_line = format!("env {} {}", env_prefix, bin_path);
                        let desktop_dir = format!("{home}/.local/share/applications");
                        let _ = std::fs::create_dir_all(&desktop_dir);
                        let desktop_content = format!(
                            "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon=application-x-executable\nCategories=Graphics;3DGraphics;\nComment={}\n",
                            pkg.name, exec_line, pkg.description
                        );
                        let _ = std::fs::write(
                            format!("{desktop_dir}/{slug}.desktop"),
                            desktop_content,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// --- Internal helpers ---

fn get_current_image() -> String {
    let output = Command::new("bootc").args(["status", "--json"]).output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout) {
                // bootc status --json has spec.image.image for the current image
                val.pointer("/spec/image/image")
                    .or_else(|| val.pointer("/status/booted/image/image/image"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string()
            } else {
                "unknown".into()
            }
        }
        _ => "unknown".into(),
    }
}
