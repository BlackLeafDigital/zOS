use std::process::Command;

pub fn suspend() {
    logind_action("Suspend");
}

pub fn reboot() {
    logind_action("Reboot");
}

pub fn shutdown() {
    logind_action("PowerOff");
}

pub fn reboot_to_windows() {
    let _ = zos_core::commands::grub::reboot_to_windows_elevated();
}

fn logind_action(method: &str) {
    let _ = Command::new("dbus-send")
        .args([
            "--system",
            "--print-reply",
            "--dest=org.freedesktop.login1",
            "/org/freedesktop/login1",
            &format!("org.freedesktop.login1.Manager.{}", method),
            "boolean:true",
        ])
        .status();
}
