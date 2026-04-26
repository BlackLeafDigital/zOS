# zos-wm — Honest Status

This file lists known gaps, defects, and deferred work in zos-wm. Update it
when you find new issues; never silently delete entries — strike them with
~~strikethrough~~ once shipped, then remove on the next entry rotation.

## Crash paths

Wave 4 (panic→error conversion) has partially landed. Here's what's known
to crash, what was just fixed, and what's still a roulette wheel.

- ~~`udev.rs:1860` `SwapBuffersError::ContextLost` panic.~~ Now logs
  `error!` and reschedules (`udev.rs:1860-1868`). Prior behavior aborted
  the compositor on NVIDIA driver suspend/resume or hotplug races.

- ~~`udev.rs:2117` `SwapBuffersError::ContextLost` unrecoverable branch.~~
  The catch-all at `udev.rs:2129-2135` now logs `error!` and reschedules.
  **Remaining sharp edge:** the `DrmError::TestFailed` branch at
  `udev.rs:2126` still calls `.expect("failed to reset drm device")` —
  if `reset_state()` fails after a TTY-switch CRTC mismatch, we abort.
  Untested in real-world.

- ~~`shell/grabs.rs` four `panic!("invalid resize state")` sites.~~ Already
  replaced with `warn!` + drop. See lines 538-539, 578-579, 757-758,
  797-798. Surface lifecycle bugs no longer crash; they log and skip the
  ack.

- **21 `.unwrap()` in `state.rs`, 33 `.unwrap()` in `udev.rs` not audited
  individually.** Most are init-time legitimately fatal (Wayland global
  registration, etc.) but each is a roulette wheel. Example still in flight:
  `state.rs:233` unwraps `dmabuf_state` — if init order ever changes, this
  panics inside DmabufHandler at runtime.

## Functional gaps (broken or stubbed end-user features)

- **Shell-app autostart not wired.** `start-zos-wm`
  (`build_files/system_files/usr/bin/start-zos-wm:41`) just `exec`s the
  compositor. zos-panel, zos-notify, zos-power, zos-monitors do not
  autostart. End user gets a black screen with windows but no
  panel/clock/tray. Workaround: launcher (`Super+Space` → `zos-launcher`)
  works.

- **Compositor IPC trait stubbed under zos-wm.**
  `zos-core/src/compositor/zos_wm.rs:20-38` returns empty `Vec`s and
  `"zos-wm IPC not yet implemented"` errors. Comment at line 3 says *"Stub
  for now (Phase 8 finishes this)"*. Net effect: zos-panel under zos-wm
  shows empty workspaces row, no active title, clicks do nothing.
  zos-monitors gets an empty list.

- **zos-monitors writes Hyprland config under zos-wm.**
  `zos-monitors/src/main.rs:355` writes `~/.config/hypr/monitors.conf`.
  zos-wm doesn't read that file — applying does nothing, silently. The
  comment at `main.rs:346-347` flags the intended Phase 8 path-switch.

- **Session lock is fake.** `zos-wm/src/state.rs:940-953` flips
  `is_locked = true` but the comment at line 952 admits *"Render path
  currently ignores lock surfaces"*. Screen NOT obscured when locked.
  **Security implication: do not rely on lock for security on this build.**

## Visual/effects gaps

- **Rounded corners shader compiled but not rendered.** Per-window render
  integration is deferred. `zos-wm/src/effects/rounded.rs:41-49` carries
  the `TODO(P4-rounded-render-integration)` marker; `zos-wm/src/render.rs:358-364`
  notes "Rounded corners are intentionally not shipped here" — the current
  shader writes a white masked shape that would tint windows rather than
  mask them.

- **Multi-GPU rounded corners explicitly skipped.** `zos-wm/src/udev.rs:216`
  flags this for `rounded_effect`; line 222 inherits the same gap for
  `shadow_effect`. With RTX 4090 + AMD iGPU, any window whose `render_node`
  differs from `primary_gpu` skips the mask. Affects mixed-GPU setups
  (the project owner's daily-driver hardware is exactly this).

- **Drop shadows ship via `MultiRenderPixelShaderElement` wrapper** (recent
  fix, `zos-wm/src/effects/multi_render.rs`). This is the one effect that
  actually renders end-to-end.

## Wayland protocol stubs (accepted but not wired)

- **Tearing-control is a no-op.** `zos-wm/src/udev.rs:2311` carries
  `TODO(tearing-async-pageflip)`. Protocol is parsed (the surface state
  helper at `udev.rs:2374` resolves correctly); nothing reaches DRM.
  Gaming "allow tear" requests do nothing.

- **Gamma-control LUT stored, not applied to DRM.**
  `zos-wm/src/protocols/gamma_control.rs:282` and `:398` both carry
  `TODO(gamma-drm-apply)`. Night-light tools (redshift, gammastep, wluma)
  appear to work; screen color does not change.

- **Screencopy: outputs only, not toplevel.** `zos-wm/src/state.rs:789`
  `TODO(screencopy-toplevel)`. OBS window source / xdg-desktop-portal
  "share window" fails. Output capture works.

- **wlr-output-management partial.** Position/scale/transform need the
  Backend trait extended; custom modes rejected at dispatch; adaptive-sync
  stored but not written to DRM. `zos-wm/src/udev.rs:530`
  `TODO(adaptive-sync-drm-prop)`. CHANGELOG line 43-44 carries the same
  list.

## Hardware not yet exercised

- **No commit references hardware testing.** Compositor has not been booted
  on the project owner's RTX 4090 + AMD iGPU + 3× 1080p60 setup as of
  this writing. Phase 8 swap is **not** triggered. Hyprland keep-alive
  remains in `build_files/scripts/install-hyprland.sh` (DO-NOT-REMOVE
  banner per CHANGELOG:64).

- **VM testing path:** see `/var/home/zach/github/zOS/docs/TESTING.md`.
  `just dev-wm` (nested winit) is the fastest smoke test.
  `just run-vm-uefi` boots the qcow2 in a VM with persistent NVRAM (UEFI).
  Both run on llvmpipe — useless for measuring framerate or reproducing
  NVIDIA-specific behavior.

## What DOES work

- XWayland default-on (`zos-wm/Cargo.toml:52`); clipboard + primary
  selection + DnD wired.
- IPC server runs and is consumed by `zos compositor` CLI subcommands;
  smoke-passes 4 unit tests.
- Per-output workspaces; dwindle tiling; modal/AlwaysOnTop bands; focus
  history; `Super+1..9`; `Super+drag` move/resize.
- Greetd `.desktop` entry exists at
  `/usr/share/wayland-sessions/zos-wm.desktop`. Launcher script is
  correct re: NVIDIA env hygiene (`start-zos-wm:24-30`).
- 44/44 unit tests passing (per CHANGELOG:519).

## Update protocol

Edit this file when you find a new gap or close an old one. ~~Strike out~~
shipped items in place rather than deleting; rotate them out on the next
substantive update. Cross-reference `CHANGELOG.md` Phase 8 readiness
checklist.
