//! zOS notification daemon — replacement for swaync.
//!
//! Phase 6 in-progress: this is a scaffold. DBus
//! `org.freedesktop.Notifications` interface, history store, and
//! layer-shell popups land in follow-up tasks.

use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("zos-notify starting (scaffold)");
    println!("zos-notify: scaffold (Phase 6 in progress)");
}
