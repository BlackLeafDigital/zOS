# zos-dock

A macOS-style dock for zOS, built with Rust on Wayland.

Uses [iced](https://iced.rs/) + [iced_layershell](https://github.com/waycrate/exwlshelleventloop) to render as a Wayland layer-shell surface on Hyprland.

## Features

- **Pinned apps** with configurable order
- **Running app indicators** with active window highlighting
- **Minimized window tracking** (third section, dimmed icons)
- **macOS-style magnification** on hover (Gaussian distance + spring physics)
- **Auto-hide** with a thin bottom-edge trigger zone (like macOS)
- **Reveal-on-minimize** — dock briefly appears when you Super+M with auto-hide on
- **Real-time updates** via Hyprland event socket (no polling lag)
- **Right-click context menu** — close, pin/unpin, restore, launch
- **Multi-monitor** — one dock instance per screen
- **Icon resolution** across Papirus, Adwaita, hicolor themes + Flatpak

## Architecture

```
src/
├── main.rs          # Entry point, layer-shell daemon setup
├── dock.rs          # Elm-architecture app: state, update, view
├── hypr.rs          # Hyprland IPC (hyprctl commands)
├── hypr_events.rs   # Real-time Hyprland event socket subscription
├── config.rs        # Persistent config (~/.config/zos/dock.json)
└── icons.rs         # Desktop file parsing + icon resolution
```

The dock is an iced daemon using the Elm architecture (Model-Update-View). It runs as a `Layer::Top` surface with no exclusive zone, meaning it floats above windows without reserving screen space. Input is restricted via `SetInputRegion` to only the visible dock area — clicks outside the dock pass through to windows below.

## Configuration

Config lives at `~/.config/zos/dock.json` and hot-reloads on save:

```json
{
  "pinned": [
    "org.wezfurlong.wezterm",
    "org.mozilla.firefox",
    "org.kde.dolphin"
  ],
  "icon_size": 48,
  "magnification": 1.6,
  "auto_hide": false
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `pinned` | wezterm, firefox, dolphin | App IDs in display order |
| `icon_size` | 48 | Base icon size in pixels |
| `magnification` | 1.6 | Hover zoom factor (1.0 = off, 2.0 = 2x) |
| `auto_hide` | false | Hide dock when cursor leaves |

## How It Works

### Window Tracking

The dock connects to Hyprland's event socket (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`) for real-time window events. A 5-second fallback poll ensures nothing is missed if the socket drops.

### Minimize

Hyprland's `movetoworkspacesilent special:minimize` moves windows to a hidden workspace. The dock detects these via the event socket and shows them as a separate section with 50% opacity. If auto-hide is on, the dock reveals itself for 2 seconds when a window is minimized, then re-hides.

### Input Region

The layer-shell surface spans the full screen width, but `SetInputRegion` restricts mouse input to only the centered dock content. When auto-hidden, a 4px strip at the very bottom edge serves as the reveal trigger — matching macOS behavior.

### Magnification

Each icon's scale is driven by a spring-physics animation (`iced_anim::Spring`). The target scale is computed from a Gaussian distance function centered on the cursor position, producing the smooth "wave" effect seen in macOS docks.

## Building

```bash
cargo build --release -p zos-dock
```

The binary is installed to `/usr/bin/zos-dock` during the zOS image build and launched by Hyprland via `exec-once = zos-dock` in the autostart config.

## Theme

Catppuccin Mocha with a translucent background (85% opacity). Consistent with the rest of zOS.
