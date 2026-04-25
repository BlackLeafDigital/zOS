//! Implementation of the unstable `zwlr_gamma_control_v1` Wayland protocol.
//!
//! This module advertises the `zwlr_gamma_control_manager_v1` global and
//! creates per-`wl_output` `zwlr_gamma_control_v1` objects so that privileged
//! clients (redshift, gammastep, wluma) can read the gamma-LUT size from the
//! server, mmap a buffer of `3 * size` u16 entries, and hand it back via
//! `set_gamma(fd)`.
//!
//! ## Scope
//!
//! This module is *only* the protocol surface: dispatch, per-output cached
//! state (the most-recent LUT and the list of active controls), and the
//! `delegate_gamma_control!` macro. **Actually programming the LUT into the
//! kernel via the DRM connector's `GAMMA_LUT` property is out of scope here.**
//! The udev backend will read [`current_gamma_lut_for_output`] from the
//! render path once that wiring lands. See the `TODO(gamma-drm-apply)`
//! marker on the `set_gamma` arm for the exact integration point.
//!
//! ## Per-output state
//!
//! Per the protocol spec, only one `zwlr_gamma_control_v1` may be active for
//! a given `wl_output` at a time. To enforce that we stash an
//! [`OutputGammaState`] on the `Output`'s [`UserDataMap`] which holds:
//!
//! 1. the LUT size advertised to clients,
//! 2. a `Vec` of weak references to every `zwlr_gamma_control_v1` ever
//!    created against this output (so we can `failed()` them on output drop
//!    or when a new control supersedes an old one), and
//! 3. the most-recently-applied LUT (flat `R…G…B…` u16 layout), to be
//!    consumed by the DRM render path.
//!
//! ## Pattern
//!
//! Hand-rolled in the same shape as
//! `crate::protocols::tearing_control` — one [`GammaControlManagerState`]
//! holding the GlobalId, plus a [`GammaControlUserData`] on each per-output
//! resource pointing back to its `WlOutput` so requests can be routed.

use std::{
    fs::File,
    io::Read,
    os::fd::{FromRawFd, IntoRawFd, OwnedFd},
    sync::Mutex,
};

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols_wlr::gamma_control::v1::server::{
            zwlr_gamma_control_manager_v1::{self, ZwlrGammaControlManagerV1},
            zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
            backend::{ClientId, GlobalId},
            protocol::wl_output::WlOutput,
        },
    },
};

/// LUT size used when we cannot probe the underlying DRM connector.
///
/// 256 is the de-facto standard sRGB ramp depth that virtually every active
/// gamma client (redshift, gammastep, wluma) expects, and matches what
/// wlroots advertises by default for outputs without a DRM backend.
const DEFAULT_GAMMA_LUT_SIZE: u32 = 256;

// ---------------------------------------------------------------------------
// Per-output cached state
// ---------------------------------------------------------------------------

/// Compositor-side gamma state attached to each [`Output`] via its
/// [`UserDataMap`].
///
/// Stored under `output.user_data()` and lazily inserted on the first
/// `get_gamma_control` for that output.
#[derive(Debug)]
struct OutputGammaState {
    inner: Mutex<OutputGammaStateInner>,
}

#[derive(Debug)]
struct OutputGammaStateInner {
    /// LUT size advertised to clients. Currently hard-coded to
    /// [`DEFAULT_GAMMA_LUT_SIZE`]; dynamic probing is a follow-up once the
    /// DRM connector handle is plumbed through to the protocol layer.
    size: u32,

    /// Most-recently-applied LUT, layout `[R0..Rn, G0..Gn, B0..Bn]`. Read
    /// by the DRM render path. `None` means no client has set a LUT yet.
    current_lut: Option<Vec<u16>>,

    /// Every gamma_control resource that's ever been created for this
    /// output. We use this list to (a) `failed()` the previous control when
    /// a new one supersedes it, and (b) `failed()` any survivors if the
    /// output is dropped. Stale (already-destroyed) entries are pruned on
    /// each `register_control` call.
    controls: Vec<Weak<ZwlrGammaControlV1>>,
}

impl OutputGammaState {
    fn new(size: u32) -> Self {
        Self {
            inner: Mutex::new(OutputGammaStateInner {
                size,
                current_lut: None,
                controls: Vec::new(),
            }),
        }
    }
}

/// Look up (or lazily create) the gamma state for an output.
fn output_state(output: &Output) -> &OutputGammaState {
    output
        .user_data()
        .get_or_insert_threadsafe(|| OutputGammaState::new(DEFAULT_GAMMA_LUT_SIZE))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Per-resource user-data attached to each `zwlr_gamma_control_v1`.
///
/// Holds a cloned `WlOutput` so that, on `set_gamma`, we can resolve the
/// associated [`Output`] without a lookup over `space.outputs()`. We hold
/// the `WlOutput` directly (not a `Weak`) because the resource's lifetime
/// is tied to its containing client; if the client drops the output the
/// resource is destroyed too.
#[derive(Debug)]
pub struct GammaControlUserData {
    output: WlOutput,
}

impl GammaControlUserData {
    fn new(output: WlOutput) -> Self {
        Self { output }
    }

    fn output(&self) -> Option<Output> {
        Output::from_resource(&self.output)
    }
}

/// Compositor-side state for the `zwlr_gamma_control_manager_v1` global.
///
/// Construct once in `AnvilState::init`, store on `AnvilState`, and register
/// dispatches via the [`crate::delegate_gamma_control!`] macro.
#[derive(Debug)]
pub struct GammaControlManagerState {
    global: GlobalId,
}

impl GammaControlManagerState {
    /// Create the `zwlr_gamma_control_manager_v1` global on `display`.
    pub fn new<D>(display: &DisplayHandle) -> Self
    where
        D: GlobalDispatch<ZwlrGammaControlManagerV1, ()>
            + Dispatch<ZwlrGammaControlManagerV1, ()>
            + Dispatch<ZwlrGammaControlV1, GammaControlUserData>
            + 'static,
    {
        let global = display.create_global::<D, ZwlrGammaControlManagerV1, _>(1, ());
        Self { global }
    }

    /// The id of the `zwlr_gamma_control_manager_v1` global, in case callers
    /// want to remove it later (we don't currently).
    pub fn global(&self) -> GlobalId {
        self.global.clone()
    }
}

/// Render-path helper: returns a copy of the most-recently-applied gamma LUT
/// for `output`, or `None` if no client has called `set_gamma` yet.
///
/// The returned vector has length `3 * size` and is laid out `R…G…B…`.
/// This is the integration point for the future DRM apply path.
pub fn current_gamma_lut_for_output(output: &Output) -> Option<Vec<u16>> {
    let state = output_state(output);
    let inner = state.inner.lock().unwrap();
    inner.current_lut.clone()
}

/// Render-path helper: returns the gamma LUT size advertised for `output`.
pub fn gamma_lut_size_for_output(output: &Output) -> u32 {
    let state = output_state(output);
    let inner = state.inner.lock().unwrap();
    inner.size
}

/// Should be called when an [`Output`] is being torn down (e.g. monitor
/// unplug). Sends `failed` to every still-live `zwlr_gamma_control_v1`
/// bound to this output so the client knows to release its resource.
pub fn output_destroyed(output: &Output) {
    let state = output_state(output);
    let mut inner = state.inner.lock().unwrap();
    for weak in inner.controls.drain(..) {
        if let Ok(resource) = weak.upgrade() {
            resource.failed();
        }
    }
    inner.current_lut = None;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Register a freshly-created control as the active one for `output`.
///
/// Sends `failed` to any previously-active controls for the same output
/// (per the spec: "There can only be at most one gamma control object per
/// output"). Returns the LUT size to advertise via `gamma_size`.
fn register_control(output: &Output, new_control: &ZwlrGammaControlV1) -> u32 {
    let state = output_state(output);
    let mut inner = state.inner.lock().unwrap();

    // Prune dead refs and `failed()` any survivors that are not the new one.
    inner.controls.retain(|weak| {
        let Ok(resource) = weak.upgrade() else {
            return false;
        };
        if resource.id() == new_control.id() {
            // Shouldn't happen (we haven't registered this one yet) but be
            // defensive: keep it without failing.
            true
        } else {
            resource.failed();
            // The client is expected to destroy the failed object; drop it
            // from our tracking list now so we don't hold the weak ref.
            false
        }
    });

    inner.controls.push(new_control.downgrade());
    inner.size
}

/// Read a `3 * size * 2` byte LUT from `fd` and store it on `output`.
///
/// Returns `Ok(())` if the LUT was successfully consumed and cached, or an
/// `Err(())` if the read failed (in which case the caller should emit
/// `failed` to the client).
fn apply_gamma_from_fd(output: &Output, fd: OwnedFd) -> Result<(), ()> {
    let state = output_state(output);
    let size = {
        let inner = state.inner.lock().unwrap();
        inner.size
    } as usize;

    let expected_bytes = size
        .checked_mul(3)
        .and_then(|n| n.checked_mul(std::mem::size_of::<u16>()))
        .ok_or(())?;

    // SAFETY: the wayland scanner hands us ownership of the fd via the
    // `OwnedFd` type. Wrapping it in a `File` transfers ownership to the
    // file (the file's `Drop` will close it exactly once).
    let mut file = unsafe { File::from_raw_fd(fd.into_raw_fd()) };

    let mut bytes = vec![0u8; expected_bytes];
    if file.read_exact(&mut bytes).is_err() {
        return Err(());
    }

    // Decode little-endian u16 entries. The protocol does not strictly
    // specify endianness, but every known client (redshift/gammastep/wluma)
    // and every other wlroots-style compositor (sway, niri, wlroots itself)
    // treats the buffer as native-endian — and Wayland is local-IPC only,
    // so native and LE coincide on every platform we ship to (x86_64,
    // aarch64).
    let lut: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_ne_bytes([chunk[0], chunk[1]]))
        .collect();

    let mut inner = state.inner.lock().unwrap();
    inner.current_lut = Some(lut);

    // TODO(gamma-drm-apply): Once the udev backend wires its
    // `DrmDevice` + connector handle through to the protocol layer, call
    // `DrmDevice::set_gamma_property(connector, &lut)` (or the equivalent
    // CRTC `GAMMA_LUT` blob set) here so the LUT actually takes effect on
    // the next page flip. For now the LUT is just cached for the render
    // path to pick up.

    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch impls
// ---------------------------------------------------------------------------

impl<D> GlobalDispatch<ZwlrGammaControlManagerV1, (), D> for GammaControlManagerState
where
    D: GlobalDispatch<ZwlrGammaControlManagerV1, ()>,
    D: Dispatch<ZwlrGammaControlManagerV1, ()>,
    D: Dispatch<ZwlrGammaControlV1, GammaControlUserData>,
    D: 'static,
{
    fn bind(
        _state: &mut D,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrGammaControlManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(resource, ());
    }
}

impl<D> Dispatch<ZwlrGammaControlManagerV1, (), D> for GammaControlManagerState
where
    D: Dispatch<ZwlrGammaControlManagerV1, ()>,
    D: Dispatch<ZwlrGammaControlV1, GammaControlUserData>,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _manager: &ZwlrGammaControlManagerV1,
        request: zwlr_gamma_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_gamma_control_manager_v1::Request::GetGammaControl { id, output } => {
                let resolved_output = Output::from_resource(&output);
                let control = data_init.init(id, GammaControlUserData::new(output));

                let Some(output) = resolved_output else {
                    // The wl_output we were handed isn't backed by a live
                    // smithay Output. Per spec, the appropriate response is
                    // to mark the freshly-created control as failed.
                    control.failed();
                    return;
                };

                // Register as active (sends `failed` to any predecessor) and
                // immediately advertise the LUT size as required by the spec
                // ("This event is sent immediately when the gamma control
                // object is created.").
                let size = register_control(&output, &control);
                control.gamma_size(size);
            }
            zwlr_gamma_control_manager_v1::Request::Destroy => {
                // Destructor on the manager. Per spec, existing per-output
                // objects remain valid; nothing to do here.
            }
            _ => unreachable!(),
        }
    }
}

impl<D> Dispatch<ZwlrGammaControlV1, GammaControlUserData, D> for GammaControlManagerState
where
    D: Dispatch<ZwlrGammaControlV1, GammaControlUserData>,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        resource: &ZwlrGammaControlV1,
        request: zwlr_gamma_control_v1::Request,
        data: &GammaControlUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_gamma_control_v1::Request::SetGamma { fd } => {
                // The associated Output may already be gone (monitor unplug
                // raced with the request). Make the call inert in that case.
                let Some(output) = data.output() else {
                    resource.failed();
                    return;
                };

                if apply_gamma_from_fd(&output, fd).is_err() {
                    resource.failed();
                }
            }
            zwlr_gamma_control_v1::Request::Destroy => {
                // Destructor: per spec, the original gamma table should be
                // restored. We clear the cached LUT so the render path stops
                // applying it; actually re-programming the DRM connector to
                // identity is a TODO together with `gamma-drm-apply`.
                if let Some(output) = data.output() {
                    let state = output_state(&output);
                    let mut inner = state.inner.lock().unwrap();
                    inner.current_lut = None;
                    inner.controls.retain(|weak| {
                        weak.upgrade().map(|r| r.id() != resource.id()).unwrap_or(false)
                    });
                }
                // TODO(gamma-drm-apply): also re-program the DRM CRTC's
                // GAMMA_LUT blob to identity here.
            }
            _ => unreachable!(),
        }
    }

    fn destroyed(
        _state: &mut D,
        _client: ClientId,
        resource: &ZwlrGammaControlV1,
        data: &GammaControlUserData,
    ) {
        // Abrupt client teardown: scrub the resource from the output's
        // tracking list so we don't leak weak refs. Don't clear the LUT
        // here — the spec only requires LUT restoration on graceful
        // `destroy`, and an abrupt teardown is more often a crash where the
        // user probably wants the last applied tint preserved until another
        // client takes over.
        if let Some(output) = data.output() {
            let state = output_state(&output);
            let mut inner = state.inner.lock().unwrap();
            inner.controls.retain(|weak| {
                weak.upgrade().map(|r| r.id() != resource.id()).unwrap_or(false)
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Delegate macro
// ---------------------------------------------------------------------------

/// Wire up `zwlr_gamma_control_manager_v1` and `zwlr_gamma_control_v1`
/// dispatch on the given type.
///
/// Mirrors the `delegate_tearing_control!` shape, including the optional
/// `@<Generics: Bound + 'static>` prefix so it can be applied to
/// `AnvilState<BackendData>`.
///
/// ```ignore
/// crate::delegate_gamma_control!(
///     @<BackendData: Backend + 'static> AnvilState<BackendData>
/// );
/// ```
#[macro_export]
macro_rules! delegate_gamma_control {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        const _: () = {
            use smithay::reexports::{
                wayland_protocols_wlr::gamma_control::v1::server::{
                    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
                    zwlr_gamma_control_v1::ZwlrGammaControlV1,
                },
                wayland_server::{delegate_dispatch, delegate_global_dispatch},
            };
            use $crate::protocols::gamma_control::{
                GammaControlManagerState, GammaControlUserData,
            };

            delegate_global_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrGammaControlManagerV1: ()] => GammaControlManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrGammaControlManagerV1: ()] => GammaControlManagerState
            );

            delegate_dispatch!(
                $(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)?
                $ty: [ZwlrGammaControlV1: GammaControlUserData] => GammaControlManagerState
            );
        };
    };
}

pub use delegate_gamma_control;
