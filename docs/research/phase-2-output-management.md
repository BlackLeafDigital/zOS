# Phase 2.D — `zwlr-output-management-v1` for zos-wm

Target compositor: `zos-wm`, fork of Smithay's `anvil` reference compositor.
Pinned Smithay SHA: `27af99ef492ab4d7dc5cd2e625374d2beb2772f7` (April 2026).
Local references:
- `zos-wm`: `/var/home/zach/github/zOS/zos-wm/`
- Smithay (pinned): `/home/zach/.cargo/git/checkouts/smithay-312425d48e59d8c8/27af99e/`
- Niri (latest main, used as reference for patterns ONLY — GPL-3.0, do not copy expressive code): `/tmp/niri-peek/`

## TL;DR

- Smithay at the pinned SHA does **not** implement `zwlr-output-management-v1`. Verified by `ls .../src/wayland/` and `rg -i wlr_output|output_management|zwlr_output` in the smithay tree (no hits except a wlr-screencopy-unrelated comment). We must roll our own dispatch.
- Smithay does ship the **wire bindings**, however: `wayland-protocols-wlr = "0.3.12"` is a hard dep with `features = ["server"]`, re-exported as `smithay::reexports::wayland_protocols_wlr::output_management::v1::server::{zwlr_output_manager_v1, zwlr_output_head_v1, zwlr_output_mode_v1, zwlr_output_configuration_v1, zwlr_output_configuration_head_v1}`. We only write the dispatch glue — no XML codegen, no wayland-scanner.
- Live niri is the closest reference (`src/protocols/output_management.rs`, 923 LoC, GPL-3.0). Cosmic-comp also implements it. We will study niri's structure for the state machine but rewrite from the protocol XML to keep the file MIT-compatible.
- Implementation lives at `zos-wm/src/protocols/output_management.rs` (new module `protocols/`), exposed via a `delegate_output_management!` macro and an `OutputManagementHandler` trait the `AnvilState` impls.
- DRM apply path uses smithay's `DrmOutputManager::use_mode(...)` (verified at `src/backend/drm/output.rs:510`) plus `output.change_current_state(...)` and `space.map_output(...)`. winit backend can only flip `enabled`/`scale`/`transform` — no real modeset; we report `failed` if a winit client tries to change mode.
- 3× 1080p60 on the user's RTX 4090 box: each connector becomes one head; ordering matters because `connector_connected` is called sequentially per CRTC. Mode-set during `apply` must release overlay planes (already empty for NVIDIA) and then call `use_mode` on a single CRTC at a time. Defer NVIDIA-specific atomic-commit pitfalls to `phase-2-c-drm-nvidia-specifics.md`.

## Smithay support status at pin

Verified by direct inspection of `/home/zach/.cargo/git/checkouts/smithay-312425d48e59d8c8/27af99e/`:

```text
ls src/wayland/ → no `output_management` directory; only `output/` (the regular wl_output / xdg-output handler)
rg -i wlr_output|output_management|zwlr_output → 0 matches in src/
grep wayland-protocols-wlr Cargo.toml → "0.3.12", features=["server"], optional, gated by `wayland_frontend`
```

What this means concretely:

- `OutputManagerState` exists in `smithay::wayland::output` (already wired in zos-wm at `state.rs:160` and constructed at `state.rs:840` via `OutputManagerState::new_with_xdg_output::<Self>`). That struct only handles `wl_output` + `xdg_output`; it has nothing to do with wlr-output-management.
- The wlr-output-management *types* are reachable as `smithay::reexports::wayland_protocols_wlr::output_management::v1::server::*`. The `wayland_frontend` feature is already enabled in zos-wm's Smithay dep (`Cargo.toml:35` features include `desktop` and `wayland_frontend`), so no Cargo changes are required to *use* them.

So the work is: implement `Dispatch` / `GlobalDispatch` impls on a new `OutputManagementManagerState` struct, store per-client and per-output bookkeeping, and call into the udev/winit backends from an `apply_output_config` hook.

## Wire-protocol object tree and state machine

Source XML: `wayland-protocols-wlr/unstable/output-management/wlr-output-management-unstable-v1.xml` (version 4 is current; niri exports `VERSION = 4`).

```
zwlr_output_manager_v1   (singleton global, advertised to filtered clients)
  ├── server→client events
  │     head(new_id<zwlr_output_head_v1>)        — vended once per Output
  │     done(serial)                              — atomic snapshot delimiter
  │     finished()                                — server retiring this manager
  └── client→server requests
        create_configuration(id, serial)          — open a new configuration session
        stop()                                    — client stops listening (server replies finished())

zwlr_output_head_v1   (one per Output, lifetime: until output unplug or manager destroy)
  ├── events (server→client, all sent before the next done())
  │     name(string)                              — connector name (e.g. "DP-1")
  │     description(string)                       — "Make Model Name"
  │     physical_size(w_mm, h_mm)
  │     mode(new_id<zwlr_output_mode_v1>)         — once per supported mode
  │     enabled(int)                              — 0/1
  │     current_mode(zwlr_output_mode_v1)
  │     position(x, y)
  │     transform(int)
  │     scale(fixed)
  │     finished()                                — when the output disappears
  │     make/model/serial_number   (v2+)
  │     adaptive_sync(state)        (v4+)
  └── requests
        release()                                  — client releases its proxy

zwlr_output_mode_v1   (one per supported mode of a head, lifetime: same as head)
  ├── events: size(w,h), refresh(mHz), preferred(), finished()
  └── requests: release()

zwlr_output_configuration_v1  (transient, one per client config attempt)
  ├── server→client events
  │     succeeded()                               — apply/test accepted
  │     failed()                                  — atomic operation rejected
  │     cancelled()                               — server's serial moved out from under this config
  └── client→server requests
        enable_head(new_id<config_head>, head)    — start configuring this head
        disable_head(head)
        apply()                                   — atomic commit
        test()                                    — atomic dry-run
        destroy()

zwlr_output_configuration_head_v1   (per-head edit handle inside a configuration)
        set_mode(mode)
        set_custom_mode(w, h, refresh)
        set_position(x, y)
        set_transform(int)
        set_scale(fixed)
        set_adaptive_sync(state)   (v4+)
```

State machine for a single configuration object (server-side):

```
                create_configuration(serial)
                       │
            serial != current_serial?
              │                 │
            yes                 no
              │                 │
        cancelled()       Ongoing(map<head,EditState>)
                                │
                     enable_head / disable_head / set_*
                                │
                          apply() or test()
                                │
                  validate against current mode list
                                │
              ┌────────────┬────┴──────────┬──────────────┐
              │            │               │              │
        invalid mode   another        DRM modeset     all good
              │       config landed   failed              │
           failed()   first              │            succeeded()
                      │                failed()
                  cancelled()
                                          │
                                      Finished
                                  (no further requests)
```

Notes on serial:
- Server keeps a monotonic `serial: u32`. It advances on every `notify_changes` that produced a non-trivial diff.
- The client receives the latest serial via `manager.done(serial)` (sent on bind and after every change burst).
- A configuration carries the serial it was created with. If by the time `apply`/`test` arrives the server's `serial` has advanced, we **must** `cancelled()` instead of attempting the modeset (clients are expected to redo their plan against the new state).

## Module layout proposal

```
zos-wm/src/
├── protocols/
│   ├── mod.rs                  (pub mod output_management;)
│   └── output_management.rs    (~600-700 LoC after rewrite for MIT)
├── state.rs                    (new field + handler + delegate macro call)
├── udev.rs                     (call notify_changes after connector_connected/disconnected;
│                                impl backend-specific apply path here)
└── winit.rs                    (call notify_changes after Output::new;
                                 reject mode-changing apply requests)
```

The `protocols/` directory is new — zos-wm currently has no such tree (it inherited anvil's flat layout). Mirror niri's choice because future protocols (`screencopy`, `gamma_control`, `tearing_control`, `foreign_toplevel`, `ext_workspace`) will all live here.

### Public surface of `protocols/output_management.rs`

```rust
// MIT-compatible rewrite. Patterns referenced from:
//   - niri  src/protocols/output_management.rs (GPL-3.0; structure only)
//   - cosmic-comp src/wayland/protocols/output_management.rs (GPL-3.0; structure only)
//   - sway sway/desktop/output.c + protocol.c (GPL-2.0; semantics)
// XML source-of-truth: wayland-protocols-wlr v0.3.12 server bindings.

pub struct OutputManagementManagerState { /* private: clients, current_state, serial */ }

pub struct OutputManagementGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

pub enum OutputConfigurationHeadState {
    Cancelled,
    Ok { output: Output, conf: ZwlrOutputConfigurationV1 },
}

pub trait OutputManagementHandler {
    fn output_management_state(&mut self) -> &mut OutputManagementManagerState;
    /// Apply the validated config. Implementor performs DRM modeset/space remap
    /// and returns `true` on success, `false` on failure. Must be synchronous;
    /// server replies `succeeded`/`failed` based on this return.
    fn apply_output_config(&mut self, config: PendingOutputConfig, test_only: bool) -> bool;
}

#[derive(Debug, Clone)]
pub struct PendingOutputConfig {
    pub heads: Vec<HeadConfig>,
}

#[derive(Debug, Clone)]
pub struct HeadConfig {
    pub output: Output,
    pub enabled: bool,
    pub mode: Option<HeadMode>,         // None = keep current
    pub position: Option<Point<i32, Logical>>,
    pub transform: Option<Transform>,
    pub scale: Option<f64>,
    pub adaptive_sync: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum HeadMode {
    Existing(Mode),                      // a smithay::output::Mode that was advertised
    Custom { size: Size<i32, Physical>, refresh_mhz: i32 },
}

impl OutputManagementManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where D: GlobalDispatch<ZwlrOutputManagerV1, OutputManagementGlobalData>
        + Dispatch<ZwlrOutputManagerV1, ()>
        + Dispatch<ZwlrOutputHeadV1, Output>
        + Dispatch<ZwlrOutputModeV1, ()>
        + Dispatch<ZwlrOutputConfigurationV1, u32>
        + Dispatch<ZwlrOutputConfigurationHeadV1, OutputConfigurationHeadState>
        + OutputManagementHandler
        + 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static;

    /// Call after a new monitor is plugged in (after `Output::new` and `create_global`).
    pub fn add_head<D: ...>(&mut self, output: &Output);

    /// Call when a monitor is unplugged (before dropping the Output / its global).
    pub fn remove_head(&mut self, output: &Output);

    /// Call after any state change visible to clients: mode swap, position remap,
    /// scale change, transform change, adaptive-sync toggle. Bumps serial and
    /// re-broadcasts.
    pub fn notify_changes<D: ...>(&mut self);
}

#[macro_export]
macro_rules! delegate_output_management { ($($t:tt)*) => { /* GlobalDispatch + 5 Dispatches */ } }
```

User-data pattern — niri stores an opaque `OutputId` (their Output identity); we instead stash the smithay `Output` directly because it's already cheaply cloneable and its `WeakOutput` lets us detect unplug. (Note: niri's `OutputId` is a `Copy` newtype; we trade `Copy` for the convenience of `Output::user_data()` lookups in `apply_output_config`.)

## Required state on `AnvilState`

Add one field to `state.rs:144 AnvilState`:

```rust
pub output_management_state: protocols::output_management::OutputManagementManagerState,
```

Initialise in `AnvilState::init` (line 793) just after `OutputManagerState::new_with_xdg_output` (line 840):

```rust
let output_management_state = OutputManagementManagerState::new::<Self, _>(&dh, |client| {
    // Same security-context filter the rest of the privileged globals use.
    client.get_data::<ClientState>()
        .is_none_or(|cs| cs.security_context.is_none())
});
```

And implement the handler (anywhere in `state.rs`, alongside the other handlers):

```rust
impl<BackendData: Backend + 'static> OutputManagementHandler for AnvilState<BackendData> {
    fn output_management_state(&mut self) -> &mut OutputManagementManagerState {
        &mut self.output_management_state
    }

    fn apply_output_config(&mut self, cfg: PendingOutputConfig, test_only: bool) -> bool {
        // 1) validate
        if !validate_pending_config(&self.space, &cfg) { return false; }
        if test_only { return true; }
        // 2) hand off to backend; backend remaps `space` and modesets DRM
        self.backend_data.apply_output_config(&mut self.space, &self.display_handle, &cfg)
    }
}
delegate_output_management!(@<BackendData: Backend + 'static> AnvilState<BackendData>);
```

To support the backend dispatch we extend the `Backend` trait at `state.rs:1369`:

```rust
pub trait Backend {
    // existing assoc consts and fns ...
    fn apply_output_config(
        &mut self,
        _space: &mut Space<WindowElement>,
        _dh: &DisplayHandle,
        _cfg: &PendingOutputConfig,
    ) -> bool { false }   // default: no-op, refuse all configs
}
```

The default body keeps `x11.rs` and any future test backend honest without forcing them to implement modesets.

### Internal bookkeeping (inside `OutputManagementManagerState`)

Niri's pattern, but adapted: keep a `HashMap<ClientId, ClientData>` where each ClientData has:

```rust
struct ClientData {
    manager: ZwlrOutputManagerV1,
    heads:  HashMap<Output, (ZwlrOutputHeadV1, Vec<ZwlrOutputModeV1>)>,
    confs:  HashMap<ZwlrOutputConfigurationV1, ConfState>,
}

enum ConfState {
    Ongoing(HashMap<Output, HeadConfigBuilder>),
    Finished,
}

struct HeadConfigBuilder {
    enabled: bool,
    mode: Option<HeadMode>,
    position: Option<(i32, i32)>,
    transform: Option<Transform>,
    scale: Option<f64>,
    adaptive_sync: Option<bool>,
    // tracking which set_* requests have already been applied — needed to
    // diagnose duplicate set_mode (protocol error AlreadySet).
    set_mode_done: bool,
    set_position_done: bool,
    set_transform_done: bool,
    set_scale_done: bool,
    set_adaptive_sync_done: bool,
}
```

Plus the global state:

```rust
pub struct OutputManagementManagerState {
    display: DisplayHandle,
    serial: u32,
    clients: HashMap<ClientId, ClientData>,
    current_state: HashMap<Output, OutputSnapshot>,
}

struct OutputSnapshot {
    name: String,
    description: String,
    physical_size_mm: (i32, i32),
    make: String,
    model: String,
    serial_number: Option<String>,
    modes: Vec<Mode>,                  // smithay::output::Mode
    current_mode: Option<usize>,
    position: (i32, i32),
    transform: Transform,
    scale: f64,
    enabled: bool,
    adaptive_sync: bool,
}
```

`OutputSnapshot` is built from a `&Output` via a helper `snapshot_output(&Output, &Space) -> OutputSnapshot`. We snapshot rather than read live because `notify_changes` needs to diff old-vs-new to decide what events to emit.

## Lifecycle hooks

### Hook A — winit backend (`winit.rs::run_winit`, line 96-128)

After `output.set_preferred(mode)` (line 127), once per startup:

```rust
state.output_management_state.add_head::<AnvilState<WinitData>>(&output);
state.output_management_state.notify_changes::<AnvilState<WinitData>>();
```

Note: the winit `output` is created *before* `state` exists in `run_winit`. The cleanest fix is to call `add_head` after `state` is constructed (look for `let mut state = AnvilState::init(...)`), passing `&output` which has been kept alive in scope.

On winit shutdown (the `PumpStatus::Exit` branch), call `remove_head` then `notify_changes`. The winit output never disconnects mid-session unless the user closes the window.

### Hook B — udev backend (`udev.rs::connector_connected`, line 889; `connector_disconnected`, line 1075)

Inside `connector_connected`, immediately after the `device.surfaces.insert(crtc, surface);` line (1066):

```rust
self.output_management_state
    .add_head::<AnvilState<UdevData>>(&output);   // `output` still in scope here
self.output_management_state
    .notify_changes::<AnvilState<UdevData>>();
```

Important: we add the head **after** the `DrmOutput` has been created and `surface` is inserted into `device.surfaces`. Otherwise `apply_output_config` could be called for a head whose CRTC has no surface, panicking on `device.surfaces.get_mut(&crtc).unwrap()`.

Inside `connector_disconnected`, before the `try_to_restore_modifiers` call (line 1098):

```rust
if let Some(surface) = ... /* the just-removed surface */ {
    self.output_management_state
        .remove_head::<AnvilState<UdevData>>(&surface.output);
    self.output_management_state
        .notify_changes::<AnvilState<UdevData>>();
}
```

The `remove_head` call iterates clients and emits `mode.finished()` for every advertised mode then `head.finished()` for the head proxy. The proxy stays alive in protocol terms until the client `release`s it, but no further events are sent.

### Hook C — Mode/position changes outside of apply()

When code elsewhere (e.g. internal config reload, hotplug re-arrangement in `fixup_positions`) mutates output state, call `notify_changes` afterwards. Concretely add it at:

- `udev.rs:1150` (end of `device_changed`, after `fixup_positions`).
- Any future internal "rearrange monitors" command (none today).

`notify_changes` is internally cheap when nothing changed (it diffs `current_state` against the live space) — safe to call optimistically.

## Apply path: validation + DRM modeset + space remap

### Validation (backend-agnostic, in `state.rs::apply_output_config` before backend handoff)

```rust
fn validate_pending_config(space: &Space<WindowElement>, cfg: &PendingOutputConfig) -> bool {
    // 1. At least one output must remain enabled
    let any_enabled = cfg.heads.iter().any(|h| h.enabled);
    if !any_enabled { return false; }

    // 2. Each head must reference an Output that's still in the space
    for h in &cfg.heads {
        if !space.outputs().any(|o| o == &h.output) {
            warn!(name = h.output.name(), "apply: head references unknown output");
            return false;
        }
    }

    // 3. set_mode (if present) must reference a mode currently advertised on that head.
    //    custom_mode is rejected unconditionally for v1; the user's NVIDIA + 1080p60
    //    monitors don't need it.
    for h in &cfg.heads {
        if let Some(HeadMode::Existing(m)) = h.mode {
            if !h.output.modes().contains(&m) {
                warn!(name = h.output.name(), ?m, "apply: requested mode not on head");
                return false;
            }
        }
        if matches!(h.mode, Some(HeadMode::Custom { .. })) {
            warn!("apply: custom_mode rejected (not implemented)");
            return false;
        }
    }

    // 4. Scale within [0.5, 4.0]; fractional scales must be representable.
    for h in &cfg.heads {
        if let Some(s) = h.scale {
            if !(0.5..=4.0).contains(&s) || !s.is_finite() {
                return false;
            }
        }
    }

    // 5. Position bounds: Logical i32, but reject negative if the resulting
    //    layout would put no output at (0,0). Cheap heuristic — niri checks
    //    "any output reachable at origin" too. Skip overlap detection for v1;
    //    panels typically accept overlap and the user can recover.
    true
}
```

### DRM apply path (udev backend)

In `udev.rs`, add an inherent method:

```rust
impl AnvilState<UdevData> {
    fn apply_output_config_udev(
        &mut self,
        cfg: &PendingOutputConfig,
    ) -> bool {
        // First pass: dry-run all use_mode calls against the current device set.
        // We can't actually atomically commit across CRTCs without a multi-CRTC
        // path, but we *can* fail fast on lookup errors.
        for head in &cfg.heads {
            let Some(udev_id) = head.output.user_data().get::<UdevOutputId>() else {
                warn!(name = head.output.name(), "apply: head missing UdevOutputId");
                return false;
            };
            let Some(device) = self.backend_data.backends.get(&udev_id.device_id) else {
                return false;
            };
            if !device.surfaces.contains_key(&udev_id.crtc) {
                return false;
            }
        }

        // Second pass: commit per-head. Order: disables first, then enables/modesets.
        // This avoids a transient state where two outputs claim overlapping CRTC
        // bandwidth on the same device.
        let (disables, enables): (Vec<_>, Vec<_>) =
            cfg.heads.iter().partition(|h| !h.enabled);

        for head in disables {
            // smithay's DrmOutputManager doesn't expose "disable connector" cleanly
            // at this rev — easiest path is to drop the surface, which forces
            // modeset to OFF on the next try_to_restore_modifiers call. Defer
            // the full disable path; for now reject.
            warn!(name = head.output.name(), "apply: disable not yet implemented");
            return false;
        }

        for head in enables {
            let udev_id = head.output.user_data().get::<UdevOutputId>().unwrap();
            let device = self.backend_data.backends.get_mut(&udev_id.device_id).unwrap();

            // Pick the mode to commit.
            let new_mode_smithay = match head.mode {
                Some(HeadMode::Existing(m)) => m,
                _ => head.output.current_mode().unwrap(),  // keep current
            };
            // Convert smithay::output::Mode back to drm::Mode by indexing the
            // connector's mode list (we stored it during connector_connected).
            // Helper: find_drm_mode(connector, smithay_mode) -> Option<drm::Mode>.
            let drm_mode = match find_drm_mode_for_output(device, udev_id.crtc, new_mode_smithay) {
                Some(m) => m,
                None => return false,
            };

            let render_node = device.render_node.unwrap_or(self.backend_data.primary_gpu);
            let mut renderer = self.backend_data.gpus.single_renderer(&render_node).unwrap();

            // Issue the modeset.
            let res = device.drm_output_manager
                .lock()
                .use_mode::<_, OutputRenderElements<UdevRenderer<'_>, WindowRenderElement<UdevRenderer<'_>>>>(
                    &udev_id.crtc,
                    drm_mode,
                    &mut renderer,
                    &DrmOutputRenderElements::default(),
                );
            if let Err(err) = res {
                warn!(?err, name = head.output.name(), "apply: DrmOutput::use_mode failed");
                return false;
            }

            // Update smithay state and space.
            let new_position = head.position
                .map(|p| (p.x, p.y).into())
                .unwrap_or_else(|| self.space.output_geometry(&head.output).unwrap().loc);
            let new_transform = head.transform.unwrap_or(head.output.current_transform());
            let new_scale = head.scale
                .map(smithay::output::Scale::Fractional)
                .unwrap_or(head.output.current_scale());

            head.output.change_current_state(
                Some(new_mode_smithay),
                Some(new_transform),
                Some(new_scale),
                Some(new_position),
            );
            self.space.map_output(&head.output, new_position);
        }

        // Force a full re-render on every output we touched.
        for head in &cfg.heads {
            self.backend_data.reset_buffers(&head.output);
        }

        // Bump our own serial so subsequent client configs use the new state.
        self.output_management_state.notify_changes::<AnvilState<UdevData>>();
        true
    }
}
```

Note `use_mode` is at `smithay/src/backend/drm/output.rs:510`, signature verified. It returns `DrmOutputManagerResult` and may force composition on other surfaces if bandwidth requires it — the existing comment in smithay says "This might cause commits on other surfaces to meet the bandwidth requirements of the new mode by temporarily disabling additional planes and forcing composition."

### Apply path (winit backend)

Trivially: reject any config that changes mode (winit owns the surface size). Allow `position`, `scale`, `transform`, and `enabled=false` is meaningless (single virtual output). Roughly:

```rust
impl Backend for WinitData {
    fn apply_output_config(
        &mut self,
        space: &mut Space<WindowElement>,
        _dh: &DisplayHandle,
        cfg: &PendingOutputConfig,
    ) -> bool {
        // Exactly one head must be present, modes can only match current.
        let [head] = cfg.heads.as_slice() else { return false; };
        if let Some(HeadMode::Existing(m)) = head.mode {
            if Some(m) != head.output.current_mode() { return false; }
        } else if matches!(head.mode, Some(HeadMode::Custom { .. })) {
            return false;
        }
        let pos = head.position.map(|p| p).unwrap_or_else(|| space.output_geometry(&head.output).unwrap().loc.into());
        let new_transform = head.transform.unwrap_or(head.output.current_transform());
        let new_scale = head.scale
            .map(smithay::output::Scale::Fractional)
            .unwrap_or(head.output.current_scale());
        head.output.change_current_state(None, Some(new_transform), Some(new_scale), Some(pos.into()));
        space.map_output(&head.output, pos.into());
        self.full_redraw = 4;
        true
    }
}
```

## 3-monitor-on-NVIDIA gotchas

(Brief — full DRM specifics live in `phase-2-c-drm-nvidia-specifics.md`.)

1. **Per-CRTC modeset, not multi-CRTC atomic.** Smithay 27af99e's `DrmOutputManager::use_mode` operates on a single CRTC handle at a time. There is no public "atomic across all CRTCs" API. Consequence: a 3-monitor config change is applied as 3 sequential modesets. The user briefly sees flicker / black on monitors B and C while monitor A is being modeset. We can't avoid this without patching smithay.

2. **Overlay planes already cleared for NVIDIA.** `udev.rs:1006-1014` already does `planes.overlay = vec![];` when the driver is NVIDIA. Modeset path doesn't need to re-clear; the existing `DrmOutputManager` retains that decision per surface. *Do not* try to re-add overlay planes during apply — the comment on line 1005 ("Using an overlay plane on a nvidia card breaks") is load-bearing for the user's daily-driver box.

3. **libseat session interactions.** Modeset requires DRM master. The `LibSeatSession` already grants this in udev.rs. `use_mode` will fail with `EACCES` if the session is paused (e.g. VT switch). Surface that cleanly: on `Err`, log and return `false`, which becomes `failed()` to the client. Do NOT retry — the client will reissue.

4. **DRM modeset can take 50-200 ms per CRTC.** This blocks the calloop thread during apply. Acceptable for a manual reconfigure (kanshi/wlr-randr); unacceptable in a hot path. Don't call apply outside of explicit client request handling.

5. **Position coords are i32 logical.** Three 1080p monitors at scale=1 fit comfortably in `i32` (max layout would be ~5760×1080 for side-by-side, well under `i32::MAX`). No overflow concerns.

6. **`modes()` ordering matters for clients.** wlr-randr expects modes sorted by preference / refresh. Smithay's `Output::modes()` returns insertion order, which mirrors the connector's mode list. We pass them through unchanged in `add_head` — same behaviour as Hyprland and Sway.

7. **VRR / adaptive_sync.** zos-wm has no current VRR plumbing. The `set_adaptive_sync` request can be accepted-and-stored on the `HeadConfigBuilder` but the `apply_output_config_udev` path should treat any non-`None` value as a no-op for now (advertise current state via `head.adaptive_sync(Disabled)`). Add a TODO referencing the niri pattern (`niri_config::Vrr { on_demand: false }`).

## Testing

Manual smoke (3-monitor box):

```sh
# advertise heads
wlr-randr               # should list DP-1, DP-2, HDMI-A-1 (or whatever)
wlr-randr --output DP-1 --mode 1920x1080@60.000
kanshi                  # apply user's saved profile
wdisplays               # GUI: drag monitors around, click apply
hyprpanel monitor-info  # verifies head events made it through
```

Validation: each tool should round-trip. After apply, `wlr-randr` re-run should show the new state without regression.

Automated (smoke test, future):

- Headless winit run with `WAYLAND_DEBUG=client` and a small Rust client that binds the global, captures the head list, and asserts every `done()` arrives after a full burst of head/mode events.

## Sources

- Protocol XML (canonical, MIT-equivalent): https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/blob/master/unstable/wlr-output-management-unstable-v1.xml
- Mirrored docs: https://wayland.app/protocols/wlr-output-management-unstable-v1
- Smithay pin: https://github.com/Smithay/smithay/tree/27af99ef492ab4d7dc5cd2e625374d2beb2772f7
  - No `output_management` module exists. Verified: `ls src/wayland/` and `rg -i output_management`.
  - `wayland-protocols-wlr` 0.3.12 is a dep (Cargo.toml:61), feature-gated by `wayland_frontend`.
  - `DrmOutputManager::use_mode`: `src/backend/drm/output.rs:510-532`.
  - `Output::change_current_state`: in `src/output/mod.rs` (regular smithay API, well-documented).
  - `Space::map_output`: in `src/desktop/space/mod.rs` (regular smithay API).
- Niri reference (GPL-3.0 — patterns only, **do not lift code**):
  - `src/protocols/output_management.rs` (923 LoC)
  - Specifically the `notify_changes` diff loop (lines 95-269), the `OutputConfigurationState::Ongoing|Finished` enum (lines 53-57), and the per-config-head dispatch with `OutputConfigurationHeadState::Cancelled|Ok` user-data (lines 59-62).
- Cosmic-comp reference (GPL-3.0 — patterns only):
  - `cosmic-comp/src/wayland/protocols/output_management.rs`
  - URL: https://github.com/pop-os/cosmic-comp/blob/master/src/wayland/protocols/output_management.rs
- Sway reference (GPL-2.0 — semantics only, C):
  - `sway/sway/desktop/output.c` and `sway/protocols.c` — Sway's apply path validates, calls `wlr_output_state_set_mode`, then `wlr_output_commit_state`. Same structure as our path, modulo wlroots vs smithay.
- Wayland-protocols-wlr crate docs: https://docs.rs/wayland-protocols-wlr/0.3.12/wayland_protocols_wlr/output_management/v1/server/
- zos-wm pre-existing structures used:
  - `UdevOutputId` at `zos-wm/src/udev.rs:126`
  - `connector_connected` at `zos-wm/src/udev.rs:889`
  - `connector_disconnected` at `zos-wm/src/udev.rs:1075`
  - `winit::run_winit` at `zos-wm/src/winit.rs:96`
  - `Backend` trait at `zos-wm/src/state.rs:1369`
  - NVIDIA overlay-plane workaround at `zos-wm/src/udev.rs:1005-1014` — already in place.
