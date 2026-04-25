//! zOS monitor configuration — replacement for nwg-displays.
//!
//! Phase 6 in-progress: this is a scaffold. Drag-to-arrange canvas,
//! monitor probing via `Compositor::monitors()`, and Hyprland
//! `monitor=` config writeback land in follow-up tasks.

use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-monitors starting (scaffold)");
    println!("zos-monitors: scaffold (Phase 6 in progress)");
}
