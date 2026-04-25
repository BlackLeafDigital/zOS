# Changelog

Living notes on the zOS rebuild. Read [ZOS_DELME.md](./ZOS_DELME.md) first for orientation, then this for granular history.

Format: reverse-chronological. Each entry = one "session" or logical chunk of work. Entries are concrete (file paths + line counts + cargo-check status), not narrative.

---

## 2026-04-24 — Phase 2 complete (3 commits)

Three commits on `main`:
- `3a684da feat(zos-wm): phase 2 compositor crate (anvil fork + 4 from-scratch protocols)` — 14043 insertions, 29 files
- `283c24a feat(image): zos-wm build + greetd session entry + hyprland keep-alive banner` — 79 insertions, 6 files
- `4849f55 docs: phase 2 research artifacts + ZOS_DELME handoff + HACKING.md` — 3834 insertions, 9 files

### Compositor (`zos-wm/`)
- Smithay anvil fork pinned to `27af99ef492ab4d7dc5cd2e625374d2beb2772f7`. MIT (NOTICE preserved).
- `main.rs` strips inherited `GBM_BACKEND` env on startup.
- `shell/ssd.rs` — real Catppuccin titlebar with usvg/resvg-rasterized SVG buttons (min/max/close), hover states, click-to-unmap minimize.

### Wayland protocols wired
| Protocol | Source | File |
|---|---|---|
| cursor-shape-v1 | smithay built-in | state.rs delegate |
| idle-inhibit-v1 | smithay built-in | state.rs handler + delegate |
| idle-notify-v1 | smithay built-in | state.rs handler + delegate |
| session-lock-v1 | smithay built-in | state.rs handler + delegate |
| pointer-warp-v1 | smithay built-in | state.rs handler + delegate |
| ext-foreign-toplevel-list-v1 | smithay built-in | state.rs + shell/xdg.rs lifecycle |
| ext-image-copy-capture-v1 (screencopy) | smithay built-in (winit + udev shm) | screencopy.rs (generic over Renderer + Bind\<T\> + Offscreen\<T\> + ExportMem) |
| xdg-decoration | smithay built-in | state.rs (request_mode honors client) |
| kde-server-decoration | smithay built-in | state.rs (default Mode::Client) |
| xwayland | anvil's existing | enabled by default |
| **wp-tearing-control-v1** | **from-scratch** | `protocols/tearing_control.rs` (381 lines) |
| **wlr-gamma-control-v1** | **from-scratch** | `protocols/gamma_control.rs` (476 lines) |
| **zwlr-output-management-v1** | **from-scratch** | `protocols/output_management.rs` (1405 lines) |

Smithay at our pin lacks the 3 from-scratch protocols. Wire bindings for all 3 are re-exported via `smithay::reexports::wayland_protocols{,_wlr}`.

### udev backend
- `apply_output_config` in `udev.rs` does real DRM modeset via `DrmOutput::use_mode`. v1 caveats:
  - position/scale/transform/disable need `Backend` trait extended to take `&mut Space` (deferred 2.D.5)
  - custom modes rejected at dispatch
  - adaptive-sync stored in user_data but DRM VRR property not yet written (deferred)
  - test_only is mode-list-only, not a real `DRM_MODE_ATOMIC_TEST_ONLY`
- output-management lifecycle hooks: `add_head` after `space.map_output` in `connector_connected`, `remove_head` before `space.unmap_output` in `connector_disconnected`.
- `ZOS_RENDER_DEVICE` env var (back-compat to `ANVIL_DRM_DEVICE`).
- `ZOS_DISABLE_SYNCOBJ` escape hatch.
- Diagnostic logs: seat name + GPU enumeration on session start, primary-GPU source, render-node-add events.
- NVIDIA overlay-plane quirk inherited from anvil (`udev.rs:1006-1014`).

### winit backend
- `add_head` after virtual-output `space.map_output`; `remove_head` on `PumpStatus::Exit`.
- Tearing-hint lookup scaffolded with TODO; trace log when clients request async (no-op in nested winit).
- `WinitEvent::Resized` → `notify_changes` not yet wired (deferred 2.D.4).

### Image build
- `Containerfile` builds `zos-wm --release --features udev,xwayland` and installs to `/usr/bin/zos-wm`. Required `COPY zos-wm /zos-wm` into `rust-ctx` scratch stage.
- `libseat-devel` + `mingw64-{gcc,binutils,headers,crt}` added to Rust build layer's `dnf5 install`.
- `Justfile` recipes: `dev-wm` (nested winit, filters EGL log spam) and `build-wm-local` (matches image build).
- `build_files/system_files/usr/share/wayland-sessions/zos-wm.desktop` — regreet picks up.
- `build_files/system_files/usr/bin/start-zos-wm` — POSIX-sh launcher with env hygiene + exec `/usr/bin/zos-wm --tty-udev`.
- `build_files/scripts/install-user-configs.sh` — copies both into image with explicit `chmod +x` on launcher.
- `build_files/scripts/install-hyprland.sh` — DO-NOT-REMOVE banner (Hyprland stays until Phase 8).

### Research docs (`docs/research/`)
- `phase-2-a-protocol-priority.md` — protocol audit
- `phase-2-b-niri-reusable-code.md` — license-aware reuse (Niri = GPL-3, zos-wm = MIT)
- `phase-2-c-drm-nvidia-specifics.md` — NVIDIA 580 + DRM/GBM + Bazzite session
- `phase-2-fix-decoration-investigation.md` — Qt-on-Wayland CSD root cause
- `phase-2-tearing-control.md` — from-scratch design
- `phase-2-output-management.md` — from-scratch design
- `phase-2-udev-gaps.md` — ~80% TTY readiness audit + 8-task plan

### Verified
- `cargo check -p zos-wm` clean
- `cargo check --workspace` clean (1 pre-existing unrelated warning in zos-dock)
- `cargo check -p zos-wm --features udev` fails on `libseat-sys` link (host missing libseat-devel; image build will succeed)

### Deferred
- async-pageflip — needs upstream smithay PR
- gamma-DRM-apply — wire `DrmDevice::set_gamma` from udev once stored LUT changes
- VT-switch resilience (`activate(true)`) — wait for empirical signal

---

## 2026-04-24 — Phase 2 deferred-cleanup batch

Three follow-up tasks knocked out (all `cargo check -p zos-wm` clean):

### 2.D.5 — Backend `apply_output_config` now reaches `Space`
- `state.rs`: `OutputManagementHandler::apply_output_config` impl extended. After `backend_data.apply_output_config(...)?`, walks changes calling new helper `apply_space_change(&change)` which handles position/scale/transform via `output.change_current_state(...)` and `space.map_output(...)`. Disable case calls `space.unmap_output` + `space.refresh()`.
- `udev.rs`: `OutputConfigAction::Disable` arm replaced. Drops `device.surfaces[crtc]` (its `Drop` impl tears down `DrmOutput` + removes `wl_output` global). `UdevOutputId` now derives `Clone`.
- Net effect: wlr-output-management `apply()` now actually changes layout, not just mode. kanshi / wlr-randr / wdisplays should work end-to-end.

### 2.D.4 — winit `Resized` → `notify_changes`
- `winit.rs`: 4 lines added at the `WinitEvent::Resized` arm calling `notify_changes(&mut state.output_management_manager_state, &output)`. Clients see new dimensions when the user resizes the nested winit window.

### 2.F.2 — screencopy dmabuf zero-copy path
- `Backend` trait gets new `screencopy_dma_constraints()` method (default `None`).
  - **Winit** override: pulls render node from `EGLDevice::device_for_display(...).try_get_render_node()` and formats from `self.backend.renderer().dmabuf_formats()`.
  - **Udev** override: `self.primary_gpu` for node, `self.gpus.single_renderer(&self.primary_gpu)?.dmabuf_formats()` for formats.
- `screencopy.rs`: `try_capture` trait bound widened with `Bind<Dmabuf>`. New fast-path at top of `try_capture` — if `get_dmabuf(&wl_buffer).is_some()`, bind dmabuf as framebuffer, render directly into it, return `Ok(())` (no readback). shm path unchanged below.
- `state.rs::capture_constraints`: now passes `dma: self.backend_data.screencopy_dma_constraints()` instead of hardcoded `None`.
- xdg-desktop-portal-pipewire and OBS get GPU-side handoff; CPU memcpy only on shm fallback.

### Verified
- `cargo check -p zos-wm` clean after each agent
- `cargo check -p zos-wm --features udev` still pre-existing libseat link (host environment)

---
