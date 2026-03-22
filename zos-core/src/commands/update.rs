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
