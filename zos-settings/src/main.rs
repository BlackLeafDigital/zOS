// === main.rs — zos-settings entry point ===

mod app;
mod pages;
mod services;
mod tray;

use clap::Parser;

#[derive(Parser)]
#[command(name = "zos-settings", about = "zOS Settings")]
struct Cli {
    #[arg(long)]
    tray: bool,
}

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

    let cli = Cli::parse();

    // Start system tray on a background thread (GTK3-based, separate from GTK4 main thread)
    if cli.tray {
        std::thread::spawn(|| {
            tray::run_tray(); // blocks forever via gtk::main()
        });
    }

    // Filter --tray from args so GTK4 doesn't choke on it
    let gtk_args: Vec<String> = std::env::args()
        .filter(|arg| arg != "--tray")
        .collect();

    let mut app = relm4::RelmApp::new("com.zos.Settings").with_args(gtk_args);
    if cli.tray {
        app = app.visible_on_activate(false);
    }
    relm4::set_global_css(include_str!("../resources/style.css"));
    app.run::<app::App>(());
}
