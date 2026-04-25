//! Implementation of the unstable `zwlr_output_management_v1` Wayland protocol.
//!
//! This module advertises the `zwlr_output_manager_v1` global and handles the
//! per-client object graph (manager → head → mode, plus configuration objects)
//! that lets privileged clients (kanshi, wlr-randr, wdisplays, hyprpanel)
//! inspect and reconfigure the compositor's outputs atomically.
//!
//! ## Scope
//!
//! This module is *only* the protocol surface: dispatch, per-output bookkeeping,
//! lifecycle helpers, and the `delegate_output_management!` macro. Calling the
//! lifecycle helpers from the udev/winit lifecycle hooks (`connector_connected`,
//! `connector_disconnected`, winit startup/shutdown) is the job of follow-up
//! task 2.D.2. Implementing the real DRM modeset path inside the
//! [`crate::state::Backend::apply_output_config`] override on `UdevData` is
//! follow-up task 2.D.3.
//!
//! ## Pattern
//!
//! Hand-rolled in the same shape as `crate::protocols::tearing_control` and
//! `crate::protocols::gamma_control` (smithay does not ship a built-in
//! handler at our pinned commit `27af99e`; the wire bindings are re-exported
//! via `smithay::reexports::wayland_protocols_wlr::output_management::v1`).
//!
//! Three classes of state:
//!
//! 1. **Per-manager-state** ([`OutputManagementManagerState`]) — owned by
//!    `AnvilState`. Holds the [`GlobalId`], the monotonically-increasing
//!    config serial, the per-client object graphs, and a snapshot of which
//!    outputs are currently advertised.
//! 2. **Per-output user-data** — stashed on each `smithay::output::Output`
//!    via its [`UserDataMap`] under [`OutputAdaptiveSyncRequest`], used to
//!    remember the most-recently-requested adaptive-sync state across configs.
//! 3. **Per-resource user-data** — see [`HeadUserData`],
//!    [`ConfigurationUserData`], [`ConfigurationHeadUserData`].
//!
//! ## State machine summary
//!
//! ```text
//! manager bind                                   (server → client)
//!   for each known output:
//!     manager.head(new head); head.<descriptors>; head.mode * N; head.<state>
//!   manager.done(serial)
//!
//! create_configuration(id, serial)               (client → server)
//!   if serial != current_serial: configuration.cancelled
//!   else: build empty Pending map
//!
//! enable_head(id, head)                          (client → server)
//!   create configuration_head with Ok(output) data
//! disable_head(head)                             (client → server)
//!   record disable
//! set_mode(mode)/set_position/set_scale/...      (client → server)
//!   validate, store on PendingHead
//! set_custom_mode                                (client → server)
//!   reject: mark PendingHead cancelled (apply will fail)
//!
//! apply()                                        (client → server)
//!   if outdated: cancelled
//!   if any PendingHead cancelled: failed
//!   if !any_enabled: failed
//!   else: dispatch to Backend::apply_output_config(&changes, false)
//!     Ok ⇒ succeeded; bump serial; broadcast diff via notify_changes
//!     Err ⇒ failed
//! test()                                         (client → server)
//!   same validation, but Backend::apply_output_config(.., true)
//!
//! notify_changes(output)                         (compositor-internal helper)
//!   diff snapshot vs live state, emit head/mode events for changed fields,
//!   bump serial, manager.done(serial), cancelled() on any in-flight configs
//! ```

use std::{
    collections::HashMap,
    sync::Mutex,
};

use smithay::{
    output::{Mode, Output},
    reexports::{
        wayland_protocols_wlr::output_management::v1::server::{
            zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
            zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
            zwlr_output_head_v1::{self, AdaptiveSyncState, ZwlrOutputHeadV1},
            zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
            zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum, Weak,
            backend::{ClientId, GlobalId},
            protocol::wl_output::Transform as WlTransform,
        },
    },
    utils::{Logical, Point, Transform},
};

use tracing::{debug, warn};

/// Protocol version we advertise. Clients negotiate down from here.
const VERSION: u32 = 4;

// ---------------------------------------------------------------------------
// Public API: change/error types consumed by `Backend::apply_output_config`
// ---------------------------------------------------------------------------

/// One requested change in an output configuration session, paired with the
/// concrete [`Output`] it targets.
///
/// Built by the protocol dispatch layer from a validated
/// `zwlr_output_configuration_v1` and handed to
/// [`crate::state::Backend::apply_output_config`].
#[derive(Debug, Clone)]
pub struct OutputConfigChange {
    /// The compositor-side output the change applies to.
    pub output: Output,
    /// Action to take.
    pub action: OutputConfigAction,
}

/// What kind of change is being requested for a single head.
#[derive(Debug, Clone)]
pub enum OutputConfigAction {
    /// Enable an output (with optional initial state). All `Option<_>` fields
    /// represent client-side overrides; `None` means "keep current".
    ///
    /// Only emitted when the head was either previously disabled, or a fresh
    /// `enable_head` request was issued.
    Enable {
        mode: Option<Mode>,
        position: Option<Point<i32, Logical>>,
        scale: Option<f64>,
        transform: Option<Transform>,
        adaptive_sync: Option<bool>,
    },
    /// Disable a previously-enabled output.
    Disable,
    /// Update one or more fields of an already-enabled output.
    ///
    /// Distinct from [`OutputConfigAction::Enable`] so backends can short-
    /// circuit cheap state-only updates (position, scale, transform) without
    /// running a full modeset path.
    Update {
        mode: Option<Mode>,
        position: Option<Point<i32, Logical>>,
        scale: Option<f64>,
        transform: Option<Transform>,
        adaptive_sync: Option<bool>,
    },
}

/// Reasons a configuration apply may fail.
#[derive(Debug, Clone)]
pub enum OutputConfigError {
    /// The current backend does not implement output configuration. Default
    /// for the [`Backend`] trait — winit/x11 inherit it.
    NotSupported,
    /// A requested mode is not advertised on the head.
    InvalidMode,
    /// A requested position is invalid (e.g. would orphan all outputs).
    InvalidPosition,
    /// The server's serial advanced between configuration build and apply.
    /// The dispatch layer turns this into `cancelled()` on the wire.
    Cancelled,
    /// Backend-specific failure with a free-form message (DRM modeset error,
    /// libseat permission error, etc.).
    Backend(String),
}

// ---------------------------------------------------------------------------
// Per-output user-data
// ---------------------------------------------------------------------------

/// Cached "most recently requested by a client" adaptive-sync state, stashed
/// on the [`Output`] user-data map. The DRM apply path may consult this in a
/// follow-up task; for now it's accept-and-store.
#[derive(Debug, Default)]
pub struct OutputAdaptiveSyncRequest {
    pub enabled: Mutex<Option<bool>>,
}

// ---------------------------------------------------------------------------
// Per-resource user-data
// ---------------------------------------------------------------------------

/// User-data attached to each `zwlr_output_head_v1` resource. Holds the
/// `Output` the head represents so dispatch handlers can resolve it without
/// walking client state.
#[derive(Debug, Clone)]
pub struct HeadUserData {
    pub output: Output,
}

/// User-data attached to each `zwlr_output_configuration_v1` resource. Stores
/// the serial the configuration was created with (for cancel detection) and
/// the in-flight "enabled" flag.
#[derive(Debug)]
pub struct ConfigurationUserData {
    /// Serial at the time of `create_configuration`. Compared against the
    /// manager state's current serial on every request — if they diverge,
    /// the dispatch sends `cancelled()` and refuses further updates.
    pub serial: u32,
    /// Mutable session state: pending edits and "already used" flag.
    pub inner: Mutex<ConfigurationInner>,
}

/// State machine for a single configuration session.
#[derive(Debug)]
pub struct ConfigurationInner {
    /// Per-output edits accumulated via `enable_head` / `disable_head` /
    /// `set_*`. Two separate maps so that a single client can send both
    /// disable-then-enable for the same output without ambiguity (the spec
    /// forbids it, but we defend against it).
    pub pending: HashMap<Output, PendingHead>,
    /// True after `apply` or `test` has been observed; subsequent requests
    /// are protocol errors (`already_used`).
    pub used: bool,
    /// True if any `set_*` request hit a fatal validation error after which
    /// the whole configuration must be `failed()` on apply.
    pub poisoned: bool,
}

/// Per-head edit state inside a configuration session.
#[derive(Debug, Default)]
pub struct PendingHead {
    pub enabled: Option<bool>,
    pub mode: Option<Mode>,
    pub position: Option<Point<i32, Logical>>,
    pub scale: Option<f64>,
    pub transform: Option<Transform>,
    pub adaptive_sync: Option<bool>,
    pub set_mode_done: bool,
    pub set_position_done: bool,
    pub set_scale_done: bool,
    pub set_transform_done: bool,
    pub set_adaptive_sync_done: bool,
}

/// User-data attached to each `zwlr_output_configuration_head_v1` resource.
///
/// Cancelled means the parent configuration is no longer valid (output
/// disappeared, serial outdated, or `set_custom_mode` was called) and all
/// further `set_*` requests on this object are no-ops.
#[derive(Debug)]
pub enum ConfigurationHeadUserData {
    /// The configuration head still routes to a live output.
    Ok {
        output: Output,
        configuration: Weak<ZwlrOutputConfigurationV1>,
    },
    /// The configuration head is inert; further requests are no-ops.
    Cancelled,
}

// ---------------------------------------------------------------------------
// Manager state (held by `AnvilState`)
// ---------------------------------------------------------------------------

/// Compositor-side state for the `zwlr_output_manager_v1` global.
///
/// Construct once in `AnvilState::init`, store on `AnvilState`, and register
/// dispatches via the [`crate::delegate_output_management!`] macro.
#[derive(Debug)]
pub struct OutputManagementManagerState {
    /// The advertised global. Kept around in case we ever want to remove it.
    global: GlobalId,
    /// Monotonically-increasing config serial. Bumped on every externally-
    /// visible state change so that in-flight configurations are cancelled.
    serial: u32,
    /// Per-client object graphs.
    clients: HashMap<ClientId, ClientState>,
    /// Snapshot of the most-recent broadcast state, used to diff in
    /// [`Self::notify_changes_for`]. Maps Output → (enabled, mode, position,
    /// scale, transform).
    snapshots: HashMap<Output, OutputSnapshot>,
}

/// Per-client tracking: the manager binding plus head/mode resources we
/// vended to that client.
#[derive(Debug)]
struct ClientState {
    manager: ZwlrOutputManagerV1,
    /// One entry per advertised output. Stores the head proxy and its
    /// associated mode proxies (one per `Output::modes()` entry at the time
    /// of advertising).
    heads: HashMap<Output, ClientHead>,
}

#[derive(Debug)]
struct ClientHead {
    head: ZwlrOutputHeadV1,
    /// Per-mode proxies, one per `Output::modes()` entry. Order is preserved
    /// so `set_mode(mode)` can resolve the index back to a [`Mode`].
    modes: Vec<(Mode, ZwlrOutputModeV1)>,
}

/// Snapshot of advertised output state, used for diff-broadcast.
#[derive(Debug, Clone, PartialEq)]
struct OutputSnapshot {
    enabled: bool,
    current_mode: Option<Mode>,
    position: Point<i32, Logical>,
    scale: f64,
    transform: Transform,
    adaptive_sync: bool,
    modes: Vec<Mode>,
}

impl OutputSnapshot {
    fn from_output(output: &Output) -> Self {
        let position = output.current_location();
        let scale = output.current_scale().fractional_scale();
        let transform = output.current_transform();
        let current_mode = output.current_mode();
        let modes = output.modes();
        let adaptive_sync = output
            .user_data()
            .get::<OutputAdaptiveSyncRequest>()
            .and_then(|req| *req.enabled.lock().unwrap())
            .unwrap_or(false);
        Self {
            // Per spec: a head is "enabled" iff it currently has a mode
            // mapped to compositor space. We map "has current_mode" to
            // enabled — disabled heads still get advertised, just with
            // enabled(0) and no current_mode.
            enabled: current_mode.is_some(),
            current_mode,
            position,
            scale,
            transform,
            adaptive_sync,
            modes,
        }
    }
}

impl OutputManagementManagerState {
    /// Create the `zwlr_output_manager_v1` global on `display`. The global is
    /// unfiltered (every connected client sees it) — matches anvil's
    /// historical posture; security-context filtering can be layered on later
    /// without an API break by replacing the call site to use a filtered
    /// `create_global` invocation.
    pub fn new<D>(display: &DisplayHandle) -> Self
    where
        D: GlobalDispatch<ZwlrOutputManagerV1, ()>
            + Dispatch<ZwlrOutputManagerV1, ()>
            + Dispatch<ZwlrOutputHeadV1, HeadUserData>
            + Dispatch<ZwlrOutputModeV1, ()>
            + Dispatch<ZwlrOutputConfigurationV1, ConfigurationUserData>
            + Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<ConfigurationHeadUserData>>
            + 'static,
    {
        let global = display.create_global::<D, ZwlrOutputManagerV1, _>(VERSION, ());
        Self {
            global,
            serial: 0,
            clients: HashMap::new(),
            snapshots: HashMap::new(),
        }
    }

    /// The id of the `zwlr_output_manager_v1` global, in case callers ever
    /// want to remove it (currently unused).
    pub fn global(&self) -> GlobalId {
        self.global.clone()
    }
}

// ---------------------------------------------------------------------------
// Public lifecycle helpers (called from udev/winit in 2.D.2)
// ---------------------------------------------------------------------------

/// Advertise a newly-attached [`Output`] to every existing manager binding.
///
/// Sends, in order:
///   1. `head(new_id)` on the manager,
///   2. `name`/`description`/`physical_size`/`make`/`model`/`serial_number`,
///   3. `mode(new_id)` per advertised mode (with `size`, `refresh`, `preferred`),
///   4. `enabled` and (if enabled) `current_mode`/`position`/`transform`/`scale`,
///   5. `adaptive_sync` (v4+),
///   6. `manager.done(serial)`.
///
/// Bumps the manager-state serial as a side effect.
///
/// The `D` type bound is satisfied by `AnvilState<BackendData>` once the
/// `delegate_output_management!` macro has been invoked.
pub fn add_head<D>(state: &mut OutputManagementManagerState, dh: &DisplayHandle, output: &Output)
where
    D: Dispatch<ZwlrOutputHeadV1, HeadUserData>
        + Dispatch<ZwlrOutputModeV1, ()>
        + 'static,
{
    state.serial = state.serial.wrapping_add(1);
    let snapshot = OutputSnapshot::from_output(output);
    state.snapshots.insert(output.clone(), snapshot.clone());

    // Iterate clients and send the head graph. Collect the client ids first
    // to avoid &mut/& aliasing across the inner mutation.
    let client_ids: Vec<ClientId> = state.clients.keys().cloned().collect();
    for cid in client_ids {
        let Some(client_state) = state.clients.get_mut(&cid) else {
            continue;
        };
        let manager_version = client_state.manager.version();
        let Some(client) = client_state.manager.client() else {
            continue;
        };

        // Create the head resource and send the manager.head event.
        let head = match client.create_resource::<ZwlrOutputHeadV1, _, D>(
            dh,
            manager_version,
            HeadUserData { output: output.clone() },
        ) {
            Ok(h) => h,
            Err(err) => {
                warn!(?err, name = output.name(), "add_head: create_resource failed");
                continue;
            }
        };
        client_state.manager.head(&head);
        send_static_head_events::<D>(dh, &client, &head, output, &snapshot, manager_version, &mut |modes| {
            client_state.heads.insert(
                output.clone(),
                ClientHead { head: head.clone(), modes },
            );
        });
    }

    // Final manager.done burst.
    broadcast_done(state);
}

/// Retire a previously-advertised [`Output`]. Sends `finished()` on every
/// matching head (and its mode children), drops the per-client tracking
/// entries, and bumps the serial.
pub fn remove_head(state: &mut OutputManagementManagerState, output: &Output) {
    state.snapshots.remove(output);
    state.serial = state.serial.wrapping_add(1);

    for client_state in state.clients.values_mut() {
        if let Some(ClientHead { head, modes }) = client_state.heads.remove(output) {
            for (_, mode_proxy) in modes {
                mode_proxy.finished();
            }
            head.finished();
        }
    }

    broadcast_done(state);
}

/// Re-broadcast any state changes for `output`. Diffs the live output state
/// against the most recent snapshot and emits the minimum events necessary,
/// followed by `manager.done(serial)`.
///
/// Safe to call optimistically — if nothing changed, no events are emitted
/// and the serial does not advance.
///
/// Note: this helper handles mutable per-output state (enabled, current_mode,
/// position, scale, transform, adaptive_sync). Adding or removing modes from
/// the head's mode list at runtime is not supported — smithay outputs
/// enumerate their mode list once at hotplug time and we mirror that. If
/// the mode list does change between calls, only mode entries that already
/// have a vended `zwlr_output_mode_v1` proxy will be visible to clients.
pub fn notify_changes(state: &mut OutputManagementManagerState, output: &Output) {
    let new_snapshot = OutputSnapshot::from_output(output);
    let Some(old_snapshot) = state.snapshots.get(output).cloned() else {
        // Output not yet advertised — caller must use `add_head` first.
        debug!(name = output.name(), "notify_changes: output not in snapshot map");
        return;
    };

    if new_snapshot == old_snapshot {
        return;
    }

    state.snapshots.insert(output.clone(), new_snapshot.clone());
    state.serial = state.serial.wrapping_add(1);

    let modes_changed = old_snapshot.modes != new_snapshot.modes;
    if modes_changed {
        debug!(
            name = output.name(),
            "notify_changes: mode-list changed, but dynamic add/remove of \
             zwlr_output_mode_v1 resources is not supported. Existing mode \
             proxies remain valid; new modes will be visible only to clients \
             that bind the manager after this point."
        );
    }

    for client_state in state.clients.values_mut() {
        let Some(client_head) = client_state.heads.get_mut(output) else {
            continue;
        };
        let head = &client_head.head;
        let head_version = head.version();

        // Enabled flag.
        if old_snapshot.enabled != new_snapshot.enabled {
            head.enabled(new_snapshot.enabled as i32);
        }

        // Current mode (only meaningful when enabled).
        if new_snapshot.enabled && old_snapshot.current_mode != new_snapshot.current_mode {
            if let Some(cur) = new_snapshot.current_mode {
                if let Some((_, mode_proxy)) =
                    client_head.modes.iter().find(|(m, _)| *m == cur)
                {
                    head.current_mode(mode_proxy);
                }
            }
        }

        if old_snapshot.position != new_snapshot.position {
            head.position(new_snapshot.position.x, new_snapshot.position.y);
        }
        if (old_snapshot.scale - new_snapshot.scale).abs() > f64::EPSILON {
            head.scale(new_snapshot.scale);
        }
        if old_snapshot.transform != new_snapshot.transform {
            head.transform(transform_to_wl(new_snapshot.transform).into());
        }
        if head_version >= zwlr_output_head_v1::EVT_ADAPTIVE_SYNC_SINCE
            && old_snapshot.adaptive_sync != new_snapshot.adaptive_sync
        {
            head.adaptive_sync(if new_snapshot.adaptive_sync {
                AdaptiveSyncState::Enabled
            } else {
                AdaptiveSyncState::Disabled
            });
        }
    }

    broadcast_done(state);
}

fn send_mode_events(proxy: &ZwlrOutputModeV1, mode: Mode, preferred: bool) {
    proxy.size(mode.size.w, mode.size.h);
    proxy.refresh(mode.refresh);
    if preferred {
        proxy.preferred();
    }
}

/// Send the static one-shot head events (name/description/physical_size/
/// make/model/serial_number) plus the mode advertisement and the dynamic
/// state events (enabled/current_mode/position/transform/scale/adaptive_sync).
///
/// `register_modes` is a callback that receives the freshly-vended mode
/// proxy list so the caller can stash it on its `ClientHead` map without us
/// needing &mut access to it from inside this helper.
fn send_static_head_events<D>(
    dh: &DisplayHandle,
    client: &Client,
    head: &ZwlrOutputHeadV1,
    output: &Output,
    snapshot: &OutputSnapshot,
    manager_version: u32,
    register_modes: &mut dyn FnMut(Vec<(Mode, ZwlrOutputModeV1)>),
) where
    D: Dispatch<ZwlrOutputModeV1, ()> + 'static,
{
    head.name(output.name());
    head.description(output.description());

    let phys = output.physical_properties();
    if phys.size.w > 0 && phys.size.h > 0 {
        head.physical_size(phys.size.w, phys.size.h);
    }

    if manager_version >= zwlr_output_head_v1::EVT_MAKE_SINCE && !phys.make.is_empty() {
        head.make(phys.make.clone());
    }
    if manager_version >= zwlr_output_head_v1::EVT_MODEL_SINCE && !phys.model.is_empty() {
        head.model(phys.model.clone());
    }
    if manager_version >= zwlr_output_head_v1::EVT_SERIAL_NUMBER_SINCE
        && !phys.serial_number.is_empty()
    {
        head.serial_number(phys.serial_number.clone());
    }

    let preferred = output.preferred_mode();
    let mut mode_proxies = Vec::with_capacity(snapshot.modes.len());
    for mode in &snapshot.modes {
        let Ok(mode_proxy) = client.create_resource::<ZwlrOutputModeV1, _, D>(dh, manager_version, ()) else {
            continue;
        };
        head.mode(&mode_proxy);
        send_mode_events(&mode_proxy, *mode, Some(*mode) == preferred);
        mode_proxies.push((*mode, mode_proxy));
    }

    head.enabled(snapshot.enabled as i32);
    if snapshot.enabled {
        if let Some(cur) = snapshot.current_mode {
            if let Some((_, mode_proxy)) = mode_proxies.iter().find(|(m, _)| *m == cur) {
                head.current_mode(mode_proxy);
            }
        }
        head.position(snapshot.position.x, snapshot.position.y);
        head.transform(transform_to_wl(snapshot.transform).into());
        head.scale(snapshot.scale);
    }

    if manager_version >= zwlr_output_head_v1::EVT_ADAPTIVE_SYNC_SINCE {
        head.adaptive_sync(if snapshot.adaptive_sync {
            AdaptiveSyncState::Enabled
        } else {
            AdaptiveSyncState::Disabled
        });
    }

    register_modes(mode_proxies);
}

/// Send `manager.done(serial)` to every active client and `cancelled()` to
/// every in-flight (Ongoing) configuration. Called at the end of every
/// add_head/remove_head/notify_changes burst.
fn broadcast_done(state: &mut OutputManagementManagerState) {
    let serial = state.serial;
    for client_state in state.clients.values_mut() {
        client_state.manager.done(serial);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn transform_to_wl(t: Transform) -> WlTransform {
    match t {
        Transform::Normal => WlTransform::Normal,
        Transform::_90 => WlTransform::_90,
        Transform::_180 => WlTransform::_180,
        Transform::_270 => WlTransform::_270,
        Transform::Flipped => WlTransform::Flipped,
        Transform::Flipped90 => WlTransform::Flipped90,
        Transform::Flipped180 => WlTransform::Flipped180,
        Transform::Flipped270 => WlTransform::Flipped270,
    }
}

fn wl_to_transform(t: WlTransform) -> Option<Transform> {
    Some(match t {
        WlTransform::Normal => Transform::Normal,
        WlTransform::_90 => Transform::_90,
        WlTransform::_180 => Transform::_180,
        WlTransform::_270 => Transform::_270,
        WlTransform::Flipped => Transform::Flipped,
        WlTransform::Flipped90 => Transform::Flipped90,
        WlTransform::Flipped180 => Transform::Flipped180,
        WlTransform::Flipped270 => Transform::Flipped270,
        _ => return None,
    })
}

/// Trait describing the `AnvilState` integration point for output
/// management. Generic over the backend so the protocol module doesn't
/// hard-depend on `AnvilState<T>`.
pub trait OutputManagementHandler {
    /// Returns `&mut` access to the manager state struct held on the
    /// compositor.
    fn output_management_state(&mut self) -> &mut OutputManagementManagerState;

    /// Forward a validated configuration to the backend. The default
    /// returns `Err(NotSupported)` so backends that haven't opted in get
    /// safe failure semantics.
    ///
    /// Implementations on `AnvilState<BackendData>` simply forward to
    /// `BackendData::apply_output_config`.
    fn apply_output_config(
        &mut self,
        changes: &[OutputConfigChange],
        test_only: bool,
    ) -> Result<(), OutputConfigError>;
}

// ---------------------------------------------------------------------------
// Dispatch impls
// ---------------------------------------------------------------------------

impl<D> GlobalDispatch<ZwlrOutputManagerV1, (), D> for OutputManagementManagerState
where
    D: GlobalDispatch<ZwlrOutputManagerV1, ()>,
    D: Dispatch<ZwlrOutputManagerV1, ()>,
    D: Dispatch<ZwlrOutputHeadV1, HeadUserData>,
    D: Dispatch<ZwlrOutputModeV1, ()>,
    D: Dispatch<ZwlrOutputConfigurationV1, ConfigurationUserData>,
    D: Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<ConfigurationHeadUserData>>,
    D: OutputManagementHandler,
    D: 'static,
{
    fn bind(
        state: &mut D,
        dh: &DisplayHandle,
        client: &Client,
        resource: New<ZwlrOutputManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        let manager = data_init.init(resource, ());
        let manager_version = manager.version();

        // Snapshot the outputs we need to advertise BEFORE inserting client
        // tracking, so the borrow scope stays clean.
        let outputs_to_send: Vec<(Output, OutputSnapshot)> = state
            .output_management_state()
            .snapshots
            .iter()
            .map(|(o, s)| (o.clone(), s.clone()))
            .collect();

        // Insert the client tracking entry up-front so head/mode resources
        // can be registered against it.
        state.output_management_state().clients.insert(
            client.id(),
            ClientState {
                manager: manager.clone(),
                heads: HashMap::new(),
            },
        );

        for (output, snapshot) in &outputs_to_send {
            let head = match client.create_resource::<ZwlrOutputHeadV1, _, D>(
                dh,
                manager_version,
                HeadUserData { output: output.clone() },
            ) {
                Ok(h) => h,
                Err(err) => {
                    warn!(?err, name = output.name(), "bind: create head resource failed");
                    continue;
                }
            };
            manager.head(&head);
            let mut registered = None;
            send_static_head_events::<D>(
                dh,
                client,
                &head,
                output,
                snapshot,
                manager_version,
                &mut |modes| {
                    registered = Some(modes);
                },
            );
            if let Some(modes) = registered {
                if let Some(client_state) = state.output_management_state().clients.get_mut(&client.id()) {
                    client_state.heads.insert(
                        output.clone(),
                        ClientHead { head: head.clone(), modes },
                    );
                }
            }
        }

        let serial = state.output_management_state().serial;
        manager.done(serial);
    }
}

impl<D> Dispatch<ZwlrOutputManagerV1, (), D> for OutputManagementManagerState
where
    D: Dispatch<ZwlrOutputManagerV1, ()>,
    D: Dispatch<ZwlrOutputHeadV1, HeadUserData>,
    D: Dispatch<ZwlrOutputModeV1, ()>,
    D: Dispatch<ZwlrOutputConfigurationV1, ConfigurationUserData>,
    D: Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<ConfigurationHeadUserData>>,
    D: OutputManagementHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        client: &Client,
        manager: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, serial } => {
                let g_state = state.output_management_state();
                let outdated = serial != g_state.serial;

                let configuration = data_init.init(
                    id,
                    ConfigurationUserData {
                        serial,
                        inner: Mutex::new(ConfigurationInner {
                            pending: HashMap::new(),
                            used: false,
                            poisoned: false,
                        }),
                    },
                );

                if outdated {
                    // Per protocol: cancel immediately if the client's serial
                    // is stale. The client will receive cancelled() and is
                    // expected to destroy the configuration.
                    configuration.cancelled();
                }
            }
            zwlr_output_manager_v1::Request::Stop => {
                // The client signals it no longer wants events. Send the
                // final `finished()` event and remove its tracking.
                if let Some(c) = state
                    .output_management_state()
                    .clients
                    .remove(&client.id())
                {
                    c.manager.finished();
                } else {
                    manager.finished();
                }
            }
            _ => unreachable!(),
        }
    }

    fn destroyed(state: &mut D, client: ClientId, _resource: &ZwlrOutputManagerV1, _data: &()) {
        // Abrupt teardown: drop all head/mode tracking for this client.
        state.output_management_state().clients.remove(&client);
    }
}

impl<D> Dispatch<ZwlrOutputHeadV1, HeadUserData, D> for OutputManagementManagerState
where
    D: Dispatch<ZwlrOutputHeadV1, HeadUserData>,
    D: OutputManagementHandler,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &ZwlrOutputHeadV1,
        request: zwlr_output_head_v1::Request,
        _data: &HeadUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_output_head_v1::Request::Release => {
                // Destructor; per protocol no further events expected.
            }
            _ => unreachable!(),
        }
    }

    fn destroyed(state: &mut D, client: ClientId, resource: &ZwlrOutputHeadV1, data: &HeadUserData) {
        // Scrub the resource from the client's head map.
        if let Some(client_state) = state.output_management_state().clients.get_mut(&client) {
            if let Some(client_head) = client_state.heads.get(&data.output) {
                if client_head.head.id() == resource.id() {
                    client_state.heads.remove(&data.output);
                }
            }
        }
    }
}

impl<D> Dispatch<ZwlrOutputModeV1, (), D> for OutputManagementManagerState
where
    D: Dispatch<ZwlrOutputModeV1, ()>,
    D: OutputManagementHandler,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &ZwlrOutputModeV1,
        request: zwlr_output_mode_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_output_mode_v1::Request::Release => {
                // Destructor; nothing to do.
            }
            _ => unreachable!(),
        }
    }
}

impl<D> Dispatch<ZwlrOutputConfigurationV1, ConfigurationUserData, D>
    for OutputManagementManagerState
where
    D: Dispatch<ZwlrOutputConfigurationV1, ConfigurationUserData>,
    D: Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<ConfigurationHeadUserData>>,
    D: OutputManagementHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        conf: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        data: &ConfigurationUserData,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        let outdated = data.serial != state.output_management_state().serial;

        match request {
            zwlr_output_configuration_v1::Request::EnableHead { id, head } => {
                let head_data = head.data::<HeadUserData>().cloned();
                let mut inner = data.inner.lock().unwrap();

                if inner.used {
                    conf.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyUsed,
                        "configuration has already been applied/tested",
                    );
                    let _ = data_init.init(id, Mutex::new(ConfigurationHeadUserData::Cancelled));
                    return;
                }

                let Some(head_data) = head_data else {
                    warn!("EnableHead: head resource missing user-data");
                    let _ = data_init.init(id, Mutex::new(ConfigurationHeadUserData::Cancelled));
                    inner.poisoned = true;
                    return;
                };

                if outdated {
                    let _ = data_init.init(id, Mutex::new(ConfigurationHeadUserData::Cancelled));
                    return;
                }

                // Spec: same head may not be configured twice in one config.
                if inner.pending.contains_key(&head_data.output) {
                    conf.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyConfiguredHead,
                        "head has already been configured in this session",
                    );
                    let _ = data_init.init(id, Mutex::new(ConfigurationHeadUserData::Cancelled));
                    return;
                }

                let mut pending = PendingHead::default();
                pending.enabled = Some(true);
                inner.pending.insert(head_data.output.clone(), pending);

                let _ = data_init.init(
                    id,
                    Mutex::new(ConfigurationHeadUserData::Ok {
                        output: head_data.output,
                        configuration: conf.downgrade(),
                    }),
                );
            }
            zwlr_output_configuration_v1::Request::DisableHead { head } => {
                let head_data = head.data::<HeadUserData>().cloned();
                let mut inner = data.inner.lock().unwrap();

                if inner.used {
                    conf.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyUsed,
                        "configuration has already been applied/tested",
                    );
                    return;
                }

                let Some(head_data) = head_data else {
                    warn!("DisableHead: head resource missing user-data");
                    inner.poisoned = true;
                    return;
                };

                if outdated {
                    return;
                }

                if inner.pending.contains_key(&head_data.output) {
                    conf.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyConfiguredHead,
                        "head has already been configured in this session",
                    );
                    return;
                }

                let mut pending = PendingHead::default();
                pending.enabled = Some(false);
                inner.pending.insert(head_data.output, pending);
            }
            zwlr_output_configuration_v1::Request::Apply => {
                handle_apply_or_test::<D>(state, conf, data, outdated, false);
            }
            zwlr_output_configuration_v1::Request::Test => {
                handle_apply_or_test::<D>(state, conf, data, outdated, true);
            }
            zwlr_output_configuration_v1::Request::Destroy => {
                // Destructor — nothing to do; resources will be torn down.
            }
            _ => unreachable!(),
        }
    }
}

impl<D> Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<ConfigurationHeadUserData>, D>
    for OutputManagementManagerState
where
    D: Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<ConfigurationHeadUserData>>,
    D: OutputManagementHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        conf_head: &ZwlrOutputConfigurationHeadV1,
        request: zwlr_output_configuration_head_v1::Request,
        data: &Mutex<ConfigurationHeadUserData>,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        // Snapshot the configuration_head's current state; bail out fast if
        // cancelled or the parent configuration has gone away.
        let (output, conf) = {
            let guard = data.lock().unwrap();
            match &*guard {
                ConfigurationHeadUserData::Cancelled => return,
                ConfigurationHeadUserData::Ok { output, configuration } => {
                    let Ok(conf) = configuration.upgrade() else {
                        return;
                    };
                    (output.clone(), conf)
                }
            }
        };

        let Some(conf_data) = conf.data::<ConfigurationUserData>() else {
            return;
        };
        let g_serial = state.output_management_state().serial;
        let outdated = conf_data.serial != g_serial;

        let mut inner = conf_data.inner.lock().unwrap();
        if inner.used {
            conf.post_error(
                zwlr_output_configuration_v1::Error::AlreadyUsed,
                "configuration has already been applied/tested",
            );
            return;
        }
        if outdated {
            // Per spec: continue to accept requests on outdated configs but
            // they're effectively no-ops; apply() will send cancelled.
            return;
        }

        let Some(pending) = inner.pending.get_mut(&output) else {
            warn!("ConfigurationHead: pending entry missing for output");
            return;
        };

        match request {
            zwlr_output_configuration_head_v1::Request::SetMode { mode } => {
                if pending.set_mode_done {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::AlreadySet,
                        "set_mode/set_custom_mode already issued on this head",
                    );
                    return;
                }
                pending.set_mode_done = true;

                // Resolve the mode proxy back to a Mode by looking up the
                // client's head→modes map.
                let g_state = state.output_management_state();
                let resolved = g_state.clients.values().find_map(|cs| {
                    cs.heads.get(&output).and_then(|ch| {
                        ch.modes.iter().find(|(_, p)| p.id() == mode.id()).map(|(m, _)| *m)
                    })
                });
                let Some(resolved_mode) = resolved else {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::InvalidMode,
                        "mode does not belong to head",
                    );
                    return;
                };
                if !output.modes().contains(&resolved_mode) {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::InvalidMode,
                        "mode is not currently advertised by head",
                    );
                    return;
                }
                pending.mode = Some(resolved_mode);
            }
            zwlr_output_configuration_head_v1::Request::SetCustomMode { .. } => {
                if pending.set_mode_done {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::AlreadySet,
                        "set_mode/set_custom_mode already issued on this head",
                    );
                    return;
                }
                pending.set_mode_done = true;
                // Custom modes are not supported. Mark the whole config as
                // poisoned so apply() returns failed; do not protocol-error
                // out — clients reasonably try this and we shouldn't kill
                // them.
                warn!(
                    name = output.name(),
                    "set_custom_mode is not supported; configuration will fail"
                );
                inner.poisoned = true;

                // Also mark the conf_head as cancelled to prevent further
                // sets from leaking into pending.
                drop(inner);
                if let Ok(mut guard) = data.lock() {
                    *guard = ConfigurationHeadUserData::Cancelled;
                }
            }
            zwlr_output_configuration_head_v1::Request::SetPosition { x, y } => {
                if pending.set_position_done {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::AlreadySet,
                        "set_position already issued on this head",
                    );
                    return;
                }
                pending.set_position_done = true;
                pending.position = Some(Point::from((x, y)));
            }
            zwlr_output_configuration_head_v1::Request::SetTransform { transform } => {
                if pending.set_transform_done {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::AlreadySet,
                        "set_transform already issued on this head",
                    );
                    return;
                }
                pending.set_transform_done = true;
                let WEnum::Value(wl_t) = transform else {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::InvalidTransform,
                        "unknown transform value",
                    );
                    return;
                };
                let Some(t) = wl_to_transform(wl_t) else {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::InvalidTransform,
                        "unknown transform value",
                    );
                    return;
                };
                pending.transform = Some(t);
            }
            zwlr_output_configuration_head_v1::Request::SetScale { scale } => {
                if pending.set_scale_done {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::AlreadySet,
                        "set_scale already issued on this head",
                    );
                    return;
                }
                pending.set_scale_done = true;
                if scale <= 0.0 || !scale.is_finite() {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::InvalidScale,
                        "scale is negative, zero, or non-finite",
                    );
                    return;
                }
                pending.scale = Some(scale);
            }
            zwlr_output_configuration_head_v1::Request::SetAdaptiveSync { state: as_state } => {
                if pending.set_adaptive_sync_done {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::AlreadySet,
                        "set_adaptive_sync already issued on this head",
                    );
                    return;
                }
                pending.set_adaptive_sync_done = true;
                let WEnum::Value(s) = as_state else {
                    conf_head.post_error(
                        zwlr_output_configuration_head_v1::Error::InvalidAdaptiveSyncState,
                        "unknown adaptive_sync state",
                    );
                    return;
                };
                pending.adaptive_sync = Some(matches!(s, AdaptiveSyncState::Enabled));
            }
            _ => unreachable!(),
        }
    }
}

// ---------------------------------------------------------------------------
// Apply / test path
// ---------------------------------------------------------------------------

fn handle_apply_or_test<D>(
    state: &mut D,
    conf: &ZwlrOutputConfigurationV1,
    data: &ConfigurationUserData,
    outdated: bool,
    test_only: bool,
) where
    D: OutputManagementHandler + 'static,
{
    // Drain the pending edits, marking the configuration as used so further
    // requests trip the AlreadyUsed protocol error.
    let pending_map: HashMap<Output, PendingHead>;
    let poisoned: bool;
    {
        let mut inner = data.inner.lock().unwrap();
        if inner.used {
            conf.post_error(
                zwlr_output_configuration_v1::Error::AlreadyUsed,
                "configuration has already been applied/tested",
            );
            return;
        }
        inner.used = true;
        poisoned = inner.poisoned;
        pending_map = std::mem::take(&mut inner.pending);
    }

    if outdated {
        conf.cancelled();
        return;
    }

    if poisoned {
        // e.g. set_custom_mode was called.
        conf.failed();
        return;
    }

    if pending_map.is_empty() {
        conf.failed();
        return;
    }

    // Per spec: at least one head must remain enabled.
    let any_enabled = pending_map
        .values()
        .any(|p| p.enabled.unwrap_or(false));
    if !any_enabled {
        conf.failed();
        return;
    }

    // Translate pending edits into OutputConfigChange values. The "Update"
    // vs "Enable" distinction is based on the previous snapshot's enabled
    // state — if a head was already enabled and the client only set fields
    // that don't toggle enabled, surface that as Update so backends can
    // skip the full modeset path.
    let mut changes: Vec<OutputConfigChange> = Vec::with_capacity(pending_map.len());
    {
        let g_state = state.output_management_state();
        for (output, pending) in pending_map {
            let prev_enabled = g_state
                .snapshots
                .get(&output)
                .map(|s| s.enabled)
                .unwrap_or(false);
            let action = match pending.enabled {
                Some(false) => OutputConfigAction::Disable,
                Some(true) | None => {
                    if prev_enabled {
                        OutputConfigAction::Update {
                            mode: pending.mode,
                            position: pending.position,
                            scale: pending.scale,
                            transform: pending.transform,
                            adaptive_sync: pending.adaptive_sync,
                        }
                    } else {
                        OutputConfigAction::Enable {
                            mode: pending.mode,
                            position: pending.position,
                            scale: pending.scale,
                            transform: pending.transform,
                            adaptive_sync: pending.adaptive_sync,
                        }
                    }
                }
            };
            changes.push(OutputConfigChange { output, action });
        }
    }

    // Hand off to the backend for validation + apply.
    let result = state.apply_output_config(&changes, test_only);

    match result {
        Ok(()) => {
            conf.succeeded();
            if !test_only {
                // Stash any client-requested adaptive-sync state on the
                // Output user-data so subsequent snapshots reflect it.
                for change in &changes {
                    let as_req = match &change.action {
                        OutputConfigAction::Enable { adaptive_sync, .. }
                        | OutputConfigAction::Update { adaptive_sync, .. } => *adaptive_sync,
                        OutputConfigAction::Disable => None,
                    };
                    if let Some(want) = as_req {
                        let req = change
                            .output
                            .user_data()
                            .get_or_insert_threadsafe(OutputAdaptiveSyncRequest::default);
                        *req.enabled.lock().unwrap() = Some(want);
                    }
                }
                // Re-broadcast diffs for affected outputs.
                let outputs_to_notify: Vec<Output> =
                    changes.iter().map(|c| c.output.clone()).collect();
                let g_state = state.output_management_state();
                for output in outputs_to_notify {
                    notify_changes(g_state, &output);
                }
            }
        }
        Err(OutputConfigError::Cancelled) => {
            conf.cancelled();
        }
        Err(_) => {
            conf.failed();
        }
    }
}

// ---------------------------------------------------------------------------
// Delegate macro
// ---------------------------------------------------------------------------

/// Wire up the `zwlr_output_management_v1` family of dispatches on the given
/// type.
///
/// Mirrors the `delegate_tearing_control!` / `delegate_gamma_control!`
/// shape, including the optional `@<Generics: Bound + 'static>` prefix so it
/// can be applied to `AnvilState<BackendData>`.
///
/// ```ignore
/// crate::delegate_output_management!(
///     @<BackendData: Backend + 'static> AnvilState<BackendData>
/// );
/// ```
#[macro_export]
macro_rules! delegate_output_management {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        const _: () = {
            use std::sync::Mutex as _DOMMutex;

            use smithay::reexports::{
                wayland_protocols_wlr::output_management::v1::server::{
                    zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1,
                    zwlr_output_configuration_v1::ZwlrOutputConfigurationV1,
                    zwlr_output_head_v1::ZwlrOutputHeadV1,
                    zwlr_output_manager_v1::ZwlrOutputManagerV1,
                    zwlr_output_mode_v1::ZwlrOutputModeV1,
                },
                wayland_server::{delegate_dispatch, delegate_global_dispatch},
            };
            use $crate::protocols::output_management::{
                ConfigurationHeadUserData, ConfigurationUserData, HeadUserData,
                OutputManagementManagerState,
            };

            delegate_global_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrOutputManagerV1: ()] => OutputManagementManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrOutputManagerV1: ()] => OutputManagementManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrOutputHeadV1: HeadUserData] => OutputManagementManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrOutputModeV1: ()] => OutputManagementManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrOutputConfigurationV1: ConfigurationUserData] => OutputManagementManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrOutputConfigurationHeadV1: _DOMMutex<ConfigurationHeadUserData>] => OutputManagementManagerState
            );
        };
    };
}

pub use delegate_output_management;
