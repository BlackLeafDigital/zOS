// === tray.rs — System tray icon for zOS Settings ===
//
// Provides a StatusNotifierItem tray using ksni.
// Left-click toggles the settings window (TODO), right-click
// shows power actions and a quit option.

use ksni::TrayMethods;

use crate::services::power;

#[derive(Debug)]
pub(crate) struct ZosTray;

impl ksni::Tray for ZosTray {
    fn id(&self) -> String {
        "zos-settings".into()
    }

    fn icon_name(&self) -> String {
        "preferences-system-symbolic".into()
    }

    fn title(&self) -> String {
        "zOS Settings".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // TODO: show/hide window
        println!("TODO: show/hide window");
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Open Settings".into(),
                activate: Box::new(|_| {
                    // TODO: show/hide window
                    println!("TODO: show/hide window");
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Suspend".into(),
                icon_name: "system-suspend-symbolic".into(),
                activate: Box::new(|_| power::suspend()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Reboot".into(),
                icon_name: "system-reboot-symbolic".into(),
                activate: Box::new(|_| power::reboot()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Shut Down".into(),
                icon_name: "system-shutdown-symbolic".into(),
                activate: Box::new(|_| power::shutdown()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Spawn the system tray icon. Returns the handle; the tray runs
/// until the handle is dropped or the process exits.
pub async fn run_tray() -> Result<ksni::Handle<ZosTray>, ksni::Error> {
    let tray = ZosTray;
    tray.spawn().await
}
