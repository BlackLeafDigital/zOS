// === commands/doctor.rs — System health diagnostics ===

use crate::commands::status::get_config_status;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "PASS"),
            CheckStatus::Fail => write!(f, "FAIL"),
            CheckStatus::Warn => write!(f, "WARN"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

/// Run all diagnostic checks and return results.
pub fn run_doctor_checks() -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    checks.push(check_hyprland());
    checks.push(check_nvidia());
    checks.push(check_pipewire());
    checks.extend(check_expected_packages());
    checks.push(check_deprecated_hypr_syntax());
    checks.extend(check_config_versions());

    checks
}

/// Summary counts: (pass, fail, warn)
pub fn summarize(checks: &[DoctorCheck]) -> (usize, usize, usize) {
    let pass = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Pass)
        .count();
    let fail = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .count();
    let warn = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Warn)
        .count();
    (pass, fail, warn)
}

// --- Individual checks ---

fn check_hyprland() -> DoctorCheck {
    let result = Command::new("hyprctl").arg("version").output();
    match result {
        Ok(output) if output.status.success() => {
            let ver = String::from_utf8_lossy(&output.stdout);
            let version_line = ver.lines().next().unwrap_or("unknown").to_string();
            DoctorCheck {
                name: "Hyprland".into(),
                status: CheckStatus::Pass,
                message: format!("Running: {}", version_line.trim()),
            }
        }
        Ok(_) => DoctorCheck {
            name: "Hyprland".into(),
            status: CheckStatus::Warn,
            message: "hyprctl returned non-zero — Hyprland may not be running".into(),
        },
        Err(_) => DoctorCheck {
            name: "Hyprland".into(),
            status: CheckStatus::Fail,
            message: "hyprctl not found — Hyprland is not installed or not in PATH".into(),
        },
    }
}

fn check_nvidia() -> DoctorCheck {
    let result = Command::new("nvidia-smi").output();
    match result {
        Ok(output) if output.status.success() => {
            let out = String::from_utf8_lossy(&output.stdout);
            let driver_line = out
                .lines()
                .find(|l| l.contains("Driver Version"))
                .unwrap_or("detected")
                .trim()
                .to_string();
            DoctorCheck {
                name: "NVIDIA Driver".into(),
                status: CheckStatus::Pass,
                message: driver_line,
            }
        }
        Ok(_) => DoctorCheck {
            name: "NVIDIA Driver".into(),
            status: CheckStatus::Warn,
            message: "nvidia-smi failed — may be AMD system or driver issue".into(),
        },
        Err(_) => DoctorCheck {
            name: "NVIDIA Driver".into(),
            status: CheckStatus::Warn,
            message: "nvidia-smi not found — assuming AMD GPU (normal for zos image)".into(),
        },
    }
}

fn check_pipewire() -> DoctorCheck {
    let result = Command::new("systemctl")
        .args(["--user", "is-active", "pipewire"])
        .output();
    match result {
        Ok(output) => {
            let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if status_str == "active" {
                DoctorCheck {
                    name: "PipeWire".into(),
                    status: CheckStatus::Pass,
                    message: "PipeWire is active".into(),
                }
            } else {
                DoctorCheck {
                    name: "PipeWire".into(),
                    status: CheckStatus::Fail,
                    message: format!("PipeWire status: {}", status_str),
                }
            }
        }
        Err(_) => DoctorCheck {
            name: "PipeWire".into(),
            status: CheckStatus::Fail,
            message: "Could not query PipeWire status".into(),
        },
    }
}

fn check_expected_packages() -> Vec<DoctorCheck> {
    let packages = [
        "waybar",
        "cursor-clip",
        "wl-clip-persist",
        "hyprlock",
        "grim",
        "slurp",
        "wl-copy",
    ];

    packages
        .iter()
        .map(|pkg| {
            let found = Command::new("which").arg(pkg).output();
            match found {
                Ok(output) if output.status.success() => DoctorCheck {
                    name: format!("Package: {}", pkg),
                    status: CheckStatus::Pass,
                    message: format!("{} is installed", pkg),
                },
                _ => DoctorCheck {
                    name: format!("Package: {}", pkg),
                    status: CheckStatus::Fail,
                    message: format!("{} not found in PATH", pkg),
                },
            }
        })
        .collect()
}

fn check_deprecated_hypr_syntax() -> DoctorCheck {
    let conf_path = crate::config::expand_home(".config/hypr/hyprland.conf");
    if !conf_path.exists() {
        return DoctorCheck {
            name: "Hyprland Syntax".into(),
            status: CheckStatus::Warn,
            message: "No hyprland.conf found".into(),
        };
    }

    let content = match std::fs::read_to_string(&conf_path) {
        Ok(c) => c,
        Err(_) => {
            return DoctorCheck {
                name: "Hyprland Syntax".into(),
                status: CheckStatus::Warn,
                message: "Could not read hyprland.conf".into(),
            }
        }
    };

    let deprecated_patterns = [
        "gaps_in",
        "gaps_out",
        "border_size",
        "no_cursor_warps",
        "cursor_inactive_timeout",
    ];

    let found: Vec<&&str> = deprecated_patterns
        .iter()
        .filter(|pat| content.contains(**pat))
        .collect();

    if found.is_empty() {
        DoctorCheck {
            name: "Hyprland Syntax".into(),
            status: CheckStatus::Pass,
            message: "No deprecated syntax detected".into(),
        }
    } else {
        DoctorCheck {
            name: "Hyprland Syntax".into(),
            status: CheckStatus::Warn,
            message: format!(
                "Deprecated keywords found: {}",
                found.iter().map(|s| **s).collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

fn check_config_versions() -> Vec<DoctorCheck> {
    get_config_status()
        .into_iter()
        .map(|area| {
            if area.up_to_date {
                DoctorCheck {
                    name: format!("Config: {}", area.name),
                    status: CheckStatus::Pass,
                    message: format!("v{} (current)", area.user_version),
                }
            } else {
                DoctorCheck {
                    name: format!("Config: {}", area.name),
                    status: CheckStatus::Warn,
                    message: format!(
                        "v{} -> v{} available",
                        area.user_version, area.system_version
                    ),
                }
            }
        })
        .collect()
}
