// === tray.rs — System tray icon with power actions ===

use gtk::prelude::*;
use libayatana_appindicator::{AppIndicator, AppIndicatorStatus};
use std::process::Command;

/// Build and register the system tray icon with its menu.
pub fn build_tray() {
    let mut indicator = AppIndicator::new("zos-daemon", "zos-settings-symbolic");
    indicator.set_status(AppIndicatorStatus::Active);
    indicator.set_title("zOS");

    let mut menu = gtk::Menu::new();

    let open_item = gtk::MenuItem::with_label("Open Settings");
    open_item.connect_activate(|_| {
        Command::new("zos-settings").spawn().ok();
    });
    menu.append(&open_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let suspend_item = gtk::MenuItem::with_label("Suspend");
    suspend_item.connect_activate(|_| logind_action("Suspend"));
    menu.append(&suspend_item);

    let reboot_item = gtk::MenuItem::with_label("Reboot");
    reboot_item.connect_activate(|_| logind_action("Reboot"));
    menu.append(&reboot_item);

    let windows_item = gtk::MenuItem::with_label("Reboot to Windows");
    windows_item.connect_activate(|_| {
        let _ = Command::new("pkexec")
            .args(["efibootmgr", "--bootnext", "0000"])
            .status();
        logind_action("Reboot");
    });
    menu.append(&windows_item);

    let shutdown_item = gtk::MenuItem::with_label("Shut Down");
    shutdown_item.connect_activate(|_| logind_action("PowerOff"));
    menu.append(&shutdown_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let quit_item = gtk::MenuItem::with_label("Quit");
    quit_item.connect_activate(|_| {
        std::process::exit(0);
    });
    menu.append(&quit_item);

    menu.show_all();
    indicator.set_menu(&mut menu);
}

fn logind_action(method: &str) {
    let _ = Command::new("dbus-send")
        .args([
            "--system",
            "--print-reply",
            "--dest=org.freedesktop.login1",
            "/org/freedesktop/login1",
            &format!("org.freedesktop.login1.Manager.{method}"),
            "boolean:true",
        ])
        .status();
}
