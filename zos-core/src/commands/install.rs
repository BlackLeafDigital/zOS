// === commands/install.rs — Search and install packages across sources ===

use color_eyre::eyre::Result;
use std::io::{self, BufRead, Write};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PackageResult {
    pub source: Source,
    pub name: String,
    pub description: String,
    pub install_cmd: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Flatpak,
    Brew,
    Mise,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Flatpak => write!(f, "Flatpak"),
            Source::Brew => write!(f, "Brew"),
            Source::Mise => write!(f, "mise"),
        }
    }
}

/// Search all package sources for the given name.
pub fn search(query: &str) -> Vec<PackageResult> {
    let mut results = Vec::new();

    // Search Flatpak
    if let Ok(output) = Command::new("flatpak")
        .args(["search", query, "--columns=application,name,description"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.splitn(3, '\t').collect();
                if parts.len() >= 2 {
                    let app_id = parts[0].trim();
                    let name = parts[1].trim();
                    let desc = parts.get(2).map(|s| s.trim()).unwrap_or("");
                    if !app_id.is_empty() {
                        results.push(PackageResult {
                            source: Source::Flatpak,
                            name: name.to_string(),
                            description: desc.to_string(),
                            install_cmd: format!("flatpak install flathub {}", app_id),
                        });
                    }
                }
            }
        }
    }

    // Search Brew (if installed)
    let brew_path = find_brew();
    if let Some(brew) = &brew_path {
        if let Ok(output) = Command::new(brew).args(["search", query]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let name = line.trim();
                    if !name.is_empty() && !name.starts_with("==>") {
                        results.push(PackageResult {
                            source: Source::Brew,
                            name: name.to_string(),
                            description: String::new(),
                            install_cmd: format!("brew install {}", name),
                        });
                    }
                }
            }
        }
    }

    // Search mise (if installed)
    if let Some(mise) = find_mise() {
        if let Ok(output) = Command::new(&mise).args(["registry"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let query_lower = query.to_lowercase();
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if let Some(name) = parts.first() {
                        if name.to_lowercase().contains(&query_lower) {
                            results.push(PackageResult {
                                source: Source::Mise,
                                name: name.to_string(),
                                description: "dev runtime".to_string(),
                                install_cmd: format!("mise install {}", name),
                            });
                        }
                    }
                }
            }
        }
    }

    results
}

/// Print search results grouped by source.
pub fn search_and_print(query: &str) -> Result<()> {
    let results = search(query);

    if results.is_empty() {
        println!("No results found for '{}'.", query);
        println!();
        println!("Try installing in a distrobox container:");
        println!("  distrobox enter -- sudo dnf install {}", query);
        return Ok(());
    }

    let flatpak: Vec<_> = results
        .iter()
        .filter(|r| r.source == Source::Flatpak)
        .collect();
    let brew: Vec<_> = results
        .iter()
        .filter(|r| r.source == Source::Brew)
        .collect();
    let mise: Vec<_> = results
        .iter()
        .filter(|r| r.source == Source::Mise)
        .collect();

    if !flatpak.is_empty() {
        println!("\x1b[1;34m── Flatpak (GUI apps) ──\x1b[0m");
        for r in &flatpak {
            if r.description.is_empty() {
                println!("  {} \x1b[2m→ {}\x1b[0m", r.name, r.install_cmd);
            } else {
                println!(
                    "  {} — {} \x1b[2m→ {}\x1b[0m",
                    r.name, r.description, r.install_cmd
                );
            }
        }
        println!();
    }

    if !mise.is_empty() {
        println!("\x1b[1;32m── mise (dev runtimes) ──\x1b[0m");
        for r in &mise {
            println!("  {} \x1b[2m→ {}\x1b[0m", r.name, r.install_cmd);
        }
        println!();
    }

    if !brew.is_empty() {
        println!("\x1b[1;33m── Brew (CLI tools) ──\x1b[0m");
        for r in &brew {
            println!("  {} \x1b[2m→ {}\x1b[0m", r.name, r.install_cmd);
        }
        println!();
    }

    Ok(())
}

/// Search and install a package, prompting if multiple sources match.
pub fn search_and_install(query: &str) -> Result<()> {
    let results = search(query);

    if results.is_empty() {
        println!("No results found for '{}'.", query);
        println!();
        println!("Try installing in a distrobox container:");
        println!("  distrobox enter -- sudo dnf install {}", query);
        return Ok(());
    }

    // Deduplicate sources — pick best match per source
    let mut by_source: Vec<PackageResult> = Vec::new();
    for source in &[Source::Mise, Source::Flatpak, Source::Brew] {
        if let Some(best) = results
            .iter()
            .filter(|r| &r.source == source)
            .find(|r| r.name.to_lowercase() == query.to_lowercase())
            .or_else(|| results.iter().find(|r| &r.source == source))
        {
            by_source.push(best.clone());
        }
    }

    let chosen = if by_source.len() == 1 {
        &by_source[0]
    } else {
        // Multiple sources — prompt user
        println!("'{}' found in multiple sources:\n", query);
        for (i, r) in by_source.iter().enumerate() {
            println!(
                "  [{}] {} — {} \x1b[2m({})\x1b[0m",
                i + 1,
                r.source,
                r.name,
                r.install_cmd
            );
        }
        println!();
        print!("Pick a source [1-{}]: ", by_source.len());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        let choice: usize = input.trim().parse().unwrap_or(0);

        if choice < 1 || choice > by_source.len() {
            println!("Cancelled.");
            return Ok(());
        }
        &by_source[choice - 1]
    };

    println!(
        "\x1b[1mInstalling via {}:\x1b[0m {}",
        chosen.source, chosen.install_cmd
    );
    println!();

    // Execute the install command
    let parts: Vec<&str> = chosen.install_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    let status = Command::new(parts[0]).args(&parts[1..]).status()?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn find_brew() -> Option<String> {
    let paths = [
        format!(
            "{}/.linuxbrew/bin/brew",
            std::env::var("HOME").unwrap_or_default()
        ),
        "/home/linuxbrew/.linuxbrew/bin/brew".to_string(),
    ];
    paths.into_iter().find(|p| std::path::Path::new(p).exists())
}

fn find_mise() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let paths = [
        format!("{}/.local/bin/mise", home),
        "/usr/bin/mise".to_string(),
    ];
    paths.into_iter().find(|p| std::path::Path::new(p).exists())
}
