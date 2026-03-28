// === main.rs — zos-settings entry point ===

mod app;
mod pages;
mod services;
mod widgets;

fn main() {
    tracing_subscriber::fmt::init();

    // Ensure GTK4 colors.css exists to suppress theme parser warning
    if let Ok(home) = std::env::var("HOME") {
        let gtk4_dir = std::path::Path::new(&home).join(".config/gtk-4.0");
        let colors_css = gtk4_dir.join("colors.css");
        if !colors_css.exists() {
            let _ = std::fs::create_dir_all(&gtk4_dir);
            let _ = std::fs::write(&colors_css, "/* zOS Catppuccin Mocha — auto-generated */\n");
        }
    }

    let app = relm4::RelmApp::new("com.zos.Settings");
    relm4::set_global_css(include_str!("../resources/style.css"));
    app.run::<app::App>(());
}
