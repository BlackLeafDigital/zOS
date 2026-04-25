//! Implementation of the staging `wp_tearing_control_v1` Wayland protocol.
//!
//! This module advertises the `wp_tearing_control_manager_v1` global and
//! handles per-`wl_surface` `wp_tearing_control_v1` objects. It stores the
//! double-buffered presentation hint (`vsync` or `async`) on each surface so
//! that the render path can later decide whether to submit an asynchronous
//! page flip for surfaces that prefer tearing over vsync latency.
//!
//! ## Scope
//!
//! This module is *only* the protocol surface: dispatch, cached state, and
//! the `delegate_tearing_control!` macro. Reading the cached hint and
//! actually submitting `DRM_MODE_PAGE_FLIP_ASYNC` to the kernel is the job of
//! the render path (`udev.rs` for the DRM backend, `winit.rs` for dev). The
//! public helper [`surface_wants_async_presentation`] is the integration
//! point for that follow-up work.
//!
//! ## Pattern
//!
//! Mirrors smithay's MIT-licensed `wp_content_type_v1` handler in shape:
//!
//! - One [`TearingControlManagerState`] held on `AnvilState` that owns the
//!   global id.
//! - One double-buffered, [`Cacheable`] per-surface state struct
//!   ([`TearingControlSurfaceCachedState`]) that the dispatch handler writes
//!   to via `pending()` and that the render path reads via `current()` after
//!   commit.
//! - One private "is a `wp_tearing_control_v1` already attached?" flag stored
//!   in the surface's `data_map` so we can raise the
//!   `tearing_control_exists` protocol error per the spec.
//! - One [`TearingControlUserData`] holding a `Weak<WlSurface>` so that a
//!   surface destruction simply makes the per-surface object inert.

use std::sync::{
    Mutex,
    atomic::{self, AtomicBool},
};

use smithay::{
    reexports::{
        wayland_protocols::wp::tearing_control::v1::server::{
            wp_tearing_control_manager_v1::{self, WpTearingControlManagerV1},
            wp_tearing_control_v1::{self, WpTearingControlV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum, Weak,
            backend::{ClientId, GlobalId},
            protocol::wl_surface::WlSurface,
        },
    },
    wayland::compositor::{self, Cacheable},
};

/// Double-buffered per-surface presentation-hint state.
///
/// Stored on `SurfaceData::cached_state`. The dispatch handler writes the
/// pending value when the client calls `set_presentation_hint`, and the
/// compositor advances pending → current automatically when the client calls
/// `wl_surface.commit` (this happens for free thanks to smithay's
/// `on_commit_buffer_handler` in the existing compositor commit path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TearingControlSurfaceCachedState {
    hint: wp_tearing_control_v1::PresentationHint,
}

impl TearingControlSurfaceCachedState {
    /// The current presentation hint for this surface.
    pub fn hint(&self) -> wp_tearing_control_v1::PresentationHint {
        self.hint
    }

    /// Convenience: `true` iff the surface is requesting `async` (tearing) presentation.
    pub fn wants_async(&self) -> bool {
        matches!(self.hint, wp_tearing_control_v1::PresentationHint::Async)
    }
}

impl Default for TearingControlSurfaceCachedState {
    fn default() -> Self {
        Self {
            hint: wp_tearing_control_v1::PresentationHint::Vsync,
        }
    }
}

impl Cacheable for TearingControlSurfaceCachedState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        *self
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// Tracks whether a `wp_tearing_control_v1` is already bound to a given
/// `wl_surface`, so a duplicate request can raise `tearing_control_exists`.
///
/// Stored on `SurfaceData::data_map` (not `cached_state`) because the
/// "already attached" question is *not* double-buffered — it's a property of
/// the resource graph, not of the committed surface state.
#[derive(Debug, Default)]
struct TearingControlSurfaceData {
    is_resource_attached: AtomicBool,
}

impl TearingControlSurfaceData {
    fn set_attached(&self, attached: bool) {
        self.is_resource_attached.store(attached, atomic::Ordering::Release);
    }

    fn is_attached(&self) -> bool {
        self.is_resource_attached.load(atomic::Ordering::Acquire)
    }
}

/// User-data attached to each `wp_tearing_control_v1` resource.
///
/// Holds a weak reference to the surface so that:
///   1. Routing a `set_presentation_hint` request to the right surface is a
///      single upgrade-and-go.
///   2. If the surface has been destroyed, the upgrade fails and the request
///      is silently dropped (the protocol says the resource becomes inert).
#[derive(Debug)]
pub struct TearingControlUserData(Mutex<Weak<WlSurface>>);

impl TearingControlUserData {
    fn new(surface: WlSurface) -> Self {
        Self(Mutex::new(surface.downgrade()))
    }

    fn wl_surface(&self) -> Option<WlSurface> {
        self.0.lock().unwrap().upgrade().ok()
    }
}

/// Compositor-side state for the `wp_tearing_control_manager_v1` global.
///
/// Construct once in `AnvilState::init`, store on `AnvilState`, and register
/// dispatches via the `delegate_tearing_control!` macro.
#[derive(Debug)]
pub struct TearingControlManagerState {
    global: GlobalId,
}

impl TearingControlManagerState {
    /// Create the `wp_tearing_control_manager_v1` global on `display`.
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

    /// The id of the `wp_tearing_control_manager_v1` global, in case callers
    /// want to remove it later (we don't currently).
    pub fn global(&self) -> GlobalId {
        self.global.clone()
    }
}

/// Render-path helper: returns `true` if the most-recently-committed
/// presentation hint on this surface is `async` (i.e. the client is asking
/// for tearing).
///
/// This is the only public entry point the udev/winit render paths should
/// need — they call it for the surface(s) that are about to be scanned out
/// and then OR an `ALLOW_TEARING` bit into the relevant `FrameFlags` (or set
/// EGL swap interval to 0 on winit) once that plumbing exists in smithay.
pub fn surface_wants_async_presentation(surface: &WlSurface) -> bool {
    compositor::with_states(surface, |states| {
        states
            .cached_state
            .get::<TearingControlSurfaceCachedState>()
            .current()
            .wants_async()
    })
}

// ---------------------------------------------------------------------------
// Dispatch impls
// ---------------------------------------------------------------------------

impl<D> GlobalDispatch<WpTearingControlManagerV1, (), D> for TearingControlManagerState
where
    D: GlobalDispatch<WpTearingControlManagerV1, ()>,
    D: Dispatch<WpTearingControlManagerV1, ()>,
    D: Dispatch<WpTearingControlV1, TearingControlUserData>,
    D: 'static,
{
    fn bind(
        _state: &mut D,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<WpTearingControlManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(resource, ());
    }
}

impl<D> Dispatch<WpTearingControlManagerV1, (), D> for TearingControlManagerState
where
    D: Dispatch<WpTearingControlManagerV1, ()>,
    D: Dispatch<WpTearingControlV1, TearingControlUserData>,
    D: 'static,
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
                // Per the spec, only one wp_tearing_control_v1 may exist per
                // surface at a time. Track the flag in the surface's data_map
                // and raise the protocol error on duplicate requests.
                let already_attached = compositor::with_states(&surface, |states| {
                    states
                        .data_map
                        .insert_if_missing_threadsafe(TearingControlSurfaceData::default);
                    let data = states
                        .data_map
                        .get::<TearingControlSurfaceData>()
                        .expect("inserted above");

                    let was_attached = data.is_attached();
                    if !was_attached {
                        data.set_attached(true);
                    }
                    was_attached
                });

                if already_attached {
                    manager.post_error(
                        wp_tearing_control_manager_v1::Error::TearingControlExists,
                        "wl_surface already has a wp_tearing_control_v1 attached",
                    );
                } else {
                    data_init.init(id, TearingControlUserData::new(surface));
                }
            }
            wp_tearing_control_manager_v1::Request::Destroy => {
                // Destructor request — destroying the manager does NOT
                // invalidate per-surface objects. Nothing for us to do here.
            }
            _ => unreachable!(),
        }
    }
}

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
                // Filter out `WEnum::Unknown` cleanly; the spec only defines
                // two values and an unknown one means a non-conformant client
                // (drop the request rather than panic).
                let WEnum::Value(hint) = hint else {
                    return;
                };

                // The surface may already be gone — make the call inert.
                let Some(surface) = data.wl_surface() else {
                    return;
                };

                compositor::with_states(&surface, |states| {
                    states
                        .cached_state
                        .get::<TearingControlSurfaceCachedState>()
                        .pending()
                        .hint = hint;
                });
            }
            wp_tearing_control_v1::Request::Destroy => {
                // Destructor: the spec says reverting to vsync is
                // double-buffered, applied on the next commit. Also clear
                // the "already attached" guard so a subsequent
                // get_tearing_control on the same surface succeeds.
                let Some(surface) = data.wl_surface() else {
                    return;
                };

                compositor::with_states(&surface, |states| {
                    if let Some(d) = states.data_map.get::<TearingControlSurfaceData>() {
                        d.set_attached(false);
                    }
                    states
                        .cached_state
                        .get::<TearingControlSurfaceCachedState>()
                        .pending()
                        .hint = wp_tearing_control_v1::PresentationHint::Vsync;
                });
            }
            _ => unreachable!(),
        }
    }

    fn destroyed(
        _state: &mut D,
        _client: ClientId,
        _resource: &WpTearingControlV1,
        _data: &TearingControlUserData,
    ) {
        // Graceful destruction is handled in the `Destroy` request arm above.
        // For abrupt client teardown the surface's destroyed handler will
        // reclaim the cached/data_map entries on its own.
    }
}

// ---------------------------------------------------------------------------
// Delegate macro
// ---------------------------------------------------------------------------

/// Wire up `wp_tearing_control_manager_v1` and `wp_tearing_control_v1`
/// dispatch on the given type.
///
/// Mirrors smithay's `delegate_idle_inhibit!` / `delegate_content_type!`
/// macro shape, including the optional `@<Generics: Bound + 'static>` prefix
/// so it can be applied to `AnvilState<BackendData>`.
///
/// ```ignore
/// crate::protocols::tearing_control::delegate_tearing_control!(
///     @<BackendData: Backend + 'static> AnvilState<BackendData>
/// );
/// ```
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

pub use delegate_tearing_control;
