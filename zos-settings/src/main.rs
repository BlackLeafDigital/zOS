// === main.rs — zos-settings entry point ===
//
// GTK4/Adwaita settings application for zOS.
// Phase 1 scaffold: minimal window.

use libadwaita::prelude::*;
use libadwaita::{Application, ApplicationWindow, HeaderBar};
use gtk4::{Box, Label, Orientation};

fn main() {
    let app = Application::builder()
        .application_id("com.zos.Settings")
        .build();

    app.connect_activate(|app| {
        let content = Box::new(Orientation::Vertical, 0);

        let header = HeaderBar::new();
        content.append(&header);

        let label = Label::builder()
            .label("zOS Settings")
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        content.append(&label);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("zOS Settings")
            .default_width(800)
            .default_height(600)
            .content(&content)
            .build();

        window.present();
    });

    app.run();
}
