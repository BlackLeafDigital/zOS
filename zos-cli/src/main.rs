// === main.rs — zos entry point ===
//
// CLI-first system management tool for zOS.
// No subcommand -> TUI dashboard.
// migrate --auto -> silent migration (no TUI, for systemd).

mod commands;
mod config;
mod tui;

use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;

#[derive(Parser)]
#[command(name = "zos", about = "zOS system management tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show system status and config versions
    Status,
    /// Migrate configs to latest version
    Migrate {
        /// Run silently without TUI (for systemd service)
        #[arg(long)]
        auto: bool,
        /// Apply all pending migrations immediately
        #[arg(long)]
        apply: bool,
    },
    /// Run system health diagnostics
    Doctor,
    /// Manage GRUB bootloader settings
    Grub,
    /// Run first-login setup steps
    Setup,
    /// Check and apply OS updates
    Update,
    /// Search for packages across Flatpak, Brew, and mise
    Search {
        /// Package name to search for
        name: String,
    },
    /// Install a package from the best available source
    Install {
        /// Package name to install
        name: String,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        None => {
            // No subcommand -> TUI dashboard
            tui::run(tui::View::Dashboard)
        }
        Some(Commands::Status) => tui::run(tui::View::Dashboard),
        Some(Commands::Migrate { auto, apply }) => {
            if auto {
                // Silent mode — no TUI, just run migrations and exit
                commands::migrate::run_auto_migrate()
            } else if apply {
                // Apply all then show result in TUI
                let mut actions = commands::migrate::plan_migrations();
                if actions.is_empty() {
                    println!("All configs are up to date. Nothing to migrate.");
                    Ok(())
                } else {
                    commands::migrate::apply_migrations(&mut actions)?;
                    let applied: Vec<&str> = actions
                        .iter()
                        .filter(|a| a.applied)
                        .map(|a| a.area.as_str())
                        .collect();
                    println!(
                        "Applied {} migration(s): {}",
                        applied.len(),
                        applied.join(", ")
                    );
                    Ok(())
                }
            } else {
                tui::run(tui::View::Migrate)
            }
        }
        Some(Commands::Doctor) => tui::run(tui::View::Doctor),
        Some(Commands::Grub) => tui::run(tui::View::Grub),
        Some(Commands::Setup) => tui::run(tui::View::Setup),
        Some(Commands::Update) => tui::run(tui::View::Update),
        Some(Commands::Search { name }) => commands::install::search_and_print(&name),
        Some(Commands::Install { name }) => commands::install::search_and_install(&name),
    }
}
