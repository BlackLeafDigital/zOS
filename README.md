# zOS

A curated Linux desktop OS that just works. Gaming, development, daily driving — all the good decisions already made.

Built on [Bazzite](https://bazzite.gg/) (Fedora Atomic), delivered as a bootable container image with atomic updates and automatic rollback.

## What's Included

### Desktop
- **KDE Plasma 6** — primary desktop environment, highly customizable
- **Hyprland** — optional tiling window manager, selectable at login screen
- **Catppuccin Mocha** — consistent theme across Hyprland, Waybar, Wezterm

### Terminal & Shell
- **Wezterm** — GPU-accelerated terminal with built-in multiplexer (tabs + splits, like iTerm2)
- **Zsh** — default shell with sane defaults
- **Starship** — fast cross-shell prompt with git/language context
- **Modern CLI tools** — `eza`, `bat`, `ripgrep`, `fd`, `fzf`, `zoxide`, `btop`, `delta`

### Gaming (inherited from Bazzite)
- Steam + Proton pre-configured
- MangoHud performance overlay
- Gamescope compositor
- Game Mode optimizations
- Lutris compatibility

### Developer Tools
- Docker + Podman + Buildah
- Distrobox (run any distro's packages in containers)
- GCC, Clang, CMake, Go, ShellCheck
- **First-login setup** installs: Homebrew, mise, Node.js LTS, Python, pnpm, uv, Rust, GitHub CLI

### Fonts
- JetBrains Mono (system-wide)
- Fira Code
- Noto Sans/Serif/Emoji
- JetBrainsMono Nerd Font (installed per-user on first login)

## Image Variants

| Image | GPU Drivers | Install Command |
|-------|-------------|-----------------|
| `zos` | AMD (open source, Mesa) | `sudo bootc switch ghcr.io/zachhandley/zos:latest` |
| `zos-nvidia` | NVIDIA (proprietary) | `sudo bootc switch ghcr.io/zachhandley/zos-nvidia:latest` |

Both variants ship identical software — only the GPU driver stack differs.

## Installation

### Rebase from an existing Fedora Atomic / Universal Blue / Bazzite system

```bash
# AMD GPU
sudo bootc switch ghcr.io/zachhandley/zos:latest

# NVIDIA GPU
sudo bootc switch ghcr.io/zachhandley/zos-nvidia:latest
```

Reboot when the switch completes. Your home directory and user data are preserved.

### Fresh install from ISO

1. Go to **Actions > Build disk images** in this repo
2. Select your variant (`zos` or `zos-nvidia`) and run the workflow
3. Download the ISO artifact
4. Flash to USB with `dd`, Ventoy, or Balena Etcher
5. Boot and follow the Anaconda installer

### First login

On first login, run `zos-first-login` (or it runs automatically if configured). This installs user-space tools:

- Homebrew (CLI package manager)
- mise (runtime version manager — manages Node.js, Python, etc.)
- Node.js LTS + pnpm
- Python + uv
- Rust toolchain
- GitHub CLI
- JetBrainsMono Nerd Font
- Sets zsh as default shell

This only runs once (tracked by `~/.config/zos-setup-done`).

## Updates & Rollback

Updates are **atomic** — the system downloads a new image, verifies it, and applies it on next boot. No partial updates, no broken systems.

```bash
# Check for updates
sudo bootc upgrade

# If an update breaks something, roll back
sudo bootc rollback

# Check current deployment status
sudo bootc status
```

Daily automatic rebuilds ensure you get the latest Bazzite base + Fedora security patches.

## Building Locally

Requires `podman` and [`just`](https://github.com/casey/just).

```bash
# Build AMD image
just build

# Build NVIDIA image
just build-nvidia

# Build a VM disk image (QCOW2)
just build-qcow2

# Boot the VM
just run-vm

# Build an installable ISO
just build-iso

# Lint build scripts
just lint

# Clean build artifacts
just clean
```

## Desktop Sessions

At the SDDM login screen, you can choose between:

| Session | Description |
|---------|-------------|
| **Plasma (Wayland)** | KDE Plasma 6 — full desktop with taskbar, system tray, app menu |
| **Hyprland (zOS)** | Tiling WM — keyboard-driven, vim-style navigation, Waybar status bar |

### Hyprland Keybindings (highlights)

| Key | Action |
|-----|--------|
| `Super + Return` | Open Wezterm |
| `Super + D` | App launcher (wofi) |
| `Super + Q` | Close window |
| `Super + H/J/K/L` | Navigate windows (vim-style) |
| `Super + 1-9` | Switch workspace |
| `Super + Shift + 1-9` | Move window to workspace |
| `Super + V` | Toggle floating |
| `Super + F` | Fullscreen |
| `Print` | Screenshot region |

Full config: `~/.config/hypr/hyprland.conf`

### Wezterm Keybindings (highlights)

| Key | Action |
|-----|--------|
| `Super + D` | Split pane horizontally |
| `Super + Shift + D` | Split pane vertically |
| `Super + T` | New tab |
| `Super + W` | Close pane |
| `Super + 1-9` | Switch to tab |
| `Super + Alt + H/J/K/L` | Navigate panes |

Full config: `~/.config/wezterm/wezterm.lua`

## Repo Structure

```
zOS/
├── Containerfile                        # Image build definition (single file, both variants)
├── Justfile                             # Local build/test commands
├── .github/workflows/
│   ├── build.yml                        # CI: builds zos + zos-nvidia daily, pushes to GHCR
│   └── build-disk.yml                   # Manual: builds ISO/QCOW2 disk images
├── build_files/
│   ├── build.sh                         # Core packages: CLI tools, fonts, services
│   ├── scripts/
│   │   ├── install-hyprland.sh          # Hyprland + waybar, wofi, mako, etc.
│   │   ├── install-dev-tools.sh         # Compilers, container tools, distrobox
│   │   ├── install-user-configs.sh      # Copies dotfiles to /etc/skel
│   │   └── zos-first-login.sh           # User-space setup (brew, mise, rust, etc.)
│   └── system_files/
│       ├── etc/skel/.config/
│       │   ├── hypr/hyprland.conf       # Hyprland config (Catppuccin, vim keys)
│       │   ├── waybar/config.jsonc      # Waybar modules and layout
│       │   ├── waybar/style.css         # Waybar Catppuccin theme
│       │   ├── wezterm/wezterm.lua      # Wezterm config (GPU, splits, iTerm2 keys)
│       │   └── starship.toml            # Starship prompt config
│       ├── etc/skel/.zshrc              # Zsh config (aliases, zoxide, fzf, starship)
│       └── usr/share/wayland-sessions/
│           └── hyprland-zos.desktop     # SDDM session entry for Hyprland
└── disk_config/
    ├── disk.toml                        # QCOW2 VM disk layout (40GB root)
    ├── iso-kde.toml                     # Anaconda ISO config (AMD)
    └── iso-kde-nvidia.toml              # Anaconda ISO config (NVIDIA)
```

## How It Works

1. `Containerfile` extends a Bazzite base image using a multi-stage build
2. Build scripts are **mounted** (not copied) via a `scratch` stage — they don't bloat the final image
3. `build.sh` and `scripts/*.sh` run `dnf5 install` and `cp` commands to customize the image
4. Files in `system_files/` get copied to their target locations in the image
5. `/etc/skel/` files become defaults for new users
6. GitHub Actions builds both variants daily and publishes signed images to GHCR
7. Users receive updates via `bootc upgrade` (or automatic background updates)

## GitHub Setup

To enable CI builds on your fork:

1. **Generate cosign keypair:**
   ```bash
   cosign generate-key-pair
   ```
2. **Add `SIGNING_SECRET`** — Go to repo Settings > Secrets > Actions, add the contents of `cosign.key` as `SIGNING_SECRET`
3. **Add `cosign.pub`** — Commit the public key to the repo root (already in `.gitignore` for the private key)
4. **Enable Actions** — The `build.yml` workflow triggers on push to `main`

## Upstream

- **Base OS**: [Bazzite](https://bazzite.gg/) (Fedora Atomic + gaming optimizations)
- **Build system**: [ublue-os/image-template](https://github.com/ublue-os/image-template) pattern
- **Desktop**: [KDE Plasma 6](https://kde.org/plasma-desktop/) + [Hyprland](https://hyprland.org/)
- **Theme**: [Catppuccin Mocha](https://catppuccin.com/)

## License

MIT
