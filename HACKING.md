# zOS — HACKING guide

Notes for developing zOS crates. This is NOT the user-facing README (see `README.md` for that).

## Workspace layout

| Crate | Purpose |
|---|---|
| `zos-core` | Shared library: commands, IPC traits, system ops |
| `zos-cli` | Command-line tool (`zos`) + TUI |
| `zos-settings` | iced GUI for settings |
| `zos-dock` | iced layer-shell app dock |
| `zos-daemon` | Background service (tray, udev listener) |
| `zos-wm` | **Wayland compositor (WIP)** — fork of Smithay's anvil |

Future crates planned per `~/.claude/plans/hey-how-do-i-jiggly-balloon.md`: `zos-ui` (component framework), `zos-panel`, `zos-power`, `zos-monitors`, `zos-notify`.

## Building

```
just check              # cargo check --workspace
just build              # AMD zOS image (podman/docker)
just build-nvidia       # NVIDIA zOS image
```

Full image builds require podman/docker. Rust crate builds only need `cargo`.

## Running the existing apps in dev

```
just dev                # runs zos-settings
just dev-release        # runs zos-settings release build
just dev-cli            # runs zos CLI TUI
```

## Running the compositor (zos-wm)

`zos-wm` is a Wayland compositor. It has three backends:

- **`winit`** (default) — opens a nested window on your current Wayland session. Use this for iteration — fastest edit-to-pixels. Does NOT support multi-output — only one window. Does NOT accurately simulate DRM-level behavior.
- **`udev`/`tty`** (via feature flag) — real bare-metal DRM backend. Boot from a spare TTY to test multi-monitor + real input. See "Testing on a TTY" below.
- **`x11`** — nested inside an X server. Rarely useful on zOS; not recommended.

### Nested dev (safest, fastest)

```
just dev-wm
```

Opens a Wayland window on your host Hyprland (or whatever) session. The compositor runs inside. If it panics, the window closes — your host session is fine.

Run a test client against it to verify surface handling:

```
# in another terminal, after dev-wm is running:
WAYLAND_DISPLAY=wayland-zos foot
```

(The exact `WAYLAND_DISPLAY` value is printed by `zos-wm` on startup.)

### Testing on a bare-metal TTY

Risk: if `zos-wm` hangs, you `Ctrl+Alt+F1` (or similar) back to your Hyprland TTY to kill it. Do NOT use this workflow from SSH unless you have a recovery plan.

Switch to a free TTY (e.g., `Ctrl+Alt+F3`), log in:

```
cargo build -p zos-wm --features tty --release
./target/release/zos-wm --tty-udev
```

Panic? `Ctrl+Alt+F2` (or whichever TTY your main session is on) gets you home.

## NVIDIA notes

Bazzite ships the `egl-gbm` package required for Smithay's GL renderer on NVIDIA. If you see `eglInitialize failed` on startup, check `eglinfo -B` — a missing `/usr/lib/gbm/nvidia-drm_gbm.so` is almost always the cause.

The winit backend on an NVIDIA Wayland host has known quirks:
- Modifier keys can get stuck ([smithay#1353](https://github.com/Smithay/smithay/issues/1353))
- Frame pacing blocks at host vsync — do not benchmark from nested mode

For real performance work, use the bare-metal TTY path.

## Common tasks

```
cargo check --workspace     # typecheck everything
cargo test -p zos-core      # run tests for one crate
cargo run -p zos-settings   # run a specific GUI
cargo fmt                   # format
cargo clippy --workspace    # lint
```

## Crate linkage

`zos-wm` depends on `smithay` pinned via git to a specific commit. When updating:
1. Change `rev = "..."` in `zos-wm/Cargo.toml`.
2. Check Smithay's CHANGELOG.md for breaking changes.
3. `cargo update -p smithay` then `cargo check -p zos-wm`.

## Plan file reference

Multi-phase work plans live in `~/.claude/plans/` (user's private). Current zOS plan tracks the zos-wm/zos-ui/shell consolidation work.
