// === doctor.rs — `zos doctor` system-health diagnostic ===
//
// Prints a colorized summary of the load-bearing pieces of a zOS install:
// Wayland session, NVIDIA driver, logind seat, hyprctl, zos-wm IPC,
// user config dir, animations.toml, and DRM outputs.
//
// Each check is infallible (worst case = Status::Fail). The command exits
// with code 1 if any check is Fail so it can be used in CI / scripts.

use std::path::Path;
use std::process::Command;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_DIM: &str = "\x1b[2m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn glyph(self) -> &'static str {
        match self {
            Status::Ok => "✓",
            Status::Warn => "⚠",
            Status::Fail => "✗",
        }
    }
    fn color(self) -> &'static str {
        match self {
            Status::Ok => ANSI_GREEN,
            Status::Warn => ANSI_YELLOW,
            Status::Fail => ANSI_RED,
        }
    }
}

struct Check {
    name: &'static str,
    status: Status,
    detail: String,
}

fn check_wayland_session() -> Check {
    let display = std::env::var("WAYLAND_DISPLAY").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    match (display, session) {
        (Some(d), Some(s)) if s == "wayland" => Check {
            name: "Wayland session",
            status: Status::Ok,
            detail: d,
        },
        (Some(d), _) => Check {
            name: "Wayland session",
            status: Status::Warn,
            detail: format!("WAYLAND_DISPLAY={} but XDG_SESSION_TYPE != wayland", d),
        },
        (None, _) => Check {
            name: "Wayland session",
            status: Status::Fail,
            detail: "WAYLAND_DISPLAY not set".into(),
        },
    }
}

fn check_nvidia() -> Check {
    let modeset = std::fs::read_to_string("/sys/module/nvidia_drm/parameters/modeset")
        .ok()
        .map(|s| s.trim().to_string());
    let fbdev = std::fs::read_to_string("/sys/module/nvidia_drm/parameters/fbdev")
        .ok()
        .map(|s| s.trim().to_string());
    let driver_ver = std::fs::read_to_string("/sys/module/nvidia/version")
        .ok()
        .map(|s| s.trim().to_string());

    match (driver_ver, modeset, fbdev) {
        (Some(ver), Some(ms), Some(fb)) => {
            let modeset_ok = ms == "Y";
            let fbdev_ok = fb == "Y";
            if modeset_ok && fbdev_ok {
                Check {
                    name: "NVIDIA driver",
                    status: Status::Ok,
                    detail: format!("v{} (modeset=Y, fbdev=Y)", ver),
                }
            } else {
                Check {
                    name: "NVIDIA driver",
                    status: Status::Warn,
                    detail: format!("v{} (modeset={}, fbdev={})", ver, ms, fb),
                }
            }
        }
        _ => Check {
            name: "NVIDIA driver",
            status: Status::Warn,
            detail: "not loaded (AMD/Intel system?)".into(),
        },
    }
}

fn check_libseat() -> Check {
    // We can't easily probe libseat from a non-compositor process.
    // Surrogate: check we have an active logind session.
    let session = std::env::var("XDG_SESSION_ID").ok();
    let user = std::env::var("USER").unwrap_or_else(|_| "?".into());
    match session {
        Some(sid) => {
            let active = Command::new("loginctl")
                .args(["show-session", &sid, "-p", "Active", "--value"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim() == "yes")
                .unwrap_or(false);
            if active {
                Check {
                    name: "logind session",
                    status: Status::Ok,
                    detail: format!("session {} as user {} (Active=yes)", sid, user),
                }
            } else {
                Check {
                    name: "logind session",
                    status: Status::Warn,
                    detail: format!("session {} as user {} not active (SSH session?)", sid, user),
                }
            }
        }
        None => Check {
            name: "logind session",
            status: Status::Warn,
            detail: "XDG_SESSION_ID not set".into(),
        },
    }
}

fn check_hyprctl() -> Check {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    if signature.is_none() {
        return Check {
            name: "hyprctl",
            status: Status::Warn,
            detail: "not running under Hyprland".into(),
        };
    }
    let version = Command::new("hyprctl")
        .arg("version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(String::from))
        .unwrap_or_else(|| "unknown".into());
    Check {
        name: "hyprctl",
        status: Status::Ok,
        detail: format!("reachable ({})", version),
    }
}

fn check_zos_wm_ipc() -> Check {
    use crate::compositor::{default_socket_path, send, Request, Response};
    let path = default_socket_path();
    if !path.exists() {
        return Check {
            name: "zos-wm IPC",
            status: Status::Warn,
            detail: format!(
                "socket not present at {} (zos-wm not running)",
                path.display()
            ),
        };
    }
    match send(&Request::Version) {
        Ok(Response::Version { ipc, build }) => Check {
            name: "zos-wm IPC",
            status: Status::Ok,
            detail: format!("v{} (build {}) reachable", ipc, build),
        },
        Ok(other) => Check {
            name: "zos-wm IPC",
            status: Status::Warn,
            detail: format!("unexpected response: {:?}", other),
        },
        Err(e) => Check {
            name: "zos-wm IPC",
            status: Status::Fail,
            detail: format!("connect failed: {}", e),
        },
    }
}

fn check_config_dir() -> Check {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let path = Path::new(&home).join(".config/zos");
    if !path.exists() {
        match std::fs::create_dir_all(&path) {
            Ok(_) => Check {
                name: "~/.config/zos",
                status: Status::Ok,
                detail: format!("created {}", path.display()),
            },
            Err(e) => Check {
                name: "~/.config/zos",
                status: Status::Fail,
                detail: format!("cannot create {}: {}", path.display(), e),
            },
        }
    } else {
        let probe = path.join(".probe");
        match std::fs::write(&probe, "") {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                Check {
                    name: "~/.config/zos",
                    status: Status::Ok,
                    detail: "writable".into(),
                }
            }
            Err(e) => Check {
                name: "~/.config/zos",
                status: Status::Fail,
                detail: format!("not writable: {}", e),
            },
        }
    }
}

fn check_animations_toml() -> Check {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let path = Path::new(&home).join(".config/zos/animations.toml");
    if path.exists() {
        Check {
            name: "animations.toml",
            status: Status::Ok,
            detail: format!("found at {}", path.display()),
        }
    } else {
        Check {
            name: "animations.toml",
            status: Status::Warn,
            detail: "missing (using compositor defaults)".into(),
        }
    }
}

fn check_drm_outputs() -> Check {
    let entries = match std::fs::read_dir("/sys/class/drm") {
        Ok(e) => e,
        Err(e) => {
            return Check {
                name: "DRM outputs",
                status: Status::Fail,
                detail: format!("cannot read /sys/class/drm: {}", e),
            };
        }
    };
    let mut connected = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        // Card subdirs look like `card0-DP-1`, `card0-HDMI-A-1`, etc.
        if !name_str.contains('-') {
            continue;
        }
        let status_path = entry.path().join("status");
        if let Ok(content) = std::fs::read_to_string(&status_path) {
            if content.trim() == "connected" {
                // Strip the "card0-" prefix
                let label = name_str.split_once('-').map(|p| p.1).unwrap_or(&name_str);
                connected.push(label.to_string());
            }
        }
    }
    if connected.is_empty() {
        Check {
            name: "DRM outputs",
            status: Status::Warn,
            detail: "no connected outputs found".into(),
        }
    } else {
        connected.sort();
        Check {
            name: "DRM outputs",
            status: Status::Ok,
            detail: format!("{} connected ({})", connected.len(), connected.join(", ")),
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let checks = vec![
        check_wayland_session(),
        check_nvidia(),
        check_libseat(),
        check_hyprctl(),
        check_zos_wm_ipc(),
        check_config_dir(),
        check_animations_toml(),
        check_drm_outputs(),
    ];

    println!("zOS doctor");
    println!(
        "{}─────────────────────────────────────{}",
        ANSI_DIM, ANSI_RESET
    );
    for c in &checks {
        println!(
            "{}{}{} {}: {}",
            c.status.color(),
            c.status.glyph(),
            ANSI_RESET,
            c.name,
            c.detail,
        );
    }
    println!(
        "{}─────────────────────────────────────{}",
        ANSI_DIM, ANSI_RESET
    );
    let ok = checks.iter().filter(|c| c.status == Status::Ok).count();
    let warn = checks.iter().filter(|c| c.status == Status::Warn).count();
    let fail = checks.iter().filter(|c| c.status == Status::Fail).count();
    println!("{} ok / {} warning / {} fail", ok, warn, fail);

    if fail > 0 {
        std::process::exit(1);
    }
    Ok(())
}
