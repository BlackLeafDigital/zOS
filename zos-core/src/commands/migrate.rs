// === commands/migrate.rs — Config migration planning and execution ===

use crate::config;
use chrono::Local;
use color_eyre::eyre::{eyre, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct MigrationAction {
    pub area: String,
    pub description: String,
    pub applied: bool,
}

/// Compare user state against system versions and produce a migration plan.
pub fn plan_migrations() -> Vec<MigrationAction> {
    let user_state = config::read_user_state().unwrap_or_default();
    let system_versions = read_system_config_versions();
    let mut actions = Vec::new();

    if user_state.hypr < system_versions.hypr {
        let user_conf = config::expand_home(".config/hypr/hyprland.conf");
        if user_conf.exists() {
            let content = fs::read_to_string(&user_conf).unwrap_or_default();
            if content.contains("usr/share/zos/hypr") {
                actions.push(MigrationAction {
                    area: "hypr".into(),
                    description: "Update Hyprland config version marker".into(),
                    applied: false,
                });
            } else {
                actions.push(MigrationAction {
                    area: "hypr".into(),
                    description: "Migrate monolithic hyprland.conf to thin loader + user overrides"
                        .into(),
                    applied: false,
                });
            }
        } else {
            actions.push(MigrationAction {
                area: "hypr".into(),
                description: "Install Hyprland config from skel".into(),
                applied: false,
            });
        }
    }

    if user_state.waybar < system_versions.waybar {
        actions.push(MigrationAction {
            area: "waybar".into(),
            description: "Update Waybar config to latest version".into(),
            applied: false,
        });
    }

    if user_state.wlogout < system_versions.wlogout {
        actions.push(MigrationAction {
            area: "wlogout".into(),
            description: "Update wlogout layout to latest version".into(),
            applied: false,
        });
    }

    if user_state.zshrc < system_versions.zshrc {
        actions.push(MigrationAction {
            area: "zshrc".into(),
            description: "Update .zshrc with latest shell configuration".into(),
            applied: false,
        });
    }

    if user_state.gitconfig < system_versions.gitconfig {
        actions.push(MigrationAction {
            area: "gitconfig".into(),
            description: "Update .gitconfig defaults".into(),
            applied: false,
        });
    }

    if user_state.wezterm < system_versions.wezterm {
        actions.push(MigrationAction {
            area: "wezterm".into(),
            description: "Update Wezterm config to latest version".into(),
            applied: false,
        });
    }

    actions
}

/// Apply all pending migration actions.
pub fn apply_migrations(actions: &mut [MigrationAction]) -> Result<()> {
    let backup_dir = config::ensure_backup_dir()?;
    let mut state = config::read_user_state().unwrap_or_default();
    let system_versions = read_system_config_versions();
    let datestamp = Local::now().format("%Y%m%d").to_string();

    for action in actions.iter_mut() {
        match action.area.as_str() {
            "hypr" => {
                apply_hypr_migration(&backup_dir, &datestamp)?;
                state.hypr = system_versions.hypr;
                action.applied = true;
            }
            "waybar" => {
                apply_skel_migration(".config/waybar", &backup_dir, &datestamp, "waybar")?;
                state.waybar = system_versions.waybar;
                action.applied = true;
            }
            "wlogout" => {
                apply_skel_migration(".config/wlogout", &backup_dir, &datestamp, "wlogout")?;
                state.wlogout = system_versions.wlogout;
                action.applied = true;
            }
            "zshrc" => {
                apply_single_file_migration(".zshrc", "/etc/skel/.zshrc", &backup_dir, &datestamp)?;
                state.zshrc = system_versions.zshrc;
                action.applied = true;
            }
            "gitconfig" => {
                apply_single_file_migration(
                    ".gitconfig",
                    "/etc/skel/.gitconfig",
                    &backup_dir,
                    &datestamp,
                )?;
                state.gitconfig = system_versions.gitconfig;
                action.applied = true;
            }
            "wezterm" => {
                apply_skel_migration(".config/wezterm", &backup_dir, &datestamp, "wezterm")?;
                state.wezterm = system_versions.wezterm;
                action.applied = true;
            }
            other => {
                return Err(eyre!("Unknown migration area: {}", other));
            }
        }
    }

    config::write_user_state(&state)?;
    Ok(())
}

/// Run migrations silently (no TUI) — for systemd service / --auto flag.
pub fn run_auto_migrate() -> Result<()> {
    let mut actions = plan_migrations();
    if actions.is_empty() {
        return Ok(());
    }

    let count = actions.len();
    apply_migrations(&mut actions)?;

    // Send desktop notification if anything was migrated
    let applied: Vec<&str> = actions
        .iter()
        .filter(|a| a.applied)
        .map(|a| a.area.as_str())
        .collect();

    if !applied.is_empty() {
        let msg = format!(
            "zOS migrated {} config area(s): {}",
            count,
            applied.join(", ")
        );
        let _ = Command::new("notify-send")
            .args(["zOS System", &msg, "--icon=system-software-update"])
            .spawn();
    }

    Ok(())
}

// --- Internal helpers ---

fn read_system_config_versions() -> config::ConfigState {
    let version_file = "/usr/share/zos/config-versions.json";
    if let Ok(content) = fs::read_to_string(version_file) {
        if let Ok(state) = serde_json::from_str::<config::ConfigState>(&content) {
            return state;
        }
    }
    config::ConfigState {
        hypr: 1,
        waybar: 1,
        wlogout: 1,
        zshrc: 1,
        gitconfig: 1,
        wezterm: 1,
    }
}

fn apply_hypr_migration(backup_dir: &Path, datestamp: &str) -> Result<()> {
    let user_hypr_dir = config::expand_home(".config/hypr");
    let user_conf = user_hypr_dir.join("hyprland.conf");

    // Back up existing config if it exists
    if user_conf.exists() {
        let content = fs::read_to_string(&user_conf).unwrap_or_default();
        let is_thin_loader = content.contains("usr/share/zos/hypr");

        let backup_name = format!("hyprland.conf.{}", datestamp);
        fs::copy(&user_conf, backup_dir.join(&backup_name))
            .wrap_err("Failed to back up hyprland.conf")?;

        if !is_thin_loader {
            // Old monolithic config — replace with thin loader
            let skel_conf = Path::new(config::SKEL_HYPR_DIR).join("hyprland.conf");
            if skel_conf.exists() {
                fs::copy(&skel_conf, &user_conf)
                    .wrap_err("Failed to copy thin loader hyprland.conf")?;
            }
        }
    } else {
        // No config at all — copy from skel
        fs::create_dir_all(&user_hypr_dir).wrap_err("Failed to create hypr config directory")?;
        let skel_conf = Path::new(config::SKEL_HYPR_DIR).join("hyprland.conf");
        if skel_conf.exists() {
            fs::copy(&skel_conf, &user_conf).wrap_err("Failed to copy hyprland.conf from skel")?;
        }
    }

    // Create user override files only if they don't already exist
    let user_override_files = [
        "monitors.conf",
        "user-settings.conf",
        "user-keybinds.conf",
        "user-windowrules.conf",
    ];
    for filename in &user_override_files {
        let user_file = user_hypr_dir.join(filename);
        if !user_file.exists() {
            let skel_file = Path::new(config::SKEL_HYPR_DIR).join(filename);
            if skel_file.exists() {
                fs::copy(&skel_file, &user_file)
                    .wrap_err_with(|| format!("Failed to copy {} from skel", filename))?;
            } else {
                // Create an empty placeholder with a header comment
                fs::write(
                    &user_file,
                    format!(
                        "# {} — zOS user overrides\n# Add your customizations here.\n",
                        filename
                    ),
                )
                .wrap_err_with(|| format!("Failed to create {}", filename))?;
            }
        }
    }

    Ok(())
}

fn apply_skel_migration(
    config_rel_path: &str,
    backup_dir: &Path,
    datestamp: &str,
    area_name: &str,
) -> Result<()> {
    let user_dir = config::expand_home(config_rel_path);
    let skel_dir = Path::new("/etc/skel").join(config_rel_path);

    // Back up existing directory
    if user_dir.exists() {
        let backup_name = format!("{}.{}", area_name, datestamp);
        let backup_path = backup_dir.join(&backup_name);
        if !backup_path.exists() {
            copy_dir_recursive(&user_dir, &backup_path)
                .wrap_err_with(|| format!("Failed to back up {}", area_name))?;
        }
    }

    // Copy from skel
    if skel_dir.exists() {
        fs::create_dir_all(&user_dir)
            .wrap_err_with(|| format!("Failed to create {} directory", area_name))?;
        copy_dir_recursive(&skel_dir, &user_dir)
            .wrap_err_with(|| format!("Failed to copy {} from skel", area_name))?;
    }

    Ok(())
}

fn apply_single_file_migration(
    home_rel_path: &str,
    skel_path: &str,
    backup_dir: &Path,
    datestamp: &str,
) -> Result<()> {
    let user_file = config::expand_home(home_rel_path);
    let skel_file = Path::new(skel_path);

    // Back up existing file
    if user_file.exists() {
        let filename = Path::new(home_rel_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let backup_name = format!("{}.{}", filename, datestamp);
        fs::copy(&user_file, backup_dir.join(&backup_name))
            .wrap_err_with(|| format!("Failed to back up {}", home_rel_path))?;
    }

    // Copy from skel
    if skel_file.exists() {
        if let Some(parent) = user_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(skel_file, &user_file)
            .wrap_err_with(|| format!("Failed to copy {} from skel", home_rel_path))?;
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
