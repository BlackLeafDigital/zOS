// === main.rs — zos entry point ===
//
// CLI-first system management tool for zOS.
// No subcommand -> TUI dashboard.
// migrate --auto -> silent migration (no TUI, for systemd).

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
    /// Check and apply updates across OS, Flatpak, custom packages, Brew, and mise
    Update {
        /// Only check for available updates; don't apply
        #[arg(long)]
        check: bool,
        /// Restrict to one source: os | flatpak | custom | brew | mise
        #[arg(long, value_name = "SOURCE")]
        only: Option<String>,
    },
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
    /// Set Windows as next boot target and reboot
    RebootToWindows,
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
                zos_core::commands::migrate::run_auto_migrate()
            } else if apply {
                // Apply all then show result in TUI
                let mut actions = zos_core::commands::migrate::plan_migrations();
                if actions.is_empty() {
                    println!("All configs are up to date. Nothing to migrate.");
                    Ok(())
                } else {
                    zos_core::commands::migrate::apply_migrations(&mut actions)?;
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
        Some(Commands::Setup) => {
            if zos_core::commands::setup::is_root() {
                eprintln!("Error: 'zos setup' must not run as root. Run as your normal user.");
                std::process::exit(1);
            }
            tui::run(tui::View::Setup)
        }
        Some(Commands::Update { check, only }) => match only {
            Some(s) => zos_core::commands::update::run_one(&s, check),
            None => zos_core::commands::update::run_all(check),
        },
        Some(Commands::Search { name }) => zos_core::commands::install::search_and_print(&name),
        Some(Commands::Install { name }) => zos_core::commands::install::search_and_install(&name),
        Some(Commands::RebootToWindows) => zos_core::commands::grub::reboot_to_windows_elevated(),
    }
}
