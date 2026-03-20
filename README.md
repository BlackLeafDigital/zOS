# zOS

A curated Linux desktop OS that just works. Gaming, development, daily driving — all the good decisions already made.

Built on [Bazzite](https://bazzite.gg/) (Fedora Atomic), delivered as a bootable container image with atomic updates and automatic rollback.

## What's Included

### Desktop
- **Hyprland** — tiling window manager with Windows/macOS-friendly keybinds
- **greetd + ReGreet** — GTK4 login screen with per-monitor wallpaper
- **Catppuccin Mocha** — consistent dark theme across everything
- **System dark mode** — GTK, Qt, and Flatpak apps all respect dark preference

### Terminal & Shell
- **Wezterm** — GPU-accelerated terminal with tabs + splits (iTerm2-style keys)
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
- **First-login setup** (`zos setup`): Homebrew, mise, Node.js LTS, Python, pnpm, uv, Rust, GitHub CLI

### Fonts
- JetBrains Mono + JetBrainsMono Nerd Font (system-wide)
- Fira Code
- Noto Sans/Serif/Emoji

## Image Variants

| Image | GPU Drivers | Install Command |
|-------|-------------|-----------------|
| `zos` | AMD (open source, Mesa) | `sudo bootc switch ghcr.io/blackleafdigital/zos:latest` |
| `zos-nvidia` | NVIDIA (proprietary) | `sudo bootc switch ghcr.io/blackleafdigital/zos-nvidia:latest` |

Both variants ship identical software — only the GPU driver stack differs.

## Installation

### Rebase from an existing Fedora Atomic / Universal Blue / Bazzite system

```bash
# AMD GPU
sudo bootc switch ghcr.io/blackleafdigital/zos:latest

# NVIDIA GPU
sudo bootc switch ghcr.io/blackleafdigital/zos-nvidia:latest
```

Reboot when the switch completes. Your home directory and user data are preserved.

### Fresh install from ISO

1. Go to **Actions > Build disk images** in this repo
2. Select your variant (`zos` or `zos-nvidia`) and run the workflow
3. Download the ISO artifact
4. Flash to USB with `dd`, Ventoy, or Balena Etcher
5. Boot and follow the Anaconda installer

### First login

Run `zos setup` to install user-space dev tools:

- Homebrew (CLI package manager)
- mise (runtime version manager — Node.js, Python, Go, etc.)
- Node.js LTS + pnpm
- Python + uv
- Rust toolchain
- GitHub CLI
- Sets zsh as default shell

### Installing software

zOS is immutable — no `dnf` at runtime. Install software with:

| What | How |
|------|-----|
| GUI apps | `flatpak install flathub <app>` |
| Dev runtimes | `mise install node`, `mise install python` |
| CLI tools | `brew install <tool>` |
| Full Linux envs | `distrobox create --name dev --image fedora:43` |

Or use `zos search <name>` / `zos install <name>` to search all sources at once.

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

## Keybindings

### Desktop (Hyprland)

| Key | Action |
|-----|--------|
| `Super + Space` | App launcher |
| `Super + T` / `Return` | Terminal (Wezterm) |
| `Super + E` | File manager |
| `Super + Q` | Close window |
| `Super + D` | Show desktop |
| `Super + L` | Lock screen |
| `Super + F` | Fullscreen |
| `Super + M` / `Down` | Minimize window |
| `Super + Up` | Maximize window |
| `Super + Arrow keys` | Navigate windows |
| `Super + Shift + Arrows` | Move window |
| `Super + Alt + Left/Right` | Move window to next monitor |
| `Super + Ctrl + Arrows` | Resize window |
| `Super + 1-9` | Switch workspace |
| `Super + Shift + 1-9` | Move window to workspace |
| `Super + V` | Toggle floating |
| `Super + N` | Notifications |
| `Super + Shift + V` | Clipboard history |
| `Super + Shift + D` | Display settings (saves layout) |
| `Super + Shift + E` | Power menu |
| `Super + F1` | Show all keybindings |
| `Print` | Screenshot region |
| `Super + hold mouse` | Drag to move/resize windows |

### Terminal (Wezterm)

| Key | Action |
|-----|--------|
| `Super + D` | Split pane horizontally |
| `Super + Shift + D` | Split pane vertically |
| `Super + T` | New tab |
| `Super + W` | Close pane |
| `Super + 1-9` | Switch to tab |
| `Super + Alt + H/J/K/L` | Navigate panes |
| `Super + F` | Search |

Full configs: `~/.config/hypr/` and `~/.config/wezterm/`

## The `zos` CLI

```bash
zos                    # TUI dashboard
zos setup              # First-login dev tool setup
zos doctor             # System health diagnostics
zos grub               # GRUB/dual-boot configuration
zos migrate            # Config migration after OS updates
zos update             # Check and apply OS updates
zos search <name>      # Search Flatpak, Brew, and mise
zos install <name>     # Install from the best available source
```

## Building Locally

Requires `podman` and [`just`](https://github.com/casey/just).

```bash
just build             # Build AMD image
just build-nvidia      # Build NVIDIA image
just build-qcow2       # Build a VM disk image (QCOW2)
just run-vm            # Boot the VM
just build-iso         # Build an installable ISO
just lint              # Lint build scripts
just clean             # Clean build artifacts
```

## Repo Structure

```
zOS/
├── Containerfile                        # Image build definition (both variants)
├── Justfile                             # Local build/test commands
├── zos-cli/                             # Rust CLI tool (zos command)
│   ├── Cargo.toml
│   └── src/
├── .forgejo/workflows/
│   ├── build.yml                        # CI: builds zos + zos-nvidia, pushes to GHCR
│   └── build-disk.yml                   # Manual: builds ISO/QCOW2 disk images
├── build_files/
│   ├── build.sh                         # Core packages: CLI tools, fonts, services
│   ├── scripts/
│   │   ├── install-hyprland.sh          # Hyprland + greetd + ecosystem
│   │   ├── install-dev-tools.sh         # Compilers, container tools, distrobox
│   │   ├── install-user-configs.sh      # Copies dotfiles to /etc/skel
│   │   └── zos-first-login.sh           # Legacy first-login script
│   └── system_files/
│       ├── etc/greetd/                  # Login screen config (greetd + ReGreet)
│       ├── etc/skel/.config/
│       │   ├── hypr/                    # Hyprland user config
│       │   ├── waybar/                  # Waybar modules and theme
│       │   ├── wezterm/                 # Wezterm terminal config
│       │   └── starship.toml            # Prompt config
│       ├── etc/skel/.zshrc              # Zsh config (aliases, integrations)
│       └── usr/share/zos/
│           ├── hypr/*.conf              # System-managed Hyprland configs
│           └── scripts/                 # Helper scripts (monitor save, etc.)
└── disk_config/
    ├── disk.toml                        # QCOW2 VM disk layout
    ├── iso-kde.toml                     # Anaconda ISO config (AMD)
    └── iso-kde-nvidia.toml              # Anaconda ISO config (NVIDIA)
```

## How It Works

1. `Containerfile` extends a Bazzite base image using a multi-stage build
2. Build scripts are **mounted** (not copied) via a `scratch` stage — they don't bloat the final image
3. `build.sh` and `scripts/*.sh` install packages and copy configs into the image
4. `zos-cli` is compiled from Rust source during the build
5. ReGreet (login greeter) is compiled from source during the build
6. `/etc/skel/` files become defaults for new users
7. Forgejo Actions builds both variants daily and publishes images to GHCR
8. Users receive updates via `bootc upgrade` (or automatic background updates)

## Upstream

- **Base OS**: [Bazzite](https://bazzite.gg/) (Fedora Atomic + gaming optimizations)
- **Build system**: [ublue-os/image-template](https://github.com/ublue-os/image-template) pattern
- **Desktop**: [Hyprland](https://hyprland.org/)
- **Theme**: [Catppuccin Mocha](https://catppuccin.com/)

## License

MIT
