// === commands/setup.rs — First-login setup management ===

use crate::config;
use color_eyre::eyre::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SetupStep {
    pub name: String,
    pub description: String,
    pub installed: bool,
    #[allow(dead_code)]
    pub check_cmd: Option<Vec<String>>,
    pub install_cmd: Vec<String>,
}

/// Build PATH that includes user-space tool locations.
fn full_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let existing = std::env::var("PATH").unwrap_or_default();
    format!(
        "{}/.local/share/mise/shims:{}/.local/bin:/home/linuxbrew/.linuxbrew/bin:{}",
        home, home, existing
    )
}

/// Returns true if running as root.
pub fn is_root() -> bool {
    std::env::var("USER").map(|u| u == "root").unwrap_or(false)
        || std::env::var("EUID").map(|u| u == "0").unwrap_or(false)
}

/// Check which setup steps are complete and which are pending.
pub fn get_setup_steps() -> Vec<SetupStep> {
    vec![
        SetupStep {
            name: "Homebrew".into(),
            description: "Linuxbrew package manager for user-space CLI tools".into(),
            installed: check_command_exists("brew"),
            check_cmd: Some(vec!["brew".into(), "--version".into()]),
            install_cmd: vec![
                "bash".into(),
                "-c".into(),
                "NONINTERACTIVE=1 /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
                    .into(),
            ],
        },
        SetupStep {
            name: "mise".into(),
            description: "Polyglot runtime manager (replaces asdf)".into(),
            installed: check_command_exists("mise"),
            check_cmd: Some(vec!["mise".into(), "--version".into()]),
            install_cmd: vec![
                "bash".into(),
                "-c".into(),
                "curl https://mise.run | sh".into(),
            ],
        },
        SetupStep {
            name: "Node LTS".into(),
            description: "Node.js LTS via mise".into(),
            installed: check_command_exists("node"),
            check_cmd: Some(vec!["node".into(), "--version".into()]),
            install_cmd: vec!["mise".into(), "use".into(), "-g".into(), "node@lts".into()],
        },
        SetupStep {
            name: "Python".into(),
            description: "Python 3 via mise".into(),
            installed: check_mise_installed("python"),
            check_cmd: Some(vec!["mise".into(), "which".into(), "python".into()]),
            install_cmd: vec!["mise".into(), "use".into(), "-g".into(), "python@latest".into()],
        },
        SetupStep {
            name: "pnpm".into(),
            description: "Fast, disk-efficient Node package manager".into(),
            installed: check_command_exists("pnpm"),
            check_cmd: Some(vec!["pnpm".into(), "--version".into()]),
            install_cmd: vec![
                "bash".into(),
                "-c".into(),
                "curl -fsSL https://get.pnpm.io/install.sh | sh -".into(),
            ],
        },
        SetupStep {
            name: "uv".into(),
            description: "Fast Python package manager".into(),
            installed: check_command_exists("uv"),
            check_cmd: Some(vec!["uv".into(), "--version".into()]),
            install_cmd: vec![
                "bash".into(),
                "-c".into(),
                "curl -LsSf https://astral.sh/uv/install.sh | sh".into(),
            ],
        },
        SetupStep {
            name: "Rust".into(),
            description: "Rust toolchain via rustup".into(),
            installed: check_command_exists("rustc"),
            check_cmd: Some(vec!["rustc".into(), "--version".into()]),
            install_cmd: vec![
                "bash".into(),
                "-c".into(),
                "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y".into(),
            ],
        },
        SetupStep {
            name: "GitHub CLI".into(),
            description: "gh — GitHub command-line tool".into(),
            installed: check_command_exists("gh"),
            check_cmd: Some(vec!["gh".into(), "--version".into()]),
            install_cmd: vec!["brew".into(), "install".into(), "gh".into()],
        },
        SetupStep {
            name: "Zsh Default".into(),
            description: "Set zsh as default shell".into(),
            installed: is_zsh_default(),
            check_cmd: None,
            install_cmd: vec!["chsh".into(), "-s".into(), "/usr/bin/zsh".into()],
        },
    ]
}

/// Execute a single setup step with inherited stdio and full PATH.
pub fn run_setup_step(step: &SetupStep) -> Result<()> {
    if step.install_cmd.is_empty() {
        return Ok(());
    }

    let (program, args) = step.install_cmd.split_first().unwrap();
    let status = Command::new(program)
        .args(args)
        .env("PATH", full_path())
        .status()
        .wrap_err_with(|| format!("Failed to run setup step: {}", step.name))?;

    if !status.success() {
        color_eyre::eyre::bail!(
            "Setup step '{}' failed (exit {})",
            step.name,
            status.code().unwrap_or(-1),
        );
    }

    Ok(())
}

/// Mark first-login setup as complete.
pub fn mark_setup_done() -> Result<()> {
    let marker = config::expand_home(config::SETUP_DONE_REL);
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&marker, "done\n").wrap_err("Failed to write setup-done marker")?;
    Ok(())
}

// --- Internal helpers ---

fn check_command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .env("PATH", full_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn check_mise_installed(tool: &str) -> bool {
    Command::new("mise")
        .args(["which", tool])
        .env("PATH", full_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_zsh_default() -> bool {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() {
        return false;
    }
    std::fs::read_to_string("/etc/passwd")
        .map(|s| {
            s.lines()
                .any(|l| l.starts_with(&format!("{}:", user)) && l.ends_with("/zsh"))
        })
        .unwrap_or(false)
}

