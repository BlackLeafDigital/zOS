// === commands/grub.rs — GRUB bootloader management ===

use color_eyre::eyre::{eyre, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct GrubStatus {
    pub current_timeout: Option<u32>,
    pub windows_detected: bool,
    pub windows_path: Option<String>,
    pub bls_entry_exists: bool,
}

const GRUB_USER_CFG: &str = "/boot/grub2/user.cfg";
const BLS_DIR: &str = "/boot/loader/entries";

/// Read current GRUB configuration status.
pub fn get_grub_status() -> GrubStatus {
    let current_timeout = read_grub_timeout();
    let (windows_detected, windows_path) = detect_windows()
        .map(|p| (true, Some(p)))
        .unwrap_or((false, None));
    let bls_entry_exists = check_windows_bls_entry();

    GrubStatus {
        current_timeout,
        windows_detected,
        windows_path,
        bls_entry_exists,
    }
}

/// Set GRUB timeout in user.cfg (requires root).
pub fn apply_grub_timeout(seconds: u32) -> Result<()> {
    check_root()?;

    let content = format!("GRUB_TIMEOUT={}\n", seconds);
    let cfg_path = Path::new(GRUB_USER_CFG);

    if let Some(parent) = cfg_path.parent() {
        fs::create_dir_all(parent).wrap_err("Failed to create GRUB config directory")?;
    }

    // If user.cfg exists, update the timeout line; otherwise create it
    if cfg_path.exists() {
        let existing =
            fs::read_to_string(cfg_path).wrap_err("Failed to read existing GRUB user.cfg")?;
        let mut found = false;
        let updated: String = existing
            .lines()
            .map(|line| {
                if line.starts_with("GRUB_TIMEOUT=") {
                    found = true;
                    format!("GRUB_TIMEOUT={}", seconds)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let final_content = if found {
            format!("{}\n", updated)
        } else {
            format!("{}{}", existing, content)
        };
        fs::write(cfg_path, final_content).wrap_err("Failed to write GRUB user.cfg")?;
    } else {
        fs::write(cfg_path, content).wrap_err("Failed to create GRUB user.cfg")?;
    }

    Ok(())
}

/// Scan EFI partitions for Windows bootloader.
pub fn detect_windows() -> Option<String> {
    // Check common EFI paths for Windows Boot Manager
    let efi_paths = [
        "/boot/efi/EFI/Microsoft/Boot/bootmgfw.efi",
        "/efi/EFI/Microsoft/Boot/bootmgfw.efi",
    ];

    for path in &efi_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Try using efibootmgr to detect Windows
    if let Ok(output) = Command::new("efibootmgr").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("Windows Boot Manager") {
                    return Some("Windows Boot Manager (EFI)".to_string());
                }
            }
        }
    }

    None
}

/// Create a BLS entry for Windows in /boot/loader/entries/.
pub fn create_windows_bls() -> Result<()> {
    check_root()?;

    let windows_path = detect_windows().ok_or_else(|| eyre!("No Windows installation detected"))?;

    let bls_dir = Path::new(BLS_DIR);
    fs::create_dir_all(bls_dir).wrap_err("Failed to create BLS entries directory")?;

    let entry_path = bls_dir.join("windows.conf");
    let entry_content = format!(
        "title Windows\nefi /EFI/Microsoft/Boot/bootmgfw.efi\n# Detected at: {}\n",
        windows_path
    );

    fs::write(&entry_path, entry_content).wrap_err("Failed to write Windows BLS entry")?;

    Ok(())
}

/// Get the EFI boot entry number for Windows Boot Manager.
pub fn get_windows_boot_num() -> Option<String> {
    let output = Command::new("efibootmgr").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("Windows Boot Manager") {
            // Line format: "Boot0000* Windows Boot Manager"
            return line
                .strip_prefix("Boot")
                .and_then(|s| s.get(..4))
                .map(|s| s.to_string());
        }
    }
    None
}

/// Set Windows as the next boot target via EFI bootnext, then reboot.
pub fn reboot_to_windows() -> Result<()> {
    let boot_num = get_windows_boot_num()
        .ok_or_else(|| eyre!("No Windows Boot Manager found in EFI entries"))?;

    let status = Command::new("efibootmgr")
        .args(["--bootnext", &boot_num])
        .status()
        .wrap_err("Failed to run efibootmgr")?;

    if !status.success() {
        return Err(eyre!(
            "efibootmgr --bootnext failed (exit {})",
            status.code().unwrap_or(-1)
        ));
    }

    // Reboot via logind D-Bus
    let _ = Command::new("dbus-send")
        .args([
            "--system",
            "--print-reply",
            "--dest=org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager.Reboot",
            "boolean:true",
        ])
        .status();

    Ok(())
}

/// Check if we are running as root.
pub fn is_root() -> bool {
    // Read effective UID without depending on libc crate
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .unwrap_or(1000)
                == 0
        })
        .unwrap_or(false)
}

// --- Internal helpers ---

fn check_root() -> Result<()> {
    if !is_root() {
        Err(eyre!(
            "This operation requires root privileges. Run with sudo."
        ))
    } else {
        Ok(())
    }
}

fn read_grub_timeout() -> Option<u32> {
    let content = fs::read_to_string(GRUB_USER_CFG).ok()?;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("GRUB_TIMEOUT=") {
            return val.trim().parse().ok();
        }
    }
    None
}

fn check_windows_bls_entry() -> bool {
    let entry_path = Path::new(BLS_DIR).join("windows.conf");
    entry_path.exists()
}
