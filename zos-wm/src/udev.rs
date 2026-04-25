// Allow in this module because of existing usage
#![allow(clippy::uninlined_format_args)]
use std::{
    collections::hash_map::HashMap,
    io,
    ops::Not,
    path::Path,
    sync::{Mutex, Once, atomic::Ordering},
    time::{Duration, Instant},
};

use crate::{
    drawing::*,
    render::*,
    shell::WindowElement,
    state::{AnvilState, Backend, take_presentation_feedback, update_primary_scanout_output},
};
use crate::{
    shell::WindowRenderElement,
    state::{DndIcon, SurfaceDmabufFeedback},
};
#[cfg(feature = "renderer_sync")]
use smithay::backend::drm::compositor::PrimaryPlaneElement;
#[cfg(feature = "egl")]
use smithay::backend::renderer::ImportEgl;
#[cfg(feature = "debug")]
use smithay::backend::renderer::{ImportMem, multigpu::MultiTexture};
use smithay::{
    backend::{
        SwapBuffersError,
        allocator::{
            Fourcc, Modifier,
            dmabuf::Dmabuf,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            CreateDrmNodeError, DrmAccessError, DrmDevice, DrmDeviceFd, DrmError, DrmEvent, DrmEventMetadata,
            DrmEventTime, DrmNode, DrmSurface, GbmBufferedSurface, NodeType,
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{self, EGLContext, EGLDevice, EGLDisplay, context::ContextPriority},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            DebugFlags, ImportDma, ImportMemWl,
            damage::Error as OutputDamageTrackerError,
            element::{AsRenderElements, RenderElementStates, memory::MemoryRenderBuffer},
            gles::{Capability, GlesRenderer},
            multigpu::{GpuManager, MultiRenderer, gbm::GbmGlesBackend},
        },
        session::{
            Event as SessionEvent, Session,
            libseat::{self, LibSeatSession},
        },
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    delegate_dmabuf, delegate_drm_lease,
    desktop::{
        space::{Space, SurfaceTree},
        utils::OutputPresentationFeedback,
    },
    input::{
        keyboard::LedState,
        pointer::{CursorImageAttributes, CursorImageStatus},
    },
    output::{Mode as WlMode, Output, PhysicalProperties},
    reexports::{
        calloop::{
            EventLoop, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::{
            Device as _,
            control::{Device, ModeTypeFlags, connector, crtc},
        },
        input::{DeviceCapability, Libinput},
        rustix::fs::OFlags,
        wayland_protocols::wp::{
            linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1,
            presentation_time::server::wp_presentation_feedback,
        },
        wayland_server::{Display, DisplayHandle, backend::GlobalId, protocol::wl_surface},
    },
    utils::{DeviceFd, IsAlive, Logical, Monotonic, Point, Scale, Time, Transform},
    wayland::{
        compositor,
        dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        drm_lease::{
            DrmLease, DrmLeaseBuilder, DrmLeaseHandler, DrmLeaseRequest, DrmLeaseState, LeaseRejected,
        },
        drm_syncobj::{DrmSyncobjHandler, DrmSyncobjState, supports_syncobj_eventfd},
        presentation::Refresh,
    },
};
use smithay_drm_extras::{
    display_info,
    drm_scanner::{DrmScanEvent, DrmScanner},
};
use tracing::{debug, error, info, trace, warn};

// we cannot simply pick the first supported format of the intersection of *all* formats, because:
// - we do not want something like Abgr4444, which looses color information, if something better is available
// - some formats might perform terribly
// - we might need some work-arounds, if one supports modifiers, but the other does not
//
// So lets just pick `ARGB2101010` (10-bit) or `ARGB8888` (8-bit) for now, they are widely supported.
const SUPPORTED_FORMATS: &[Fourcc] = &[
    Fourcc::Abgr2101010,
    Fourcc::Argb2101010,
    Fourcc::Abgr8888,
    Fourcc::Argb8888,
];
const SUPPORTED_FORMATS_8BIT_ONLY: &[Fourcc] = &[Fourcc::Abgr8888, Fourcc::Argb8888];

type UdevRenderer<'a> = MultiRenderer<
    'a,
    'a,
    GbmGlesBackend<GlesRenderer, DrmDeviceFd>,
    GbmGlesBackend<GlesRenderer, DrmDeviceFd>,
>;

#[derive(Debug, Clone, PartialEq)]
struct UdevOutputId {
    device_id: DrmNode,
    crtc: crtc::Handle,
}

pub struct UdevData {
    pub session: LibSeatSession,
    dh: DisplayHandle,
    dmabuf_state: Option<(DmabufState, DmabufGlobal)>,
    syncobj_state: Option<DrmSyncobjState>,
    primary_gpu: DrmNode,
    gpus: GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>,
    backends: HashMap<DrmNode, BackendData>,
    pointer_images: Vec<(xcursor::parser::Image, MemoryRenderBuffer)>,
    pointer_element: PointerElement,
    #[cfg(feature = "debug")]
    fps_texture: Option<MultiTexture>,
    pointer_image: crate::cursor::Cursor,
    debug_flags: DebugFlags,
    keyboards: Vec<smithay::reexports::input::Device>,
    /// Compiled rounded-corners pixel-shader program, bound to the primary
    /// GPU's `GlesRenderer`. `None` if shader compilation failed (logged
    /// at backend init) or if no primary renderer was available. The
    /// render path consults this when drawing per-window rounded masks.
    ///
    /// NOTE: For multi-GPU setups this single program belongs to the
    /// primary GPU's renderer; cross-GPU rendering (when a window's
    /// render_node differs from primary_gpu) currently skips the rounded
    /// mask. Per-GPU programs are tracked in
    /// `TODO(P4-multi-gpu-rounded)`.
    pub rounded_effect: Option<crate::effects::rounded::RoundedCornersEffect>,
}

impl UdevData {
    pub fn set_debug_flags(&mut self, flags: DebugFlags) {
        if self.debug_flags != flags {
            self.debug_flags = flags;

            for (_, backend) in self.backends.iter_mut() {
                for (_, surface) in backend.surfaces.iter_mut() {
                    surface.drm_output.set_debug_flags(flags);
                }
            }
        }
    }

    pub fn debug_flags(&self) -> DebugFlags {
        self.debug_flags
    }
}

impl DmabufHandler for AnvilState<UdevData> {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.backend_data.dmabuf_state.as_mut().unwrap().0
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        if self
            .backend_data
            .gpus
            .single_renderer(&self.backend_data.primary_gpu)
            .and_then(|mut renderer| renderer.import_dmabuf(&dmabuf, None))
            .is_ok()
        {
            dmabuf.set_node(self.backend_data.primary_gpu);
            let _ = notifier.successful::<AnvilState<UdevData>>();
        } else {
            notifier.failed();
        }
    }
}
delegate_dmabuf!(AnvilState<UdevData>);

impl Backend for UdevData {
    const HAS_RELATIVE_MOTION: bool = true;
    const HAS_GESTURES: bool = true;

    fn seat_name(&self) -> String {
        self.session.seat()
    }

    fn reset_buffers(&mut self, output: &Output) {
        if let Some(id) = output.user_data().get::<UdevOutputId>() {
            if let Some(gpu) = self.backends.get_mut(&id.device_id) {
                if let Some(surface) = gpu.surfaces.get_mut(&id.crtc) {
                    surface.drm_output.reset_buffers();
                }
            }
        }
    }

    fn early_import(&mut self, surface: &wl_surface::WlSurface) {
        if let Err(err) = self.gpus.early_import(self.primary_gpu, surface) {
            warn!("Early buffer import failed: {}", err);
        }
    }

    fn update_led_state(&mut self, led_state: LedState) {
        for keyboard in self.keyboards.iter_mut() {
            keyboard.led_update(led_state.into());
        }
    }

    /// Apply (or test) a validated output configuration on the udev/DRM
    /// backend.
    ///
    /// Scope of this implementation (task 2.D.3):
    ///   * Only the DRM-modeset side of the apply path is handled here.
    ///     Position / scale / transform are not DRM-level concerns and are
    ///     left to the dispatch layer (and to a follow-up task that needs
    ///     `&mut Space<WindowElement>`, which `Backend` does not have access
    ///     to).
    ///   * `Disable` is not yet supported: dropping a `SurfaceData` requires
    ///     coordinating `space.unmap_output` which we don't have access to
    ///     here. See `// TODO(output-disable)` below.
    ///   * Adaptive-sync requests are stashed in the [`Output`] user-data
    ///     so subsequent snapshots reflect the request, but the actual DRM
    ///     property change is deferred. See `// TODO(adaptive-sync-drm-prop)`.
    ///   * `test_only` walks the change set and validates the requested mode
    ///     exists in the connector's mode list. Smithay does not currently
    ///     expose `DRM_MODE_ATOMIC_TEST_ONLY`, so the validation is purely
    ///     mode-list-based for v1.
    fn apply_output_config(
        &mut self,
        changes: &[crate::protocols::output_management::OutputConfigChange],
        test_only: bool,
    ) -> Result<(), crate::protocols::output_management::OutputConfigError> {
        use crate::protocols::output_management::{OutputConfigAction, OutputConfigError};

        for change in changes {
            // Resolve the Output → (DrmNode, crtc) routing via UdevOutputId.
            let id = change.output.user_data().get::<UdevOutputId>().ok_or_else(|| {
                tracing::warn!(
                    name = change.output.name(),
                    "apply_output_config: output is missing UdevOutputId user-data"
                );
                OutputConfigError::Backend("output is not driven by the udev backend".into())
            })?;

            match &change.action {
                OutputConfigAction::Disable => {
                    // The Space-side unmap happens in
                    // `AnvilState::apply_space_change` (state.rs) once we
                    // return `Ok(())`. Here we only handle the DRM side:
                    // drop the SurfaceData, whose `Drop` impl tears down
                    // the DrmOutput and removes the GlobalId. This
                    // mirrors the surface-removal logic in
                    // `connector_disconnected` (udev.rs).
                    if test_only {
                        // No DRM-test path for disable; the dispatch
                        // already verified the output exists and has a
                        // UdevOutputId. Nothing else to validate.
                        tracing::debug!(
                            name = change.output.name(),
                            "apply_output_config: Disable test_only: ok"
                        );
                        continue;
                    }
                    if let Some(device) = self.backends.get_mut(&id.device_id) {
                        // Dropping the SurfaceData runs its Drop impl
                        // which tears down DrmOutput and unregisters the
                        // wl_output global. Mirrors connector_disconnected
                        // surface-teardown.
                        let _ = device.surfaces.remove(&id.crtc);
                        tracing::info!(
                            name = change.output.name(),
                            crtc = ?id.crtc,
                            "apply_output_config: disabled output (DRM surface dropped)"
                        );
                    } else {
                        tracing::warn!(
                            name = change.output.name(),
                            device = ?id.device_id,
                            "apply_output_config: Disable: backend device not found"
                        );
                    }
                }
                OutputConfigAction::Enable {
                    mode,
                    adaptive_sync,
                    ..
                }
                | OutputConfigAction::Update {
                    mode,
                    adaptive_sync,
                    ..
                } => {
                    // Mode change: only do work if a mode was requested.
                    if let Some(requested) = mode {
                        // Validate the mode against the smithay Output's
                        // advertised mode list first — this is the only
                        // validation we can do without touching DRM state.
                        if !change.output.modes().contains(requested) {
                            tracing::warn!(
                                name = change.output.name(),
                                ?requested,
                                "apply_output_config: requested mode is not advertised on output"
                            );
                            return Err(OutputConfigError::InvalidMode);
                        }

                        // For test_only: mode-list validation already passed
                        // above — there's no DRM-level test-commit primitive
                        // exposed by smithay, so we accept the mode if it's
                        // advertised on the head and call it a successful
                        // test. For real applies, fall through to the modeset.
                        if !test_only {
                            // Resolve the requested smithay::output::Mode to
                            // a concrete drm::control::Mode by walking the
                            // DRM device's connectors and finding the one
                            // currently driving our crtc.
                            let device =
                                self.backends.get_mut(&id.device_id).ok_or_else(|| {
                                    tracing::error!(
                                        name = change.output.name(),
                                        device = ?id.device_id,
                                        "apply_output_config: backend device not found"
                                    );
                                    OutputConfigError::Backend(
                                        "backend device not found".into(),
                                    )
                                })?;

                            let drm_mode = match find_drm_mode_for_crtc(
                                device.drm_output_manager.device(),
                                id.crtc,
                                requested,
                            ) {
                                Some(m) => m,
                                None => {
                                    tracing::warn!(
                                        name = change.output.name(),
                                        ?requested,
                                        crtc = ?id.crtc,
                                        "apply_output_config: no matching DRM mode found on connector for crtc"
                                    );
                                    return Err(OutputConfigError::InvalidMode);
                                }
                            };

                            let surface =
                                device.surfaces.get_mut(&id.crtc).ok_or_else(|| {
                                    tracing::error!(
                                        name = change.output.name(),
                                        crtc = ?id.crtc,
                                        "apply_output_config: surface not found for crtc"
                                    );
                                    OutputConfigError::Backend(
                                        "surface not found for crtc".into(),
                                    )
                                })?;

                            // Acquire a renderer on the appropriate GPU (the
                            // device's render node, falling back to the
                            // primary GPU). Splitting the borrow on
                            // self.gpus and self.backends works because they
                            // are disjoint fields of UdevData.
                            let render_node =
                                surface.render_node.unwrap_or(self.primary_gpu);
                            let mut renderer =
                                self.gpus.single_renderer(&render_node).map_err(|err| {
                                    tracing::error!(
                                        ?err,
                                        ?render_node,
                                        "apply_output_config: failed to obtain renderer"
                                    );
                                    OutputConfigError::Backend(format!(
                                        "renderer acquisition failed: {:?}",
                                        err
                                    ))
                                })?;

                            // Drive the modeset. We feed an empty render
                            // element list ("simulate" a black frame) — the
                            // existing `connector_disconnected` path uses
                            // the same idiom (see `try_to_restore_modifiers`
                            // call site). A flicker-free path would resubmit
                            // the live frame; that's a future optimization.
                            if let Err(err) = surface.drm_output.use_mode::<
                                _,
                                OutputRenderElements<
                                    UdevRenderer<'_>,
                                    WindowRenderElement<UdevRenderer<'_>>,
                                >,
                            >(
                                drm_mode,
                                &mut renderer,
                                &DrmOutputRenderElements::default(),
                            ) {
                                tracing::error!(
                                    ?err,
                                    name = change.output.name(),
                                    crtc = ?id.crtc,
                                    "apply_output_config: DrmOutput::use_mode failed"
                                );
                                return Err(OutputConfigError::Backend(format!(
                                    "modeset failed: {:?}",
                                    err
                                )));
                            }

                            // Update the smithay Output's advertised current
                            // mode so subsequent snapshots (and the
                            // `notify_changes` broadcast that the dispatch
                            // emits after we return Ok) report the new mode.
                            // We don't touch position/transform/scale here —
                            // those are the dispatch / space's responsibility.
                            let new_wl_mode = WlMode::from(drm_mode);
                            change.output.change_current_state(
                                Some(new_wl_mode),
                                None,
                                None,
                                None,
                            );

                            tracing::info!(
                                name = change.output.name(),
                                crtc = ?id.crtc,
                                ?new_wl_mode,
                                "apply_output_config: modeset applied"
                            );
                        }
                    }

                    // Adaptive-sync handling: stash the requested state on
                    // the Output user-data so the dispatch's snapshot
                    // (OutputAdaptiveSyncRequest) reflects it. We don't
                    // touch DRM properties here.
                    //
                    // TODO(adaptive-sync-drm-prop): wire this through to the
                    // VRR_ENABLED connector property once smithay exposes a
                    // safe path. On NVIDIA 580 + multi-monitor the research
                    // doc recommends advertising as Disabled regardless, so
                    // accepting the request without hardware change is the
                    // right v1 behavior.
                    if let Some(want) = adaptive_sync {
                        let req = change.output.user_data().get_or_insert_threadsafe(
                            crate::protocols::output_management::OutputAdaptiveSyncRequest::default,
                        );
                        *req.enabled.lock().unwrap() = Some(*want);
                    }
                }
            }
        }

        Ok(())
    }

    /// Advertise dmabuf-capture formats from the primary GPU's renderer.
    ///
    /// `DmabufConstraints.node` is the DRM render node clients should
    /// allocate from (so they don't have to copy across GPUs); `formats`
    /// is the per-fourcc list of supported modifiers from the primary
    /// GPU's GLES renderer. If we can't reach the primary renderer right
    /// now (e.g. the device list is mid-enumeration), fall back to
    /// shm-only by returning `None`.
    fn screencopy_dma_constraints(
        &mut self,
    ) -> Option<smithay::wayland::image_copy_capture::DmabufConstraints> {
        use std::collections::HashMap;

        use smithay::wayland::image_copy_capture::DmabufConstraints;

        let renderer = self.gpus.single_renderer(&self.primary_gpu).ok()?;
        let mut by_fourcc: HashMap<Fourcc, Vec<Modifier>> = HashMap::new();
        for fmt in renderer.dmabuf_formats().iter() {
            by_fourcc.entry(fmt.code).or_default().push(fmt.modifier);
        }
        let formats: Vec<(Fourcc, Vec<Modifier>)> = by_fourcc.into_iter().collect();
        if formats.is_empty() {
            return None;
        }
        Some(DmabufConstraints {
            node: self.primary_gpu,
            formats,
        })
    }
}

/// Walk the DRM device's connectors looking for one whose current encoder is
/// bound to `crtc`, and return the `drm::control::Mode` on that connector
/// whose smithay [`WlMode`] equivalent matches `requested`.
///
/// Returns `None` if no connector matches, or if the connector has no mode
/// matching the request.
fn find_drm_mode_for_crtc(
    drm_device: &DrmDevice,
    crtc: crtc::Handle,
    requested: &WlMode,
) -> Option<smithay::reexports::drm::control::Mode> {
    let resources = drm_device.resource_handles().ok()?;
    for connector_handle in resources.connectors() {
        let info = match drm_device.get_connector(*connector_handle, false) {
            Ok(info) => info,
            Err(err) => {
                tracing::trace!(
                    ?err,
                    connector = ?connector_handle,
                    "find_drm_mode_for_crtc: get_connector failed"
                );
                continue;
            }
        };

        // Skip connectors that aren't actively driving a crtc, or are bound
        // to a different crtc than ours.
        let Some(encoder_handle) = info.current_encoder() else {
            continue;
        };
        let Ok(encoder_info) = drm_device.get_encoder(encoder_handle) else {
            continue;
        };
        if encoder_info.crtc() != Some(crtc) {
            continue;
        }

        // Found the connector driving our crtc — match its modes.
        for drm_mode in info.modes() {
            if WlMode::from(*drm_mode) == *requested {
                return Some(*drm_mode);
            }
        }
    }
    None
}

pub fn run_udev() {
    let mut event_loop = EventLoop::try_new().unwrap();
    let display = Display::new().unwrap();
    let mut display_handle = display.handle();

    /*
     * Initialize session
     */
    let (session, notifier) = match LibSeatSession::new() {
        Ok(ret) => ret,
        Err(err) => {
            error!("Could not initialize a session: {}", err);
            return;
        }
    };

    info!(seat = %session.seat(), "libseat session opened");

    match all_gpus(session.seat()) {
        Ok(gpus) => info!(count = gpus.len(), "DRM devices on seat: {:?}", gpus),
        Err(err) => warn!(?err, "Failed to enumerate GPUs on seat"),
    }

    /*
     * Initialize the compositor
     */
    let mut primary_gpu_source: &'static str = "primary_gpu()";
    let env_override = if let Ok(v) = std::env::var("ZOS_RENDER_DEVICE") {
        primary_gpu_source = "ZOS_RENDER_DEVICE";
        Some(v)
    } else if let Ok(v) = std::env::var("ANVIL_DRM_DEVICE") {
        primary_gpu_source = "ANVIL_DRM_DEVICE (deprecated)";
        Some(v)
    } else {
        None
    };
    let primary_gpu = if let Some(var) = env_override {
        DrmNode::from_path(var).expect("Invalid drm device path")
    } else {
        primary_gpu(session.seat())
            .unwrap()
            .and_then(|x| DrmNode::from_path(x).ok()?.node_with_type(NodeType::Render)?.ok())
            .unwrap_or_else(|| {
                primary_gpu_source = "all_gpus() fallback";
                all_gpus(session.seat())
                    .unwrap()
                    .into_iter()
                    .find_map(|x| DrmNode::from_path(x).ok())
                    .expect("No GPU!")
            })
    };
    info!(?primary_gpu, ?primary_gpu_source, "Selected primary GPU");

    let gpus = GpuManager::new(GbmGlesBackend::with_factory(|display| {
        let context = EGLContext::new_with_priority(display, ContextPriority::High)?;
        let mut capabilities = unsafe { GlesRenderer::supported_capabilities(&context)? };
        if std::env::var("ANVIL_GLES_DISABLE_INSTANCING").is_ok() {
            capabilities.retain(|capability| *capability != Capability::Instancing);
        }
        Ok(unsafe { GlesRenderer::with_capabilities(context, capabilities)? })
    }))
    .unwrap();

    let data = UdevData {
        dh: display_handle.clone(),
        dmabuf_state: None,
        syncobj_state: None,
        session,
        primary_gpu,
        gpus,
        backends: HashMap::new(),
        pointer_image: crate::cursor::Cursor::load(),
        pointer_images: Vec::new(),
        pointer_element: PointerElement::default(),
        #[cfg(feature = "debug")]
        fps_texture: None,
        debug_flags: DebugFlags::empty(),
        keyboards: Vec::new(),
        // Filled in below once `device_added(primary_gpu, ...)` has set up
        // the primary renderer; see `state.backend_data.rounded_effect = ...`.
        rounded_effect: None,
    };
    let mut state = AnvilState::init(display, event_loop.handle(), data, true);

    /*
     * Initialize the udev backend
     */
    let udev_backend = match UdevBackend::new(&state.seat_name) {
        Ok(ret) => ret,
        Err(err) => {
            error!(error = ?err, "Failed to initialize udev backend");
            return;
        }
    };

    /*
     * Initialize libinput backend
     */
    let mut libinput_context = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        state.backend_data.session.clone().into(),
    );
    libinput_context.udev_assign_seat(&state.seat_name).unwrap();
    let libinput_backend = LibinputInputBackend::new(libinput_context.clone());

    /*
     * Bind all our objects that get driven by the event loop
     */
    event_loop
        .handle()
        .insert_source(libinput_backend, move |mut event, _, data| {
            let dh = data.backend_data.dh.clone();
            if let InputEvent::DeviceAdded { device } = &mut event {
                if device.has_capability(DeviceCapability::Keyboard) {
                    if let Some(led_state) = data.seat.get_keyboard().map(|keyboard| keyboard.led_state()) {
                        device.led_update(led_state.into());
                    }
                    data.backend_data.keyboards.push(device.clone());
                }
            } else if let InputEvent::DeviceRemoved { ref device } = event {
                if device.has_capability(DeviceCapability::Keyboard) {
                    data.backend_data.keyboards.retain(|item| item != device);
                }
            }

            data.process_input_event(&dh, event)
        })
        .unwrap();

    event_loop
        .handle()
        .insert_source(notifier, move |event, &mut (), data| match event {
            SessionEvent::PauseSession => {
                libinput_context.suspend();
                info!("pausing session");

                for backend in data.backend_data.backends.values_mut() {
                    backend.drm_output_manager.pause();
                    backend.active_leases.clear();
                    if let Some(lease_global) = backend.leasing_global.as_mut() {
                        lease_global.suspend();
                    }
                }
            }
            SessionEvent::ActivateSession => {
                info!("resuming session");

                if let Err(err) = libinput_context.resume() {
                    error!("Failed to resume libinput context: {:?}", err);
                }
                for (node, backend) in data
                    .backend_data
                    .backends
                    .iter_mut()
                    .map(|(handle, backend)| (*handle, backend))
                {
                    // if we do not care about flicking (caused by modesetting) we could just
                    // pass true for disable connectors here. this would make sure our drm
                    // device is in a known state (all connectors and planes disabled).
                    // but for demonstration we choose a more optimistic path by leaving the
                    // state as is and assume it will just work. If this assumption fails
                    // we will try to reset the state when trying to queue a frame.
                    backend
                        .drm_output_manager
                        .lock()
                        .activate(false)
                        .expect("failed to activate drm backend");
                    if let Some(lease_global) = backend.leasing_global.as_mut() {
                        lease_global.resume::<AnvilState<UdevData>>();
                    }
                    data.handle
                        .insert_idle(move |data| data.render(node, None, data.clock.now()));
                }
            }
        })
        .unwrap();

    // We try to initialize the primary node before others to make sure
    // any display only node can fall back to the primary node for rendering
    let primary_node = primary_gpu
        .node_with_type(NodeType::Primary)
        .and_then(|node| node.ok());
    let primary_device = udev_backend.device_list().find(|(device_id, _)| {
        primary_node
            .map(|primary_node| *device_id == primary_node.dev_id())
            .unwrap_or(false)
            || *device_id == primary_gpu.dev_id()
    });

    if let Some((device_id, path)) = primary_device {
        let node = DrmNode::from_dev_id(device_id).expect("failed to get primary node");
        state
            .device_added(node, path)
            .expect("failed to initialize primary node");
    }

    let primary_device_id = primary_device.map(|(device_id, _)| device_id);
    for (device_id, path) in udev_backend.device_list() {
        if Some(device_id) == primary_device_id {
            continue;
        }

        if let Err(err) = DrmNode::from_dev_id(device_id)
            .map_err(DeviceAddError::DrmNode)
            .and_then(|node| state.device_added(node, path))
        {
            error!("Skipping device {device_id}: {err}");
        }
    }
    state.shm_state.update_formats(
        state
            .backend_data
            .gpus
            .single_renderer(&primary_gpu)
            .unwrap()
            .shm_formats(),
    );

    #[cfg_attr(not(feature = "egl"), allow(unused_mut))]
    let mut renderer = state.backend_data.gpus.single_renderer(&primary_gpu).unwrap();

    #[cfg(feature = "debug")]
    {
        #[allow(deprecated)]
        let fps_image =
            image::io::Reader::with_format(std::io::Cursor::new(FPS_NUMBERS_PNG), image::ImageFormat::Png)
                .decode()
                .unwrap();
        let fps_texture = renderer
            .import_memory(
                &fps_image.to_rgba8(),
                Fourcc::Abgr8888,
                (fps_image.width() as i32, fps_image.height() as i32).into(),
                false,
            )
            .expect("Unable to upload FPS texture");

        for backend in state.backend_data.backends.values_mut() {
            for surface in backend.surfaces.values_mut() {
                surface.fps_element = Some(FpsElement::new(fps_texture.clone()));
            }
        }
        state.backend_data.fps_texture = Some(fps_texture);
    }

    #[cfg(feature = "egl")]
    {
        info!(?primary_gpu, "Trying to initialize EGL Hardware Acceleration",);
        match renderer.bind_wl_display(&display_handle) {
            Ok(_) => info!("EGL hardware-acceleration enabled"),
            Err(err) => info!(?err, "Failed to initialize EGL hardware-acceleration"),
        }
    }

    // init dmabuf support with format list from our primary gpu
    let dmabuf_formats = renderer.dmabuf_formats();
    let default_feedback = DmabufFeedbackBuilder::new(primary_gpu.dev_id(), dmabuf_formats)
        .build()
        .unwrap();
    let mut dmabuf_state = DmabufState::new();
    let global = dmabuf_state
        .create_global_with_default_feedback::<AnvilState<UdevData>>(&display_handle, &default_feedback);
    state.backend_data.dmabuf_state = Some((dmabuf_state, global));

    // ---- Compile rounded-corners pixel shader on the primary GPU's
    //      `GlesRenderer`. Drop the dmabuf-init `renderer` borrow first
    //      (it borrows `state.backend_data.gpus`); take a fresh
    //      `single_renderer` here, then unwrap the `MultiRenderer` to
    //      `&mut GlesRenderer` via `AsMut`. Failure does NOT panic — we
    //      log a warning and leave `rounded_effect = None`, which the
    //      render path treats as "no rounding".
    drop(renderer);
    {
        match state.backend_data.gpus.single_renderer(&primary_gpu) {
            Ok(mut multi) => {
                let gles: &mut GlesRenderer = multi.as_mut();
                match crate::effects::rounded::RoundedCornersEffect::new(gles) {
                    Ok(effect) => {
                        info!(
                            ?primary_gpu,
                            "Compiled rounded-corners pixel shader on primary GPU"
                        );
                        state.backend_data.rounded_effect = Some(effect);
                    }
                    Err(err) => {
                        warn!(
                            ?err,
                            ?primary_gpu,
                            "Failed to compile rounded-corners pixel shader; \
                             windows will render without corner rounding"
                        );
                    }
                }
            }
            Err(err) => {
                warn!(
                    ?err,
                    ?primary_gpu,
                    "Could not bind primary renderer for rounded-corners shader compile; \
                     windows will render without corner rounding"
                );
            }
        }
    }

    let gpus = &mut state.backend_data.gpus;
    state
        .backend_data
        .backends
        .iter_mut()
        .for_each(|(node, backend_data)| {
            // Update the per drm surface dmabuf feedback
            backend_data.surfaces.values_mut().for_each(|surface_data| {
                surface_data.dmabuf_feedback = surface_data.dmabuf_feedback.take().or_else(|| {
                    surface_data.drm_output.with_compositor(|compositor| {
                        get_surface_dmabuf_feedback(
                            primary_gpu,
                            surface_data.render_node,
                            *node,
                            gpus,
                            compositor.surface(),
                        )
                    })
                });
            });
        });

    // Expose syncobj protocol if supported by primary GPU
    if std::env::var("ZOS_DISABLE_SYNCOBJ").is_ok() {
        info!("Explicit-sync (syncobj) disabled by ZOS_DISABLE_SYNCOBJ env var");
    } else if let Some(primary_node) = state
        .backend_data
        .primary_gpu
        .node_with_type(NodeType::Primary)
        .and_then(|x| x.ok())
    {
        if let Some(backend) = state.backend_data.backends.get(&primary_node) {
            let import_device = backend.drm_output_manager.device().device_fd().clone();
            if supports_syncobj_eventfd(&import_device) {
                let syncobj_state =
                    DrmSyncobjState::new::<AnvilState<UdevData>>(&display_handle, import_device);
                state.backend_data.syncobj_state = Some(syncobj_state);
                info!("Explicit-sync (syncobj) enabled (NVIDIA-friendly path)");
            } else {
                warn!("Explicit-sync (syncobj) unavailable: kernel rejected the eventfd probe (need >= 6.6)");
            }
        }
    }

    event_loop
        .handle()
        .insert_source(udev_backend, move |event, _, data| match event {
            UdevEvent::Added { device_id, path } => {
                if let Err(err) = DrmNode::from_dev_id(device_id)
                    .map_err(DeviceAddError::DrmNode)
                    .and_then(|node| data.device_added(node, &path))
                {
                    error!("Skipping device {device_id}: {err}");
                }
            }
            UdevEvent::Changed { device_id } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    data.device_changed(node)
                }
            }
            UdevEvent::Removed { device_id } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    data.device_removed(node)
                }
            }
        })
        .unwrap();

    /*
     * Start XWayland if supported
     */
    #[cfg(feature = "xwayland")]
    state.start_xwayland();

    // Start the zos-wm IPC server. The handle is bound to the loop's
    // lifetime — Drop removes the socket file, so we keep it alive until
    // the main loop exits.
    let _ipc_server = state.start_ipc_server();

    /*
     * And run our loop
     */

    while state.running.load(Ordering::SeqCst) {
        let result = event_loop.dispatch(Some(Duration::from_millis(16)), &mut state);
        if result.is_err() {
            state.running.store(false, Ordering::SeqCst);
        } else {
            state.space.refresh();
            state.popups.cleanup();
            display_handle.flush_clients().unwrap();
        }
    }
}

impl DrmLeaseHandler for AnvilState<UdevData> {
    fn drm_lease_state(&mut self, node: DrmNode) -> &mut DrmLeaseState {
        self.backend_data
            .backends
            .get_mut(&node)
            .unwrap()
            .leasing_global
            .as_mut()
            .unwrap()
    }

    fn lease_request(
        &mut self,
        node: DrmNode,
        request: DrmLeaseRequest,
    ) -> Result<DrmLeaseBuilder, LeaseRejected> {
        let backend = self
            .backend_data
            .backends
            .get(&node)
            .ok_or(LeaseRejected::default())?;

        let drm_device = backend.drm_output_manager.device();
        let mut builder = DrmLeaseBuilder::new(drm_device);
        for conn in request.connectors {
            if let Some((_, crtc)) = backend
                .non_desktop_connectors
                .iter()
                .find(|(handle, _)| *handle == conn)
            {
                builder.add_connector(conn);
                builder.add_crtc(*crtc);
                let planes = drm_device.planes(crtc).map_err(LeaseRejected::with_cause)?;
                let (primary_plane, primary_plane_claim) = planes
                    .primary
                    .iter()
                    .find_map(|plane| {
                        drm_device
                            .claim_plane(plane.handle, *crtc)
                            .map(|claim| (plane, claim))
                    })
                    .ok_or_else(LeaseRejected::default)?;
                builder.add_plane(primary_plane.handle, primary_plane_claim);
                if let Some((cursor, claim)) = planes.cursor.iter().find_map(|plane| {
                    drm_device
                        .claim_plane(plane.handle, *crtc)
                        .map(|claim| (plane, claim))
                }) {
                    builder.add_plane(cursor.handle, claim);
                }
            } else {
                tracing::warn!(?conn, "Lease requested for desktop connector, denying request");
                return Err(LeaseRejected::default());
            }
        }

        Ok(builder)
    }

    fn new_active_lease(&mut self, node: DrmNode, lease: DrmLease) {
        let backend = self.backend_data.backends.get_mut(&node).unwrap();
        backend.active_leases.push(lease);
    }

    fn lease_destroyed(&mut self, node: DrmNode, lease: u32) {
        let backend = self.backend_data.backends.get_mut(&node).unwrap();
        backend.active_leases.retain(|l| l.id() != lease);
    }
}

delegate_drm_lease!(AnvilState<UdevData>);

impl DrmSyncobjHandler for AnvilState<UdevData> {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.backend_data.syncobj_state.as_mut()
    }
}
smithay::delegate_drm_syncobj!(AnvilState<UdevData>);

pub type RenderSurface = GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, Option<OutputPresentationFeedback>>;

pub type GbmDrmCompositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmDevice<DrmDeviceFd>,
    Option<OutputPresentationFeedback>,
    DrmDeviceFd,
>;

struct SurfaceData {
    dh: DisplayHandle,
    device_id: DrmNode,
    render_node: Option<DrmNode>,
    output: Output,
    global: Option<GlobalId>,
    drm_output: DrmOutput<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        Option<OutputPresentationFeedback>,
        DrmDeviceFd,
    >,
    disable_direct_scanout: bool,
    #[cfg(feature = "debug")]
    fps: fps_ticker::Fps,
    #[cfg(feature = "debug")]
    fps_element: Option<FpsElement<MultiTexture>>,
    dmabuf_feedback: Option<SurfaceDmabufFeedback>,
    last_presentation_time: Option<Time<Monotonic>>,
    vblank_throttle_timer: Option<RegistrationToken>,
}

impl Drop for SurfaceData {
    fn drop(&mut self) {
        self.output.leave_all();
        if let Some(global) = self.global.take() {
            self.dh.remove_global::<AnvilState<UdevData>>(global);
        }
    }
}

struct BackendData {
    surfaces: HashMap<crtc::Handle, SurfaceData>,
    non_desktop_connectors: Vec<(connector::Handle, crtc::Handle)>,
    leasing_global: Option<DrmLeaseState>,
    active_leases: Vec<DrmLease>,
    drm_output_manager: DrmOutputManager<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        Option<OutputPresentationFeedback>,
        DrmDeviceFd,
    >,
    drm_scanner: DrmScanner,
    render_node: Option<DrmNode>,
    registration_token: RegistrationToken,
}

#[derive(Debug, thiserror::Error)]
enum DeviceAddError {
    #[error("Failed to open device using libseat: {0}")]
    DeviceOpen(libseat::Error),
    #[error("Failed to initialize drm device: {0}")]
    DrmDevice(DrmError),
    #[error("Failed to initialize gbm device: {0}")]
    GbmDevice(std::io::Error),
    #[error("Failed to access drm node: {0}")]
    DrmNode(CreateDrmNodeError),
    #[error("Failed to add device to GpuManager: {0}")]
    AddNode(egl::Error),
    #[error("The device has no render node")]
    NoRenderNode,
    #[error("Primary GPU is missing")]
    PrimaryGpuMissing,
}

fn get_surface_dmabuf_feedback(
    primary_gpu: DrmNode,
    render_node: Option<DrmNode>,
    scanout_node: DrmNode,
    gpus: &mut GpuManager<GbmGlesBackend<GlesRenderer, DrmDeviceFd>>,
    surface: &DrmSurface,
) -> Option<SurfaceDmabufFeedback> {
    let primary_formats = gpus.single_renderer(&primary_gpu).ok()?.dmabuf_formats();
    let render_formats = if let Some(render_node) = render_node {
        gpus.single_renderer(&render_node).ok()?.dmabuf_formats()
    } else {
        FormatSet::default()
    };

    let all_render_formats = primary_formats
        .iter()
        .chain(render_formats.iter())
        .copied()
        .collect::<FormatSet>();

    let planes = surface.planes().clone();

    // We limit the scan-out tranche to formats we can also render from
    // so that there is always a fallback render path available in case
    // the supplied buffer can not be scanned out directly
    let planes_formats = surface
        .plane_info()
        .formats
        .iter()
        .copied()
        .chain(planes.overlay.into_iter().flat_map(|p| p.formats))
        .collect::<FormatSet>()
        .intersection(&all_render_formats)
        .copied()
        .collect::<FormatSet>();

    let builder = DmabufFeedbackBuilder::new(primary_gpu.dev_id(), primary_formats);
    let render_feedback = if let Some(render_node) = render_node {
        builder
            .clone()
            .add_preference_tranche(render_node.dev_id(), None, render_formats.clone())
            .build()
            .unwrap()
    } else {
        builder.clone().build().unwrap()
    };

    let scanout_feedback = builder
        .add_preference_tranche(
            surface.device_fd().dev_id().unwrap(),
            Some(zwp_linux_dmabuf_feedback_v1::TrancheFlags::Scanout),
            planes_formats,
        )
        .add_preference_tranche(scanout_node.dev_id(), None, render_formats)
        .build()
        .unwrap();

    Some(SurfaceDmabufFeedback {
        render_feedback,
        scanout_feedback,
    })
}

impl AnvilState<UdevData> {
    fn device_added(&mut self, node: DrmNode, path: &Path) -> Result<(), DeviceAddError> {
        // Try to open the device
        let fd = self
            .backend_data
            .session
            .open(
                path,
                OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
            )
            .map_err(DeviceAddError::DeviceOpen)?;

        let fd = DrmDeviceFd::new(DeviceFd::from(fd));

        let (drm, notifier) = DrmDevice::new(fd.clone(), true).map_err(DeviceAddError::DrmDevice)?;
        let gbm = GbmDevice::new(fd).map_err(DeviceAddError::GbmDevice)?;

        let registration_token = self
            .handle
            .insert_source(
                notifier,
                move |event, metadata, data: &mut AnvilState<_>| match event {
                    DrmEvent::VBlank(crtc) => {
                        profiling::scope!("vblank", &format!("{crtc:?}"));
                        data.frame_finish(node, crtc, metadata);
                    }
                    DrmEvent::Error(error) => {
                        error!("{:?}", error);
                    }
                },
            )
            .unwrap();

        let mut try_initialize_gpu = || {
            let display = unsafe { EGLDisplay::new(gbm.clone()).map_err(DeviceAddError::AddNode)? };
            let egl_device = EGLDevice::device_for_display(&display).map_err(DeviceAddError::AddNode)?;

            if egl_device.is_software() {
                return Err(DeviceAddError::NoRenderNode);
            }

            let render_node = egl_device.try_get_render_node().ok().flatten().unwrap_or(node);
            info!(?node, ?render_node, "Added DRM node to GpuManager");
            self.backend_data
                .gpus
                .as_mut()
                .add_node(render_node, gbm.clone())
                .map_err(DeviceAddError::AddNode)?;

            std::result::Result::<DrmNode, DeviceAddError>::Ok(render_node)
        };

        let render_node = try_initialize_gpu()
            .inspect_err(|err| {
                warn!(?err, "failed to initialize gpu");
            })
            .ok();

        let allocator = render_node
            .is_some()
            .then(|| GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT))
            .or_else(|| {
                info!(
                    "No render node for {:?}; falling back to primary GPU's allocator",
                    node
                );
                self.backend_data
                    .backends
                    .get(&self.backend_data.primary_gpu)
                    .or_else(|| {
                        self.backend_data
                            .backends
                            .values()
                            .find(|backend| backend.render_node == Some(self.backend_data.primary_gpu))
                    })
                    .map(|backend| backend.drm_output_manager.allocator().clone())
            })
            .ok_or(DeviceAddError::PrimaryGpuMissing)?;

        let framebuffer_exporter = GbmFramebufferExporter::new(gbm.clone(), render_node.into());

        let color_formats = if std::env::var("ANVIL_DISABLE_10BIT").is_ok() {
            SUPPORTED_FORMATS_8BIT_ONLY
        } else {
            SUPPORTED_FORMATS
        };
        let mut renderer = self
            .backend_data
            .gpus
            .single_renderer(&render_node.unwrap_or(self.backend_data.primary_gpu))
            .unwrap();
        let render_formats = renderer
            .as_mut()
            .egl_context()
            .dmabuf_render_formats()
            .iter()
            .filter(|format| render_node.is_some() || format.modifier == Modifier::Linear)
            .copied()
            .collect::<FormatSet>();

        let drm_output_manager = DrmOutputManager::new(
            drm,
            allocator,
            framebuffer_exporter,
            Some(gbm),
            color_formats.iter().copied(),
            render_formats,
        );

        self.backend_data.backends.insert(
            node,
            BackendData {
                registration_token,
                drm_output_manager,
                drm_scanner: DrmScanner::new(),
                non_desktop_connectors: Vec::new(),
                render_node,
                surfaces: HashMap::new(),
                leasing_global: DrmLeaseState::new::<AnvilState<UdevData>>(&self.display_handle, &node)
                    .inspect_err(|err| {
                        warn!(?err, "Failed to initialize drm lease global for: {}", node);
                    })
                    .ok(),
                active_leases: Vec::new(),
            },
        );

        self.device_changed(node);

        Ok(())
    }

    fn connector_connected(&mut self, node: DrmNode, connector: connector::Info, crtc: crtc::Handle) {
        let device = if let Some(device) = self.backend_data.backends.get_mut(&node) {
            device
        } else {
            return;
        };

        let render_node = device.render_node.unwrap_or(self.backend_data.primary_gpu);
        let mut renderer = self.backend_data.gpus.single_renderer(&render_node).unwrap();

        let output_name = format!("{}-{}", connector.interface().as_str(), connector.interface_id());
        info!(?crtc, "Trying to setup connector {}", output_name,);

        let drm_device = device.drm_output_manager.device();

        let non_desktop = drm_device
            .get_properties(connector.handle())
            .ok()
            .and_then(|props| {
                let (info, value) = props
                    .into_iter()
                    .filter_map(|(handle, value)| {
                        let info = drm_device.get_property(handle).ok()?;

                        Some((info, value))
                    })
                    .find(|(info, _)| info.name().to_str() == Ok("non-desktop"))?;

                info.value_type().convert_value(value).as_boolean()
            })
            .unwrap_or(false);

        let display_info = display_info::for_connector(drm_device, connector.handle());

        let make = display_info
            .as_ref()
            .and_then(|info| info.make())
            .unwrap_or_else(|| "Unknown".into());

        let model = display_info
            .as_ref()
            .and_then(|info| info.model())
            .unwrap_or_else(|| "Unknown".into());

        let serial_number = display_info
            .as_ref()
            .and_then(|info| info.serial())
            .unwrap_or_else(|| "Unknown".into());

        if non_desktop {
            info!("Connector {} is non-desktop, setting up for leasing", output_name);
            device.non_desktop_connectors.push((connector.handle(), crtc));
            if let Some(lease_state) = device.leasing_global.as_mut() {
                lease_state.add_connector::<AnvilState<UdevData>>(
                    connector.handle(),
                    output_name,
                    format!("{make} {model}"),
                );
            }
        } else {
            let mode_id = connector
                .modes()
                .iter()
                .position(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
                .unwrap_or(0);

            let drm_mode = connector.modes()[mode_id];
            let wl_mode = WlMode::from(drm_mode);

            let (phys_w, phys_h) = connector.size().unwrap_or((0, 0));
            let output = Output::new(
                output_name,
                PhysicalProperties {
                    size: (phys_w as i32, phys_h as i32).into(),
                    subpixel: connector.subpixel().into(),
                    make,
                    model,
                    serial_number,
                },
            );
            let global = output.create_global::<AnvilState<UdevData>>(&self.display_handle);

            let x = self
                .space
                .outputs()
                .fold(0, |acc, o| acc + self.space.output_geometry(o).unwrap().size.w);
            let position = (x, 0).into();

            output.set_preferred(wl_mode);
            output.change_current_state(Some(wl_mode), None, None, Some(position));
            self.space.map_output(&output, position);

            output.user_data().insert_if_missing(|| UdevOutputId {
                crtc,
                device_id: node,
            });

            // Bootstrap an OutputState for this output. Workspace 1 is created
            // automatically by `OutputState::new`. If this is the first output,
            // also set it as focused.
            let output_state = crate::shell::output_state::OutputState::new(output.clone());
            let output_state_id = output_state.id;
            self.outputs.insert(output_state_id, output_state);
            if self.focused_output.is_none() {
                self.focused_output = Some(output_state_id);
            }

            #[cfg(feature = "debug")]
            let fps_element = self.backend_data.fps_texture.clone().map(FpsElement::new);

            let driver = match drm_device.get_driver() {
                Ok(driver) => driver,
                Err(err) => {
                    warn!("Failed to query drm driver: {}", err);
                    return;
                }
            };

            let mut planes = match drm_device.planes(&crtc) {
                Ok(planes) => planes,
                Err(err) => {
                    warn!("Failed to query crtc planes: {}", err);
                    return;
                }
            };

            // Using an overlay plane on a nvidia card breaks
            if driver.name().to_string_lossy().to_lowercase().contains("nvidia")
                || driver
                    .description()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("nvidia")
            {
                planes.overlay = vec![];
            }

            let drm_output = match device
                .drm_output_manager
                .lock()
                .initialize_output::<_, OutputRenderElements<UdevRenderer<'_>, WindowRenderElement<UdevRenderer<'_>>>>(
                    crtc,
                    drm_mode,
                    &[connector.handle()],
                    &output,
                    Some(planes),
                    &mut renderer,
                    &DrmOutputRenderElements::default(),
                ) {
                Ok(drm_output) => drm_output,
                Err(err) => {
                    warn!("Failed to initialize drm output: {}", err);
                    return;
                }
            };

            let disable_direct_scanout = std::env::var("ANVIL_DISABLE_DIRECT_SCANOUT").is_ok();

            let dmabuf_feedback = drm_output.with_compositor(|compositor| {
                compositor.set_debug_flags(self.backend_data.debug_flags);

                get_surface_dmabuf_feedback(
                    self.backend_data.primary_gpu,
                    device.render_node,
                    node,
                    &mut self.backend_data.gpus,
                    compositor.surface(),
                )
            });

            let surface = SurfaceData {
                dh: self.display_handle.clone(),
                device_id: node,
                render_node: device.render_node,
                output,
                global: Some(global),
                drm_output,
                disable_direct_scanout,
                #[cfg(feature = "debug")]
                fps: fps_ticker::Fps::default(),
                #[cfg(feature = "debug")]
                fps_element,
                dmabuf_feedback,
                last_presentation_time: None,
                vblank_throttle_timer: None,
            };

            device.surfaces.insert(crtc, surface);

            // Advertise this output to wlr-output-management clients.
            // Must happen after `space.map_output` (so position/mode are
            // already set) and after the surface is inserted (so the output
            // is fully part of the compositor's known set).
            let head_output = device.surfaces[&crtc].output.clone();
            crate::protocols::output_management::add_head::<AnvilState<UdevData>>(
                &mut self.output_management_manager_state,
                &self.display_handle,
                &head_output,
            );

            // kick-off rendering
            self.handle.insert_idle(move |state| {
                state.render_surface(node, crtc, state.clock.now());
            });
        }
    }

    fn connector_disconnected(&mut self, node: DrmNode, connector: connector::Info, crtc: crtc::Handle) {
        let device = if let Some(device) = self.backend_data.backends.get_mut(&node) {
            device
        } else {
            return;
        };

        if let Some(pos) = device
            .non_desktop_connectors
            .iter()
            .position(|(handle, _)| *handle == connector.handle())
        {
            let _ = device.non_desktop_connectors.remove(pos);
            if let Some(leasing_state) = device.leasing_global.as_mut() {
                leasing_state.withdraw_connector(connector.handle());
            }
        } else if let Some(surface) = device.surfaces.remove(&crtc) {
            // Retire the head from wlr-output-management before unmapping,
            // mirroring the order used in `connector_connected` (add_head ran
            // after `space.map_output`, so remove_head runs before
            // `space.unmap_output`).
            crate::protocols::output_management::remove_head(
                &mut self.output_management_manager_state,
                &surface.output,
            );
            self.space.unmap_output(&surface.output);
            self.space.refresh();

            // Find the OutputId tied to this Output and remove the OutputState.
            let target_output = surface.output.clone();
            let output_id = self
                .outputs
                .iter()
                .find(|(_, os)| os.output == target_output)
                .map(|(id, _)| *id);
            if let Some(id) = output_id {
                self.outputs.remove(&id);
                if self.focused_output == Some(id) {
                    self.focused_output = self.outputs.keys().next().copied();
                }
            }
        }

        let render_node = device.render_node.unwrap_or(self.backend_data.primary_gpu);
        let mut renderer = self.backend_data.gpus.single_renderer(&render_node).unwrap();
        let _ = device.drm_output_manager.lock().try_to_restore_modifiers::<_, OutputRenderElements<
            UdevRenderer<'_>,
            WindowRenderElement<UdevRenderer<'_>>,
        >>(
            &mut renderer,
            // FIXME: For a flicker free operation we should return the actual elements for this output..
            // Instead we just use black to "simulate" a modeset :)
            &DrmOutputRenderElements::default(),
        );
    }

    fn device_changed(&mut self, node: DrmNode) {
        let device = if let Some(device) = self.backend_data.backends.get_mut(&node) {
            device
        } else {
            return;
        };

        let scan_result = match device
            .drm_scanner
            .scan_connectors(device.drm_output_manager.device())
        {
            Ok(scan_result) => scan_result,
            Err(err) => {
                tracing::warn!(?err, "Failed to scan connectors");
                return;
            }
        };

        for event in scan_result {
            match event {
                DrmScanEvent::Connected {
                    connector,
                    crtc: Some(crtc),
                } => {
                    self.connector_connected(node, connector, crtc);
                }
                DrmScanEvent::Disconnected {
                    connector,
                    crtc: Some(crtc),
                } => {
                    self.connector_disconnected(node, connector, crtc);
                }
                // The connector's mode list changed while it stayed connected (e.g. EDID
                // arrived after the initial probe returned empty/fallback modes). Compositors
                // should re-evaluate the output's mode selection and recreate the surface here.
                DrmScanEvent::Changed { .. } => {}
                _ => {}
            }
        }

        // fixup window coordinates
        crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
    }

    fn device_removed(&mut self, node: DrmNode) {
        let device = if let Some(device) = self.backend_data.backends.get_mut(&node) {
            device
        } else {
            return;
        };

        let crtcs: Vec<_> = device
            .drm_scanner
            .crtcs()
            .map(|(info, crtc)| (info.clone(), crtc))
            .collect();

        for (connector, crtc) in crtcs {
            self.connector_disconnected(node, connector, crtc);
        }

        debug!("Surfaces dropped");

        // drop the backends on this side
        if let Some(mut backend_data) = self.backend_data.backends.remove(&node) {
            if let Some(mut leasing_global) = backend_data.leasing_global.take() {
                leasing_global.disable_global::<AnvilState<UdevData>>();
            }

            if let Some(render_node) = backend_data.render_node {
                self.backend_data.gpus.as_mut().remove_node(&render_node);
            }

            self.handle.remove(backend_data.registration_token);

            debug!("Dropping device");
        }

        crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
    }

    fn frame_finish(&mut self, dev_id: DrmNode, crtc: crtc::Handle, metadata: &mut Option<DrmEventMetadata>) {
        profiling::scope!("frame_finish", &format!("{crtc:?}"));

        let device_backend = match self.backend_data.backends.get_mut(&dev_id) {
            Some(backend) => backend,
            None => {
                error!("Trying to finish frame on non-existent backend {}", dev_id);
                return;
            }
        };

        let surface = match device_backend.surfaces.get_mut(&crtc) {
            Some(surface) => surface,
            None => {
                error!("Trying to finish frame on non-existent crtc {:?}", crtc);
                return;
            }
        };

        if let Some(timer_token) = surface.vblank_throttle_timer.take() {
            self.handle.remove(timer_token);
        }

        let output = if let Some(output) = self.space.outputs().find(|o| {
            o.user_data().get::<UdevOutputId>()
                == Some(&UdevOutputId {
                    device_id: surface.device_id,
                    crtc,
                })
        }) {
            output.clone()
        } else {
            // somehow we got called with an invalid output
            return;
        };

        let Some(frame_duration) = output
            .current_mode()
            .map(|mode| Duration::from_secs_f64(1_000f64 / mode.refresh as f64))
        else {
            return;
        };

        let tp = metadata.as_ref().and_then(|metadata| match metadata.time {
            smithay::backend::drm::DrmEventTime::Monotonic(tp) => tp.is_zero().not().then_some(tp),
            smithay::backend::drm::DrmEventTime::Realtime(_) => None,
        });

        let seq = metadata.as_ref().map(|metadata| metadata.sequence).unwrap_or(0);

        let (clock, flags) = if let Some(tp) = tp {
            (
                tp.into(),
                wp_presentation_feedback::Kind::Vsync
                    | wp_presentation_feedback::Kind::HwClock
                    | wp_presentation_feedback::Kind::HwCompletion,
            )
        } else {
            (self.clock.now(), wp_presentation_feedback::Kind::Vsync)
        };

        let vblank_remaining_time = surface.last_presentation_time.map(|last_presentation_time| {
            frame_duration.saturating_sub(Time::elapsed(&last_presentation_time, clock))
        });

        if let Some(vblank_remaining_time) = vblank_remaining_time {
            if vblank_remaining_time > frame_duration / 2 {
                static WARN_ONCE: Once = Once::new();
                WARN_ONCE.call_once(|| {
                    warn!("display running faster than expected, throttling vblanks and disabling HwClock")
                });
                let throttled_time = tp
                    .map(|tp| tp.saturating_add(vblank_remaining_time))
                    .unwrap_or(Duration::ZERO);
                let throttled_metadata = DrmEventMetadata {
                    sequence: seq,
                    time: DrmEventTime::Monotonic(throttled_time),
                };
                let timer_token = self
                    .handle
                    .insert_source(Timer::from_duration(vblank_remaining_time), move |_, _, data| {
                        data.frame_finish(dev_id, crtc, &mut Some(throttled_metadata));
                        TimeoutAction::Drop
                    })
                    .expect("failed to register vblank throttle timer");
                surface.vblank_throttle_timer = Some(timer_token);
                return;
            }
        }
        surface.last_presentation_time = Some(clock);

        let submit_result = surface
            .drm_output
            .frame_submitted()
            .map_err(Into::<SwapBuffersError>::into);

        let schedule_render = match submit_result {
            Ok(user_data) => {
                if let Some(mut feedback) = user_data.flatten() {
                    feedback.presented(clock, Refresh::fixed(frame_duration), seq as u64, flags);
                }

                true
            }
            Err(err) => {
                warn!("Error during rendering: {:?}", err);
                match err {
                    SwapBuffersError::AlreadySwapped => true,
                    // If the device has been deactivated do not reschedule, this will be done
                    // by session resume
                    SwapBuffersError::TemporaryFailure(err)
                        if matches!(err.downcast_ref::<DrmError>(), Some(&DrmError::DeviceInactive)) =>
                    {
                        false
                    }
                    SwapBuffersError::TemporaryFailure(err) => matches!(
                        err.downcast_ref::<DrmError>(),
                        Some(DrmError::Access(DrmAccessError {
                            source,
                            ..
                        })) if source.kind() == io::ErrorKind::PermissionDenied
                    ),
                    SwapBuffersError::ContextLost(err) => panic!("Rendering loop lost: {err}"),
                }
            }
        };

        if schedule_render {
            let next_frame_target = clock + frame_duration;

            // What are we trying to solve by introducing a delay here:
            //
            // Basically it is all about latency of client provided buffers.
            // A client driven by frame callbacks will wait for a frame callback
            // to repaint and submit a new buffer. As we send frame callbacks
            // as part of the repaint in the compositor the latency would always
            // be approx. 2 frames. By introducing a delay before we repaint in
            // the compositor we can reduce the latency to approx. 1 frame + the
            // remaining duration from the repaint to the next VBlank.
            //
            // With the delay it is also possible to further reduce latency if
            // the client is driven by presentation feedback. As the presentation
            // feedback is directly sent after a VBlank the client can submit a
            // new buffer during the repaint delay that can hit the very next
            // VBlank, thus reducing the potential latency to below one frame.
            //
            // Choosing a good delay is a topic on its own so we just implement
            // a simple strategy here. We just split the duration between two
            // VBlanks into two steps, one for the client repaint and one for the
            // compositor repaint. Theoretically the repaint in the compositor should
            // be faster so we give the client a bit more time to repaint. On a typical
            // modern system the repaint in the compositor should not take more than 2ms
            // so this should be safe for refresh rates up to at least 120 Hz. For 120 Hz
            // this results in approx. 3.33ms time for repainting in the compositor.
            // A too big delay could result in missing the next VBlank in the compositor.
            //
            // A more complete solution could work on a sliding window analyzing past repaints
            // and do some prediction for the next repaint.
            let repaint_delay = Duration::from_secs_f64(frame_duration.as_secs_f64() * 0.6f64);

            let timer = if surface
                .render_node
                .map(|render_node| render_node != self.backend_data.primary_gpu)
                .unwrap_or(true)
            {
                // However, if we need to do a copy, that might not be enough.
                // (And without actual comparison to previous frames we cannot really know.)
                // So lets ignore that in those cases to avoid thrashing performance.
                trace!("scheduling repaint timer immediately on {:?}", crtc);
                Timer::immediate()
            } else {
                trace!(
                    "scheduling repaint timer with delay {:?} on {:?}",
                    repaint_delay, crtc
                );
                Timer::from_duration(repaint_delay)
            };

            self.handle
                .insert_source(timer, move |_, _, data| {
                    data.render(dev_id, Some(crtc), next_frame_target);
                    TimeoutAction::Drop
                })
                .expect("failed to schedule frame timer");
        }
    }

    // If crtc is `Some()`, render it, else render all crtcs
    fn render(&mut self, node: DrmNode, crtc: Option<crtc::Handle>, frame_target: Time<Monotonic>) {
        let device_backend = match self.backend_data.backends.get_mut(&node) {
            Some(backend) => backend,
            None => {
                error!("Trying to render on non-existent backend {}", node);
                return;
            }
        };

        if let Some(crtc) = crtc {
            self.render_surface(node, crtc, frame_target);
        } else {
            let crtcs: Vec<_> = device_backend.surfaces.keys().copied().collect();
            for crtc in crtcs {
                self.render_surface(node, crtc, frame_target);
            }
        };
    }

    fn render_surface(&mut self, node: DrmNode, crtc: crtc::Handle, frame_target: Time<Monotonic>) {
        profiling::scope!("render_surface", &format!("{crtc:?}"));

        let output = if let Some(output) = self.space.outputs().find(|o| {
            o.user_data().get::<UdevOutputId>()
                == Some(&UdevOutputId {
                    device_id: node,
                    crtc,
                })
        }) {
            output.clone()
        } else {
            // somehow we got called with an invalid output
            return;
        };

        // Advance all animations before assembling the per-frame element list.
        // Single Instant is shared across this frame's tick_animations call so
        // workspace-level and per-window AnimatedValues stay in lockstep.
        let frame_now = std::time::Instant::now();
        self.tick_animations(frame_now);
        // Notify all in-process extensions that a new frame is starting.
        // Disjoint-field borrow: only touches `self.extension_registry`.
        self.extension_registry.pre_frame_all(frame_now);

        self.pre_repaint(&output, frame_target);

        let device = if let Some(device) = self.backend_data.backends.get_mut(&node) {
            device
        } else {
            return;
        };

        let surface = if let Some(surface) = device.surfaces.get_mut(&crtc) {
            surface
        } else {
            return;
        };

        let start = Instant::now();

        // TODO get scale from the rendersurface when supporting HiDPI
        let frame = self
            .backend_data
            .pointer_image
            .get_image(1 /*scale*/, self.clock.now().into());

        let primary_gpu = self.backend_data.primary_gpu;
        let render_node = surface.render_node.unwrap_or(primary_gpu);
        let mut renderer = if primary_gpu == render_node {
            self.backend_data.gpus.single_renderer(&render_node)
        } else {
            let format = surface.drm_output.format();
            self.backend_data
                .gpus
                .renderer(&primary_gpu, &render_node, format)
        }
        .unwrap();

        let pointer_images = &mut self.backend_data.pointer_images;
        let pointer_image = pointer_images
            .iter()
            .find_map(|(image, texture)| {
                if image == &frame {
                    Some(texture.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                let buffer = MemoryRenderBuffer::from_slice(
                    &frame.pixels_rgba,
                    Fourcc::Argb8888,
                    (frame.width as i32, frame.height as i32),
                    1,
                    Transform::Normal,
                    None,
                );
                pointer_images.push((frame, buffer.clone()));
                buffer
            });

        // Look up the active workspace for this output. None when the
        // output hasn't been bootstrapped into `self.outputs` yet —
        // render.rs falls back to the smithay path in that case so the
        // screen still produces pixels during the first frames.
        let active_workspace_for_render: Option<&crate::shell::workspace::Workspace> = self
            .outputs
            .values()
            .find(|os| os.output == output)
            .map(|os| os.active());

        let result = render_surface(
            surface,
            &mut renderer,
            &self.space,
            active_workspace_for_render,
            &output,
            self.pointer.current_location(),
            &pointer_image,
            &mut self.backend_data.pointer_element,
            &self.dnd_icon,
            &mut self.cursor_status,
            self.show_window_preview,
            &mut self.pending_screencopy,
            Duration::from(frame_target),
        );
        // Whether any animation across any workspace is still in flight.
        // We capture this before the `match` so the borrow on `self` doesn't
        // overlap with the post_repaint mutable call below.
        let animations_in_flight = self.any_animating();
        let reschedule = match result {
            Ok((has_rendered, states)) => {
                let dmabuf_feedback = surface.dmabuf_feedback.clone();
                self.post_repaint(&output, frame_target, dmabuf_feedback, &states);
                // Re-arm if either nothing was damaged this frame OR an
                // animation is still running. Without this, an idle
                // compositor that's mid-animation would skip frames and
                // freeze the animation visually until external damage
                // arrives. The `is_animating` cutoff above naturally settles
                // once all AnimatedValues finish.
                !has_rendered || animations_in_flight
            }
            Err(err) => {
                warn!("Error during rendering: {:#?}", err);
                match err {
                    SwapBuffersError::AlreadySwapped => false,
                    SwapBuffersError::TemporaryFailure(err) => match err.downcast_ref::<DrmError>() {
                        Some(DrmError::DeviceInactive) => true,
                        Some(DrmError::Access(DrmAccessError { source, .. })) => {
                            source.kind() == io::ErrorKind::PermissionDenied
                        }
                        _ => false,
                    },
                    SwapBuffersError::ContextLost(err) => match err.downcast_ref::<DrmError>() {
                        Some(DrmError::TestFailed(_)) => {
                            // reset the complete state, disabling all connectors and planes in case we hit a test failed
                            // most likely we hit this after a tty switch when a foreign master changed CRTC <-> connector bindings
                            // and we run in a mismatch
                            device
                                .drm_output_manager
                                .device_mut()
                                .reset_state()
                                .expect("failed to reset drm device");
                            true
                        }
                        _ => panic!("Rendering loop lost: {err}"),
                    },
                }
            }
        };

        if reschedule {
            let output_refresh = match output.current_mode() {
                Some(mode) => mode.refresh,
                None => return,
            };

            // If reschedule is true we either hit a temporary failure or more likely rendering
            // did not cause any damage on the output. In this case we just re-schedule a repaint
            // after approx. one frame to re-test for damage.
            let next_frame_target = frame_target + Duration::from_millis(1_000_000 / output_refresh as u64);
            let reschedule_timeout =
                Duration::from(next_frame_target).saturating_sub(self.clock.now().into());
            trace!(
                "reschedule repaint timer with delay {:?} on {:?}",
                reschedule_timeout, crtc,
            );
            let timer = Timer::from_duration(reschedule_timeout);
            self.handle
                .insert_source(timer, move |_, _, data| {
                    data.render(node, Some(crtc), next_frame_target);
                    TimeoutAction::Drop
                })
                .expect("failed to schedule frame timer");
        } else {
            let elapsed = start.elapsed();
            tracing::trace!(?elapsed, "rendered surface");
        }

        // Notify all in-process extensions that the frame has finished.
        // Mirrors the `pre_frame_all` call above; disjoint-field borrow on
        // `self.extension_registry` only.
        self.extension_registry.post_frame_all(frame_now);

        profiling::finish_frame!();
    }
}

#[allow(clippy::too_many_arguments)]
#[profiling::function]
fn render_surface<'a>(
    surface: &'a mut SurfaceData,
    renderer: &mut UdevRenderer<'a>,
    space: &Space<WindowElement>,
    workspace: Option<&crate::shell::workspace::Workspace>,
    output: &Output,
    pointer_location: Point<f64, Logical>,
    pointer_image: &MemoryRenderBuffer,
    pointer_element: &mut PointerElement,
    dnd_icon: &Option<DndIcon>,
    cursor_status: &mut CursorImageStatus,
    show_window_preview: bool,
    pending_screencopy: &mut Vec<crate::screencopy::PendingScreencopy>,
    presented_at: Duration,
) -> Result<(bool, RenderElementStates), SwapBuffersError> {
    let output_geometry = space.output_geometry(output).unwrap();
    let scale = Scale::from(output.current_scale().fractional_scale());

    let mut custom_elements: Vec<CustomRenderElements<_>> = Vec::new();

    if output_geometry.to_f64().contains(pointer_location) {
        let cursor_hotspot = if let CursorImageStatus::Surface(surface) = cursor_status {
            compositor::with_states(surface, |states| {
                states
                    .data_map
                    .get::<Mutex<CursorImageAttributes>>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .hotspot
            })
        } else {
            (0, 0).into()
        };
        let cursor_pos = pointer_location - output_geometry.loc.to_f64();

        // set cursor
        pointer_element.set_buffer(pointer_image.clone());

        // draw the cursor as relevant
        {
            // reset the cursor if the surface is no longer alive
            let mut reset = false;
            if let CursorImageStatus::Surface(ref surface) = *cursor_status {
                reset = !surface.alive();
            }
            if reset {
                *cursor_status = CursorImageStatus::default_named();
            }

            pointer_element.set_status(cursor_status.clone());
        }

        custom_elements.extend(
            pointer_element.render_elements(
                renderer,
                (cursor_pos - cursor_hotspot.to_f64())
                    .to_physical(scale)
                    .to_i32_round(),
                scale,
                1.0,
            ),
        );

        // draw the dnd icon if applicable
        {
            if let Some(icon) = dnd_icon.as_ref() {
                let dnd_icon_pos = (cursor_pos + icon.offset.to_f64())
                    .to_physical(scale)
                    .to_i32_round();
                if icon.surface.alive() {
                    custom_elements.extend(AsRenderElements::<UdevRenderer<'a>>::render_elements(
                        &SurfaceTree::from_surface(&icon.surface),
                        renderer,
                        dnd_icon_pos,
                        scale,
                        1.0,
                    ));
                }
            }
        }
    }

    #[cfg(feature = "debug")]
    if let Some(element) = surface.fps_element.as_mut() {
        element.update_fps(surface.fps.avg().round() as u32);
        surface.fps.tick();
        custom_elements.push(CustomRenderElements::Fps(element.clone()));
    }

    let (elements, clear_color) = output_elements(
        output,
        space,
        workspace,
        custom_elements,
        renderer,
        show_window_preview,
    );

    let frame_mode = if surface.disable_direct_scanout {
        FrameFlags::empty()
    } else {
        FrameFlags::DEFAULT
    };
    let _wants_tearing = output_wants_tearing(space, output);
    if _wants_tearing {
        tracing::trace!(output_name = %output.name(), "tearing requested for output");
    }
    // TODO(tearing-async-pageflip): once smithay's FrameFlags has ALLOW_TEARING
    // and DrmSurface::page_flip supports DRM_MODE_PAGE_FLIP_ASYNC, gate the bit
    // on `_wants_tearing`. See docs/research/phase-2-tearing-control.md.
    let (rendered, states) = surface
        .drm_output
        .render_frame(renderer, &elements, clear_color, frame_mode)
        .map(|render_frame_result| {
            #[cfg(feature = "renderer_sync")]
            if let PrimaryPlaneElement::Swapchain(element) = render_frame_result.primary_element {
                element.sync.wait();
            }
            (!render_frame_result.is_empty, render_frame_result.states)
        })
        .map_err(|err| match err {
            smithay::backend::drm::compositor::RenderFrameError::PrepareFrame(err) => {
                SwapBuffersError::from(err)
            }
            smithay::backend::drm::compositor::RenderFrameError::RenderFrame(
                OutputDamageTrackerError::Rendering(err),
            ) => SwapBuffersError::from(err),
            _ => unreachable!(),
        })?;

    update_primary_scanout_output(space, output, dnd_icon, cursor_status, &states);

    // Service queued screencopy captures using the same element list we
    // just rendered to the real framebuffer. Mirrors the winit drain in
    // `winit.rs`; both back ends ultimately use a `GlesTexture` as the
    // offscreen target.
    crate::screencopy::drain_pending_for_output::<
        _,
        smithay::backend::renderer::gles::GlesTexture,
        _,
    >(
        pending_screencopy,
        output,
        renderer,
        &elements,
        presented_at,
    );

    if rendered {
        let output_presentation_feedback = take_presentation_feedback(output, space, &states);
        surface
            .drm_output
            .queue_frame(Some(output_presentation_feedback))
            .map_err(Into::<SwapBuffersError>::into)?;
    }

    Ok((rendered, states))
}

/// Returns `true` if any window mapped to `output` has its primary scanout on
/// `output` and is requesting `async` (tearing) presentation via the
/// `wp_tearing_control_v1` protocol.
///
/// This is the read side of the tearing-control protocol. The actual flip to
/// `DRM_MODE_PAGE_FLIP_ASYNC` is gated on smithay growing a
/// `FrameFlags::ALLOW_TEARING` bit; see the TODO at the call site in
/// `render_surface` and `docs/research/phase-2-tearing-control.md`.
///
/// Note: we deliberately do the primary-scanout check and the tearing-hint
/// read inside a single `with_states` callback. Calling
/// `crate::protocols::tearing_control::surface_wants_async_presentation`
/// directly would re-enter `with_states` on the same surface and deadlock on
/// the per-surface user-data lock.
fn output_wants_tearing(space: &Space<WindowElement>, output: &Output) -> bool {
    use smithay::desktop::utils::surface_primary_scanout_output;

    use crate::protocols::tearing_control::TearingControlSurfaceCachedState;

    space.elements().any(|window| {
        let Some(surface) = window.wl_surface() else {
            return false;
        };
        compositor::with_states(&surface, |states| {
            // Only count surfaces whose primary scanout is this output —
            // otherwise a tearing-hint surface on display A would force the
            // tearing log line on display B.
            if surface_primary_scanout_output(&surface, states).as_ref() != Some(output) {
                return false;
            }
            states
                .cached_state
                .get::<TearingControlSurfaceCachedState>()
                .current()
                .wants_async()
        })
    })
}
