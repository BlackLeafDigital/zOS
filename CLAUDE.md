# zOS — Project Guide for AI Agents

## What This Project Is

zOS is a **custom Linux desktop OS** built as a bootable OCI container image. It extends [Bazzite](https://bazzite.gg/) (which is itself Fedora Atomic + gaming optimizations) with curated packages, desktop configs, and developer tools.

**This repo IS the OS definition.** There is no separate installer, no package manager config, no Ansible playbooks. The `Containerfile` + `build_files/` directory define everything that gets baked into the image.

## Architecture at a Glance

```
Fedora Atomic (base OS, immutable root filesystem)
  └─ Bazzite (gaming layer: Steam, Proton, MangoHud, Gamescope, kernel tweaks)
      └─ zOS (our layer: Hyprland, Wezterm, dev tools, curated configs)
```

Two image variants exist differing **only** in GPU drivers:
- `zos` — AMD (open source Mesa drivers, inherited from Bazzite)
- `zos-nvidia` — NVIDIA (proprietary drivers, inherited from Bazzite NVIDIA)

The variant is controlled by a single `BASE_IMAGE` build arg in the `Containerfile`.

## Build System

### How it builds

1. `Containerfile` uses a two-stage build:
   - **Stage 1 (`scratch AS ctx`)**: Copies `build_files/` into a temporary layer
   - **Stage 2 (Bazzite base)**: Mounts that layer at `/ctx/` during build, runs scripts
   - Build files are **never copied into the final image** — only their effects remain

2. Build scripts run in order:
   - `build.sh` — Core system packages (CLI tools, fonts, services)
   - `scripts/install-hyprland.sh` — Hyprland WM + ecosystem (waybar, wofi, mako)
   - `scripts/install-dev-tools.sh` — Compilers, container tools, distrobox
   - `scripts/install-user-configs.sh` — Copies dotfiles to `/etc/skel/` + installs first-login script

3. All scripts use `set -ouex pipefail` — any failure stops the build immediately.

### CI/CD

- **`.github/workflows/build.yml`** — Triggered on push to main, daily schedule, or manual dispatch. Builds both AMD and NVIDIA variants in a matrix, pushes to GHCR, signs with cosign.
- **`.github/workflows/build-disk.yml`** — Manual only. Builds installable ISOs or QCOW2 VM images from the published container images.

### Local builds

```bash
just build          # AMD variant
just build-nvidia   # NVIDIA variant
just build-qcow2    # VM disk image
just run-vm         # Boot the VM
just lint           # ShellCheck on build scripts
```

Requires `podman` and `just`. Does NOT work in WSL (needs real Linux for container builds with bootc).

## Key File Locations

| File | Purpose |
|------|---------|
| `Containerfile` | Image build definition — the single source of truth |
| `build_files/build.sh` | Core package installation (dnf5) |
| `build_files/scripts/install-hyprland.sh` | Hyprland + compositor ecosystem |
| `build_files/scripts/install-dev-tools.sh` | System-level dev tools |
| `build_files/scripts/install-user-configs.sh` | Deploys dotfiles to /etc/skel |
| `build_files/scripts/zos-first-login.sh` | User-space setup (brew, mise, rust) — runs once per user |
| `build_files/system_files/etc/skel/.config/` | Default user configs (hypr, waybar, wezterm, starship) |
| `build_files/system_files/etc/skel/.zshrc` | Default zsh config |
| `build_files/system_files/usr/share/wayland-sessions/hyprland-zos.desktop` | SDDM session entry |
| `disk_config/iso-kde.toml` | Anaconda ISO config for AMD |
| `disk_config/iso-kde-nvidia.toml` | Anaconda ISO config for NVIDIA |
| `disk_config/disk.toml` | QCOW2 VM disk layout |

## How to Make Changes

### Adding a system package
Edit `build_files/build.sh` (or the appropriate `scripts/install-*.sh`) and add the package to a `dnf5 install -y` block.

### Adding a Flatpak app
Flatpaks are NOT baked into the image — they're installed by users post-boot. To pre-configure Flatpak repos or defaults, add to `build_files/build.sh`.

### Adding/changing user default configs
Place files under `build_files/system_files/etc/skel/`. These become defaults for new users. The `install-user-configs.sh` script must explicitly copy them.

### Adding a new build script
1. Create `build_files/scripts/your-script.sh` with `#!/bin/bash` and `set -ouex pipefail`
2. Make it executable: `chmod +x`
3. Add a `RUN` line or chain it in the existing `RUN` block in `Containerfile`
4. Reference files via `/ctx/` prefix (the mount point during build)

### Changing the base image
Edit the `BASE_IMAGE` ARG default in `Containerfile` and the matrix entries in `.github/workflows/build.yml`.

## Important Patterns

### `/etc/skel/` pattern
Files placed in `/etc/skel/` during build become the default contents of new users' home directories. This is how zOS ships default configs without overwriting existing user files on updates.

### `/ctx/` mount pattern
The `Containerfile` mounts `build_files/` at `/ctx/` during the build RUN step. This means build scripts and system_files are available during build but NOT present in the final image. Always reference build-time files as `/ctx/path`.

### First-login vs build-time
- **Build-time** (`build.sh`, `install-*.sh`): For system-level packages and configs. Runs once during image creation. Uses `dnf5`.
- **First-login** (`zos-first-login.sh`): For user-space tools that shouldn't be root-level (Homebrew, mise, rustup). Runs once per user on first login. Tracked by `~/.config/zos-setup-done` marker file.

### Immutable root filesystem
The base OS root (`/usr`, `/etc` system files) is **read-only at runtime**. Users cannot `dnf install` on a running system. Options for installing additional software:
- **Flatpak** — Desktop apps (recommended for GUI apps)
- **Homebrew** — CLI tools (user-space, no root needed)
- **Distrobox** — Full containerized distro environments
- **Layering** — `rpm-ostree install` for system packages (persists across updates but may conflict)

## Theme

All visual configs (Hyprland, Waybar, Wezterm) use **Catppuccin Mocha**:
- Background: `#1e1e2e`
- Foreground: `#cdd6f4`
- Accent blue: `#89b4fa`
- Accent purple: `#cba6f7`
- Green: `#a6e3a1`
- Red: `#f38ba8`

When adding new visual configs, maintain Catppuccin Mocha consistency.

## Testing Changes

1. **Build locally**: `just build` (requires podman on real Linux, not WSL)
2. **Test in VM**: `just build-qcow2 && just run-vm`
3. **Verify**: Boot, check login screen shows both Plasma and Hyprland sessions, verify packages are installed, test terminal configs
4. **Lint**: `just lint` runs ShellCheck on all build scripts

## Upstream References

- Bazzite docs: https://docs.bazzite.gg/
- Universal Blue image template: https://github.com/ublue-os/image-template
- Fedora Atomic docs: https://docs.fedoraproject.org/en-US/atomic-desktops/
- bootc docs: https://containers.github.io/bootc/
- Hyprland wiki: https://wiki.hyprland.org/
- Wezterm docs: https://wezfurlong.org/wezterm/
- Catppuccin: https://catppuccin.com/

## Conventions

- All build scripts: `#!/bin/bash` + `set -ouex pipefail`
- Package installs: Use `dnf5 install -y`, group related packages together
- Comments: Use `# --- Section Name ---` for major sections within scripts
- File headers: Use `# ===` block comment with script name and purpose
- Config theme: Catppuccin Mocha everywhere
- Keybindings: Vim-style (h/j/k/l) where applicable
