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
const GRUB_THEME_IMAGE: &str = "/usr/share/grub/themes/catppuccin-mocha";
const GRUB_THEME_BOOT: &str = "/boot/grub2/themes/catppuccin-mocha";
const THEME_START_MARKER: &str = "# zos-grub-theme-managed";
const THEME_END_MARKER: &str = "# zos-grub-theme-managed-end";

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

    let content = format!("set timeout={}\n", seconds);
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
                if line.starts_with("set timeout=") || line.starts_with("GRUB_TIMEOUT=") {
                    found = true;
                    format!("set timeout={}", seconds)
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

/// Install the Catppuccin Mocha GRUB theme to /boot and activate it via
/// /boot/grub2/user.cfg. Idempotent. Requires root.
pub fn apply_grub_theme() -> Result<()> {
    check_root()?;

    let image = Path::new(GRUB_THEME_IMAGE);
    let boot = Path::new(GRUB_THEME_BOOT);

    if !image.exists() {
        return Err(eyre!(
            "GRUB theme not installed in image at {}. \
             Was install-grub-theme.sh run during build?",
            GRUB_THEME_IMAGE
        ));
    }

    let needs_copy = !boot.join("theme.txt").exists();
    if needs_copy {
        copy_dir_recursive(image, boot)
            .wrap_err_with(|| format!("Failed to copy theme to {}", boot.display()))?;
    }

    write_user_cfg_theme_stanza()?;
    Ok(())
}

/// Walk `src` and copy every regular file/directory to `dst` (creating
/// intermediate directories). Overwrites existing files at the destination.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .wrap_err_with(|| format!("Failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src)
        .wrap_err_with(|| format!("Failed to read {}", src.display()))?
    {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            fs::copy(&from, &to)
                .wrap_err_with(|| format!("Failed to copy {}", from.display()))?;
        }
        // Skip symlinks/special files — theme is plain files.
    }
    Ok(())
}

/// Write the marker-delimited theme stanza into /boot/grub2/user.cfg,
/// replacing any existing zOS-managed block.
fn write_user_cfg_theme_stanza() -> Result<()> {
    let cfg_path = Path::new(GRUB_USER_CFG);
    let stanza_lines: [&str; 6] = [
        THEME_START_MARKER,
        "set theme=/boot/grub2/themes/catppuccin-mocha/theme.txt",
        "set gfxmode=auto",
        "insmod gfxterm",
        "terminal_output gfxterm",
        THEME_END_MARKER,
    ];
    let stanza = stanza_lines.join("\n") + "\n";

    if let Some(parent) = cfg_path.parent() {
        fs::create_dir_all(parent)
            .wrap_err("Failed to create GRUB config directory")?;
    }

    let new_content = if !cfg_path.exists() {
        stanza
    } else {
        let existing = fs::read_to_string(cfg_path)
            .wrap_err("Failed to read /boot/grub2/user.cfg")?;
        let lines: Vec<&str> = existing.lines().collect();
        if let Some(start_idx) = lines.iter().position(|l| *l == THEME_START_MARKER) {
            let end_relative = lines[start_idx..]
                .iter()
                .position(|l| *l == THEME_END_MARKER);
            let end_idx = match end_relative {
                Some(off) => start_idx + off,
                None => lines.len() - 1, // defensive: replace from start to EOF
            };
            let mut out: Vec<String> = lines[..start_idx]
                .iter()
                .map(|s| (*s).to_string())
                .collect();
            out.extend(stanza_lines.iter().map(|s| (*s).to_string()));
            out.extend(lines[end_idx + 1..].iter().map(|s| (*s).to_string()));
            let mut s = out.join("\n");
            if !s.ends_with('\n') {
                s.push('\n');
            }
            s
        } else {
            let mut s = existing;
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&stanza);
            s
        }
    };

    fs::write(cfg_path, new_content)
        .wrap_err("Failed to write /boot/grub2/user.cfg")?;
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
/// Requires root — use `reboot_to_windows_elevated()` from non-root contexts.
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

    logind_reboot();
    Ok(())
}

/// Set Windows as the next boot target using pkexec for privilege elevation.
/// Safe to call from non-root GUI/daemon contexts.
pub fn reboot_to_windows_elevated() -> Result<()> {
    let boot_num = get_windows_boot_num().ok_or_else(|| {
        eyre!(
            "No Windows Boot Manager found in EFI entries. \
             Run `sudo zos grub` to register a BLS entry for Windows."
        )
    })?;

    let output = Command::new("pkexec")
        .args(["efibootmgr", "--bootnext", &boot_num])
        .output()
        .wrap_err("Failed to spawn pkexec efibootmgr")?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_trim = stderr.trim();
        // pkexec exits 126 when authorization can't proceed (no agent / declined).
        // 127 when the helper is missing entirely.
        if code == 126 || code == 127
            || stderr_trim.contains("Authorization required")
            || stderr_trim.contains("polkit-agent")
            || stderr_trim.contains("Cancelled")
        {
            return Err(eyre!(
                "polkit did not authorize efibootmgr (exit {code}). \
                 Is the polkit agent running? Try: \
                 `systemctl --user status hyprpolkitagent`. \
                 stderr: {stderr_trim}"
            ));
        }
        return Err(eyre!(
            "pkexec efibootmgr --bootnext failed (exit {code}): {stderr_trim}"
        ));
    }

    logind_reboot();
    Ok(())
}

/// Set EFI BootNext to the given boot number using pkexec for elevation.
/// Does NOT reboot.
pub fn set_bootnext_only_elevated(boot_num: &str) -> Result<()> {
    if boot_num.len() != 4 || !boot_num.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(eyre!("Invalid boot number {:?}: must be 4 hex digits", boot_num));
    }
    let output = Command::new("pkexec")
        .args(["efibootmgr", "--bootnext", boot_num])
        .output()
        .wrap_err("Failed to spawn pkexec efibootmgr")?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "pkexec efibootmgr --bootnext {} failed (exit {}): {}",
            boot_num,
            code,
            stderr.trim()
        ));
    }
    Ok(())
}

/// Convenience: set BootNext to Windows Boot Manager (without rebooting).
pub fn set_bootnext_windows_only_elevated() -> Result<()> {
    let boot_num = get_windows_boot_num().ok_or_else(|| {
        eyre!(
            "No Windows Boot Manager found in EFI entries. \
             Run `sudo zos grub` first."
        )
    })?;
    set_bootnext_only_elevated(&boot_num)
}

/// Targets recognized by `set_persistent_boot_target_elevated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootTarget {
    Windows,
    /// The currently-running entry (typically Bazzite/Fedora). Resolved at
    /// call time via `BootCurrent: NNNN` in efibootmgr output.
    CurrentSystem,
}

/// Read the current BootOrder as a Vec of 4-hex-digit strings (in order).
pub fn get_boot_order() -> Result<Vec<String>> {
    let output = Command::new("efibootmgr")
        .output()
        .wrap_err("Failed to run efibootmgr")?;
    if !output.status.success() {
        return Err(eyre!("efibootmgr exited non-zero"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("BootOrder:") {
            return Ok(rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_hexdigit()))
                .collect());
        }
    }
    Err(eyre!("BootOrder not found in efibootmgr output"))
}

/// Read BootCurrent (the entry that booted this session) as a 4-hex-digit string.
pub fn get_boot_current() -> Option<String> {
    let output = Command::new("efibootmgr").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("BootCurrent:") {
            let s = rest.trim();
            if s.len() == 4 && s.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Move `boot_num` to the front of the EFI BootOrder (idempotent).
/// Uses pkexec for elevation. Does NOT reboot.
pub fn set_boot_order_first_elevated(boot_num: &str) -> Result<Vec<String>> {
    if boot_num.len() != 4 || !boot_num.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(eyre!("Invalid boot number {:?}", boot_num));
    }
    let mut order = get_boot_order()?;
    order.retain(|n| n != boot_num);
    order.insert(0, boot_num.to_string());
    let order_arg = order.join(",");

    let output = Command::new("pkexec")
        .args(["efibootmgr", "-o", &order_arg])
        .output()
        .wrap_err("Failed to spawn pkexec efibootmgr -o")?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "pkexec efibootmgr -o {} failed (exit {}): {}",
            order_arg,
            code,
            stderr.trim()
        ));
    }
    Ok(order)
}

/// Make `target` the persistent default boot entry by moving it to the
/// front of the EFI BootOrder. Returns the new order.
pub fn set_persistent_boot_target_elevated(target: BootTarget) -> Result<Vec<String>> {
    let boot_num = match target {
        BootTarget::Windows => get_windows_boot_num().ok_or_else(|| {
            eyre!(
                "No Windows Boot Manager found in EFI entries. \
                 Run `sudo zos grub` first."
            )
        })?,
        BootTarget::CurrentSystem => get_boot_current().ok_or_else(|| {
            eyre!("Could not determine current boot entry from efibootmgr")
        })?,
    };
    set_boot_order_first_elevated(&boot_num)
}

fn logind_reboot() {
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
        // Support both syntaxes: "set timeout=N" (correct) and "GRUB_TIMEOUT=N" (legacy)
        if let Some(val) = line.strip_prefix("set timeout=") {
            return val.trim().parse().ok();
        }
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
