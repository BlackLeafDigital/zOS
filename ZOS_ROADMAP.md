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
│       └── commands/       # All backend logic
│           ├── status.rs   # SystemInfo, config versions
│           ├── doctor.rs   # Health checks
│           ├── setup.rs    # First-login setup steps
│           ├── install.rs  # Package search (Flatpak/Brew/mise)
│           ├── update.rs   # OS update check/apply (bootc)
│           ├── grub.rs     # GRUB/bootloader management
│           └── migrate.rs  # Config migration (hypr, waybar, wezterm, etc.)
├── zos-cli/                # CLI/TUI (depends on zos-core)
├── zos-settings/           # GTK4 GUI (depends on zos-core)
│   ├── Cargo.toml
│   ├── resources/
│   │   └── style.css       # Catppuccin Mocha theme
│   └── src/
│       ├── main.rs         # Entry: AdwApplication + tray
│       ├── app.rs          # Root: Sidebar + Stack navigation
│       ├── tray.rs         # System tray icon (ksni)
│       ├── pages/          # 9 settings pages
│       │   ├── overview.rs     # System info + health
│       │   ├── display.rs      # Monitor layout canvas + config
│       │   ├── audio.rs        # VoiceMeeter-style mixer
│       │   ├── appearance.rs   # Theme, cursor, wallpaper
│       │   ├── input.rs        # Keyboard, mouse, touchpad
│       │   ├── network.rs      # WiFi, ethernet, VPN
│       │   ├── devsetup.rs     # First-login dev tools
│       │   ├── power.rs        # Power actions + profiles
│       │   └── boot.rs         # GRUB + dual boot
│       └── services/       # Backend service modules
│           ├── pipewire.rs     # wpctl/pw-link wrapper
│           ├── hyprctl.rs      # Hyprland IPC wrapper
│           └── power.rs        # logind D-Bus wrapper
└── Containerfile
```

## Current Status (What's Done)

### ✅ Complete
- [x] Cargo workspace (zos-core, zos-cli, zos-settings)
- [x] All 9 pages compile and render with real backends
- [x] System tray with power menu (ksni)
- [x] Catppuccin Mocha CSS theme
- [x] Display: monitor canvas with position preview + Apply writes monitors.conf
- [x] Audio: output/input device selection, volume, mute, virtual bus controls, PipeWire routing with port dropdowns
- [x] Input: keyboard layout, repeat rate, mouse, touchpad — live apply + persists to user-settings.conf
- [x] Network: WiFi scan, connect with password, IP details
- [x] Appearance: dark mode toggle, cursor size, wallpaper picker
- [x] Power: suspend/reboot/shutdown with confirmation dialogs
- [x] Boot: GRUB timeout, Windows dual-boot detection
- [x] Developer Setup: per-step install buttons from zos-core
- [x] Overview: system info + health checks from zos-core
- [x] Wezterm config migration in zos-core
- [x] Desktop entry, autostart, SUPER+I keybind, window rule
- [x] Polkit policy for GRUB elevation
- [x] Shared services (hyprctl, power, pipewire)
- [x] `just dev` / `just dev-release` / `just check` targets

### 🔄 In Progress
- [ ] Display: live monitor thumbnails via grim (periodic refresh)
- [ ] Audio: VoiceMeeter-style mixer UI improvements

### ❌ Not Started
- [ ] Display: drag-to-arrange monitors
- [ ] Display: live PipeWire screen capture (replace grim with real-time video)
- [ ] Audio: per-app volume routing UI
- [ ] Audio: gain controls, EQ integration
- [ ] Appearance: Catppuccin flavor selector (cross-app theme switching)
- [ ] Appearance: cursor theme picker, font picker
- [ ] Network: VPN management, hotspot, WiFi toggle
- [ ] Power: power profiles (performance/balanced/power-saver)
- [ ] Power: screen lock / idle timeout settings
- [ ] Packages page (search + install UI)
- [ ] Hook system (shell hooks for events)
- [ ] Window state persistence
- [ ] Toast notifications for errors
- [ ] About dialog
- [ ] Tray: show/hide window communication (currently just prints)
- [ ] Reactive page updates (convert static `build()` to Relm4 components)

---

## Future Milestones

### v0.2 — Live Display Preview
**Goal:** Show actual monitor content in the display canvas, refreshing periodically.

- Capture via `grim -o <name> -t png -s 0.2 -` (stdout) every 2 seconds
- Load as `cairo::ImageSurface` and paint into monitor rectangles
- `glib::timeout_add_seconds_local` for periodic refresh + `canvas.queue_draw()`

### v0.3 — PipeWire Screen Capture (True Live)
**Goal:** Replace grim screenshots with real-time PipeWire video frames.

**Approach:**
1. Use `ashpd` crate to call `org.freedesktop.portal.ScreenCast` D-Bus API
2. Start a screencast session per monitor
3. Get PipeWire node IDs for each stream
4. Use `pipewire-rs` crate (or build a minimal PipeWire consumer) to receive video frames
5. Convert frames to `cairo::ImageSurface` and paint into canvas
6. This is the same pipeline OBS, GNOME Screenshot, and other screen capture tools use

**Dependencies needed:**
- `ashpd` — Rust bindings for xdg-desktop-portal (screencast, screenshot, etc.)
- `pipewire-rs` — Rust bindings for libpipewire (may need to write or fork if not mature enough)
- `pipewire-devel` — already conflicts with Bazzite's build, may need to use the system libpipewire directly via FFI

**Alternative:** If `pipewire-rs` is too immature, write a small C shim that does the PipeWire frame capture and exposes it via FFI to Rust.

**Prerequisites:**
- `xdg-desktop-portal-hyprland` — already installed, handles screencast portal
- `pipewire` — already running as the audio/video server
- `libpipewire-0.3` — already available at runtime (from Bazzite)

### v0.4 — Drag-to-Arrange Monitors
**Goal:** Users can drag monitor rectangles in the canvas to rearrange them.

- `GestureDrag` controller on the DrawingArea
- Hit-test on mouse down to select which monitor
- Update monitor x/y positions as user drags
- Snap-to-edge when rectangles are near each other
- Apply writes the new positions to monitors.conf

### v0.5 — VoiceMeeter-Style Audio Mixer
**Goal:** Full audio routing interface like VoiceMeeter Potato.

**Layout (columns):**
- Left: Physical + virtual inputs (mics, monitors)
- Center: Virtual buses (Main, Music, Chat) with volume faders
- Right: Physical outputs (speakers, headphones, HDMI)
- Routing: checkboxes or drag lines to connect input → bus → output

**Features:**
- Per-app routing: assign which bus each app sends audio to
- Per-bus: volume, mute, solo, EQ preset selector
- Per-output: volume, mute, device lock
- Gain control per input/bus
- VU meters showing live audio levels (read from PipeWire)
- Save/load routing presets

**Backend:** `pw-link` for connections, `wpctl` for volume, PipeWire config for persistence

### v0.6 — Packages Page
**Goal:** GUI package search and install across Flatpak, Brew, and mise.

- Search bar with debounced input → `zos_core::commands::install::search()`
- Results grouped by source with Install buttons
- Installed packages list with Remove
- Async search with per-source streaming results

### v0.7 — Reactive Components
**Goal:** Convert all pages from static `build() -> gtk::Box` to Relm4 `Component` with message passing.

- Live data refresh on page switch
- Background async operations (no UI freezing)
- Toast notifications for success/failure
- Window state persistence (size, position, last page)

---

## PipeWire Virtual Audio Config

Ships at `~/.config/pipewire/pipewire.conf.d/10-zos-virtual-devices.conf`:

```
context.objects = [
    { factory = adapter
      args = {
          factory.name   = support.null-audio-sink
          node.name       = "zos-main"
          node.description = "Main Output"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }
    }
    { factory = adapter
      args = {
          factory.name   = support.null-audio-sink
          node.name       = "zos-music"
          node.description = "Music"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }
    }
    { factory = adapter
      args = {
          factory.name   = support.null-audio-sink
          node.name       = "zos-chat"
          node.description = "Chat / Voice"
          media.class     = "Audio/Sink"
          audio.position  = [ FL FR ]
          object.linger   = true
      }
    }
]
```

## Quick Start

```bash
# Dev headers already installed in the zOS image
just dev            # Launch settings app
just dev --tray     # Launch tray-only mode
just dev-release    # Release build
just check          # Compile check workspace
just build          # Build full OS image
```
