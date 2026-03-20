# zOS Settings — Roadmap & Implementation Guide

## Overview

`zos-settings` is a native GTK4 system settings app for zOS. It runs as a tray icon daemon on login and opens a full settings window on demand. Think macOS System Preferences meets GNOME Settings, themed with Catppuccin Mocha.

**Stack:** Rust + GTK4-rs + Relm4 + libadwaita + ksni (tray) + zbus (D-Bus)

## Architecture

```
zOS/
├── Cargo.toml              # Workspace root
├── zos-core/               # Shared backend library
│   └── src/
│       ├── lib.rs
│       ├── config.rs       # Paths, state, constants
│       ├── colors.rs       # Catppuccin Mocha RGB values
│       └── commands/       # All backend logic (extracted from zos-cli)
│           ├── status.rs   # SystemInfo, config versions
│           ├── doctor.rs   # Health checks
│           ├── setup.rs    # First-login setup steps
│           ├── install.rs  # Package search (Flatpak/Brew/mise)
│           ├── update.rs   # OS update check/apply
│           ├── grub.rs     # GRUB/bootloader management
│           └── migrate.rs  # Config migration
├── zos-cli/                # CLI/TUI (depends on zos-core)
├── zos-settings/           # GTK4 GUI (depends on zos-core)
│   ├── Cargo.toml
│   ├── build.rs
│   ├── resources/
│   │   └── style.css       # Catppuccin Mocha theme
│   └── src/
│       ├── main.rs         # Entry: AdwApplication + tray
│       ├── app.rs          # Root: NavigationSplitView
│       ├── pages/          # Settings pages
│       ├── services/       # D-Bus, Hyprland IPC, PipeWire
│       ├── tray.rs         # System tray icon (ksni)
│       └── plugins/        # Hook system
└── Containerfile
```

## Dependencies

```toml
[dependencies]
zos-core = { path = "../zos-core" }
relm4 = { version = "0.10.1", features = ["macros", "libadwaita", "gnome_47"] }
relm4-components = "0.10.1"
relm4-icons = "0.10.1"
ksni = { version = "0.3.3", features = ["tokio"] }
zbus = "5.14"
hyprland = "0.4.0-beta.3"
tokio = { version = "1.47", features = ["rt", "rt-multi-thread", "sync", "macros"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

**DO NOT use:** libadwaita 0.9.x or gtk4 0.11.x — incompatible with relm4 0.10.x.

## Pages

### 1. Overview / Dashboard
- OS version, image name, kernel, uptime
- Config sync status (system vs user versions)
- Health check summary (green/yellow/red indicators)
- "Check for Updates" button + pending update banner
- **Backend:** `status::get_system_info()`, `doctor::run_doctor_checks()`, `update::check_for_updates()`
- **Widgets:** `AdwStatusPage`, `AdwPreferencesGroup`, `AdwBanner`

### 2. Display / Monitors
- List connected monitors with resolution, refresh rate, scale
- Drag-to-arrange canvas for monitor positions (2 bottom + 1 top, etc.)
- Per-monitor settings: resolution dropdown, refresh rate, scale, rotation
- "Apply" saves to `~/.config/hypr/monitors.conf` + applies live via hyprctl
- **Backend:** `hyprland::data::Monitors::get()`, `hyprctl keyword monitor`
- **Widgets:** Custom `gtk::DrawingArea` for layout, `AdwComboRow` for settings

### 3. Audio / Sound
- Output device selector (speakers, headphones, HDMI)
- Input device selector (microphone)
- Per-app volume routing (which app goes to which output)
- PipeWire node graph visualization (simplified)
- Link to qpwgraph for advanced routing
- **Backend:** PipeWire D-Bus / `pw-cli` / `wpctl` (WirePlumber)
- **Widgets:** `AdwComboRow` for device select, `gtk::Scale` for volume

### 4. Power
- Suspend / Reboot / Shut Down / Hibernate buttons
- Power profile selector (performance / balanced / power saver)
- Auto-suspend timeout
- **Backend:** `zbus` proxy to `org.freedesktop.login1.Manager`
- **Widgets:** `AdwPreferencesGroup`, `AdwActionRow` with buttons, `AdwComboRow`

### 5. Appearance
- Dark/Light mode toggle (currently forced dark)
- Catppuccin flavor selector (Mocha, Macchiato, Frappe, Latte)
- Cursor theme / size
- Font settings
- GTK theme propagation
- **Backend:** `gsettings`, hyprctl env vars, GTK4 CSS reload
- **Widgets:** `AdwSwitchRow`, `AdwComboRow`, `AdwSpinRow`

### 6. Network
- WiFi networks (scan, connect, saved)
- Ethernet status
- VPN connections
- IP address, DNS info
- **Backend:** NetworkManager D-Bus via `zbus`
- **Widgets:** `AdwPreferencesGroup`, `AdwActionRow`, `AdwEntryRow`

### 7. Keyboard & Input
- Keyboard layout selector
- Key repeat rate / delay
- Touchpad settings (natural scroll, tap-to-click, speed)
- Mouse sensitivity
- **Backend:** `hyprctl keyword input:*`
- **Widgets:** `AdwComboRow`, `AdwSwitchRow`, `AdwSpinRow`

### 8. Packages
- Search bar → searches Flatpak, Brew, mise simultaneously
- Results grouped by source with "Install" buttons
- Installed packages list with "Remove" option
- **Backend:** `install::search()`, `install::search_and_install()`
- **Widgets:** `AdwEntryRow`, `AdwActionRow` per result

### 9. Developer Setup
- First-login setup wizard (Homebrew, mise, Node, Python, pnpm, uv, Rust, gh, zsh)
- Checklist with per-step install buttons + "Run All"
- Progress indicators
- Refuses to run as root
- **Backend:** `setup::get_setup_steps()`, `setup::run_setup_step()`
- **Widgets:** `AdwPreferencesGroup`, `AdwActionRow` with status icons

### 10. Boot / GRUB
- GRUB timeout slider
- Windows dual-boot detection + BLS entry creation
- Boot order info
- Requires root (polkit elevation)
- **Backend:** `grub::get_grub_status()`, `grub::apply_grub_timeout()`
- **Widgets:** `AdwSpinRow`, `AdwActionRow`, `AdwBanner` for root warning

### 11. Updates
- Current image version + available update info
- "Upgrade Now" button (runs `bootc upgrade`)
- Rollback button (runs `bootc rollback`)
- Auto-update toggle
- **Backend:** `update::check_for_updates()`, `update::apply_update()`
- **Widgets:** `AdwPreferencesGroup`, `AdwSwitchRow`, `AdwActionRow`

### 12. About
- `AdwAboutDialog` — app name, version, credits, license, links

## System Tray

Runs as a daemon on login via `exec-once = zos-settings --tray` in Hyprland autostart.

- **Left-click:** Show/hide settings window
- **Right-click menu:**
  - Open Settings
  - ---
  - Suspend
  - Reboot
  - Shut Down
  - ---
  - Quit

Uses `ksni` crate (StatusNotifierItem D-Bus protocol). Shows in Waybar's tray module.

## Plugin / Hook System

### v1: Shell Hooks
```
~/.config/zos/hooks/
├── on-power-action/         # Before reboot/shutdown/suspend
├── on-monitor-change/       # After display config saved
├── on-theme-change/         # After appearance change
├── on-update/               # After OS update applied
├── on-package-install/      # After package installed
└── on-startup/              # After tray daemon starts
```

Scripts run alphabetically, get `$ZOS_HOOK` and `$ZOS_HOOK_DATA` env vars. Must be executable.

### v2: D-Bus Plugin Registry
Plugins register as D-Bus services. Settings app discovers them and can embed their widgets.

### v3: WASM Plugins
Sandboxed WASM modules via wasmtime/extism for cross-platform, safe plugin execution.

## Theming

Force Catppuccin Mocha dark mode via:
1. `AdwStyleManager::set_color_scheme(ForceDark)`
2. App-level CSS with `@define-color` overrides for all libadwaita color variables
3. CSS file embedded via `relm4::set_global_css(include_str!(...))`

## Implementation Phases

### Phase 1: Foundation
- [ ] Create Cargo workspace root
- [ ] Extract `zos-core` library from `zos-cli`
- [ ] Make `zos-cli` depend on `zos-core`
- [ ] Verify `cargo build --workspace` works
- [ ] Scaffold `zos-settings` with empty `AdwApplicationWindow`

### Phase 2: Core Pages
- [ ] Overview page (system info + health checks)
- [ ] Power page (reboot/shutdown/suspend via D-Bus)
- [ ] Catppuccin CSS theme
- [ ] System tray icon + right-click menu

### Phase 3: Display & Audio
- [ ] Display page (monitor list + config save)
- [ ] Audio page (output/input selection, volume)
- [ ] Monitor drag-to-arrange canvas

### Phase 4: Settings Pages
- [ ] Appearance page (dark mode, cursor, fonts)
- [ ] Keyboard & Input page
- [ ] Network page

### Phase 5: Package & Dev Tools
- [ ] Packages page (search + install)
- [ ] Developer Setup wizard
- [ ] Boot/GRUB page

### Phase 6: Polish
- [ ] Hook system
- [ ] Desktop entry + autostart integration
- [ ] Containerfile build integration
- [ ] Error handling + toast notifications
- [ ] Window state persistence (size, position)

## Quick Start (on zOS machine)

```bash
# Install GTK4 + libadwaita dev headers
sudo rpm-ostree install gtk4-devel libadwaita-devel

# Or in a toolbox/distrobox
distrobox create --name zos-dev --image fedora:43
distrobox enter zos-dev
sudo dnf install gtk4-devel libadwaita-devel rust cargo

# Clone and build
cd ~/GitHub/zOS
cargo build -p zos-settings

# Run
./target/debug/zos-settings

# Run tray-only mode
./target/debug/zos-settings --tray
```

## Build Dependencies (for Containerfile)

```bash
dnf5 install -y rust cargo gtk4-devel libadwaita-devel
```

These are build-time only — the compiled binary has no dev header dependency at runtime. GTK4 and libadwaita shared libraries are already in Bazzite's base image.
