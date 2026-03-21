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
