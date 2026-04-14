// === commands/install.rs — Search and install packages across sources ===

use color_eyre::eyre::Result;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
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
    Custom,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Flatpak => write!(f, "Flatpak"),
            Source::Brew => write!(f, "Brew"),
            Source::Mise => write!(f, "mise"),
            Source::Custom => write!(f, "Custom"),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FlatpakOverrides {
    pub app_id: String,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub filesystems: Vec<String>,
    #[serde(default)]
    pub sockets: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CustomPackage {
    pub name: String,
    pub description: String,
    pub search_terms: Vec<String>,
    pub install_type: String,
    #[serde(default)]
    pub github_repo: Option<String>,
    #[serde(default)]
    pub asset_pattern: Option<String>,
    #[serde(default)]
    pub flathub_app_id: Option<String>,
    pub flatpak_overrides: Option<FlatpakOverrides>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

pub fn load_custom_packages() -> Vec<CustomPackage> {
    let path = std::path::Path::new("/usr/share/zos/custom-packages.json");
    if let Ok(data) = std::fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Installed-package manifest
//
// Tracks which custom packages (AppImage / github-flatpak / flathub) have been
// installed via `zos install`, along with the release tag for github sources.
// Consumed by `zos update` to decide what needs a refresh.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstalledEntry {
    pub name: String,
    pub install_type: String,
    pub tag: Option<String>,
    pub installed_at: String,
}

pub fn manifest_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            format!("{home}/.local/share")
        });
    let dir = PathBuf::from(base).join("zos");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("custom-installed.json")
}

pub fn load_manifest() -> HashMap<String, InstalledEntry> {
    let path = manifest_path();
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn save_manifest(map: &HashMap<String, InstalledEntry>) {
    let path = manifest_path();
    if let Ok(data) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(path, data);
    }
}

pub(crate) fn record_install(pkg: &CustomPackage, tag: Option<&str>) {
    let mut map = load_manifest();
    map.insert(
        slugify(&pkg.name),
        InstalledEntry {
            name: pkg.name.clone(),
            install_type: pkg.install_type.clone(),
            tag: tag.map(|s| s.to_string()),
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    save_manifest(&map);
}

fn search_custom(query: &str, packages: &[CustomPackage]) -> Vec<PackageResult> {
    let q = query.to_lowercase();
    packages
        .iter()
        .filter(|p| {
            p.name.to_lowercase().contains(&q)
                || p.search_terms.iter().any(|t| t.to_lowercase().contains(&q))
        })
        .map(|p| PackageResult {
            source: Source::Custom,
            name: p.name.clone(),
            description: p.description.clone(),
            install_cmd: format!("__custom__{}", p.name),
        })
        .collect()
}

/// Resolve the latest GitHub release and find the download URL for a matching asset.
///
/// Tries `/releases/latest` first (the strict endpoint that only returns
/// non-prerelease releases). If that 404s or has no matching asset, falls back
/// to `/releases` (which includes prereleases, sorted newest first) and scans
/// until a release with a matching asset is found. This is needed for repos
/// like `ratdoux/OrcaSlicer-FullSpectrum` where every release is marked as a
/// prerelease, so the strict endpoint always returns 404.
pub fn resolve_github_release(repo: &str, asset_pattern: &str) -> Result<(String, String)> {
    if let Ok(result) = try_resolve_release(
        &format!("https://api.github.com/repos/{repo}/releases/latest"),
        asset_pattern,
        false,
    ) {
        return Ok(result);
    }

    try_resolve_release(
        &format!("https://api.github.com/repos/{repo}/releases"),
        asset_pattern,
        true,
    )
    .map_err(|e| {
        color_eyre::eyre::eyre!("No release with asset matching '{asset_pattern}' in {repo}: {e}")
    })
}

/// Hit a GitHub releases API URL and find the first release whose assets
/// include one matching `asset_pattern`. `is_array` controls whether the
/// response is a single release object (`/releases/latest`) or an array of
/// releases (`/releases`).
fn try_resolve_release(
    api_url: &str,
    asset_pattern: &str,
    is_array: bool,
) -> Result<(String, String)> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github.v3+json",
            api_url,
        ])
        .output()?;

    if !output.status.success() {
        return Err(color_eyre::eyre::eyre!("GitHub API call failed: {api_url}"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let releases: Vec<&serde_json::Value> = if is_array {
        json.as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default()
    } else {
        vec![&json]
    };

    let pattern_lower = asset_pattern.to_lowercase();
    let pattern_parts: Vec<&str> = pattern_lower.split('*').collect();

    for release in releases {
        let tag = release
            .get("tag_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let assets = release.get("assets").and_then(|v| v.as_array());
        if let Some(assets) = assets {
            for asset in assets {
                if let Some(name) = asset.get("name").and_then(|v| v.as_str()) {
                    let name_lower = name.to_lowercase();
                    let matches = pattern_parts.iter().all(|part| {
                        if part.is_empty() {
                            return true;
                        }
                        name_lower.contains(part)
                    });
                    if matches {
                        if let Some(url) =
                            asset.get("browser_download_url").and_then(|v| v.as_str())
                        {
                            return Ok((tag, url.to_string()));
                        }
                    }
                }
            }
        }
    }

    Err(color_eyre::eyre::eyre!(
        "No asset matching '{asset_pattern}' found"
    ))
}

pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Apply sensible XDG filesystem overrides so flatpak apps can open/save files
/// in the user's standard directories (Downloads, Documents, etc.) without
/// needing `--filesystem=home`. Called after every flatpak install.
pub(crate) fn apply_xdg_overrides(app_id: &str) {
    for arg in &[
        "--filesystem=xdg-download",
        "--filesystem=xdg-documents",
        "--filesystem=xdg-pictures",
        "--filesystem=xdg-videos",
        "--filesystem=xdg-music",
    ] {
        let _ = Command::new("flatpak")
            .args(["override", "--user", arg, app_id])
            .status();
    }
}

/// Apply the env/filesystem/socket overrides declared in a CustomPackage.
pub(crate) fn apply_flatpak_overrides(overrides: &FlatpakOverrides) {
    for fs in &overrides.filesystems {
        let _ = Command::new("flatpak")
            .args([
                "override",
                "--user",
                &format!("--filesystem={}", fs),
                &overrides.app_id,
            ])
            .status();
    }
    for sock in &overrides.sockets {
        let _ = Command::new("flatpak")
            .args([
                "override",
                "--user",
                &format!("--socket={}", sock),
                &overrides.app_id,
            ])
            .status();
    }
    for (key, value) in &overrides.env {
        let env_arg = format!("--env={}={}", key, value);
        let _ = Command::new("flatpak")
            .args(["override", "--user", &env_arg, &overrides.app_id])
            .status();
    }
}

/// Install a custom package from the registered source.
pub fn install_custom_package(pkg: &CustomPackage) -> Result<()> {
    match pkg.install_type.as_str() {
        "github-flatpak" => {
            let repo = pkg.github_repo.as_deref().ok_or_else(|| {
                color_eyre::eyre::eyre!("github-flatpak install_type requires github_repo")
            })?;
            let pattern = pkg.asset_pattern.as_deref().ok_or_else(|| {
                color_eyre::eyre::eyre!("github-flatpak install_type requires asset_pattern")
            })?;
            let (tag, url) = resolve_github_release(repo, pattern)?;
            println!("Found {} {} — downloading...", pkg.name, tag);

            let tmp = "/tmp/zos-custom-install.flatpak";
            let dl_status = Command::new("curl")
                .args(["-fsSL", "-o", tmp, &url])
                .status()?;
            if !dl_status.success() {
                let _ = std::fs::remove_file(tmp);
                return Err(color_eyre::eyre::eyre!("Failed to download {}", url));
            }
            let install_status = Command::new("flatpak")
                .args(["install", "--user", "-y", tmp])
                .status()?;
            let _ = std::fs::remove_file(tmp);
            if !install_status.success() {
                return Err(color_eyre::eyre::eyre!("flatpak install failed"));
            }
            if let Some(overrides) = &pkg.flatpak_overrides {
                apply_xdg_overrides(&overrides.app_id);
                apply_flatpak_overrides(overrides);
            }
            record_install(pkg, Some(&tag));
            println!("{} {} installed.", pkg.name, tag);
        }
        "github-appimage" => {
            let repo = pkg.github_repo.as_deref().ok_or_else(|| {
                color_eyre::eyre::eyre!("github-appimage install_type requires github_repo")
            })?;
            let pattern = pkg.asset_pattern.as_deref().ok_or_else(|| {
                color_eyre::eyre::eyre!("github-appimage install_type requires asset_pattern")
            })?;
            let (tag, url) = resolve_github_release(repo, pattern)?;
            println!("Found {} {} — downloading...", pkg.name, tag);

            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            let slug = slugify(&pkg.name);
            let bin_dir = format!("{home}/.local/bin");
            let _ = std::fs::create_dir_all(&bin_dir);
            let bin_path = format!("{bin_dir}/{slug}");

            let dl_status = Command::new("curl")
                .args(["-fsSL", "-o", &bin_path, &url])
                .status()?;
            if !dl_status.success() {
                let _ = std::fs::remove_file(&bin_path);
                return Err(color_eyre::eyre::eyre!("Failed to download {}", url));
            }
            let _ = Command::new("chmod").args(["+x", &bin_path]).status();

            // Create .desktop launcher entry (with env overrides if configured)
            let exec_line = if let Some(env_vars) = &pkg.env {
                let env_prefix: String = env_vars
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("env {} {}", env_prefix, bin_path)
            } else {
                bin_path.clone()
            };
            let desktop_dir = format!("{home}/.local/share/applications");
            let _ = std::fs::create_dir_all(&desktop_dir);
            let desktop_content = format!(
                "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon=application-x-executable\nCategories=Graphics;3DGraphics;\nComment={}\n",
                pkg.name, exec_line, pkg.description
            );
            let _ = std::fs::write(format!("{desktop_dir}/{slug}.desktop"), desktop_content);
            record_install(pkg, Some(&tag));
            println!("{} {} installed.", pkg.name, tag);
        }
        "flathub" => {
            let app_id = pkg.flathub_app_id.as_deref().ok_or_else(|| {
                color_eyre::eyre::eyre!("flathub install_type requires flathub_app_id")
            })?;
            println!("Installing {} from Flathub ({})...", pkg.name, app_id);

            let install_status = Command::new("flatpak")
                .args(["install", "--user", "-y", "flathub", app_id])
                .status()?;
            if !install_status.success() {
                return Err(color_eyre::eyre::eyre!("flatpak install failed"));
            }

            apply_xdg_overrides(app_id);
            if let Some(overrides) = &pkg.flatpak_overrides {
                apply_flatpak_overrides(overrides);
            }
            record_install(pkg, None);
            println!(
                "\x1b[32m✓\x1b[0m {} installed with XDG filesystem overrides applied.",
                pkg.name
            );
        }
        other => {
            return Err(color_eyre::eyre::eyre!("Unknown install type: {other}"));
        }
    }

    Ok(())
}

/// Search all package sources for the given name.
pub fn search(query: &str) -> Vec<PackageResult> {
    let mut results = Vec::new();

    // Search custom packages registry
    let custom_packages = load_custom_packages();
    results.extend(search_custom(query, &custom_packages));

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

    let custom: Vec<_> = results
        .iter()
        .filter(|r| r.source == Source::Custom)
        .collect();
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

    if !custom.is_empty() {
        println!("\x1b[1;35m── Custom (zOS curated) ──\x1b[0m");
        for r in &custom {
            println!("  {} — {}", r.name, r.description);
        }
        println!();
    }

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
    for source in &[Source::Custom, Source::Mise, Source::Flatpak, Source::Brew] {
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

    // Handle custom packages separately (they have their own install flow)
    if chosen.source == Source::Custom {
        let custom_packages = load_custom_packages();
        if let Some(pkg) = custom_packages.iter().find(|p| p.name == chosen.name) {
            return install_custom_package(pkg);
        }
        println!("Custom package not found in registry.");
        return Ok(());
    }

    // Flatpak source: use --user -y and apply sensible XDG overrides so
    // downloads, file pickers, and drag-and-drop work without the user
    // needing to know flatpak permissions exist.
    if chosen.source == Source::Flatpak {
        let app_id = chosen
            .install_cmd
            .split_whitespace()
            .last()
            .unwrap_or("")
            .to_string();
        if app_id.is_empty() {
            return Err(color_eyre::eyre::eyre!(
                "Could not parse flatpak app id from install_cmd"
            ));
        }
        println!(
            "\x1b[1mInstalling via Flatpak:\x1b[0m flatpak install --user -y flathub {}",
            app_id
        );
        println!();
        let status = Command::new("flatpak")
            .args(["install", "--user", "-y", "flathub", &app_id])
            .status()?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        apply_xdg_overrides(&app_id);
        println!(
            "\x1b[32m✓\x1b[0m {} installed with XDG filesystem overrides applied.",
            app_id
        );
        return Ok(());
    }

    println!(
        "\x1b[1mInstalling via {}:\x1b[0m {}",
        chosen.source, chosen.install_cmd
    );
    println!();

    // Execute the install command (brew, mise)
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

pub(crate) fn find_brew() -> Option<String> {
    let paths = [
        format!(
            "{}/.linuxbrew/bin/brew",
            std::env::var("HOME").unwrap_or_default()
        ),
        "/home/linuxbrew/.linuxbrew/bin/brew".to_string(),
    ];
    paths.into_iter().find(|p| std::path::Path::new(p).exists())
}

pub(crate) fn find_mise() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let paths = [
        format!("{}/.local/bin/mise", home),
        "/usr/bin/mise".to_string(),
    ];
    paths.into_iter().find(|p| std::path::Path::new(p).exists())
}
