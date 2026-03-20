// === commands/update.rs — OS update management via rpm-ostree ===

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

    let output = Command::new("rpm-ostree")
        .args(["upgrade", "--check"])
        .output()
        .wrap_err("Failed to run rpm-ostree upgrade --check")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // rpm-ostree returns exit 0 if update available, and prints info to stdout/stderr
    let combined = format!("{}\n{}", stdout, stderr);
    let pending = combined.contains("AvailableUpdate")
        || combined.contains("Available update")
        || combined.contains("Upgrading");

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
    let output = Command::new("rpm-ostree")
        .arg("upgrade")
        .output()
        .wrap_err("Failed to run rpm-ostree upgrade")?;

    Ok(output)
}

/// Get a reboot suggestion message.
pub fn reboot_message() -> &'static str {
    "Update applied. Please reboot to boot into the new deployment:\n  systemctl reboot"
}

// --- Internal helpers ---

fn get_current_image() -> String {
    let output = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout) {
                val.pointer("/deployments/0/container-image-reference")
                    .or_else(|| val.pointer("/deployments/0/origin"))
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
