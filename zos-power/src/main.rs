//! zOS power menu — replacement for wlogout.
//!
//! Phase 6 in-progress: this is a scaffold. Layer-shell overlay +
//! actions (lock/logout/reboot/shutdown) land in follow-up tasks.

use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-power starting (scaffold)");
    println!("zos-power: scaffold (Phase 6 in progress)");
}
