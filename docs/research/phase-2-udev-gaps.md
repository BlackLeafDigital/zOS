# Phase 2 — udev backend readiness audit for shipping `zos-wm` from a TTY

Scope: walk every meaningful gap between the current state of
`zos-wm/src/udev.rs` (inherited anvil, ~1708 lines) and a clean
`zos-wm --tty-udev` boot from a Linux virtual console on Zach's daily
driver: RTX 4090 (proprietary `nvidia` 580.95.05) + AMD iGPU on a
9950X3D, 3× DisplayPort 1080p60 monitors, Razer Naga Pro on USB,
Bazzite NVIDIA stable. Cargo & Smithay pinned to rev
`27af99ef492ab4d7dc5cd2e625374d2beb2772f7`.

This audit **layers on top of** `phase-2-c-drm-nvidia-specifics.md` —
do not re-read NVIDIA driver / GBM / explicit-sync / overlay-plane
content from there; cited only as `phase-2-c §section`.

---

## TL;DR

- **Compositor source:** ~80 % ready. Anvil-style udev backend is
  fully wired (session, libinput, udev hotplug, primary GPU detect,
  GpuManager, dmabuf feedback, syncobj, lease scaffolding, NVIDIA
  overlay-plane quirk, vblank throttle, atomic DRM, GBM allocator).
- **The unblockers, in order:**
  1. **Cargo workspace member is exposed but the `udev` feature is
     not built by default.** `Cargo.toml:48` defaults to
     `["winit", "xwayland"]`. A consumer must build with
     `--features udev,xwayland` and there is no top-level binary
     install path. Need a `just`/Containerfile target that builds
     `zos-wm` with `udev,xwayland` and copies it to `/usr/bin/`.
  2. **No greetd / wayland-sessions integration.** No
     `zos-wm.desktop` exists; `greetd/config.toml:5` hard-codes
     `start-hyprland`. Need a session entry + a tiny launcher script,
     and (later) a separate greeter session — but greetd plumbing for
     zos-wm-as-user-session is independent of the greeter and can
     ship first.
  3. **Primary-GPU selection is the anvil default and works on this
     box, but with a footgun.** Logic at `udev.rs:238-251` uses
     `ANVIL_DRM_DEVICE` env override which is awkward to expose as a
     production knob. Recommend renaming to `ZOS_RENDER_DEVICE` and
     adding a config field. **Not a blocker for first boot** — the
     fallback to `primary_gpu(seat)` will pick the NVIDIA on Zach's
     box (it's `boot_vga=1`), see phase-2-c §"Bazzite / libseat
     session setup".
- **NVIDIA-specific items already done:** overlay-plane quirk
  (`udev.rs:1006-1014`); `GBM_BACKEND` env scrubbed in `main.rs:24`;
  syncobj wiring (`udev.rs:484-498` + 618-623); 10-bit format list
  (`udev.rs:110-115`); EGL high-priority context.
- **Estimated path to first TTY boot:** 2-3 small (1-2 file) tasks
  for build/install plumbing + 2-3 small tasks for session/launcher +
  1 task for a config-friendly `ZOS_RENDER_DEVICE` rename. **Total
  effort to a "boots and you can see weston-terminal" state: ~1
  focused day.** Polish (multi-monitor layout config, lease
  toggle, screencopy) lives in sibling docs.

---

## 1. Build-time check

### 1.1 Cargo feature graph at HEAD

`Cargo.toml:51-64`:
```toml
udev = [
  "smithay-drm-extras",
  "smithay/backend_libinput",
  "smithay/backend_udev",
  "smithay/backend_drm",
  "smithay/backend_gbm",
  "smithay/backend_egl",
  "smithay/backend_session_libseat",
  "image",
  "smithay/renderer_gl",
  "smithay/renderer_pixman",
  "smithay/renderer_multi",
  "xcursor",
]
```

Notes:
- `smithay/backend_vulkan` was previously in the set per
  phase-2-c §"Cargo.toml audit" but **has already been removed at
  HEAD** — good. (phase-2-c recommended dropping it; that has
  landed.)
- `smithay/renderer_pixman` is still present. phase-2-c kept it as
  a CPU-fallback bring-up crutch. Leave alone for Phase 2.
- `xwayland` is **not** composed into `udev` — must be requested
  separately. The user-facing build invocation is therefore
  `cargo build --release -p zos-wm --features udev,xwayland`.
  This matches anvil's pattern; document in README.
- `default = ["winit", "xwayland"]` (`Cargo.toml:48`) means a naive
  `cargo build -p zos-wm` produces a winit-only binary. The
  Containerfile would need an explicit `--features` flag if/when
  zos-wm is added to the image build.

### 1.2 System library deps

- **`libseat-devel`** — already added to the Containerfile at
  `Containerfile:26`. No additional packages required for the build.
- Build links against `libudev`, `libinput`, `libdrm`, `libgbm`,
  `libEGL`, `libGLESv2`, `libxkbcommon` — all present on Bazzite by
  default (verified live; phase-2-c §"Bazzite / libseat session
  setup").
- `libxcb` + `libxkbcommon-x11` for XWayland — present.

### 1.3 Things that would block a `cargo check --features udev` today

Static read of the source surfaces no obvious red flags:
- All `cfg(feature = "udev")` gates resolve cleanly through
  the imports in `udev.rs:1-101`.
- The single `cfg(feature = "renderer_sync")` block at
  `udev.rs:23,1654-1658` is gated and won't break the build.
- The `cfg(feature = "egl")` block at `udev.rs:25,442-449` is
  optional — currently unbuilt unless you also pass `--features egl`.
  This only enables `bind_wl_display` (wl-eglstream import path,
  unused on NVIDIA 555+ which uses dmabuf+gbm); skipping it is fine.
- The `cfg(feature = "debug")` blocks gate FPS overlay; skip.

**Verdict:** the udev feature should `cargo check` cleanly with
just `libseat-devel` available. **Do not actually build** in this
research pass — the user requested no side effects — but the static
audit finds no missing deps, missing modules, or feature
misconfigurations.

### 1.4 Workspace member status

`Cargo.toml:3` — `members = [..., "zos-wm"]`. zos-wm is a workspace
member. The workspace `[profile.release]` settings at the top of the
workspace `Cargo.toml` apply (`strip = true`, `lto = true`,
`opt-level = "z"`). For a compositor LTO is fine; opt-level "z"
(size) is **not** ideal for a compositor's hot path (we want "3" or
"s"). This is a workspace-wide setting that affects every crate;
flag for Phase 3 cleanup, not a Phase 2 blocker.

---

## 2. Runtime gap audit (walking `udev.rs`)

### 2.1 `LibSeatSession::new()` — `udev.rs:227-233`

```rust
let (session, notifier) = match LibSeatSession::new() { ... };
```

- **Seat name:** libseat opens `seat0` by default
  (Smithay's `libseat.rs:43-80`). On Bazzite this maps to the
  active console seat via systemd-logind. No code change needed.
- **Permissions:** libseat falls back to logind when seatd is
  absent (phase-2-c §"Bazzite / libseat session setup"). Zach's
  user is in `wheel zlayer` only — **no `video`/`input`/`render`
  needed**, ACL handles it. **Pre-flight: must run from physical
  TTY**, not over SSH (`Active=yes` required).
- **Failure mode:** if launched over SSH, you'll see
  `Could not initialize a session: ...` at `udev.rs:230` and the
  process exits early with `return`. This is the correct
  behavior; document in the launcher script that it must be
  launched from a TTY.

**Verdict: DONE upstream, no changes needed.** Just document the
TTY-only requirement.

### 2.2 `primary_gpu(seat)` selection — `udev.rs:238-251`

```rust
let primary_gpu = if let Ok(var) = std::env::var("ANVIL_DRM_DEVICE") {
    DrmNode::from_path(var).expect("Invalid drm device path")
} else {
    primary_gpu(session.seat())
        .unwrap()
        .and_then(|x| DrmNode::from_path(x).ok()?.node_with_type(NodeType::Render)?.ok())
        .unwrap_or_else(|| {
            all_gpus(session.seat())
                .unwrap()
                .into_iter()
                .find_map(|x| DrmNode::from_path(x).ok())
                .expect("No GPU!")
        })
};
```

Behavior on Zach's box (3 paths):
1. **Env override `ANVIL_DRM_DEVICE`** — present, will use it. Fine
   for dev iteration but the name is wrong ("ANVIL").
2. **`primary_gpu(seat)` fallback** — Smithay queries the udev DB
   for the `master-of-seat` tagged device on `seat0`. On a desktop
   with a single `boot_vga=1` GPU (the NVIDIA at PCI 0000:01:00.0),
   this returns the NVIDIA card device. Then `node_with_type(Render)`
   converts to `renderD129`. This is the **expected happy path**.
3. **`all_gpus` last-resort** — if (2) returns None, picks the first
   GPU enumerated. On Zach's box that could pick the AMD iGPU
   (`renderD128`) which does not drive a display — bad. Mitigation:
   enumerate order on Bazzite is PCI-bus order; NVIDIA at
   `0000:01:00.0` comes before AMD at `0000:71:00.0`, so first-found
   would also be NVIDIA. Holds today, fragile in principle.

**Footgun:** the env var name `ANVIL_DRM_DEVICE` is jarring for an
end user. cosmic-comp uses `COSMIC_RENDER_DEVICE`; the consistent
zos-wm name should be `ZOS_RENDER_DEVICE`. Recommend renaming
**and** adding fallback to keep `ANVIL_DRM_DEVICE` for backward
compat (1-line change at 238).

**Recommendation for production:** add a config field
`compositor.render_device = "/dev/dri/by-path/pci-0000:01:00.0-card"`
(read by `state.rs` or a new `config.rs` module). For Phase 2 first
boot, leave the env override as the only knob and document it.

**Verdict: WORKS on this box, env-var name needs renaming, config
support deferred.**

### 2.3 NVIDIA overlay-plane quirk — `udev.rs:1006-1014`

```rust
if driver.name().to_string_lossy().to_lowercase().contains("nvidia")
    || driver.description().to_string_lossy().to_lowercase().contains("nvidia")
{
    planes.overlay = vec![];
}
```

Already ported per phase-2-c §"Atomic modesetting" / cosmic-comp
ref. **DONE.**

### 2.4 3-monitor scenario

Walk through what happens for Zach's 3 displays:

1. `device_added(NVIDIA, /dev/dri/card2)` runs at `udev.rs:763`
   during boot (called from `udev.rs:386-405`).
2. `device_changed` is called immediately at `udev.rs:884`.
3. `device_changed` runs `DrmScanner::scan_connectors`
   (`udev.rs:1116-1125`) which returns `Connected`
   events for each connected DP-1, DP-2, DP-3 connector with their
   assigned CRTC.
4. For each, `connector_connected` fires (`udev.rs:889-1073`).

In `connector_connected`, the position math at **`udev.rs:971-975`**:

```rust
let x = self
    .space
    .outputs()
    .fold(0, |acc, o| acc + self.space.output_geometry(o).unwrap().size.w);
let position = (x, 0).into();
```

- **Behavior:** stacks each new output to the right of the previous
  one at y=0. With 3× 1080p, you get DP-1 at (0,0,1920,1080), DP-2 at
  (1920,0,3840,1080), DP-3 at (3840,0,5760,1080).
- **Risk:** the **enumeration order from DrmScanner is not
  deterministic across boots.** On the first boot DP-2 might come
  first; on the next, DP-1. This means windows / cursor focus will
  jump between physical monitors after a reboot.
- **Mitigation for first boot:** harmless — you'll still see
  *something* on each monitor. Cursor warps and window placement
  will be wrong relative to user expectation but not crashing.
- **Real fix:** wlr-output-management (sibling research doc) +
  config-driven layout. **Out of scope for Phase 2 first-boot.**
- **Disconnect side:** `connector_disconnected` (`udev.rs:1075-1107`)
  removes the surface from `device.surfaces` and unmaps from the
  space. Position recompute on next add is **not** triggered, which
  means after unplug+replug a monitor will get the `acc` that
  includes the surviving outputs' widths and end up correctly
  placed. Edge case: unplug DP-1, plug it back — it gets x=3840
  (after DP-2+DP-3), not x=0. Cosmetic, fix in wlr-om phase.

**Verdict: LIVES for first boot. Multi-monitor layout polish is a
sibling-doc concern.**

### 2.5 DRM lease (non-desktop, VR) — `udev.rs:546-616, 875-879`

- VR HMDs export a `non-desktop` connector property which causes
  `connector_connected` to register the connector as leasable
  (`udev.rs:938-947`) instead of as a normal output.
- `DrmLeaseHandler` impl at 546-613 lets a Wayland client (e.g.,
  Monado) lease the connector + CRTC + primary plane.
- Pause/resume clears active leases and suspends the lease global
  (`udev.rs:336-339,364-366`).

Zach has no VR HMD on this box. **Not exercised.** Code path is
inert when no `non-desktop` connectors exist. Leave as-is.

**Verdict: SCAFFOLDED, not relevant to first boot, no action.**

### 2.6 Cursor plane / pointer rendering

- `pointer_image` is a `crate::cursor::Cursor` loaded from XCursor
  theme at `udev.rs:272`.
- Per-frame, in `render_surface` at `udev.rs:1430-1468`, the
  current frame is fetched via `get_image(scale, time)`, looked up
  in the cache (`pointer_images`), and a `MemoryRenderBuffer` is
  built lazily.
- `render_surface` (the free function at 1556-1682) builds custom
  render elements via `pointer_element.render_elements(...)` at
  `udev.rs:1606-1615` and feeds them to `drm_output.render_frame`.
- Smithay's `DrmCompositor` automatically promotes the cursor to a
  hardware cursor plane when it can. NVIDIA exposes a hardware
  cursor plane but with format restrictions (ARGB8888 64x64 typical
  for Ada). The pointer image at default scale=1 is 24x24 or 32x32
  → fits.
- DnD icon path at 1619-1633 — simple, works.

**Risk:** XCursor theme path. `Cursor::load()` walks `XCURSOR_THEME`
+ search paths. If a TTY launch has empty env, it falls back to
"default" theme. On Bazzite the default Adwaita XCursor is in
`/usr/share/icons/Adwaita/cursors`; will work. The launcher script
should set `XCURSOR_THEME=Adwaita XCURSOR_SIZE=24` to be explicit.

**Verdict: WORKS, cosmetic env vars in launcher script.**

### 2.7 Dmabuf feedback path — `udev.rs:699-760`, `udev.rs:1037-1047`

`get_surface_dmabuf_feedback` builds two feedbacks:
- `render_feedback` — primary GPU as default tranche, render node
  as preference tranche (if different from primary).
- `scanout_feedback` — primary as default, scanout-flagged tranche
  with the **plane formats intersected with all-render formats**
  (`udev.rs:724-733`), then a render-node tranche.

NVIDIA modifier set per phase-2-c §"DRM format modifiers":
mostly `LINEAR` + `NVIDIA_BLOCK_LINEAR_2D_*`. Smithay's NVIDIA EGL
extension support exposes these via `dmabuf_render_formats`. The
intersection of plane formats and render formats yields the small
set of NVIDIA-tiled modifiers that scanout AND render can both
handle.

**Risk:** if NVIDIA exposes the BlockLinear modifier but the
EGL_EXT_image_dma_buf_import_modifiers query doesn't list it, the
intersection is empty and clients get an empty scanout tranche →
no direct scanout, fallback to render-only path. **This is OK
behavior** — clients still render correctly via the render-feedback
default tranche.

**Verdict: BUILDS CORRECTLY. Empty scanout tranche on NVIDIA is
expected and benign — direct scanout opt-in still happens via the
plane format check at queue time, not via the dmabuf-feedback hint.**

### 2.8 `frame_finish` vblank throttle — `udev.rs:1190-1375`

The flow:
1. Look up surface for (dev_id, crtc).
2. Cancel any pending throttle timer.
3. Compute `frame_duration` from output mode refresh.
4. Pull monotonic timestamp from DRM event metadata.
5. Compare elapsed since last presentation vs frame_duration.
6. If display is running faster than expected (vblank_remaining_time
   > frame_duration/2), schedule a `frame_finish` re-trigger after
   `vblank_remaining_time` and bail.
7. Otherwise, mark `last_presentation_time = clock`,
   `frame_submitted()` → presentation feedback.
8. Schedule next render at `clock + frame_duration` (with 60 % delay
   on same-GPU path).

**Math check:**
- `frame_duration = 1000ms / refresh_mhz` per `udev.rs:1227-1228` —
  but `mode.refresh` is in millihertz. So for 60 Hz this is
  `1000/60000 = 0.0166 s = 16.6 ms` — **wait, that's
  `Duration::from_secs_f64(1_000f64 / mode.refresh as f64)`, which
  with `refresh = 60_000` gives `1000.0 / 60000.0 = 0.01666 s`
  which is correct.** ✓
- 60 % repaint delay at `udev.rs:1348` = 10 ms, leaves 6.6 ms for
  compositor repaint + scanout — fine for 60 Hz, tight at 144 Hz
  (3.5 ms compositor budget).
- Vblank-faster-than-mode detection (`udev.rs:1255-1278`) — the
  threshold "more than half a frame" is conservative; will fire
  on VRR panels at high frame rates. The throttle inserts a Timer
  that re-calls `frame_finish` with synthesized metadata. **This
  is correct.**

**Risk:** vrr-faster-than-expected handling on G-Sync DP panels —
the user reports `Supported` for vrr_capable on these panels (per
phase-2-c §"VRR / adaptive-sync"), but `use_vrr(true)` is never
called by zos-wm anywhere in the udev path. So VRR is NOT enabled
by default, and the throttle math holds.

**Verdict: ROBUST.** Enabling VRR is a sibling concern.

### 2.9 syncobj path — `udev.rs:484-498, 618-623`

```rust
if let Some(primary_node) = state.backend_data.primary_gpu
    .node_with_type(NodeType::Primary).and_then(|x| x.ok())
{
    if let Some(backend) = state.backend_data.backends.get(&primary_node) {
        let import_device = backend.drm_output_manager.device().device_fd().clone();
        if supports_syncobj_eventfd(&import_device) {
            let syncobj_state = DrmSyncobjState::new::<...>(&display_handle, import_device);
            state.backend_data.syncobj_state = Some(syncobj_state);
        }
    }
}
```

- **Probe:** `supports_syncobj_eventfd` at Smithay
  `src/wayland/drm_syncobj/mod.rs:67-76` calls the
  `drm_ioctl_syncobj_eventfd` ioctl with a placeholder eventfd; if
  the kernel returns anything other than `ENOENT`, returns true.
- **Kernel requirement:** ≥ 6.6 (eventfd-based syncobj wait). Zach
  on 6.17.7 — fine.
- **Driver requirement:** NVIDIA 555+ supports the syncobj path.
  580.95.05 — fine.
- **Wiring:** `DrmSyncobjHandler` impl at 618-623 is the trivial
  pass-through. `delegate_drm_syncobj!` macro at 623.

**Risk:** the protocol global is created **before** any device is
added if the lookup at 484 finds no backend → silently disabled. But
in `run_udev` ordering, primary `device_added` runs at 386-391
**before** the syncobj block at 484. So `backends.get(&primary_node)`
will find it.

**Verdict: WIRED CORRECTLY.** Add a tracing log on success/failure
so first-boot diagnostics tell you syncobj is active. Cosmic-comp's
`COSMIC_DISABLE_SYNCOBJ` analog (`ZOS_DISABLE_SYNCOBJ`) is a
nice-to-have.

### 2.10 Pause / resume — `udev.rs:328-371`

Both branches inherited from anvil. phase-2-c §"Bazzite / libseat
session setup" item 3 noted: anvil uses `activate(false)`,
cosmic-comp uses `activate(true)`. zos-wm currently passes `false`
at `udev.rs:362` — same as anvil.

**Recommendation per phase-2-c:** start with `false`, flip to
`true` after first VT-switch-induces-corruption report. **Leave
alone for first boot.**

### 2.11 Other minor things

- **`device_added` `try_initialize_gpu`** at `udev.rs:795-811` — if
  `EGLDevice::is_software()` returns true, returns
  `NoRenderNode`. On NVIDIA-only systems with broken drivers this
  could fire. Bazzite ships the proprietary driver baked in;
  unlikely to misfire.
- **Workspace allocator fallback** at `udev.rs:822-833` — if no
  render node, falls back to the primary GPU's allocator. AMD iGPU
  is enumerated as a secondary device with no displays attached;
  this code path keeps it from crashing the compositor when the
  iGPU is added to backends.
- **Initial enumeration logic** at `udev.rs:374-405` — primary
  device first, then all others. Good. **Do NOT skip the AMD iGPU
  enumeration**; the AMD iGPU is a future reverse-prime offload
  target. Keeping it in `backends` with no surfaces is harmless
  (no crtcs assigned, no leases, just a render node available in
  GpuManager).

---

## 3. Greetd integration plan

### 3.1 Constraints

Per memory `feedback_greeter.md`: **DO NOT change `greetd/config.toml`
to use `start-hyprland` for any session that isn't Hyprland.** That
memory is about the greeter session itself (which still runs
Hyprland for the regreet UI). The session-after-login is independent.

### 3.2 What needs to exist

Three files:

1. **Wayland session entry**:
   `build_files/system_files/usr/share/wayland-sessions/zos-wm.desktop`
   ```ini
   [Desktop Entry]
   Name=zOS (zos-wm)
   Comment=zOS native Wayland compositor
   Exec=/usr/bin/start-zos-wm
   Type=Application
   DesktopNames=zos-wm
   ```

2. **Launcher script**: `build_files/system_files/usr/bin/start-zos-wm`
   ```sh
   #!/bin/sh
   # Launches zos-wm in TTY/udev mode from a logind seat.
   # Invoked by greetd after user picks the "zOS (zos-wm)" session.
   set -eu

   # --- Wayland identity
   export XDG_SESSION_TYPE=wayland
   export XDG_CURRENT_DESKTOP=zos-wm
   export XDG_SESSION_DESKTOP=zos-wm

   # --- Toolkit hints
   export QT_QPA_PLATFORM=wayland
   export QT_WAYLAND_DISABLE_WINDOWDECORATION=1
   export GDK_BACKEND=wayland
   export MOZ_ENABLE_WAYLAND=1
   export _JAVA_AWT_WM_NONREPARENTING=1
   export SDL_VIDEODRIVER=wayland
   export CLUTTER_BACKEND=wayland

   # --- Cursor (Smithay's xcursor crate honors these)
   export XCURSOR_THEME="${XCURSOR_THEME:-Adwaita}"
   export XCURSOR_SIZE="${XCURSOR_SIZE:-24}"

   # --- NVIDIA / GBM hygiene (per phase-2-c)
   # zos-wm strips GBM_BACKEND in main.rs already, but unset here too
   # so nothing leaks to its forked clients.
   unset GBM_BACKEND
   unset AQ_DRM_DEVICES
   unset __GLX_VENDOR_LIBRARY_NAME
   unset __EGL_VENDOR_LIBRARY_FILENAMES

   # --- Optional: pin render device to NVIDIA (until config support lands)
   # Uncomment to override auto-detect.
   # export ZOS_RENDER_DEVICE=/dev/dri/by-path/pci-0000:01:00.0-card

   # --- Logging
   export RUST_LOG="${RUST_LOG:-info,smithay=info}"
   export RUST_BACKTRACE=1

   # --- Hand off to compositor. exec, not background.
   exec /usr/bin/zos-wm --tty-udev
   ```

3. **Binary install**: build `zos-wm --release --features udev,xwayland`
   in the Containerfile and copy to `/usr/bin/zos-wm`.

### 3.3 Integration with the existing greetd setup

`greetd/config.toml:5` — `start-hyprland -- -c /etc/greetd/hyprland.conf`
runs the greeter UI itself in Hyprland; **leave this alone**. After
the user picks "zOS (zos-wm)" from the regreet session list, regreet
spawns `start-zos-wm` (the launcher script) as the user — this is
the standard greetd → wayland-session flow.

**No greetd config change is needed for this.** The session list is
populated automatically from `/usr/share/wayland-sessions/*.desktop`.

### 3.4 What about greetd-as-zos-wm later?

Phase 3+ goal: replace the regreet-in-Hyprland greeter with
`regreet-in-zos-wm`. That requires zos-wm to run *as the greeter*
(user `greetd`, no home dir) and is a separate question. For Phase
2, focus on user-after-login.

---

## 4. udev rules / polkit / systemd-tmpfiles

Audit of zos-wm's runtime needs:

| Capability needed | How handled today | Action |
|---|---|---|
| Open `/dev/dri/card*` as user | systemd-logind ACL on session activate | none |
| Open `/dev/input/event*` as user | systemd-logind ACL + libinput-with-libseat | none |
| Switch VTs (Ctrl+Alt+F1-F6) | logind owns this; libseat signals pause/resume | none |
| Read EDID / DRM properties | comes free with `/dev/dri/card*` access | none |
| Pipewire / xdg-desktop-portal | userspace, no extra perms | future |

**Verdict: nothing needed beyond what Bazzite already ships.**

Polkit rules: not required for the compositor. Required for
`gnome-control-center`-style settings (NetworkManager,
TimeDateMechanism) — which `zos-settings` would consume; not a
compositor concern.

systemd-tmpfiles: not required.

---

## 5. Subtask breakdown

Ordered "do first" → "do last". Each is 1-2 file scope, hand-off-able
to a single rust-expert agent.

### 5.1 Build & install plumbing (foundation)

**Task U-1 — Add `zos-wm` Containerfile build step.**
- Files: `Containerfile`.
- Add a `RUN` block after the existing Rust workspace build (around
  `Containerfile:32`) that runs
  `cargo build --release -p zos-wm --features udev,xwayland` then
  `cp /tmp/cargo-target/release/zos-wm /usr/bin/zos-wm`. The block
  re-uses the existing rust-ctx mount and cargo-target tmpfs.
- Acceptance: `podman build` produces an image with `/usr/bin/zos-wm`
  present (verify via `podman run --rm IMAGE ls -la /usr/bin/zos-wm`).
- Effort: 15 min. Trivial.

**Task U-2 — Add `just build-wm-local` recipe.**
- Files: `Justfile`.
- Adds `cargo build --release -p zos-wm --features udev,xwayland`
  for local-on-host iteration.
- Acceptance: `just build-wm-local` produces
  `target/release/zos-wm`. Prerequisite: libseat-devel installed.
- Effort: 5 min.

### 5.2 Greetd / session integration

**Task U-3 — Create `zos-wm.desktop` wayland-session entry.**
- Files: `build_files/system_files/usr/share/wayland-sessions/zos-wm.desktop`.
- Content per §3.2 above.
- Update `build_files/scripts/install-user-configs.sh` (or
  whichever script copies wayland-sessions) to copy the new file.
- Acceptance: after `podman build && bootc switch`, the regreet
  session picker shows "zOS (zos-wm)" alongside "Hyprland (zOS)".
- Effort: 15 min.

**Task U-4 — Create `start-zos-wm` launcher script.**
- Files: `build_files/system_files/usr/bin/start-zos-wm`.
- Content per §3.2 above.
- Update install script to `chmod +x` and copy to `/usr/bin/`.
- Acceptance: `which start-zos-wm` resolves on the booted image and
  the file is executable.
- Effort: 15 min.

### 5.3 Source polish

**Task U-5 — Rename `ANVIL_DRM_DEVICE` → `ZOS_RENDER_DEVICE`,
support both for back-compat.**
- Files: `zos-wm/src/udev.rs` (line 238).
- Change:
  ```rust
  let primary_gpu = if let Ok(var) = std::env::var("ZOS_RENDER_DEVICE")
      .or_else(|_| std::env::var("ANVIL_DRM_DEVICE"))
  ```
- Update tracing log at 252 to mention the env var consulted.
- Acceptance: setting `ZOS_RENDER_DEVICE=/dev/dri/by-path/...-card`
  from the launcher script overrides primary GPU detection.
- Effort: 10 min.

**Task U-6 — Add `ZOS_DISABLE_SYNCOBJ` escape hatch.**
- Files: `zos-wm/src/udev.rs` (lines 484-498).
- Wrap the `if supports_syncobj_eventfd(...)` check in
  `if !std::env::var("ZOS_DISABLE_SYNCOBJ").is_ok() && supports_syncobj_eventfd(...)`.
- Add `info!` log when syncobj is enabled and a separate `info!`
  when disabled by env.
- Acceptance: launching with `ZOS_DISABLE_SYNCOBJ=1` skips the
  syncobj global init (visible in logs).
- Effort: 10 min.

**Task U-7 — Add explicit info log for primary GPU + render node
selection.**
- Files: `zos-wm/src/udev.rs` (around lines 252, 803, 813).
- Currently logs `Using {} as primary gpu.` at 252. Also log:
  - "DRM node {} added to GpuManager (render_node={:?})" at 808.
  - "Skipping device {device_id}: {err}" already present at 403.
- Acceptance: `RUST_LOG=info zos-wm --tty-udev` from a TTY shows
  primary GPU + secondary GPU enumeration in the journal.
- Effort: 10 min.

### 5.4 First-boot diagnostics

**Task U-8 — Add `tracing` log at session-start showing seat name,
session active state, and seat0 device list.**
- Files: `zos-wm/src/udev.rs` (after line 233).
- Log `session.seat()`, the result of
  `all_gpus(session.seat())`, and the primary device path before
  `device_added` runs.
- Acceptance: journal shows seat0 enumeration before any DRM
  activity.
- Effort: 15 min.

### 5.5 Polish (post-first-boot, deferred but fits the task list)

**Task U-9 (deferred) — VT-switch resilience: try `activate(true)`
on resume.**
- Files: `zos-wm/src/udev.rs` (line 362).
- Change `activate(false)` → `activate(true)` only if first-boot
  shows VT-switch corruption. Wait for empirical signal.
- Effort: 5 min when needed.

**Task U-10 (deferred) — Read primary GPU from a config file.**
- Files: new `zos-wm/src/config.rs`, plus `udev.rs` consumer.
- Defer until wlr-output-management lands and we have a
  unified config layer.

---

## 6. Pre-flight checklist for first TTY-mode boot

Run **before** `zos-wm --tty-udev`:

1. **You are at a physical TTY.** `tty` returns `/dev/tty3` (or
   similar, NOT `/dev/pts/*`). SSH will fail with seat-not-active.
2. **Active logind session.**
   ```
   loginctl show-session $XDG_SESSION_ID | grep -E '^(Active|State)='
   ```
   Expect `Active=yes`, `State=active`.
3. **No other Wayland / X11 compositor running on this seat.**
   ```
   pgrep -a 'sway|hyprland|gnome-shell|kwin|niri|cosmic-comp|Xwayland'
   ```
   Empty output.
4. **DRM devices visible to your user.**
   ```
   getfacl /dev/dri/card2 | grep "user:$(whoami)"
   ```
   Should show `user:zach:rw-` (or similar).
5. **NVIDIA driver loaded with modeset=1 + fbdev=1.** Per phase-2-c
   §"Pre-flight checklist" items 1, 2, 8.
6. **Kernel ≥ 6.6 for syncobj.** `uname -r` → 6.17 on the live box.
7. **No `GBM_BACKEND` / `AQ_DRM_DEVICES` in env.**
   ```
   env | grep -iE 'gbm|aq_drm|glx_vendor|egl_vendor'
   ```
   Expect empty (zos-wm + launcher both unset, but verify).
8. **`/usr/bin/zos-wm` exists, is executable, links to libseat.**
   ```
   ldd /usr/bin/zos-wm | grep libseat
   ```
   Should show libseat.so.1.
9. **`RUST_LOG=info` exported.** Logs go to stderr; greetd captures
   to its own log under journalctl. To see them:
   ```
   journalctl -fb -u greetd
   ```
   Or run zos-wm directly from a TTY (without greetd) and stderr
   prints to the terminal.

**Env vars that matter at first launch:**

| Var | Effect | Default to set |
|---|---|---|
| `RUST_LOG` | tracing filter | `info,smithay=info` |
| `RUST_BACKTRACE` | panic backtraces | `1` |
| `XCURSOR_THEME` | cursor visual | `Adwaita` |
| `XCURSOR_SIZE` | cursor size | `24` |
| `XDG_SESSION_TYPE` | client hint | `wayland` |
| `XDG_CURRENT_DESKTOP` | client hint | `zos-wm` |
| `ZOS_RENDER_DEVICE` (after U-5) | force primary GPU | unset (auto) |
| `ZOS_DISABLE_SYNCOBJ` (after U-6) | debug only | unset |
| `GBM_BACKEND` | breaks compositor GBM | **must be unset** |
| `AQ_DRM_DEVICES` | unrelated, leakage from Hyprland | **must be unset** |

**What to grep in journalctl after first launch:**

- `INFO zos_wm::udev: Using ... as primary gpu` — confirms NVIDIA
  selection.
- `INFO smithay::backend::drm::device:` — DRM device opened
  successfully.
- `INFO ...: DrmSyncobjState initialized` (or equivalent after
  Task U-7) — explicit-sync globally available.
- Connector connect events for DP-1, DP-2, DP-3.
- `WARN smithay_drm_extras::display_info` — only if EDID parse
  fails, non-fatal.
- **Red flags:** `failed to initialize drm output`, `Could not
  initialize a session`, panics from `frame_finish` /
  `render_surface`.

**What `weston-terminal` / a Wayland client should see:**

- `WAYLAND_DISPLAY=wayland-1` after compositor starts (Smithay
  default).
- `XDG_RUNTIME_DIR` set by logind — required for the socket
  rendezvous.
- Connection succeeds → black screen with cursor + a terminal
  window. If the screen is black with no cursor, see phase-2-c
  §"Failure signatures" item 4 (overlay quirk regression — but the
  quirk is in place already at `udev.rs:1006`, so this should not
  fire).

---

## 7. Sources

### Local reads (zos-wm at HEAD)
- `/var/home/zach/github/zOS/zos-wm/src/udev.rs:1-1708` —
  full backend audit; specific cites inline above (227-251 session
  + primary GPU; 386-405 device enumeration; 484-498 syncobj;
  618-623 syncobj handler; 763-887 device_added; 889-1073
  connector_connected; 971-975 multi-monitor position; 1006-1014
  NVIDIA overlay quirk; 1075-1107 connector_disconnected;
  1190-1375 frame_finish; 1397-1551 render_surface).
- `/var/home/zach/github/zOS/zos-wm/Cargo.toml:1-72` — feature
  graph.
- `/var/home/zach/github/zOS/zos-wm/src/main.rs:17-78` — entrypoint
  (NVIDIA env scrub at 24, `--tty-udev` dispatch at 55).
- `/var/home/zach/github/zOS/zos-wm/src/state.rs:1-100` — state
  shape (only the imports / handler list relevant).
- `/var/home/zach/github/zOS/Cargo.toml:1-9` — workspace declaration
  (`zos-wm` is member, profile.release uses opt-level "z" — flag).
- `/var/home/zach/github/zOS/Containerfile:1-105` — image build,
  libseat-devel at line 26 already.

### Local reads (system_files)
- `/var/home/zach/github/zOS/build_files/system_files/usr/share/wayland-sessions/hyprland-zos.desktop` — pattern to copy for zos-wm.desktop.
- `/var/home/zach/github/zOS/build_files/system_files/etc/greetd/config.toml` — greeter session (untouched by this work).

### Sibling research
- `/var/home/zach/github/zOS/docs/research/phase-2-c-drm-nvidia-specifics.md` — NVIDIA driver, GBM, explicit-sync, overlay-plane quirk, libseat-on-Bazzite, Cargo audit, pre-flight items 1-11.
- `/var/home/zach/github/zOS/docs/research/phase-2-a-protocol-priority.md` — for context on which protocols zos-wm should expose.
- `/var/home/zach/github/zOS/docs/research/phase-2-b-niri-reusable-code.md` — for niri patterns.

### Memory references
- `feedback_greeter.md` — do not touch greetd config to use start-hyprland for non-Hyprland sessions.
- `feedback_aq_drm_devices.md` — clear `AQ_DRM_DEVICES` in the launcher.
- `project_hardware_zos_box.md` — NVIDIA primary on `pci-0000:01:00.0`, AMD iGPU offload-only on `pci-0000:71:00.0`, 3× 1080p60.
- `project_compositor_direction.md` — zos-wm is the Smithay anvil fork pinned to rev 27af99ef.

### Smithay (referenced indirectly via phase-2-c)
- `/tmp/smithay-peek/anvil/src/udev.rs` — anvil reference (zos-wm's ancestor); same shape.
- `/tmp/smithay-peek/src/wayland/drm_syncobj/mod.rs:67-76` — `supports_syncobj_eventfd` probe.
- `/tmp/smithay-peek/src/backend/session/libseat.rs:43-80` — LibSeatSession::new.
