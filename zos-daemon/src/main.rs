// === zos-daemon — Background daemon for zOS ===
//
// Always-running process that provides:
// - System tray icon with power actions (absorbed from zos-tray)
// - PipeWire audio config enforcement (ensures bus routing persists)
//
// Uses GTK3 main loop for the tray + GLib timers for periodic tasks.

mod audio;
mod tray;

use std::cell::RefCell;

fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("zos-daemon starting");

    gtk::init().expect("failed to init GTK for zos-daemon");

    // Build the system tray icon
    tray::build_tray();

    // Start the audio config enforcer (checks every 5 seconds)
    let enforcer = RefCell::new(audio::AudioEnforcer::new());
    gtk::glib::timeout_add_seconds_local(5, move || {
        enforcer.borrow_mut().tick();
        gtk::glib::Continue(true)
    });

    tracing::info!("zos-daemon running");
    gtk::main();
}
