// === tray.rs — System tray icon via libayatana-appindicator ===
//
// Uses GTK3-based AppIndicator (same protocol as nm-applet, blueman).
// Runs on a separate thread from the GTK4 main app.

use gtk3::prelude::*;
use libayatana_appindicator::{AppIndicator, AppIndicatorStatus};

use crate::services::power;

/// Run the system tray. Blocks forever (calls gtk3::main).
/// Must be called from a dedicated thread — NOT the GTK4 main thread.
pub fn run_tray() {
    gtk3::init().expect("failed to init GTK3 for tray");

    let mut indicator = AppIndicator::new("zos-settings", "zos-settings");
    indicator.set_status(AppIndicatorStatus::Active);
    indicator.set_title("zOS Settings");

    let mut menu = gtk3::Menu::new();

    let open_item = gtk3::MenuItem::with_label("Open Settings");
    open_item.connect_activate(|_| {
        std::process::Command::new("zos-settings").spawn().ok();
    });
    menu.append(&open_item);

    menu.append(&gtk3::SeparatorMenuItem::new());

    let suspend_item = gtk3::MenuItem::with_label("Suspend");
    suspend_item.connect_activate(|_| power::suspend());
    menu.append(&suspend_item);

    let reboot_item = gtk3::MenuItem::with_label("Reboot");
    reboot_item.connect_activate(|_| power::reboot());
    menu.append(&reboot_item);

    let shutdown_item = gtk3::MenuItem::with_label("Shut Down");
    shutdown_item.connect_activate(|_| power::shutdown());
    menu.append(&shutdown_item);

    menu.append(&gtk3::SeparatorMenuItem::new());

    let quit_item = gtk3::MenuItem::with_label("Quit");
    quit_item.connect_activate(|_| {
        std::process::exit(0);
    });
    menu.append(&quit_item);

    menu.show_all();
    indicator.set_menu(&mut menu);

    gtk3::main();
}
