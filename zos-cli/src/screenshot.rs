// === screenshot.rs — `zos screenshot` Wayland-native screenshot wrapper ===
//
// Wraps `grim` (capture) and `slurp` (region selector) to produce timestamped
// PNGs in ~/Pictures/Screenshots/. Optional `--copy` pipes the PNG bytes
// through `wl-copy` so the image lands on the Wayland clipboard as well.
//
// All external tools are probed with `--version` first so a missing dep
// produces a helpful error instead of a confusing exec failure.

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, Default)]
pub struct ScreenshotOpts<'a> {
    pub region: bool,
    pub copy: bool,
    pub output: Option<&'a str>,
    pub quiet: bool,
}

pub fn run(opts: ScreenshotOpts) -> Result<(), Box<dyn std::error::Error>> {
    // Verify grim is available
    if Command::new("grim").arg("--version").output().is_err() {
        return Err("grim is not installed (install: dnf install grim)".into());
    }

    let path = make_screenshot_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Build grim command
    let mut cmd = Command::new("grim");

    if opts.region {
        // Verify slurp is available
        if Command::new("slurp").arg("--version").output().is_err() {
            return Err("slurp is not installed (install: dnf install slurp)".into());
        }
        // Get region selection from slurp
        let slurp_out = Command::new("slurp")
            .output()
            .map_err(|e| format!("slurp failed: {}", e))?;
        if !slurp_out.status.success() {
            // User cancelled — exit cleanly
            return Ok(());
        }
        let region = String::from_utf8_lossy(&slurp_out.stdout).trim().to_string();
        if region.is_empty() {
            return Ok(()); // cancelled
        }
        cmd.arg("-g").arg(region);
    } else if let Some(output) = opts.output {
        cmd.arg("-o").arg(output);
    }

    cmd.arg(&path);

    let status = cmd.status().map_err(|e| format!("grim failed: {}", e))?;
    if !status.success() {
        return Err(format!("grim exited with status {}", status).into());
    }

    if opts.copy {
        // Pipe the file through wl-copy
        if Command::new("wl-copy").arg("--version").output().is_err() {
            // Saved successfully but couldn't copy
            if !opts.quiet {
                eprintln!("(warning) wl-copy not installed; skipping clipboard copy");
            }
        } else {
            let bytes = std::fs::read(&path)?;
            let mut child = Command::new("wl-copy")
                .arg("--type")
                .arg("image/png")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("wl-copy spawn failed: {}", e))?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(&bytes)?;
            }
            let _ = child.wait();
        }
    }

    if !opts.quiet {
        println!("Saved: {}", path.display());
    }
    Ok(())
}

fn make_screenshot_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
    let dir = PathBuf::from(&home).join("Pictures/Screenshots");
    let now = current_timestamp();
    let path = dir.join(format!("zos-{}.png", now));
    Ok(path)
}

fn current_timestamp() -> String {
    // Use system date command for ISO-ish formatting since we don't depend on chrono.
    // Fallback to UTC seconds if `date` isn't available.
    if let Ok(out) = Command::new("date").args(["+%Y-%m-%d_%H-%M-%S"]).output() {
        if let Ok(s) = String::from_utf8(out.stdout) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    // Fallback
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
