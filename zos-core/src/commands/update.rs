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

/// A pending update for a custom package — tag we have vs. tag upstream has.
#[derive(Debug, Clone)]
pub struct CustomUpdate {
    pub name: String,
    pub slug: String,
    pub install_type: String,
    pub installed_tag: String,
    pub latest_tag: String,
}

/// Check which custom packages have a newer GitHub release than what's recorded
/// in `~/.local/share/zos/custom-installed.json`. Flathub entries are skipped
/// here — `flatpak update` covers them.
pub fn check_custom_updates() -> Result<Vec<CustomUpdate>> {
    use super::install::{load_custom_packages, load_manifest, resolve_github_release, slugify};

    let packages = load_custom_packages();
    let manifest = load_manifest();
    let mut updatable = Vec::new();

    for pkg in &packages {
        if pkg.install_type == "flathub" {
            continue;
        }
        let slug = slugify(&pkg.name);
        let Some(entry) = manifest.get(&slug) else {
            continue; // not installed via zos install
        };
        let installed_tag = entry.tag.clone().unwrap_or_default();

        let (Some(repo), Some(pattern)) = (pkg.github_repo.as_deref(), pkg.asset_pattern.as_deref())
        else {
            continue;
        };

        let latest_tag = match resolve_github_release(repo, pattern) {
            Ok((tag, _)) => tag,
            Err(_) => continue, // network / API issue; skip silently
        };

        if latest_tag != installed_tag {
            updatable.push(CustomUpdate {
                name: pkg.name.clone(),
                slug,
                install_type: pkg.install_type.clone(),
                installed_tag,
                latest_tag,
            });
        }
    }

    Ok(updatable)
}

/// Apply updates to custom packages: re-install any `github-appimage` /
/// `github-flatpak` entry whose manifest tag differs from the latest GitHub
/// release. `flathub` entries are left to `flatpak update`.
pub fn apply_custom_updates() -> Result<Vec<String>> {
    use super::install::{install_custom_package, load_custom_packages, slugify};

    let pending = check_custom_updates()?;
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let packages = load_custom_packages();
    let mut updated = Vec::new();

    for up in &pending {
        let Some(pkg) = packages.iter().find(|p| slugify(&p.name) == up.slug) else {
            continue;
        };
        match install_custom_package(pkg) {
            Ok(()) => updated.push(format!("{} {} → {}", up.name, up.installed_tag, up.latest_tag)),
            Err(e) => eprintln!("Failed to update {}: {}", pkg.name, e),
        }
    }

    Ok(updated)
}

/// Ensure flatpak/appimage overrides from custom-packages.json are applied
/// to already-installed packages. Idempotent — safe to call on every update.
pub fn ensure_custom_overrides() -> Result<()> {
    use super::install::{
        apply_flatpak_overrides, apply_xdg_overrides, load_custom_packages, slugify,
    };

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
                        apply_xdg_overrides(&overrides.app_id);
                        apply_flatpak_overrides(overrides);
                    }
                }
            }
            "flathub" => {
                if let Some(app_id) = pkg.flathub_app_id.as_deref() {
                    if installed_flatpaks.lines().any(|l| l.trim() == app_id) {
                        apply_xdg_overrides(app_id);
                        if let Some(overrides) = &pkg.flatpak_overrides {
                            apply_flatpak_overrides(overrides);
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

// ---------------------------------------------------------------------------
// Brew / mise
// ---------------------------------------------------------------------------

/// Run `brew update && brew upgrade` with live output. No-op if brew isn't
/// installed.
pub fn update_brew() -> Result<()> {
    let Some(brew) = super::install::find_brew() else {
        println!("brew not installed — skipping.");
        return Ok(());
    };
    let _ = Command::new(&brew).arg("update").status();
    let _ = Command::new(&brew).arg("upgrade").status();
    Ok(())
}

/// Run `mise self-update -y && mise upgrade` with live output. No-op if mise
/// isn't installed.
pub fn update_mise() -> Result<()> {
    let Some(mise) = super::install::find_mise() else {
        println!("mise not installed — skipping.");
        return Ok(());
    };
    let _ = Command::new(&mise).args(["self-update", "-y"]).status();
    let _ = Command::new(&mise).arg("upgrade").status();
    Ok(())
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

fn header(name: &str) {
    println!("\x1b[1;35m== {name} ==\x1b[0m");
}

/// Update every source zOS manages: OS (bootc), Flatpak, custom packages
/// (AppImage + Flathub from custom-packages.json), Brew, mise. `check_only`
/// limits to diagnostics — no system changes.
pub fn run_all(check_only: bool) -> Result<()> {
    let mut os_applied = false;

    header("OS");
    match check_for_updates() {
        Ok(status) => {
            println!("Current image: {}", status.current_image);
            if status.pending {
                if let Some(d) = &status.pending_details {
                    println!("{d}");
                }
                if !check_only {
                    println!("Applying bootc upgrade...");
                    match apply_update() {
                        Ok(out) if out.status.success() => {
                            let s = String::from_utf8_lossy(&out.stdout);
                            if !s.trim().is_empty() {
                                println!("{}", s.trim());
                            }
                            os_applied = true;
                        }
                        Ok(out) => eprintln!(
                            "bootc upgrade failed: {}",
                            String::from_utf8_lossy(&out.stderr).trim()
                        ),
                        Err(e) => eprintln!("bootc upgrade error: {e}"),
                    }
                }
            } else {
                println!("Up to date.");
            }
        }
        Err(e) => eprintln!("OS check failed: {e}"),
    }
    println!();

    header("Flatpak");
    match check_flatpak_updates() {
        Ok(updates) if updates.is_empty() => println!("All flatpaks up to date."),
        Ok(updates) => {
            println!("{} update(s) pending:", updates.len());
            for u in &updates {
                println!("  {u}");
            }
            if !check_only {
                let _ = Command::new("flatpak").args(["update", "-y"]).status();
                let _ = ensure_custom_overrides();
            }
        }
        Err(e) => eprintln!("Flatpak check failed: {e}"),
    }
    println!();

    header("Custom packages");
    match check_custom_updates() {
        Ok(pending) if pending.is_empty() => println!("All custom packages up to date."),
        Ok(pending) => {
            for p in &pending {
                println!("  {} {} → {}", p.name, p.installed_tag, p.latest_tag);
            }
            if !check_only {
                match apply_custom_updates() {
                    Ok(updated) => {
                        for u in &updated {
                            println!("  ✓ {u}");
                        }
                    }
                    Err(e) => eprintln!("Custom update error: {e}"),
                }
            }
        }
        Err(e) => eprintln!("Custom check failed: {e}"),
    }
    println!();

    header("Brew");
    if check_only {
        if super::install::find_brew().is_some() {
            println!("brew installed — run 'zos update --only brew' to upgrade.");
        } else {
            println!("brew not installed.");
        }
    } else {
        let _ = update_brew();
    }
    println!();

    header("mise");
    if check_only {
        if super::install::find_mise().is_some() {
            println!("mise installed — run 'zos update --only mise' to upgrade.");
        } else {
            println!("mise not installed.");
        }
    } else {
        let _ = update_mise();
    }
    println!();

    if os_applied {
        println!("{}", reboot_message());
    }

    Ok(())
}

/// Run a single update source. Valid sources: os | flatpak | custom | brew | mise.
pub fn run_one(source: &str, check_only: bool) -> Result<()> {
    match source {
        "os" => {
            let status = check_for_updates()?;
            println!("Current image: {}", status.current_image);
            if !status.pending {
                println!("Up to date.");
                return Ok(());
            }
            if let Some(d) = &status.pending_details {
                println!("{d}");
            }
            if !check_only {
                let out = apply_update()?;
                if out.status.success() {
                    println!("{}", reboot_message());
                } else {
                    eprintln!(
                        "bootc upgrade failed: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                }
            }
        }
        "flatpak" => {
            let updates = check_flatpak_updates()?;
            if updates.is_empty() {
                println!("All flatpaks up to date.");
            } else if check_only {
                for u in &updates {
                    println!("  {u}");
                }
            } else {
                let _ = Command::new("flatpak").args(["update", "-y"]).status();
                let _ = ensure_custom_overrides();
            }
        }
        "custom" => {
            let pending = check_custom_updates()?;
            if pending.is_empty() {
                println!("All custom packages up to date.");
            } else if check_only {
                for p in &pending {
                    println!("  {} {} → {}", p.name, p.installed_tag, p.latest_tag);
                }
            } else {
                let updated = apply_custom_updates()?;
                for u in &updated {
                    println!("  ✓ {u}");
                }
            }
        }
        "brew" => {
            if check_only {
                if super::install::find_brew().is_some() {
                    println!("brew installed.");
                } else {
                    println!("brew not installed.");
                }
            } else {
                update_brew()?;
            }
        }
        "mise" => {
            if check_only {
                if super::install::find_mise().is_some() {
                    println!("mise installed.");
                } else {
                    println!("mise not installed.");
                }
            } else {
                update_mise()?;
            }
        }
        other => {
            eprintln!("Unknown source '{other}'. Valid: os | flatpak | custom | brew | mise");
            std::process::exit(2);
        }
    }
    Ok(())
}
