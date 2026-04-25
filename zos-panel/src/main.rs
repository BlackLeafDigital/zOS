//! zOS panel — top-bar replacement for HyprPanel.
//!
//! Phase 6 in-progress: this is a scaffold. Modules + iced_layershell
//! integration land in follow-up tasks.

use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-panel starting (scaffold)");
    println!("zos-panel: scaffold (Phase 6 in progress)");
}
