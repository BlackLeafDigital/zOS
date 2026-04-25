# Phase 2 Research: `wp-tearing-control-v1` from Scratch

## TL;DR

- Smithay at our pinned commit `27af99ef492ab4d7dc5cd2e625374d2beb2772f7` has **no** `tearing_control` module under `src/wayland/`. We must implement the protocol ourselves in `zos-wm/src/protocols/tearing_control.rs`.
- The Rust wire bindings already ship: `smithay::reexports::wayland_protocols::wp::tearing_control::v1::server::*` (smithay enables `wayland-protocols` with the `staging` feature).
- The dispatch shape is small: one `GlobalDispatch` for the manager, one `Dispatch` per request (manager + per-surface), one `Cacheable` for double-buffered hint state, and a per-surface "already attached" guard mirroring smithay's `content_type` implementation.
- The render-side hookup is non-trivial: smithay 27af99e's `DrmCompositor::page_flip`/`commit` does **not** expose `AtomicCommitFlags::PAGE_FLIP_ASYNC`. We can land the protocol now (clients stop being unhappy, hint is honored at the protocol level) but the actual async page-flip submission needs a follow-up — either a smithay patch upstream or a thin `DrmSurface::page_flip` shim until then.
- Reference compositors (niri, cosmic-comp) do **not** ship this protocol either, so we're alone on patterns. We pull the dispatch shape from smithay's own `content_type` (which is similarly per-surface, double-buffered) — that file is MIT, so the patterns are licence-clean.

---

## 1. Verification: Smithay 27af99e Lacks `wp_tearing_control`

Checked locally at `/home/zach/.cargo/git/checkouts/smithay-312425d48e59d8c8/27af99e/`:

```
$ ls src/wayland/
alpha_modifier  background_effect  buffer  commit_timing  compositor
content_type  cursor_shape.rs  dmabuf  drm_lease  drm_syncobj  fifo
fixes.rs  foreign_toplevel_list  fractional_scale  idle_inhibit
idle_notify  image_capture_source  image_copy_capture  input_method
keyboard_shortcuts_inhibit  mod.rs  output  pointer_constraints.rs
pointer_gestures.rs  pointer_warp.rs  presentation  relative_pointer.rs
seat  security_context  selection  session_lock  shell  shm
single_pixel_buffer  socket.rs  tablet_manager  text_input  viewporter
virtual_keyboard  xdg_activation  xdg_foreign  xdg_system_bell.rs
xdg_toplevel_icon.rs  xdg_toplevel_tag.rs  xwayland_keyboard_grab.rs
xwayland_shell.rs
```

No `tearing_control` directory. A grep across the whole tree finds only one hit, in `src/backend/egl/context.rs:518` — a doc-comment about preventing screen tearing during EGL swap, completely unrelated to the protocol.

I also verified upstream smithay master (via the tree listing on github.com/Smithay/smithay) — still no tearing_control as of the time of this research. Niri (`YaLTeR/niri`) does not implement it either; cosmic-comp's `src/wayland/protocols/` and `src/wayland/handlers/` likewise do not contain a tearing module. We are first movers among Smithay-based compositors I could find.

The Rust **wire** bindings, however, are present. `wayland-protocols` 0.32.11 (which smithay 27af99e pulls as 0.32.12, same surface) ships:

```
src/wp.rs:135  #[cfg(feature = "staging")]
               pub mod tearing_control {
                   pub mod v1 {
                       wayland_protocol!(
                           "./protocols/staging/tearing-control/tearing-control-v1.xml",
                           []
                       );
                   }
               }
```

Smithay's `Cargo.toml` enables `wayland-protocols = { features = ["unstable", "staging", "server"] }`, so we get the generated server-side types via `smithay::reexports::wayland_protocols::wp::tearing_control::v1::server`.

---

## 2. Wire-Protocol Summary

XML at `wayland-protocols/protocols/staging/tearing-control/tearing-control-v1.xml`. Two interfaces, both v1.

### `wp_tearing_control_manager_v1`

Global factory.

- **Request** `destroy` (destructor) — destroying the factory does NOT invalidate already-handed-out per-surface objects.
- **Request** `get_tearing_control(new_id<wp_tearing_control_v1>, object<wl_surface>)` — instantiate the per-surface extension. If the surface already has one, raise the protocol error `tearing_control_exists` (= 0).

### `wp_tearing_control_v1`

Per-surface child object.

- **Request** `set_presentation_hint(uint hint)` — `hint` is a `presentation_hint` enum:
  - `vsync` = 0 (default, sync to vblank)
  - `async` = 1 (tearing OK)

  State is **double-buffered** — applied on the next `wl_surface.commit`.
- **Request** `destroy` (destructor) — reverts the hint to `vsync` on the next commit.

If the parent `wl_surface` is destroyed, the object becomes inert (the spec says clients "should destroy" it; we just no-op).

There are **no events** sent back to the client.

---

## 3. Module Layout: `zos-wm/src/protocols/tearing_control.rs`

We don't currently have a `protocols/` directory under `src/`. This will be the first occupant. Wire it in `src/lib.rs` next to `pub mod state;`:

```rust
// src/lib.rs additions
pub mod protocols {
    pub mod tearing_control;
}
```

### Public API surface

```rust
// src/protocols/tearing_control.rs

use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};

use smithay::{
    reexports::{
        wayland_protocols::wp::tearing_control::v1::server::{
            wp_tearing_control_manager_v1::{self, WpTearingControlManagerV1},
            wp_tearing_control_v1::{self, WpTearingControlV1, PresentationHint},
        },
        wayland_server::{
            backend::{ClientId, GlobalId},
            protocol::wl_surface::WlSurface,
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
        },
    },
    wayland::compositor::{self, Cacheable},
};

/// Double-buffered per-surface state. Read by the render path via
/// `compositor::with_states(&surface, |s| s.cached_state.get::<TearingControlSurfaceCachedState>().current().hint)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TearingControlSurfaceCachedState {
    hint: PresentationHint,
}

impl TearingControlSurfaceCachedState {
    pub fn hint(&self) -> PresentationHint { self.hint }
    pub fn wants_async(&self) -> bool {
        matches!(self.hint, PresentationHint::Async)
    }
}

impl Cacheable for TearingControlSurfaceCachedState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self { *self }
    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) { *into = self; }
}

/// Marker stored in `SurfaceData::data_map` so we can reject a second
/// `get_tearing_control` for the same surface with `tearing_control_exists`.
#[derive(Debug, Default)]
struct TearingControlSurfaceData {
    is_resource_attached: AtomicBool,
}

/// Per-`wp_tearing_control_v1` user-data. Keeps a weak ref to the surface
/// so we can route `set_presentation_hint` to its cached state, and so a
/// surface destruction makes the object inert without panicking.
#[derive(Debug)]
pub struct TearingControlUserData(Mutex<Weak<WlSurface>>);

impl TearingControlUserData {
    fn new(s: WlSurface) -> Self { Self(Mutex::new(s.downgrade())) }
    fn wl_surface(&self) -> Option<WlSurface> { self.0.lock().unwrap().upgrade().ok() }
}

/// Manager-state object — held in `AnvilState`.
#[derive(Debug)]
pub struct TearingControlManagerState {
    global: GlobalId,
}

impl TearingControlManagerState {
    pub fn new<D>(display: &DisplayHandle) -> Self
    where
        D: GlobalDispatch<WpTearingControlManagerV1, ()>
            + Dispatch<WpTearingControlManagerV1, ()>
            + Dispatch<WpTearingControlV1, TearingControlUserData>
            + 'static,
    {
        let global = display.create_global::<D, WpTearingControlManagerV1, _>(1, ());
        Self { global }
    }

    pub fn global(&self) -> GlobalId { self.global.clone() }
}

/// Convenience read-side helper used by the render path.
pub fn surface_wants_async_presentation(surface: &WlSurface) -> bool {
    compositor::with_states(surface, |states| {
        states
            .cached_state
            .get::<TearingControlSurfaceCachedState>()
            .current()
            .wants_async()
    })
}
```

### Dispatch impls (sketch)

`GlobalDispatch<WpTearingControlManagerV1, ()>` just forwards to `data_init.init(resource, ())` — same as every smithay manager.

`Dispatch<WpTearingControlManagerV1, ()>` handles two requests:

```rust
impl<D> Dispatch<WpTearingControlManagerV1, (), D> for TearingControlManagerState
where
    D: Dispatch<WpTearingControlManagerV1, ()>
        + Dispatch<WpTearingControlV1, TearingControlUserData>
        + 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        manager: &WpTearingControlManagerV1,
        request: wp_tearing_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            wp_tearing_control_manager_v1::Request::GetTearingControl { id, surface } => {
                let already = compositor::with_states(&surface, |states| {
                    states
                        .data_map
                        .insert_if_missing_threadsafe(TearingControlSurfaceData::default);
                    let data = states.data_map.get::<TearingControlSurfaceData>().unwrap();
                    let already = data.is_resource_attached.load(Ordering::Acquire);
                    if !already {
                        data.is_resource_attached.store(true, Ordering::Release);
                    }
                    already
                });

                if already {
                    manager.post_error(
                        wp_tearing_control_manager_v1::Error::TearingControlExists,
                        "wl_surface already has a wp_tearing_control_v1 attached",
                    );
                } else {
                    data_init.init(id, TearingControlUserData::new(surface));
                }
            }
            wp_tearing_control_manager_v1::Request::Destroy => {}
            _ => unreachable!(),
        }
    }
}
```

`Dispatch<WpTearingControlV1, TearingControlUserData>` handles the per-surface object:

```rust
impl<D> Dispatch<WpTearingControlV1, TearingControlUserData, D> for TearingControlManagerState
where
    D: Dispatch<WpTearingControlV1, TearingControlUserData>,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &WpTearingControlV1,
        request: wp_tearing_control_v1::Request,
        data: &TearingControlUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            wp_tearing_control_v1::Request::SetPresentationHint { hint } => {
                let wayland_server::WEnum::Value(hint) = hint else { return };
                let Some(surface) = data.wl_surface() else { return };
                compositor::with_states(&surface, |states| {
                    states
                        .cached_state
                        .get::<TearingControlSurfaceCachedState>()
                        .pending()
                        .hint = hint;
                });
            }
            wp_tearing_control_v1::Request::Destroy => {
                // Spec: revert to vsync on next commit, double-buffered.
                let Some(surface) = data.wl_surface() else { return };
                compositor::with_states(&surface, |states| {
                    if let Some(d) = states.data_map.get::<TearingControlSurfaceData>() {
                        d.is_resource_attached.store(false, Ordering::Release);
                    }
                    states
                        .cached_state
                        .get::<TearingControlSurfaceCachedState>()
                        .pending()
                        .hint = PresentationHint::Vsync;
                });
            }
            _ => unreachable!(),
        }
    }

    fn destroyed(_state: &mut D, _client: ClientId, _object: &WpTearingControlV1, _data: &TearingControlUserData) {
        // Nothing — graceful Destroy is handled above; client-disappearing
        // path lets the WlSurface destructor reclaim our SurfaceData entry.
    }
}
```

### Delegate macro

```rust
#[macro_export]
macro_rules! delegate_tearing_control {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        const _: () = {
            use smithay::reexports::{
                wayland_protocols::wp::tearing_control::v1::server::{
                    wp_tearing_control_manager_v1::WpTearingControlManagerV1,
                    wp_tearing_control_v1::WpTearingControlV1,
                },
                wayland_server::{delegate_dispatch, delegate_global_dispatch},
            };
            use $crate::protocols::tearing_control::{
                TearingControlManagerState, TearingControlUserData,
            };

            delegate_global_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [WpTearingControlManagerV1: ()] => TearingControlManagerState
            );
            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [WpTearingControlManagerV1: ()] => TearingControlManagerState
            );
            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [WpTearingControlV1: TearingControlUserData] => TearingControlManagerState
            );
        };
    };
}
```

`delegate_tearing_control!(@<BackendData: Backend + 'static> AnvilState<BackendData>);` is the call site.

---

## 4. State Changes Needed in `state.rs`

### New field on `AnvilState`

```rust
pub struct AnvilState<BackendData: Backend + 'static> {
    // ...existing fields...
    pub tearing_control_state: crate::protocols::tearing_control::TearingControlManagerState,
}
```

### Init in `AnvilState::init`

In the same block where `FifoManagerState::new::<Self>(&dh)` etc. are constructed (around line 855):

```rust
let tearing_control_state =
    crate::protocols::tearing_control::TearingControlManagerState::new::<Self>(&dh);
```

…and add `tearing_control_state,` to the struct literal at the bottom of `init`.

### Delegate at module scope

After `smithay::delegate_fifo!(...)` near the bottom of the impl section (around line 635):

```rust
crate::delegate_tearing_control!(@<BackendData: Backend + 'static> AnvilState<BackendData>);
```

(`crate::` because we exported the macro from `lib.rs` via `#[macro_export]`.)

### No `Handler` trait needed

The protocol has no callback that requires policy from the compositor — the spec says the compositor "is free to dynamically respect or ignore this hint". All decision-making lives in the render path, which reads the cached state directly. So we do **not** need a `TearingControlHandler` trait; the dispatch impls drop the hint into `SurfaceData::cached_state` and the render path picks it up on its own schedule.

This is the same pattern smithay uses for `content_type` (no handler trait either).

---

## 5. Render-Path Consumption

### Where to look up the hint

The hint lives per-`wl_surface`. We want to know: for a given **output** we are about to flip, does any surface that's about to be scanned out on it want async presentation? In the simplest first implementation (and probably the right one), we ask: does the **primary scanout surface** for this output want async?

This question is answerable in `udev::render_surface` (around `udev.rs:1646`) and in `winit::render` (around `winit.rs:387`). In both, we already have `&Space<WindowElement>` and the `&Output`. The lookup looks like:

```rust
fn output_wants_tearing(
    space: &Space<WindowElement>,
    output: &Output,
) -> bool {
    use smithay::desktop::utils::surface_primary_scanout_output;
    use smithay::wayland::compositor::with_states;

    space.elements().any(|window| {
        let Some(toplevel) = window.wl_surface() else { return false };
        with_states(&toplevel, |states| {
            // Only count the surface as "tearing-eligible" if its primary scanout
            // is on this output. Otherwise a fullscreen tearing window on display A
            // would force tearing on display B.
            let primary = surface_primary_scanout_output(&toplevel, states);
            if primary.as_ref() != Some(output) { return false; }

            states
                .cached_state
                .get::<crate::protocols::tearing_control::TearingControlSurfaceCachedState>()
                .current()
                .wants_async()
        })
    })
}
```

We may want to refine this to "primary scanout AND fullscreen-ish-covering-the-output" before we trust async to actually look good, but the v1 cut can be the simpler "any tearing-hint surface on this output's primary scanout".

### Where to apply the bit

This is the awkward part. Smithay 27af99e's render/submit pipeline:

```
DrmCompositor::render_frame(renderer, &elements, clear_color, FrameFlags)
                ↓ (queues a frame internally)
DrmOutput::queue_frame(user_data)
                ↓ (eventually)
DrmCompositor::submit_composited_frame
                ↓
DrmSurface::commit / DrmSurface::page_flip
                ↓
drm::AtomicCommitFlags::PAGE_FLIP_EVENT | NONBLOCK
```

In `src/backend/drm/compositor/mod.rs:1024` the `FrameFlags` bitflags has:

```rust
const ALLOW_PRIMARY_PLANE_SCANOUT      = 1;
const ALLOW_PRIMARY_PLANE_SCANOUT_ANY  = 2;
const ALLOW_OVERLAY_PLANE_SCANOUT      = 4;
const ALLOW_CURSOR_PLANE_SCANOUT       = 8;
const SKIP_CURSOR_ONLY_UPDATES         = 16;
```

There is **no `ALLOW_TEARING` / `ALLOW_SYNC_BREAK` bit** at our pinned commit. The internal `surface.page_flip(...)` (atomic.rs:868) hard-codes `AtomicCommitFlags::PAGE_FLIP_EVENT | AtomicCommitFlags::NONBLOCK`, with no `PAGE_FLIP_ASYNC` knob to flip.

The `drm` crate (0.14.1) at `src/control/mod.rs:1525` *does* expose:

```rust
const PAGE_FLIP_ASYNC = ffi::drm_sys::DRM_MODE_PAGE_FLIP_ASYNC;
```

…so the kernel-side support is reachable; the issue is plumbing.

This research recommends a two-step landing:

1. **Phase 2.B (now):** ship the protocol surface, store the hint, expose `output_wants_tearing` via a public function in `crate::protocols::tearing_control`. Plumb it as a no-op into the render path with a `TODO(tearing-flip)` comment so clients don't get `tearing_control_exists` errors and screen-recording tools / games stop complaining about the missing global. This unblocks app compatibility today.
2. **Phase 2.B-followup:** either patch smithay (PR upstream and bump our pin) to add `FrameFlags::ALLOW_TEARING` plumbed through to `AtomicCommitFlags::PAGE_FLIP_ASYNC`, or fork the relevant `submit_composited_frame` path locally. Niri/cosmic-comp have not done this either, so we'd be a useful upstream contribution.

The render-path call after step 1 looks like:

```rust
// udev.rs::render_surface, replacing the existing FrameFlags branch
let mut frame_mode = if surface.disable_direct_scanout {
    FrameFlags::empty()
} else {
    FrameFlags::DEFAULT
};

// TODO(tearing-flip): when smithay grows FrameFlags::ALLOW_TEARING, OR it in here.
// Until then this is a no-op at the DRM submission layer; the protocol still
// reports correctly to clients and we keep the hook ready for the bump.
let _wants_async = output_wants_tearing(space, output);

let (rendered, states) = surface
    .drm_output
    .render_frame(renderer, &elements, clear_color, frame_mode)
    /* … */;
```

…and identically in `winit.rs::render` near line 314 (winit/`WinitGraphicsBackend` does not have a tearing concept — it submits via EGL `eglSwapBuffers` which honors `EGL_SWAP_INTERVAL`; the cleanest read here is to set swap interval to 0 when `_wants_async` is true, but since winit is dev-only this is best left as a `TODO(tearing-winit)` and prioritized last).

### Why double-buffered state, not direct read of the resource

The wl_surface state model is double-buffered: clients call `set_presentation_hint`, then `wl_surface.commit`, and the new value should only take effect from the commit forward. The smithay `Cacheable` infrastructure (used by `content_type`, `presentation` feedback, `fifo_barrier`, etc.) handles that for us — `pending()` is what we write to in dispatch, `current()` is what the render path reads. No manual flip on commit needed.

---

## 6. Sources

- **Wayland protocol XML:** `wayland-protocols/protocols/staging/tearing-control/tearing-control-v1.xml`
  Local: `/home/zach/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wayland-protocols-0.32.11/protocols/staging/tearing-control/tearing-control-v1.xml`
  Web: <https://wayland.app/protocols/tearing-control-v1>

- **Rust bindings:** `wayland-protocols` crate, mod `wp::tearing_control::v1::server`. Defined at `wayland-protocols-0.32.11/src/wp.rs:135-150` (gated by `staging` feature; smithay enables this feature by default with `wayland_frontend`).

- **Smithay pin:** commit `27af99ef492ab4d7dc5cd2e625374d2beb2772f7`
  - `src/wayland/` — confirmed no `tearing_control` directory.
  - `src/wayland/idle_inhibit/mod.rs` — manager-state pattern reference (MIT).
  - `src/wayland/content_type/mod.rs` + `dispatch.rs` — closest analog: per-surface, double-buffered, `data_map` "already attached" guard, no handler trait (MIT). This is the file we mirror most closely.
  - `src/backend/drm/compositor/mod.rs:1021-1042` — `FrameFlags` bitflags definition, currently lacks tearing.
  - `src/backend/drm/surface/atomic.rs:868-925` — `DrmSurface::page_flip`, hard-coded `PAGE_FLIP_EVENT | NONBLOCK`.

- **`drm` crate 0.14.1:** `src/control/mod.rs:1525` exposes `AtomicCommitFlags::PAGE_FLIP_ASYNC`. Smithay's wrappers currently don't propagate it.

- **Reference compositors (consulted, did NOT copy code):**
  - Niri `YaLTeR/niri@main` — `src/protocols/mod.rs` and the wider tree have **no** tearing module. (GPL-3.0; reference only.)
  - cosmic-comp `pop-os/cosmic-comp@master` — `src/wayland/protocols/` and `src/wayland/handlers/` have **no** tearing module. (GPL-3.0; reference only.)
  - Both do, however, demonstrate the general pattern of `protocols/<name>.rs` + `handlers/<name>.rs` split and the `Dispatch + GlobalDispatch + delegate macro` shape — same shape we're using here, taken from smithay's MIT-licensed `content_type`.

- **Project files referenced:**
  - `/var/home/zach/github/zOS/zos-wm/Cargo.toml` — smithay rev pin.
  - `/var/home/zach/github/zOS/zos-wm/src/lib.rs` — module list to extend.
  - `/var/home/zach/github/zOS/zos-wm/src/state.rs` — delegate registration site (around line 635), `AnvilState` struct (around line 145), `init()` (around line 793).
  - `/var/home/zach/github/zOS/zos-wm/src/shell/mod.rs:173` — `CompositorHandler::commit` already calls `on_commit_buffer_handler::<Self>(surface)`, which advances `cached_state` for us. Nothing to add there.
  - `/var/home/zach/github/zOS/zos-wm/src/udev.rs:1646` — `FrameFlags` selection site for udev render.
  - `/var/home/zach/github/zOS/zos-wm/src/winit.rs:314` — winit render site (lower priority).

---

## 7. Open Questions / Risks

1. **PAGE_FLIP_ASYNC plumbing.** Already discussed. Without it, the protocol is "implemented" only at the wire level. This matches every other Smithay-based compositor today, so we're not unusual — but games/recording tools that bind the global expecting tearing won't actually tear until the smithay patch lands.

2. **VRR interaction.** Tearing + variable refresh rate is messy. If a surface requests `async` AND the connector has VRR enabled, the kernel typically prefers VRR over async flips. Worth a `tracing::debug!` line on the first frame after a hint flip so we can see what we ended up doing on real hardware, but no policy decision needed v1.

3. **NVIDIA driver support.** The 4090 + proprietary driver supports `PAGE_FLIP_ASYNC` only on >=550 series drivers, and historically only on the primary plane. When we wire the FrameFlag, we should test on the daily-driver box before flipping it on by default. Out of scope for the protocol research, but flagged here so phase 2.E doesn't forget.

4. **Subsurface trees.** The spec is silent on whether the hint applies recursively to subsurfaces. Current consensus (from KWin, wlroots) is "the hint on the toplevel decides; subsurfaces are along for the ride". Our `output_wants_tearing` walks `space.elements()` which gives us toplevels — correct by construction.

5. **Cursor-only updates.** When `FrameFlags::SKIP_CURSOR_ONLY_UPDATES` short-circuits a frame, we shouldn't be flipping async anyway (no real content change). The current sketch is fine because the FrameFlags path runs first.
