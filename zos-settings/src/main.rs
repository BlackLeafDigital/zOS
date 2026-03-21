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

    // Parse our args first — clap consumes --tray before GTK sees it
    let cli = Cli::parse();

    // Start system tray on a background thread if requested
    let _tray_handle = if cli.tray {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        let handle = std::thread::spawn(move || {
            rt.block_on(async {
                match tray::run_tray().await {
                    Ok(handle) => {
                        tracing::info!("System tray started");
                        Some(handle)
                    }
                    Err(e) => {
                        tracing::error!("Failed to start system tray: {e}");
                        None
                    }
                }
            })
        });
        Some(handle)
    } else {
        None
    };

    // Filter args for GTK: keep program name, drop --tray (already consumed by clap),
    // pass through any GTK-recognized args like --display, --name, etc.
    let gtk_args: Vec<String> = std::env::args()
        .enumerate()
        .filter(|(_, arg)| arg != "--tray")
        .map(|(_, arg)| arg)
        .collect();

    let mut app = relm4::RelmApp::new("com.zos.Settings").with_args(gtk_args);
    if cli.tray {
        app = app.visible_on_activate(false);
    }
    relm4::set_global_css(include_str!("../resources/style.css"));
    app.run::<app::App>(());
}
