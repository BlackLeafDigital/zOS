# Phase 2.C — DRM backend + NVIDIA specifics for zos-wm

Scope: everything `zos-wm` needs to know to run its `--udev` backend as the
Phase-2 compositor on Zach's daily driver (RTX 4090 + AMD 9950X3D iGPU,
Bazzite NVIDIA-Open, driver 580.95.05, 3× DisplayPort 1080p60). Sources are
the pinned Smithay checkout at `/tmp/smithay-peek/` (rev
`27af99ef492ab4d7dc5cd2e625374d2beb2772f7`, which `zos-wm` forks), niri at
`/tmp/niri-peek/`, and cosmic-comp (pop-os/cosmic-comp, master, fetched via
GitHub API). Exact lines cited inline.

---

## TL;DR

- **libseat is already usable on Bazzite via systemd-logind.** The `seatd`
  service does **not exist** on Bazzite (`systemctl status seatd` → "could
  not be found") and there is no `seatd` binary; only `libseat-0.9.3` is
  installed. `libseat` transparently falls back to the logind backend, so
  Smithay's `backend_session_libseat` works unmodified. Do **not** try to
  enable `seatd.service`.
- **`nvidia_drm.modeset=1` and `fbdev=1` are already on**, configured by
  Bazzite's `/usr/lib/modprobe.d/nvidia-modeset.conf` (confirmed live:
  `/sys/module/nvidia_drm/parameters/modeset = Y`,
  `/sys/module/nvidia_drm/parameters/fbdev = Y`, driver version
  `580.95.05`). No kernel command-line edit is needed.
- **Explicit sync (`linux-drm-syncobj-v1`) is required for correctness on
  NVIDIA 555+.** `zos-wm` already inherits the anvil wiring at
  `zos-wm/src/udev.rs:94,135,267,484-498,618-623` — keep it. Do not disable
  it unless debugging.
- **Do NOT set `GBM_BACKEND=nvidia-drm` globally.** Modern compositor-side
  GBM on NVIDIA 555+ works through the ordinary `libgbm` that picks up the
  `nvidia-drm` backend by filename once `nvidia_drm.modeset=1` is set. That
  env var is a client-side legacy knob for EGL applications; setting it in
  the compositor's environment can break native-GBM paths. Memory already
  has user feedback that `AQ_DRM_DEVICES` colons shred Hyprland; don't
  replicate that class of bug.
- **Bind only the NVIDIA render node (`renderD129`,
  `pci-0000:01:00.0-render`) as primary.** The AMD iGPU (`renderD128`,
  `pci-0000:71:00.0-render`) is not driving a display and should be
  **ignored** at startup, not added as a secondary render node. niri and
  cosmic-comp both do this: they probe the primary, and any additional node
  only joins `GpuManager` when a surface on that node actually needs
  rendering (e.g., a secondary GPU display-only node with a client render
  node on the primary — inverse of our topology).
- **Apply the COSMIC "no overlay planes on NVIDIA" quirk.** Cosmic-comp
  explicitly clears `planes.overlay` when the driver name contains
  `"nvidia"` (`src/backend/kms/mod.rs:876-887`), citing "QUIRK: Using an
  overlay plane on a nvidia card breaks the display controller (wtf...)".
  `zos-wm` should port this quirk into its device-added path before it
  burns the first hour of testing on a mystery black-screen.
- **Cargo features need two edits.** (1) Add `smithay/xwayland` +
  `smithay/x11rb_event_source` to the `udev` feature set (or at least
  ensure the `xwayland` feature composes). (2) `renderer_pixman` is
  CPU-fallback and won't help NVIDIA — keep it only if you want a
  well-known-good recovery path for renderer bring-up, otherwise cut.
  `backend_vulkan` is currently compiled but unused by any code path we
  ship in Phase 2; leave it for now since the compile-time cost is already
  paid via the Smithay build.

---

## Smithay backend stack

The zos-wm `udev` binary is a hand-me-down of
`/tmp/smithay-peek/anvil/src/udev.rs` (~2200 lines). The structural pieces
it composes:

### UdevBackend — hotplug monitor

`/tmp/smithay-peek/src/backend/udev.rs:58-128`

- `UdevBackend::new("seat0")` opens a `udev::MonitorSocket` filtering the
  `"drm"` subsystem, takes an initial snapshot via `all_gpus(seat)`, and
  implements `calloop::EventSource`.
- Events: `UdevEvent::Added { device_id, path }`,
  `UdevEvent::Changed { device_id }`, `UdevEvent::Removed { device_id }`.
- Consumed in `anvil/src/udev.rs:502-522` — added devices feed
  `device_added(DrmNode, &Path)`, changed feed `device_changed`, removed
  feed `device_removed`.
- `device_list()` is the initial enumeration the compositor calls *before*
  inserting the source into the event loop — see
  `anvil/src/udev.rs:379-396`.

### DrmDevice + DrmSurface — per-GPU atomic state

`/tmp/smithay-peek/src/backend/drm/mod.rs:1-100` for the module docs;
`src/backend/drm/device.rs`, `src/backend/drm/surface/*.rs` for the impl.

- `DrmDevice::new(fd: DrmDeviceFd, disable_connectors: bool) ->
  (DrmDevice, DrmDeviceNotifier)` — cosmic-comp calls this in
  `device_added()` (cosmic-comp `src/backend/kms/device.rs:220-227`). The
  returned notifier is a calloop source yielding `DrmEvent::VBlank(crtc)`
  and `DrmEvent::Error(err)`.
- Atomic vs legacy is auto-detected: `DrmSurfaceInternal::Atomic` /
  `Legacy` enum at `src/backend/drm/surface/mod.rs:172-176`, selected by
  whether the kernel driver exposes `DRM_CLIENT_CAP_ATOMIC`. NVIDIA since
  495+ does. `drm.is_atomic()` is queried; see cosmic-comp
  `src/backend/kms/device.rs:214`.
- `DrmSurface::vrr_supported(conn)` at
  `src/backend/drm/surface/mod.rs:307-317` — returns
  `VrrSupport::NotSupported` for legacy surfaces, otherwise queries the
  `vrr_capable` connector property via atomic
  (`src/backend/drm/surface/atomic.rs:558-615`). The three states:
  `NotSupported`, `RequiresModeset`, `Supported`. NVIDIA on DP
  G-Sync-Compatible panels reports `Supported` on 525.53+.
- `DrmCompositor<A, F, U, G>` (at
  `src/backend/drm/compositor/mod.rs`) is the hardware-composition
  fast-path that stitches scanout-plane promotion and damage tracking. It
  is the type anvil / zos-wm wraps inside `DrmOutput` /
  `DrmOutputManager`.

### Session / libseat

`/tmp/smithay-peek/src/backend/session/mod.rs:1-60` trait;
`/tmp/smithay-peek/src/backend/session/libseat.rs:43-80` impl.

- `LibSeatSession::new() -> (LibSeatSession, LibSeatSessionNotifier)`
  opens the seat via the C `libseat_open_seat` callback; the notifier is a
  calloop source producing `SessionEvent::PauseSession` and
  `::ActivateSession`.
- `Session::open(&Path, OFlags) -> OwnedFd` is what the compositor hands
  to `DrmDevice::new` — libseat delegates to logind if seatd is absent, so
  the compositor process doesn't need CAP_SYS_ADMIN on `/dev/dri/card*`.
- Pause/resume flow: anvil `udev.rs:328-371` — on pause, call
  `libinput_context.suspend()`, walk `backends` and call
  `drm_output_manager.pause()` on each; on activate, call
  `libinput_context.resume()` and
  `drm_output_manager.lock().activate(false)`. zos-wm already has this.

### GBM allocator

`/tmp/smithay-peek/src/backend/allocator/gbm.rs`

- `GbmDevice::new(fd)` wraps the `DrmDeviceFd` in a `gbm_device*`.
- `GbmAllocator::new(gbm, flags: GbmBufferFlags)` produces BOs for
  framebuffers. Flags used by anvil: `GbmBufferFlags::RENDERING |
  GbmBufferFlags::SCANOUT` — same used by cosmic-comp, same used by niri.

### Renderer: GlesRenderer over EGL

`/tmp/smithay-peek/src/backend/renderer/gles/mod.rs`; EGL context creation
at `src/backend/egl/context.rs`. The anvil-style factory:

```rust
GpuManager::new(GbmGlesBackend::with_factory(|display| {
    let context = EGLContext::new_with_priority(display, ContextPriority::High)?;
    let mut capabilities = unsafe { GlesRenderer::supported_capabilities(&context)? };
    Ok(unsafe { GlesRenderer::with_capabilities(context, capabilities)? })
}))
```
(anvil `src/udev.rs:254-262`). niri does the same at
`src/backend/tty.rs:463-464` but with `GbmGlesBackend::with_context_priority`.

### MultiRenderer / GpuManager

`/tmp/smithay-peek/src/backend/renderer/multigpu/mod.rs:1-40` module docs.

- A `GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>` tracks one or
  more GPUs (identified by `DrmNode`). Rendering always executes on the
  **render-gpu**; the result is blitted to the **target-gpu** if they
  differ. On our box, render-gpu == target-gpu == NVIDIA 4090, so the
  multi path collapses to a single-renderer fast path (`single_renderer`
  at anvil `src/udev.rs:175,410,416,1437`).
- Critical: "This module will not keep you from selecting sub-optimal
  configurations" (src/backend/renderer/multigpu/mod.rs:37-40). The
  compositor author must decide which nodes to `add_node`. niri adds a
  node only when a device is successfully probed; cosmic-comp does the
  same. Neither eagerly enumerates all `all_gpus()`.

### smithay-drm-extras

`/tmp/smithay-peek/smithay-drm-extras/` provides `DrmScanner` (the
connector-change diff helper used in anvil's `connector_connected` /
`connector_disconnected` paths) and `display_info` (EDID parsing). Already
a dep of `zos-wm` via `[dependencies.smithay-drm-extras]`.

---

## NVIDIA-specific requirements

### GBM backend selection (`GBM_BACKEND=nvidia-drm`)

For a **compositor** using Smithay on NVIDIA 555+: **do not set it**.
Smithay opens the DRM device via libseat, then creates a `GbmDevice` on
that fd. Mesa's `libgbm` dispatches by kernel driver name on the fd, not by
`GBM_BACKEND`. `GBM_BACKEND` is a client-side selector that some EGL apps
use when creating a GBM device off an *arbitrary* render node (e.g.,
hardware-video-decode libraries); setting it in the compositor's
environment can break clients that inherit the env and then try to open
Mesa-backed render nodes.

Current compositor stance across the ecosystem (from perplexity + upstream
bug trackers):
- Hyprland historically required it on hybrid laptops
  (hyprwm/Hyprland#4274, #1878) but that is a *compositor-level* workaround
  for Hyprland's own EGL init path, not a general Wayland rule.
- niri documents zero mention of `GBM_BACKEND` in its Nvidia wiki page
  (`/tmp/niri-peek/docs/wiki/Nvidia.md`); it only calls out the
  `GLVidHeapReuseRatio` VRAM-leak workaround.
- cosmic-comp has zero references to `GBM_BACKEND` or `nvidia-drm` in its
  backend (grepped mod.rs + device.rs).

**Recommendation:** unset `GBM_BACKEND` explicitly in `zos-wm`'s process
env before spawning clients, so whatever SDDM / greetd / user rc file set
it does not leak to Wayland clients. `std::env::remove_var("GBM_BACKEND")`
in `main.rs` before any EGL init.

### DRM format modifiers exported by NVIDIA for scanout

NVIDIA 555+ exports a small, well-known modifier set for scanout:
`DRM_FORMAT_MOD_LINEAR`, and vendor-specific `DRM_FORMAT_MOD_NVIDIA_*`
tiled modifiers (`NVIDIA_BLOCK_LINEAR_2D` family). The specific list
changes per ASIC generation; on Ada (RTX 40-series) expect `XRGB8888` /
`ARGB8888` / `ABGR2101010` / `ARGB2101010` with linear + NV block-linear
modifiers. No public NVIDIA docs enumerate them exhaustively (see
perplexity note for 580.x: "exported modifiers ... are not documented
here").

What matters for Smithay: the format list you pass to
`GbmDrmOutputManager::new(...)` is intersected with what the driver
actually exports on the fly. Cosmic-comp passes `[Abgr2101010,
Argb2101010, Abgr8888, Argb8888]` (cosmic-comp
`src/backend/kms/device.rs:291-295`); anvil passes the same
(`anvil/src/udev.rs:110-115`). Copy that list.

`DMA_BUF_IOCTL_EXPORT_SYNC_FILE`: implicit sync via dma-buf sync file is
spotty on NVIDIA and is the whole reason `linux-drm-syncobj-v1` was
pushed. Assume you cannot rely on implicit sync.

`EGL_ANDROID_native_fence_sync`: present on NVIDIA's EGL implementation
for a long time (525+), used by Smithay internally via
`src/backend/renderer/gles/sync.rs`. Works.

### Explicit sync (`linux-drm-syncobj-v1`)

Required for correctness on NVIDIA 555+. Without it you get frame tearing,
intermittent black frames on app startup, and screencast glitches (the
last of which niri observed and fixed upstream;
`/tmp/niri-peek/docs/wiki/Nvidia.md:43-55`).

Smithay's wiring is concentrated in two files:
- Protocol impl: `src/wayland/drm_syncobj/mod.rs:1-40` (the public API —
  `DrmSyncobjState`, `DrmSyncobjHandler`, `supports_syncobj_eventfd`).
- Capability probe: `src/wayland/drm_syncobj/mod.rs:67-76` — calls the
  `drm_ffi::syncobj::eventfd` ioctl with a placeholder eventfd; returns
  `true` iff the kernel returns anything other than ENOENT
  (kernel 6.6+).

Compositor-side work (already present in `zos-wm/src/udev.rs` inherited
from anvil):
1. After primary GPU is selected, pull its `DrmDeviceFd` (anvil
   `udev.rs:491`).
2. If `supports_syncobj_eventfd(&import_device)` returns true, create
   `DrmSyncobjState::new::<State>(display_handle, import_device)` and
   store on backend data (`udev.rs:492-496`).
3. Implement `DrmSyncobjHandler` (`udev.rs:618-623`).
4. Macro-delegate: `smithay::delegate_drm_syncobj!(AnvilState<UdevData>)`.

cosmic-comp has an escape hatch env var
`COSMIC_DISABLE_SYNCOBJ` (`src/backend/kms/mod.rs:459`); consider adding
`ZOS_DISABLE_SYNCOBJ` for debugging.

### Atomic modesetting

Anvil / zos-wm always request atomic via `DrmDevice::new(fd, false)` (the
`false` here is `disable_connectors`, not atomic-opt-in — atomic is
auto-used when the driver supports it). NVIDIA 525+ advertises atomic on
Volta+; RTX 4090 = Ada = yes.

Known atomic-fail mode on NVIDIA, **ported from cosmic-comp** and worth
replicating:

```rust
// cosmic-comp src/backend/kms/mod.rs:876-887
let driver = drm.device().get_driver().ok();
if driver.as_ref().is_some_and(|d| {
    d.name().to_string_lossy().to_lowercase().contains("nvidia")
}) {
    planes.overlay = vec![]; // QUIRK: nvidia + overlay plane = dead display controller
}
```

Apply at the same point zos-wm enumerates planes for a crtc (the
`DrmOutputManager::initialize_output` call site, which today pulls planes
implicitly). If you construct `DrmCompositor` directly via the low-level
API, filter `planes.overlay` to empty on NVIDIA before passing to
`DrmCompositor::new`.

### VRR / adaptive-sync

Supported. Flow:
1. After surface/crtc setup, for each connector call
   `drm_output.with_compositor(|c| c.vrr_supported(connector))` — returns
   `VrrSupport::Supported`, `::RequiresModeset`, or `::NotSupported`.
2. Per user config, call `drm_output.use_vrr(true)` (cosmic-comp does this
   inside `populate_modes()` — sets `vrr: AdaptiveSync::Enabled` by
   default at `src/backend/kms/device.rs:644-645`).
3. If `::RequiresModeset`, the change applies on the next
   full-commit (mode change). If `::Supported`, it applies immediately.

For Zach's G-Sync Compatible 1080p DP panels: expect `::Supported` after
the NVIDIA 525.53+ vrr_capable property fix.

### HDR / Color Management

Out of scope for Phase 2. Smithay has no HDR scaffolding yet; NVIDIA's
Wayland HDR support (as of 580.95) is experimental and goes through the
`wp_color_management_v1` draft protocol which Smithay does not yet
implement. Skip entirely for Phase 2.

### Primary vs secondary GPU

On Zach's box the topology is: NVIDIA drives all 3 displays; AMD iGPU has
no scanout duty. Correct configuration:

- Primary node: `/dev/dri/by-path/pci-0000:01:00.0-card` → card2 →
  `nvidia-drm`. Render node: `renderD129`.
- Do not add the AMD iGPU to `GpuManager` at startup.
- Only probe/add the AMD iGPU if a client specifically requests it
  (future: render-offload for specific apps). For now, mirror niri's
  behavior: `ignored_nodes = compute_ignored_nodes()`
  (`/tmp/niri-peek/src/backend/tty.rs:522`) — niri has explicit
  config-driven ignored-nodes logic.

Cosmic-comp reference: `determine_primary_gpu` at cosmic-comp
`src/backend/kms/mod.rs:199-237` — (a) honor `COSMIC_RENDER_DEVICE` env,
(b) prefer the GPU with a built-in display (`eDP`, `LVDS`, `DSI`), (c)
fall back to boot GPU. For zos-wm, adopt the same pattern with
`ZOS_RENDER_DEVICE`. On a desktop with no built-in panel, step (c) fires
and picks whichever GPU has a connected display; on Zach's box that's the
NVIDIA.

Anvil's primary-gpu logic is simpler (`anvil/src/udev.rs:238-251`) —
env-override via `ANVIL_DRM_DEVICE`, else `primary_gpu(session.seat())`
from Smithay (boot_vga udev attribute), else first-found. On Zach's
machine the NVIDIA is `boot_vga=1` (standard for a desktop with a
dedicated GPU), so `primary_gpu` will return it without config.

---

## Bazzite / libseat session setup

Observed live on the machine at research time (same hardware, same driver
stack zos-wm targets):

| Check | Value |
|---|---|
| `systemctl status seatd` | `Unit seatd.service could not be found.` |
| `systemctl status systemd-logind` | active (running) |
| `rpm -qa '*seat*'` | `libseat-0.9.3-1.fc43.x86_64` only |
| `ls /usr/bin/seatd*` | no match |
| `/dev/dri/card1,card2` permissions | `crw-rw----+` (ACL, `+`) |
| User groups (zach) | `zach wheel zlayer` (**not** video/input/render) |
| `/sys/module/nvidia_drm/parameters/modeset` | `Y` |
| `/sys/module/nvidia_drm/parameters/fbdev` | `Y` |
| Driver version | `580.95.05` |
| Kernel cmdline | no `nvidia-drm.modeset=1` (loaded via `/usr/lib/modprobe.d/nvidia-modeset.conf`) |
| Bazzite nvidia module location | `/lib/modules/.../kernel/drivers/custom/nvidia-lts/` |

Implications for `zos-wm`:

1. **`LibSeatSession::new()` will use the logind backend** (libseat's
   runtime detection: if `$XDG_SESSION_ID` or systemd D-Bus is reachable
   and seatd is absent, it uses logind). Do not gate on `seatd` being
   present in any install script.
2. **User does not need to be in video/input groups.** The ACL on
   `/dev/dri/card*` (from systemd-logind seat-assignment) and libinput's
   own logind integration cover access. If you later ship a seatd-only
   install (no systemd, e.g., a recovery initrd), that changes.
3. **VT switching**: on `Ctrl+Alt+F3`, logind signals libseat →
   `SessionEvent::PauseSession` fires in the calloop source. Handler must:
   - `libinput_context.suspend()` (paused libinput keeps fds but ignores
     events).
   - `drm_output_manager.pause()` per device (drops master, closes KMS
     fences).
   - Clear active leases, suspend lease globals.
   On return (`ActivateSession`):
   - `libinput_context.resume()` (may fail once; log and continue).
   - `drm_output_manager.lock().activate(false)` — the `false` is "don't
     reset connectors"; anvil uses `false`, cosmic-comp uses `true`
     (`apply_reset = true` in `resume_session`). `true` gives a
     clean-slate modeset at the cost of a visible flash; `false` hopes
     the state survived. **For zos-wm, start with `false` and flip to
     `true` if we hit corruption after VT switch.**
4. **Running `zos-wm --udev` from a TTY**: as the logged-in user. No
   root, no setuid. Ensure the user has an active logind session
   (`loginctl show-session $XDG_SESSION_ID` — check `Active=yes`). If
   launched via `ssh`, `$XDG_SESSION_TYPE` will be tty but `Active=no`
   and libseat will refuse; this is fine for dev (run from physical
   TTY).

---

## COSMIC-comp reference patterns

Cosmic-comp is the closest-mapping Smithay-based compositor we can crib
from; both anvil and cosmic-comp share Smithay internals. Specific
patterns worth lifting:

- **Device init orchestration** (`src/backend/kms/mod.rs:114-145`,
  `init_backend`): session → libinput → udev → enumerate
  `device_list()` → `device_added()` → `select_primary_gpu()`. Same order
  anvil uses.
- **Primary GPU selection with env override + built-in display
  preference** (`src/backend/kms/mod.rs:199-237`). Port to `zos-wm` with
  `ZOS_RENDER_DEVICE`.
- **Explicit-sync probe with kill switch**
  (`src/backend/kms/mod.rs:459-478`): `if
  !bool_var("COSMIC_DISABLE_SYNCOBJ") { if supports_syncobj_eventfd() {
  ... } }`. Add a `ZOS_DISABLE_SYNCOBJ` escape hatch.
- **NVIDIA overlay-plane quirk** (`src/backend/kms/mod.rs:876-887`).
  Port verbatim.
- **Pause/resume**
  (`src/backend/kms/mod.rs:359-433`): `pause_session` suspends libinput +
  lease state + drm; `resume_session` activates drm with
  `apply_reset=true` and schedules an idle callback. Anvil is similar but
  uses `activate(false)`.
- **Format list** — same four formats:
  `[Abgr2101010, Argb2101010, Abgr8888, Argb8888]` (cosmic-comp
  `device.rs:291-295`). Already matches anvil/zos-wm.
- **EVDI quirk** (cursor planes on EVDI; `mod.rs:890-898`) — not
  relevant to Zach's hardware but a reminder that vendor quirks live in
  this code path.

Not lifted (out of scope for Phase 2 or too opinionated):
- COSMIC's `apply_config_for_outputs` machinery (monitor-layout persistence) —
  zos-wm doesn't have a config format yet; hard-code 3-monitor arrangement
  or defer to Phase 3.
- Screencast + pipewire integration.

---

## Pre-flight checklist

Walk before the first `zos-wm --udev` boot.

1. **Kernel modules loaded with modeset=1**
   Check: `lsmod | grep nvidia_drm`; `sudo cat
   /sys/module/nvidia_drm/parameters/modeset` should print `Y`.
   Fix if wrong: ensure `/usr/lib/modprobe.d/nvidia-modeset.conf`
   contains `options nvidia-drm modeset=1`, regenerate initramfs
   (`sudo dracut --force`), reboot. On Bazzite this is shipped by
   default; if it drifts, file against the Bazzite NVIDIA image.
2. **Driver version at or above 555**
   Check: `modinfo nvidia | grep ^version` → expect `580.95.05`.
   Fix: rebase container image; the Bazzite NVIDIA image ships a pinned
   driver.
3. **libseat + logind reachable**
   Check: `loginctl show-session $XDG_SESSION_ID | grep -E
   'Active|State'` → `Active=yes`, `State=active`. `rpm -q libseat`
   (should succeed).
   Fix: log in at the physical console, not over ssh. libseat-0.9.3+ is
   always present on Bazzite.
4. **DRI devices visible + ACL matches logged-in user**
   Check: `ls -la /dev/dri/card*` → look for the `+` at the end of the
   permission bits (ACL); `getfacl /dev/dri/card2` → should list the
   logged-in user.
   Fix: log in at the physical console (ACL is granted by
   systemd-logind on session activation).
5. **by-path symlinks present for GPU routing**
   Check: `ls -la /dev/dri/by-path/` → expect
   `pci-0000:01:00.0-{card,render}` (NVIDIA) and
   `pci-0000:71:00.0-{card,render}` (AMD).
   Fix: missing symlinks = missing udev rules (unlikely on Bazzite); check
   `/usr/lib/udev/rules.d/60-drm.rules`.
6. **No rogue `GBM_BACKEND` in env**
   Check: `env | grep -i gbm`. Expect no output.
   Fix: unset in the launch script; add `std::env::remove_var("GBM_BACKEND")`
   in `zos-wm/src/main.rs` before EGL init.
7. **No rogue `AQ_DRM_DEVICES` in env**
   (Per existing memory: Hyprland-specific var; if leaked from a previous
   Hyprland config, can confuse libs that happen to read it.)
   Check: `env | grep -i aq_drm`.
   Fix: unset before launch.
8. **`nvidia_drm.fbdev=1`**
   Check: `sudo cat /sys/module/nvidia_drm/parameters/fbdev` → `Y`. This
   gives us a fbdev for plymouth / greetd hand-off.
   Fix: add `options nvidia-drm fbdev=1` to the modprobe file; already
   the 570+ default.
9. **Explicit-sync kernel support**
   Check: kernel ≥ 6.6. Bazzite tracks mainline; `uname -r` → 6.17+ on
   observed system. If `supports_syncobj_eventfd()` returns false at
   runtime, check kernel; the Smithay probe is at
   `src/wayland/drm_syncobj/mod.rs:67-76`.
10. **Cargo feature set** — see next section.
11. **SELinux context** (Bazzite-specific)
    Check: `id -Z` returns `unconfined_u:unconfined_r:unconfined_t` (as
    observed). If you ever ship zos-wm under a confined domain, DRM
    access needs explicit policy. Not a Phase-2 problem.

---

## Cargo.toml audit

Current `udev` feature set (`zos-wm/Cargo.toml:48-62`):

```toml
udev = [
  "smithay-drm-extras",
  "smithay/backend_libinput",
  "smithay/backend_udev",
  "smithay/backend_drm",
  "smithay/backend_gbm",
  "smithay/backend_vulkan",
  "smithay/backend_egl",
  "smithay/backend_session_libseat",
  "image",
  "smithay/renderer_gl",
  "smithay/renderer_pixman",
  "smithay/renderer_multi",
  "xcursor",
]
```

Assessment:

| Feature | Verdict | Reason |
|---|---|---|
| `smithay-drm-extras` | keep | `DrmScanner`, `display_info` used throughout udev.rs |
| `smithay/backend_libinput` | keep | `LibinputInputBackend` |
| `smithay/backend_udev` | keep | `UdevBackend` |
| `smithay/backend_drm` | keep | `DrmDevice`, `DrmSurface`, `DrmCompositor` |
| `smithay/backend_gbm` | keep | `GbmDevice`, `GbmAllocator` |
| `smithay/backend_vulkan` | **remove** | No code path in zos-wm uses Vulkan renderer; only pulls in ash + libloading + 1-2MB compile cost for nothing. Revisit in Phase 3 if we want a Vulkan fast-path. |
| `smithay/backend_egl` | keep | EGL context creation |
| `smithay/backend_session_libseat` | keep | session handling |
| `image` | keep | `MemoryRenderBuffer` cursor import |
| `smithay/renderer_gl` | keep | `GlesRenderer` |
| `smithay/renderer_pixman` | **keep for now** | CPU fallback; useless on NVIDIA but a good bring-up crutch. Drop once udev boots cleanly. |
| `smithay/renderer_multi` | keep | `MultiRenderer` / `GpuManager` (even single-GPU path routes through it in anvil style) |
| `xcursor` | keep | cursor theme |

**Missing / recommended additions:**

- `smithay/xwayland` + `smithay/x11rb_event_source` — currently gated
  under `zos-wm`'s separate `xwayland` feature (Cargo.toml:65). The
  `udev` feature should compose XWayland in when available:
  ```toml
  udev = [ ..., "xwayland" ]  # or require users to build with --features "udev,xwayland"
  ```
  Pick one; anvil's pattern is the latter. Document in README.
- `smithay/wayland_frontend` — already pulled in implicitly via the
  default `features = ["desktop", "wayland_frontend"]` at
  Cargo.toml:32. Fine.
- `smithay/use_system_lib` (via `egl` feature) — already a separate
  feature. Not required for NVIDIA; useful if you want to link against
  the system's `libEGL` rather than the EGL loader stub. Leave as
  opt-in.

**Diff to apply** (minimal version — keep pixman for now):

```diff
 udev = [
   "smithay-drm-extras",
   "smithay/backend_libinput",
   "smithay/backend_udev",
   "smithay/backend_drm",
   "smithay/backend_gbm",
-  "smithay/backend_vulkan",
   "smithay/backend_egl",
   "smithay/backend_session_libseat",
   "image",
   "smithay/renderer_gl",
   "smithay/renderer_pixman",
   "smithay/renderer_multi",
   "xcursor",
 ]
```

Document in `zos-wm/README.md`: "For NVIDIA daily-driver, build with
`cargo build --release --features udev,xwayland`."

---

## First-boot sanity-check

### Happy path output

Running `RUST_LOG=info zos-wm --udev` from a TTY should produce,
roughly in order:

```
INFO backend_session{type=libseat}: Seat enabled
INFO backend_udev{seat=seat0}: 2 DRM devices in initial snapshot
INFO zos_wm::udev: Using /dev/dri/renderD129 (DrmNode{...}) as primary gpu.
INFO smithay::backend::drm::device: DRM device opened: driver=nvidia-drm atomic=true
INFO zos_wm::udev: DrmSyncobjState initialized on primary GPU
INFO smithay::backend::renderer::gles: GLES 3.2, vendor=NVIDIA, renderer=NVIDIA GeForce RTX 4090/PCIe/SSE2
INFO zos_wm::udev: Connector DP-1 connected, preferred mode 1920x1080@60
INFO zos_wm::udev: Connector DP-2 connected, preferred mode 1920x1080@60
INFO zos_wm::udev: Connector DP-3 connected, preferred mode 1920x1080@60
INFO zos_wm::udev: VRR: DP-1 Supported, DP-2 Supported, DP-3 Supported
INFO smithay::backend::drm::compositor: Initialized DrmCompositor for crtc 0
...
```

Followed by a black screen (no clients yet) with cursor — at that point
`WAYLAND_DISPLAY=wayland-1 weston-terminal` from another TTY /
ssh-with-XDG_RUNTIME_DIR should open a window.

### Failure signatures

1. **`backend_session_libseat: Error: open_seat failed: No such file or
   directory`** → running from ssh or a non-active session. Fix: move to
   physical TTY, re-run.

2. **`DrmDevice::new: EINVAL on DRM_IOCTL_SET_CLIENT_CAP`** or
   `AtomicDrmSurface::new: kernel doesn't support atomic` →
   `nvidia-drm.modeset=1` not effective. Fix: checklist item 1; reboot
   after modprobe change.

3. **Long stall (30+ s) then `GlesRenderer::new: EGL_NOT_INITIALIZED`** →
   EGL can't find `libEGL_nvidia.so.0`. Fix: on Bazzite-NVIDIA this is a
   broken image; `rpm-ostree status` and rebase to a known-good tag.
   Verify `/usr/lib64/libEGL_nvidia.so.0` exists.

4. **Works, then black display after first `render_frame` call, kernel
   log spams `nvidia-drm: [drm]` errors about plane state** → the NVIDIA
   overlay-plane bug. Fix: apply the cosmic-comp quirk from this doc.

5. **Clients render but visibly tear or show black-frames-on-startup** →
   explicit sync not active. Check for `DrmSyncobjState initialized`
   log line; if missing, `supports_syncobj_eventfd` returned false →
   kernel < 6.6 or syncobj disabled. Fix: verify kernel ≥ 6.6;
   double-check `ZOS_DISABLE_SYNCOBJ` not set.

6. **VT switch (`Ctrl+Alt+F3`) returns to a black compositor** → pause/
   resume handler crashed mid-activate. Logs will show
   `DrmCompositor::commit: EPERM` — compositor lost DRM master and did
   not reacquire. Fix: in `ActivateSession` handler, try
   `drm_output_manager.lock().activate(true)` instead of `false` (see
   Bazzite section item 3).

7. **`all_gpus()` returns only the AMD iGPU** → NVIDIA not in the seat's
   allowed-device list. Check `udevadm info /dev/dri/card2 | grep
   TAGS` — should include `master-of-seat`. On Bazzite this is default;
   if broken, file against Bazzite.

---

## Sources

### Smithay (local, pinned rev 27af99ef)
- `/tmp/smithay-peek/src/backend/udev.rs` — UdevBackend, 58-128
- `/tmp/smithay-peek/src/backend/drm/mod.rs` — DRM module docs, 1-100
- `/tmp/smithay-peek/src/backend/drm/surface/mod.rs` — VrrSupport enum, 160-170
- `/tmp/smithay-peek/src/backend/drm/surface/atomic.rs` — vrr_capable query, 558-615
- `/tmp/smithay-peek/src/backend/session/mod.rs` — Session trait, 1-60
- `/tmp/smithay-peek/src/backend/session/libseat.rs` — LibSeatSession, 43-80
- `/tmp/smithay-peek/src/backend/renderer/multigpu/mod.rs` — GpuManager docs, 1-40
- `/tmp/smithay-peek/src/backend/allocator/gbm.rs` — GbmAllocator
- `/tmp/smithay-peek/src/wayland/drm_syncobj/mod.rs` — explicit-sync protocol, 1-80
- `/tmp/smithay-peek/anvil/src/udev.rs` — reference compositor (zos-wm's ancestor):
  primary GPU selection 238-251, GpuManager init 254-262, pause/resume 328-371,
  device enumeration 374-396, syncobj wiring 483-498, DrmSyncobjHandler 618-623.

### zos-wm (local)
- `/var/home/zach/github/zOS/zos-wm/Cargo.toml` — current features, lines 48-62.
- `/var/home/zach/github/zOS/zos-wm/src/udev.rs` — inherited anvil udev backend.

### niri (local)
- `/tmp/niri-peek/src/backend/tty.rs` — primary_render_node selection 466-502;
  single-renderer pattern, ignored_nodes 522.
- `/tmp/niri-peek/docs/wiki/Nvidia.md` — VRAM leak workaround, screencast fix.

### cosmic-comp (fetched via GitHub API, master branch)
- `src/backend/kms/mod.rs` — init_backend 114-145; determine_primary_gpu 199-237;
  syncobj probe 459-478; NVIDIA overlay quirk 876-887; pause/resume 359-433.
- `src/backend/kms/device.rs` — DrmDevice::new call 220-227; VRR default 644-645;
  format list 291-295.

### Machine state (observed live at research time)
- `/sys/module/nvidia_drm/parameters/{modeset,fbdev}` → both `Y`.
- `/usr/lib/modprobe.d/nvidia-modeset.conf` → `options nvidia-drm modeset=1`.
- `modinfo nvidia` → version 580.95.05, from
  `/lib/modules/6.17.7-ba29.fc43.x86_64/kernel/drivers/custom/nvidia-lts/nvidia.ko.xz`.
- `/dev/dri/by-path/` → `pci-0000:01:00.0-card` → card2 (NVIDIA),
  `pci-0000:71:00.0-card` → card1 (AMD).
- `rpm -qa '*seat*'` → `libseat-0.9.3-1.fc43`; no seatd binary.
- `systemctl status seatd` → `Unit seatd.service could not be found.`
- `systemctl status systemd-logind` → active (running).

### Perplexity (web-grounded, 2024-2025 sources)
- NVIDIA 580.x + GBM_BACKEND / modeset=1 / fbdev=1 state: hyprwm/Hyprland#4274,
  hyprwm/Hyprland#1878, forums.developer.nvidia.com t/204068, t/341254
  (580.65.06 DRM handoff regression), bbs.archlinux.org viewtopic.php?id=301525.
- VRR on NVIDIA: forums.developer.nvidia.com t/220822 (G-Sync/FreeSync Wayland),
  t/248605 (G-Sync not working properly).
- Explicit-sync requirement: kernel 6.12+ syncobj-v1 thread.
- Smithay multi-GPU behavior: smithay.github.io/smithay/.../multigpu/index.html.
- Bazzite / Fedora libseat ↔ logind coexistence: docs.fedoraproject.org
  packaging-guidelines/DefaultServices/, bbs.archlinux.org viewtopic.php?id=277711,
  git.sr.ht/~kennylevinsen/seatd.

### Upstream reference (not re-researched)
- Bazzite NVIDIA image: ublue-os/bazzite-nvidia (pinned nvidia-lts 580.95.05).
- Smithay pin: rev 27af99ef492ab4d7dc5cd2e625374d2beb2772f7 (matches
  `zos-wm/Cargo.toml:24,30`).
