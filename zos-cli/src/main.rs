// === main.rs — zos entry point ===
//
// CLI-first system management tool for zOS.
// No subcommand -> TUI dashboard.
// migrate --auto -> silent migration (no TUI, for systemd).

mod compositor;
mod doctor;
mod screenshot;
mod tui;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, eyre};

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
    /// Talk to the zos-wm compositor over its IPC socket
    Compositor {
        #[command(subcommand)]
        cmd: CompositorCmd,
    },
    /// Take a screenshot via grim.
    Screenshot {
        /// Use slurp to select a region first.
        #[arg(long)]
        region: bool,
        /// Also copy to clipboard via wl-copy.
        #[arg(long)]
        copy: bool,
        /// Capture only the specified output (e.g., DP-1).
        #[arg(long)]
        output: Option<String>,
        /// Suppress the "Saved: ..." path output.
        #[arg(long)]
        quiet: bool,
    },
}

#[derive(Subcommand)]
enum CompositorCmd {
    /// Show zos-wm IPC + build version
    Version,
    /// List workspaces across all outputs
    Workspaces {
        /// Emit raw JSON instead of a human-readable table
        #[arg(long)]
        json: bool,
        /// Re-run continuously every <MS> milliseconds (default 1000); Ctrl-C to quit
        #[arg(long, value_name = "MS", num_args = 0..=1, default_missing_value = "1000")]
        watch: Option<u64>,
    },
    /// List windows across all workspaces
    Windows {
        /// Emit raw JSON instead of a human-readable table
        #[arg(long)]
        json: bool,
        /// Re-run continuously every <MS> milliseconds (default 1000); Ctrl-C to quit
        #[arg(long, value_name = "MS", num_args = 0..=1, default_missing_value = "1000")]
        watch: Option<u64>,
    },
    /// List connected monitors
    Monitors {
        /// Emit raw JSON instead of a human-readable table
        #[arg(long)]
        json: bool,
        /// Re-run continuously every <MS> milliseconds (default 1000); Ctrl-C to quit
        #[arg(long, value_name = "MS", num_args = 0..=1, default_missing_value = "1000")]
        watch: Option<u64>,
    },
    /// Show the currently focused window
    Active {
        /// Emit raw JSON instead of a human-readable summary
        #[arg(long)]
        json: bool,
        /// Re-run continuously every <MS> milliseconds (default 1000); Ctrl-C to quit
        #[arg(long, value_name = "MS", num_args = 0..=1, default_missing_value = "1000")]
        watch: Option<u64>,
    },
    /// Switch the focused output to workspace <id>
    Switch {
        /// Workspace id
        id: u32,
    },
    /// Move the focused window to workspace <id>
    MoveToWorkspace {
        /// Target workspace id
        id: u32,
    },
    /// Focus a window by its WindowId
    FocusWindow {
        /// Window id
        id: u32,
    },
    /// Close the currently focused window
    CloseFocused,
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
        Some(Commands::Doctor) => doctor::run().map_err(|e| eyre!(e.to_string())),
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
        Some(Commands::Compositor { cmd }) => run_compositor(cmd).map_err(|e| eyre!(e.to_string())),
        Some(Commands::Screenshot {
            region,
            copy,
            output,
            quiet,
        }) => {
            let opts = screenshot::ScreenshotOpts {
                region,
                copy,
                output: output.as_deref(),
                quiet,
            };
            screenshot::run(opts).map_err(|e| eyre!(e.to_string()))
        }
    }
}

fn run_compositor(cmd: CompositorCmd) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        CompositorCmd::Version => compositor::cmd_version(),
        CompositorCmd::Workspaces { json, watch } => match watch {
            Some(interval_ms) => compositor::watch_loop(interval_ms, move || {
                compositor::cmd_workspaces(json)
            }),
            None => compositor::cmd_workspaces(json),
        },
        CompositorCmd::Windows { json, watch } => match watch {
            Some(interval_ms) => {
                compositor::watch_loop(interval_ms, move || compositor::cmd_windows(json))
            }
            None => compositor::cmd_windows(json),
        },
        CompositorCmd::Monitors { json, watch } => match watch {
            Some(interval_ms) => {
                compositor::watch_loop(interval_ms, move || compositor::cmd_monitors(json))
            }
            None => compositor::cmd_monitors(json),
        },
        CompositorCmd::Active { json, watch } => match watch {
            Some(interval_ms) => {
                compositor::watch_loop(interval_ms, move || compositor::cmd_active(json))
            }
            None => compositor::cmd_active(json),
        },
        CompositorCmd::Switch { id } => compositor::cmd_switch(id),
        CompositorCmd::MoveToWorkspace { id } => compositor::cmd_move_to_workspace(id),
        CompositorCmd::FocusWindow { id } => compositor::cmd_focus_window(id),
        CompositorCmd::CloseFocused => compositor::cmd_close_focused(),
    }
}
