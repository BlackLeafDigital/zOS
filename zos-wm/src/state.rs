#[cfg(feature = "xwayland")]
use std::os::unix::io::OwnedFd;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use tracing::{info, warn};

use smithay::{
    backend::{
        input::TabletToolDescriptor,
        renderer::element::{
            RenderElementStates, default_primary_scanout_output_compare, utils::select_dmabuf_feedback,
        },
    },
    delegate_compositor, delegate_data_control, delegate_data_device, delegate_fixes,
    delegate_fractional_scale, delegate_input_method_manager, delegate_keyboard_shortcuts_inhibit,
    delegate_layer_shell, delegate_output, delegate_pointer_constraints, delegate_pointer_gestures,
    delegate_presentation, delegate_primary_selection, delegate_relative_pointer, delegate_seat,
    delegate_security_context, delegate_shm, delegate_tablet_manager, delegate_text_input_manager,
    delegate_viewporter, delegate_virtual_keyboard_manager, delegate_xdg_activation, delegate_xdg_decoration,
    delegate_xdg_shell,
    desktop::{
        PopupKind, PopupManager, Space,
        space::SpaceElement,
        utils::{
            OutputPresentationFeedback, surface_presentation_feedback_flags_from_states,
            surface_primary_scanout_output, update_surface_primary_scanout_output,
            with_surfaces_surface_tree,
        },
    },
    input::{
        Seat, SeatHandler, SeatState,
        dnd::{DnDGrab, DndGrabHandler, DndTarget, GrabType, Source},
        keyboard::{Keysym, LedState, XkbConfig},
        pointer::{CursorImageStatus, Focus, PointerHandle},
    },
    output::Output,
    reexports::{
        calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic},
        wayland_protocols::xdg::decoration::{
            self as xdg_decoration, zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        },
        wayland_server::{
            Client, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_output::WlOutput, wl_pointer::WlPointer, wl_surface::WlSurface},
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle, Serial, Time},
    wayland::{
        commit_timing::{CommitTimerBarrierStateUserData, CommitTimingManagerState},
        compositor::{CompositorClientState, CompositorHandler, CompositorState, get_parent, with_states},
        cursor_shape::CursorShapeManagerState,
        dmabuf::DmabufFeedback,
        fifo::{FifoBarrierCachedState, FifoManagerState},
        fixes::FixesState,
        foreign_toplevel_list::{ForeignToplevelListHandler, ForeignToplevelListState},
        fractional_scale::{FractionalScaleHandler, FractionalScaleManagerState, with_fractional_scale},
        idle_inhibit::{IdleInhibitHandler, IdleInhibitManagerState},
        idle_notify::{IdleNotifierHandler, IdleNotifierState},
        image_capture_source::{
            ImageCaptureSource, ImageCaptureSourceHandler, ImageCaptureSourceState,
            OutputCaptureSourceHandler, OutputCaptureSourceState,
        },
        image_copy_capture::{
            BufferConstraints, Frame, ImageCopyCaptureHandler, ImageCopyCaptureState, Session, SessionRef,
        },
        input_method::{InputMethodHandler, InputMethodManagerState, PopupSurface},
        keyboard_shortcuts_inhibit::{
            KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
        },
        output::{OutputHandler, OutputManagerState},
        pointer_constraints::{PointerConstraintsHandler, PointerConstraintsState, with_pointer_constraint},
        pointer_gestures::PointerGesturesState,
        pointer_warp::{PointerWarpHandler, PointerWarpManager},
        presentation::PresentationState,
        relative_pointer::RelativePointerManagerState,
        seat::WaylandFocus,
        session_lock::{
            LockSurface, LockSurfaceConfigure, SessionLockHandler, SessionLockManagerState, SessionLocker,
        },
        security_context::{
            SecurityContext, SecurityContextHandler, SecurityContextListenerSource, SecurityContextState,
        },
        selection::{
            SelectionHandler,
            data_device::{DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler, set_data_device_focus},
            primary_selection::{PrimarySelectionHandler, PrimarySelectionState, set_primary_focus},
            wlr_data_control::{DataControlHandler, DataControlState},
        },
        shell::{
            kde::decoration::{KdeDecorationHandler, KdeDecorationState},
            wlr_layer::WlrLayerShellState,
            xdg::{
                ToplevelSurface, XdgShellState,
                decoration::{XdgDecorationHandler, XdgDecorationState},
            },
        },
        shm::{ShmHandler, ShmState},
        single_pixel_buffer::SinglePixelBufferState,
        socket::ListeningSocketSource,
        tablet_manager::{TabletManagerState, TabletSeatHandler},
        text_input::TextInputManagerState,
        viewporter::ViewporterState,
        virtual_keyboard::VirtualKeyboardManagerState,
        xdg_activation::{
            XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData,
        },
        xdg_foreign::{XdgForeignHandler, XdgForeignState},
    },
};

#[cfg(feature = "xwayland")]
use crate::cursor::Cursor;
use crate::{
    focus::{KeyboardFocusTarget, PointerFocusTarget},
    protocols::gamma_control::GammaControlManagerState,
    protocols::output_management::{
        OutputConfigChange, OutputConfigError, OutputManagementHandler, OutputManagementManagerState,
    },
    protocols::tearing_control::TearingControlManagerState,
    screencopy::PendingScreencopy,
    shell::WindowElement,
};
#[cfg(feature = "xwayland")]
use smithay::{
    delegate_xwayland_keyboard_grab, delegate_xwayland_shell,
    utils::Size,
    wayland::selection::{SelectionSource, SelectionTarget},
    wayland::xwayland_keyboard_grab::{XWaylandKeyboardGrabHandler, XWaylandKeyboardGrabState},
    wayland::xwayland_shell,
    xwayland::{X11Wm, XWayland, XWaylandEvent},
};

#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    pub security_context: Option<SecurityContext>,
}
impl ClientData for ClientState {
    /// Notification that a client was initialized
    fn initialized(&self, _client_id: ClientId) {}
    /// Notification that a client is disconnected
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

#[derive(Debug)]
pub struct AnvilState<BackendData: Backend + 'static> {
    pub backend_data: BackendData,
    pub socket_name: Option<String>,
    pub display_handle: DisplayHandle,
    pub running: Arc<AtomicBool>,
    pub handle: LoopHandle<'static, AnvilState<BackendData>>,

    // desktop
    pub space: Space<WindowElement>,
    pub popups: PopupManager,

    // smithay state
    pub compositor_state: CompositorState,
    pub data_device_state: DataDeviceState,
    pub layer_shell_state: WlrLayerShellState,
    pub output_manager_state: OutputManagerState,
    pub primary_selection_state: PrimarySelectionState,
    pub data_control_state: DataControlState,
    pub seat_state: SeatState<AnvilState<BackendData>>,
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    pub shm_state: ShmState,
    pub viewporter_state: ViewporterState,
    pub xdg_activation_state: XdgActivationState,
    pub xdg_decoration_state: XdgDecorationState,
    pub kde_decoration_state: KdeDecorationState,
    pub xdg_shell_state: XdgShellState,
    pub presentation_state: PresentationState,
    pub fractional_scale_manager_state: FractionalScaleManagerState,
    pub xdg_foreign_state: XdgForeignState,
    pub foreign_toplevel_list_state: ForeignToplevelListState,
    #[cfg(feature = "xwayland")]
    pub xwayland_shell_state: xwayland_shell::XWaylandShellState,
    pub single_pixel_buffer_state: SinglePixelBufferState,
    pub fifo_manager_state: FifoManagerState,
    pub commit_timing_manager_state: CommitTimingManagerState,
    pub image_capture_source_state: ImageCaptureSourceState,
    pub output_capture_source_state: OutputCaptureSourceState,
    pub image_copy_capture_state: ImageCopyCaptureState,
    /// Queue of ext-image-copy-capture frames awaiting the next post-render.
    /// Drained by backends (`winit.rs`, `udev.rs`) via
    /// `crate::screencopy::drain_pending_for_output`.
    pub pending_screencopy: Vec<PendingScreencopy>,
    pub idle_inhibit_manager_state: IdleInhibitManagerState,
    pub idle_notifier_state: IdleNotifierState<AnvilState<BackendData>>,
    pub idle_inhibitors: HashSet<WlSurface>,
    pub tearing_control_manager_state: TearingControlManagerState,
    pub gamma_control_manager_state: GammaControlManagerState,
    pub output_management_manager_state: OutputManagementManagerState,
    pub session_lock_manager_state: SessionLockManagerState,
    pub is_locked: bool,

    pub dnd_icon: Option<DndIcon>,

    // input-related fields
    pub suppressed_keys: Vec<Keysym>,
    pub cursor_status: CursorImageStatus,
    pub seat_name: String,
    pub seat: Seat<AnvilState<BackendData>>,
    pub clock: Clock<Monotonic>,
    pub pointer: PointerHandle<AnvilState<BackendData>>,

    #[cfg(feature = "xwayland")]
    pub xwm: Option<X11Wm>,
    #[cfg(feature = "xwayland")]
    pub xdisplay: Option<u32>,

    #[cfg(feature = "debug")]
    pub renderdoc: Option<renderdoc::RenderDoc<renderdoc::V141>>,

    pub show_window_preview: bool,

    // Phase 3: per-output workspaces, focus tracking, parking lot.
    /// Per-output state holding workspaces. Keyed by `OutputId` for stable
    /// access during hotplug.
    pub outputs: HashMap<crate::shell::output_state::OutputId, crate::shell::output_state::OutputState>,
    /// Currently-focused output for keyboard / spawn placement.
    pub focused_output: Option<crate::shell::output_state::OutputId>,
    /// Recent (output, workspace) pairs for "go-back" navigation.
    pub workspace_history: Vec<(crate::shell::output_state::OutputId, crate::shell::WorkspaceId)>,
    /// Windows whose home output disconnected; restored when a matching
    /// output reappears (or on user rescue).
    pub parking_lot: Vec<crate::shell::WindowEntry>,
    /// Click-to-focus vs follows-mouse; controls focus-on-pointer-enter.
    pub focus_mode: FocusMode,

    // Phase 3 input dispatch: bind table + suppression sets.
    /// Bind table — `KeyCombo` → `Action`.
    pub bindings: HashMap<crate::binds::KeyCombo, crate::binds::Action>,
    /// Keycodes whose press already triggered a binding/grab. The matching
    /// release event is dropped so the client never sees a hanging release.
    pub suppressed_keycodes: HashSet<smithay::input::keyboard::Keycode>,
    /// Same idea but for mouse buttons (Linux input button codes).
    pub suppressed_buttons: HashSet<u32>,

    // Phase 4 animations: registry of named curves + per-property config.
    /// Drives window-open / window-close / fade / workspace-switch
    /// animations. Defaults sourced from `AnimationManager::default()`;
    /// TOML config parsing is deferred to a later task.
    pub animation_manager: crate::anim::AnimationManager,

    // Phase 4 effects: rounded-corners radius (in logical pixels).
    /// Default corner radius applied to windows by the rounded-corners
    /// pixel shader. 8.0 px matches the Catppuccin/zOS reference theme.
    /// The actual application of this radius into per-window
    /// `PixelShaderElement`s lives in the render path (see
    /// `crate::effects::rounded` and the `TODO(P4-render-integration)`
    /// in `crate::render`).
    pub corner_radius: f32,

    // Phase 4 effects: drop-shadow parameters (in logical pixels).
    /// Gaussian blur radius for the drop-shadow pixel shader. 16.0 px
    /// is a soft default that reads as a real shadow without dominating
    /// the layout. Render-path application is the same `MultiRenderer`-
    /// trait gap as rounded corners — see
    /// `crate::effects::shadow` and `TODO(P4-render-integration)`.
    pub shadow_radius: f32,
    /// Shadow offset from the window in logical pixels, `(x, y)`.
    /// Positive `y` drops the shadow downward (the conventional "drop"
    /// shadow). Default `(0.0, 4.0)`.
    pub shadow_offset: (f32, f32),
    /// Premultiplied RGBA shadow color, 0..1 per channel. Default
    /// `[0.0, 0.0, 0.0, 0.5]` — 50% black, matching the Catppuccin/zOS
    /// reference look.
    pub shadow_color: [f32; 4],

    /// Compile-time extension registry. Populated in `init` with whatever
    /// the compositor wants running in-process every frame. Each frame the
    /// backends call `pre_frame_all` / `post_frame_all` around render.
    pub extension_registry: crate::extension::ExtensionRegistry,
}

#[derive(Debug)]
pub struct DndIcon {
    pub surface: WlSurface,
    pub offset: Point<i32, Logical>,
}

/// Pointer focus policy. Controls whether moving the pointer over a window
/// changes keyboard focus and/or raises it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FocusMode {
    #[default]
    ClickToFocus,
    FollowMouse,
    /// Hover focuses, but raise only on click.
    FollowMouseClickToRaise,
}

delegate_compositor!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> DataDeviceHandler for AnvilState<BackendData> {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl<BackendData: Backend> WaylandDndGrabHandler for AnvilState<BackendData> {
    fn dnd_requested<S: Source>(
        &mut self,
        source: S,
        icon: Option<WlSurface>,
        seat: Seat<Self>,
        serial: Serial,
        type_: GrabType,
    ) {
        self.dnd_icon = icon.map(|surface| DndIcon {
            surface,
            offset: (0, 0).into(),
        });

        match type_ {
            GrabType::Pointer => {
                let pointer = seat.get_pointer().unwrap();
                let start_data = pointer.grab_start_data().unwrap();
                pointer.set_grab(
                    self,
                    DnDGrab::new_pointer(&self.display_handle, start_data, source, seat),
                    serial,
                    Focus::Keep,
                );
            }
            GrabType::Touch => {
                let touch = seat.get_touch().unwrap();
                let start_data = touch.grab_start_data().unwrap();
                touch.set_grab(
                    self,
                    DnDGrab::new_touch(&self.display_handle, start_data, source, seat),
                    serial,
                );
            }
        }
    }
}

impl<BackendData: Backend> DndGrabHandler for AnvilState<BackendData> {
    fn dropped(
        &mut self,
        _target: Option<DndTarget<'_, Self>>,
        _validated: bool,
        _seat: Seat<Self>,
        _location: Point<f64, Logical>,
    ) {
        self.dnd_icon = None;
    }
}
delegate_data_device!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> OutputHandler for AnvilState<BackendData> {}
delegate_output!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> SelectionHandler for AnvilState<BackendData> {
    type SelectionUserData = ();

    #[cfg(feature = "xwayland")]
    fn new_selection(&mut self, ty: SelectionTarget, source: Option<SelectionSource>, _seat: Seat<Self>) {
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.new_selection(ty, source.map(|source| source.mime_types())) {
                warn!(?err, ?ty, "Failed to set Xwayland selection");
            }
        }
    }

    #[cfg(feature = "xwayland")]
    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        _user_data: &(),
    ) {
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.send_selection(ty, mime_type, fd) {
                warn!(?err, "Failed to send primary (X11 -> Wayland)");
            }
        }
    }
}

impl<BackendData: Backend> PrimarySelectionHandler for AnvilState<BackendData> {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.primary_selection_state
    }
}
delegate_primary_selection!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> DataControlHandler for AnvilState<BackendData> {
    fn data_control_state(&mut self) -> &mut DataControlState {
        &mut self.data_control_state
    }
}

delegate_data_control!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> ShmHandler for AnvilState<BackendData> {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}
delegate_shm!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> SeatHandler for AnvilState<BackendData> {
    type KeyboardFocus = KeyboardFocusTarget;
    type PointerFocus = PointerFocusTarget;
    type TouchFocus = PointerFocusTarget;

    fn seat_state(&mut self) -> &mut SeatState<AnvilState<BackendData>> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, target: Option<&KeyboardFocusTarget>) {
        let dh = &self.display_handle;

        let wl_surface = target.and_then(WaylandFocus::wl_surface);

        let focus = wl_surface.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, focus.clone());
        set_primary_focus(dh, seat, focus);
    }
    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }

    fn led_state_changed(&mut self, _seat: &Seat<Self>, led_state: LedState) {
        self.backend_data.update_led_state(led_state)
    }
}
delegate_seat!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> TabletSeatHandler for AnvilState<BackendData> {
    fn tablet_tool_image(&mut self, _tool: &TabletToolDescriptor, image: CursorImageStatus) {
        // TODO: tablet tools should have their own cursors
        self.cursor_status = image;
    }
}
delegate_tablet_manager!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

smithay::delegate_cursor_shape!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_text_input_manager!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> InputMethodHandler for AnvilState<BackendData> {
    fn new_popup(&mut self, surface: PopupSurface) {
        if let Err(err) = self.popups.track_popup(PopupKind::from(surface)) {
            warn!("Failed to track popup: {}", err);
        }
    }

    fn popup_repositioned(&mut self, _: PopupSurface) {}

    fn dismiss_popup(&mut self, surface: PopupSurface) {
        if let Some(parent) = surface.get_parent().map(|parent| parent.surface.clone()) {
            let _ = PopupManager::dismiss_popup(&parent, &PopupKind::from(surface));
        }
    }

    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, smithay::utils::Logical> {
        self.space
            .elements()
            .find_map(|window| (window.wl_surface().as_deref() == Some(parent)).then(|| window.geometry()))
            .unwrap_or_default()
    }
}

delegate_input_method_manager!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> KeyboardShortcutsInhibitHandler for AnvilState<BackendData> {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }

    fn new_inhibitor(&mut self, inhibitor: KeyboardShortcutsInhibitor) {
        // Just grant the wish for everyone
        inhibitor.activate();
    }
}

delegate_keyboard_shortcuts_inhibit!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_virtual_keyboard_manager!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_pointer_gestures!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_relative_pointer!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> PointerConstraintsHandler for AnvilState<BackendData> {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        // XXX region
        let Some(current_focus) = pointer.current_focus() else {
            return;
        };
        if current_focus.wl_surface().as_deref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |constraint| {
                constraint.unwrap().activate();
            });
        }
    }

    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        pointer: &PointerHandle<Self>,
        location: Point<f64, Logical>,
    ) {
        if with_pointer_constraint(surface, pointer, |constraint| {
            constraint.is_some_and(|c| c.is_active())
        }) {
            let origin = self
                .space
                .elements()
                .find_map(|window| {
                    (window.wl_surface().as_deref() == Some(surface)).then(|| window.geometry())
                })
                .unwrap_or_default()
                .loc
                .to_f64();

            pointer.set_location(origin + location);
        }
    }
}
delegate_pointer_constraints!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_viewporter!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> XdgActivationHandler for AnvilState<BackendData> {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn token_created(&mut self, _token: XdgActivationToken, data: XdgActivationTokenData) -> bool {
        if let Some((serial, seat)) = data.serial {
            let keyboard = self.seat.get_keyboard().unwrap();
            Seat::from_resource(&seat) == Some(self.seat.clone())
                && keyboard
                    .last_enter()
                    .map(|last_enter| serial.is_no_older_than(&last_enter))
                    .unwrap_or(false)
        } else {
            false
        }
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if token_data.timestamp.elapsed().as_secs() < 10 {
            // Just grant the wish
            let w = self
                .space
                .elements()
                .find(|window| window.wl_surface().map(|s| *s == surface).unwrap_or(false))
                .cloned();
            if let Some(window) = w {
                self.space.raise_element(&window, true);
            }
        }
    }
}
delegate_xdg_activation!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> XdgDecorationHandler for AnvilState<BackendData> {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        use xdg_decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        // Set the default to client side
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
    }
    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
        use xdg_decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;

        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(match mode {
                DecorationMode::ServerSide => Mode::ServerSide,
                _ => Mode::ClientSide,
            });
        });

        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        use xdg_decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });

        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}
delegate_xdg_decoration!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> KdeDecorationHandler for AnvilState<BackendData> {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.kde_decoration_state
    }
}
smithay::delegate_kde_decoration!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_xdg_shell!(@<BackendData: Backend + 'static> AnvilState<BackendData>);
delegate_layer_shell!(@<BackendData: Backend + 'static> AnvilState<BackendData>);
delegate_presentation!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> FractionalScaleHandler for AnvilState<BackendData> {
    fn new_fractional_scale(
        &mut self,
        surface: smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) {
        // Here we can set the initial fractional scale
        //
        // First we look if the surface already has a primary scan-out output, if not
        // we test if the surface is a subsurface and try to use the primary scan-out output
        // of the root surface. If the root also has no primary scan-out output we just try
        // to use the first output of the toplevel.
        // If the surface is the root we also try to use the first output of the toplevel.
        //
        // If all the above tests do not lead to a output we just use the first output
        // of the space (which in case of anvil will also be the output a toplevel will
        // initially be placed on)
        #[allow(clippy::redundant_clone)]
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }

        with_states(&surface, |states| {
            let primary_scanout_output = surface_primary_scanout_output(&surface, states)
                .or_else(|| {
                    if root != surface {
                        with_states(&root, |states| {
                            surface_primary_scanout_output(&root, states).or_else(|| {
                                self.window_for_surface(&root).and_then(|window| {
                                    self.space.outputs_for_element(&window).first().cloned()
                                })
                            })
                        })
                    } else {
                        self.window_for_surface(&root)
                            .and_then(|window| self.space.outputs_for_element(&window).first().cloned())
                    }
                })
                .or_else(|| self.space.outputs().next().cloned());
            if let Some(output) = primary_scanout_output {
                with_fractional_scale(states, |fractional_scale| {
                    fractional_scale.set_preferred_scale(output.current_scale().fractional_scale());
                });
            }
        });
    }
}
delegate_fractional_scale!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend + 'static> SecurityContextHandler for AnvilState<BackendData> {
    fn context_created(&mut self, source: SecurityContextListenerSource, security_context: SecurityContext) {
        self.handle
            .insert_source(source, move |client_stream, _, data| {
                let client_state = ClientState {
                    security_context: Some(security_context.clone()),
                    ..ClientState::default()
                };
                if let Err(err) = data
                    .display_handle
                    .insert_client(client_stream, Arc::new(client_state))
                {
                    warn!("Error adding wayland client: {}", err);
                };
            })
            .expect("Failed to init wayland socket source");
    }
}
delegate_security_context!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

#[cfg(feature = "xwayland")]
impl<BackendData: Backend + 'static> XWaylandKeyboardGrabHandler for AnvilState<BackendData> {
    fn keyboard_focus_for_xsurface(&self, surface: &WlSurface) -> Option<KeyboardFocusTarget> {
        let elem = self
            .space
            .elements()
            .find(|elem| elem.wl_surface().as_deref() == Some(surface))?;
        Some(KeyboardFocusTarget::Window(elem.0.clone()))
    }
}
#[cfg(feature = "xwayland")]
delegate_xwayland_keyboard_grab!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

#[cfg(feature = "xwayland")]
delegate_xwayland_shell!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> XdgForeignHandler for AnvilState<BackendData> {
    fn xdg_foreign_state(&mut self) -> &mut XdgForeignState {
        &mut self.xdg_foreign_state
    }
}
smithay::delegate_xdg_foreign!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> ForeignToplevelListHandler for AnvilState<BackendData> {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.foreign_toplevel_list_state
    }
}
smithay::delegate_foreign_toplevel_list!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

smithay::delegate_single_pixel_buffer!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

smithay::delegate_fifo!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

smithay::delegate_commit_timing!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

delegate_fixes!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> ImageCaptureSourceHandler for AnvilState<BackendData> {
    fn source_destroyed(&mut self, _source: ImageCaptureSource) {
        // Anvil doesn't track sources
    }
}
smithay::delegate_image_capture_source!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> OutputCaptureSourceHandler for AnvilState<BackendData> {
    fn output_capture_source_state(&mut self) -> &mut OutputCaptureSourceState {
        &mut self.output_capture_source_state
    }

    fn output_source_created(&mut self, source: ImageCaptureSource, output: &Output) {
        source.user_data().insert_if_missing(|| output.downgrade());
    }
}
smithay::delegate_output_capture_source!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> ImageCopyCaptureHandler for AnvilState<BackendData> {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.image_copy_capture_state
    }

    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        use smithay::output::WeakOutput;
        let weak_output = source.user_data().get::<WeakOutput>()?;
        let output = weak_output.upgrade()?;
        let mode = output.current_mode()?;

        // Ask the backend for dmabuf format constraints. Backends that don't
        // advertise dmabuf capture (or can't reach a renderer right now)
        // return `None` from the default impl, which falls back to shm-only.
        #[cfg(any(feature = "udev", feature = "winit", feature = "x11"))]
        let dma = self.backend_data.screencopy_dma_constraints();

        Some(BufferConstraints {
            size: mode
                .size
                .to_logical(1)
                .to_buffer(1, smithay::utils::Transform::Normal),
            shm: vec![
                smithay::reexports::wayland_server::protocol::wl_shm::Format::Argb8888,
                smithay::reexports::wayland_server::protocol::wl_shm::Format::Xrgb8888,
            ],
            #[cfg(any(feature = "udev", feature = "winit", feature = "x11"))]
            dma,
        })
    }

    fn new_session(&mut self, _session: Session) {
        // Anvil doesn't track sessions; they clean up on drop
    }

    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        use smithay::output::WeakOutput;

        // Look up the Output this capture is bound to — stashed in the source's
        // user-data by `OutputCaptureSourceHandler::output_source_created`.
        let source = session.source();
        let Some(weak_output) = source.user_data().get::<WeakOutput>().cloned() else {
            // Non-output source (e.g. toplevel) — not supported yet.
            // TODO(screencopy-toplevel): implement toplevel capture.
            frame.fail(smithay::wayland::image_copy_capture::CaptureFailureReason::Stopped);
            return;
        };

        // Verify the output is still alive before enqueueing; fail fast if
        // the monitor was unplugged between session creation and capture.
        if weak_output.upgrade().is_none() {
            frame.fail(smithay::wayland::image_copy_capture::CaptureFailureReason::Stopped);
            return;
        }

        self.pending_screencopy.push(PendingScreencopy {
            output: weak_output,
            session: session.clone(),
            frame,
        });
    }
}
smithay::delegate_image_copy_capture!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend> IdleInhibitHandler for AnvilState<BackendData> {
    fn inhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.insert(surface);
        self.idle_notifier_state.set_is_inhibited(!self.idle_inhibitors.is_empty());
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle_inhibitors.remove(&surface);
        self.idle_notifier_state.set_is_inhibited(!self.idle_inhibitors.is_empty());
    }
}
smithay::delegate_idle_inhibit!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend + 'static> IdleNotifierHandler for AnvilState<BackendData> {
    fn idle_notifier_state(&mut self) -> &mut IdleNotifierState<Self> {
        &mut self.idle_notifier_state
    }
}
smithay::delegate_idle_notify!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

crate::delegate_tearing_control!(@<BackendData: Backend + 'static> AnvilState<BackendData>);
crate::delegate_gamma_control!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend + 'static> OutputManagementHandler for AnvilState<BackendData> {
    fn output_management_state(&mut self) -> &mut OutputManagementManagerState {
        &mut self.output_management_manager_state
    }

    fn apply_output_config(
        &mut self,
        changes: &[OutputConfigChange],
        test_only: bool,
    ) -> Result<(), OutputConfigError> {
        // Phase 1: backend does the DRM-side work (modeset, surface
        // teardown, plane disable, adaptive-sync stash). The default
        // `Backend::apply_output_config` returns `NotSupported`, so
        // winit/x11 trivially refuse output reconfiguration.
        self.backend_data.apply_output_config(changes, test_only)?;

        // Phase 2 (skipped on test_only): apply Space-side updates that
        // the backend cannot reach. The Backend trait only sees its own
        // BackendData (DRM/winit), not `Space<WindowElement>` which lives
        // on `AnvilState`. Mode/refresh changes were already committed by
        // the backend; here we update the smithay Output's logical state
        // (transform, scale, position) and re-map the output in `Space`
        // so layouts reflect the new geometry.
        if !test_only {
            for change in changes {
                self.apply_space_change(change);
            }
        }
        Ok(())
    }
}
crate::delegate_output_management!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend + 'static> AnvilState<BackendData> {
    /// Post-DRM Space-side updates for a single output config change.
    ///
    /// Runs *after* `Backend::apply_output_config` returned `Ok(())`, so
    /// any DRM-level work (modeset, surface teardown for Disable) is
    /// already committed. This helper is infallible by construction:
    /// `Output::change_current_state` just stamps fields, and
    /// `Space::map_output`/`unmap_output` cannot fail.
    fn apply_space_change(
        &mut self,
        change: &crate::protocols::output_management::OutputConfigChange,
    ) {
        use crate::protocols::output_management::OutputConfigAction;
        use smithay::output::Scale;
        match &change.action {
            OutputConfigAction::Enable {
                mode: _, // already applied by Backend::apply_output_config
                position,
                scale,
                transform,
                adaptive_sync: _, // stashed in Output user-data by the dispatch
            } => {
                // Re-stamp logical state for any field the client set.
                // `change_current_state` treats `None` as "leave alone",
                // so we map `Option<f64>` → `Option<Scale::Fractional>`.
                let scale_arg = scale.map(Scale::Fractional);
                change.output.change_current_state(
                    None,
                    *transform,
                    scale_arg,
                    *position,
                );
                // (Re)map the output into Space at the requested
                // position; default to the origin if the client didn't
                // supply one (matches how connector_connected handles a
                // freshly-enabled head).
                let pos = position.unwrap_or_else(|| (0, 0).into());
                self.space.map_output(&change.output, pos);
            }
            OutputConfigAction::Disable => {
                // Backend has torn down the DRM surface (udev) or
                // returned NotSupported (winit, in which case `?`
                // already short-circuited and we never get here). All
                // that remains is to drop the output from the layout.
                self.space.unmap_output(&change.output);
                self.space.refresh();
            }
            OutputConfigAction::Update {
                mode: _, // already applied by Backend::apply_output_config
                position,
                scale,
                transform,
                adaptive_sync: _,
            } => {
                let scale_arg = scale.map(Scale::Fractional);
                change.output.change_current_state(
                    None,
                    *transform,
                    scale_arg,
                    *position,
                );
                if let Some(pos) = position {
                    self.space.map_output(&change.output, *pos);
                }
            }
        }
    }
}

impl<BackendData: Backend> SessionLockHandler for AnvilState<BackendData> {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_manager_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        // Single-user system: accept the lock immediately. Real rendering
        // bypass for the lock surface lives in the render path (future work).
        self.is_locked = true;
        confirmation.lock();
    }

    fn unlock(&mut self) {
        self.is_locked = false;
    }

    fn new_surface(&mut self, _surface: LockSurface, _output: WlOutput) {
        // Render path currently ignores lock surfaces; wire-up only for now.
    }

    fn ack_configure(&mut self, _surface: WlSurface, _configure: LockSurfaceConfigure) {}
}
smithay::delegate_session_lock!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend + 'static> PointerWarpHandler for AnvilState<BackendData> {
    fn warp_pointer(
        &mut self,
        surface: WlSurface,
        _pointer: WlPointer,
        pos: Point<f64, Logical>,
        _serial: Serial,
    ) {
        // Only honor the warp if the surface currently has pointer focus.
        let Some(current_focus) = self.pointer.current_focus() else {
            return;
        };
        if current_focus.wl_surface().as_deref() != Some(&surface) {
            return;
        }

        let origin = self
            .space
            .elements()
            .find_map(|window| {
                (window.wl_surface().as_deref() == Some(&surface)).then(|| window.geometry())
            })
            .unwrap_or_default()
            .loc
            .to_f64();

        self.pointer.set_location(origin + pos);
    }
}
smithay::delegate_pointer_warp!(@<BackendData: Backend + 'static> AnvilState<BackendData>);

impl<BackendData: Backend + 'static> AnvilState<BackendData> {
    pub fn init(
        display: Display<AnvilState<BackendData>>,
        handle: LoopHandle<'static, AnvilState<BackendData>>,
        backend_data: BackendData,
        listen_on_socket: bool,
    ) -> AnvilState<BackendData> {
        let dh = display.handle();

        let clock = Clock::new();

        // init wayland clients
        let socket_name = if listen_on_socket {
            let source = ListeningSocketSource::new_auto().unwrap();
            let socket_name = source.socket_name().to_string_lossy().into_owned();
            handle
                .insert_source(source, |client_stream, _, data| {
                    if let Err(err) = data
                        .display_handle
                        .insert_client(client_stream, Arc::new(ClientState::default()))
                    {
                        warn!("Error adding wayland client: {}", err);
                    };
                })
                .expect("Failed to init wayland socket source");
            info!(name = socket_name, "Listening on wayland socket");
            Some(socket_name)
        } else {
            None
        };
        handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, data| {
                    profiling::scope!("dispatch_clients");
                    // Safety: we don't drop the display
                    unsafe {
                        display.get_mut().dispatch_clients(data).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("Failed to init wayland server source");

        // init globals
        let compositor_state = CompositorState::new::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let data_control_state =
            DataControlState::new::<Self, _>(&dh, Some(&primary_selection_state), |_| true);
        let mut seat_state = SeatState::new();
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let viewporter_state = ViewporterState::new::<Self>(&dh);
        let xdg_activation_state = XdgActivationState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let kde_decoration_state = KdeDecorationState::new::<Self>(
            &dh,
            smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode::Client,
        );
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let presentation_state = PresentationState::new::<Self>(&dh, clock.id() as u32);
        let fractional_scale_manager_state = FractionalScaleManagerState::new::<Self>(&dh);
        let xdg_foreign_state = XdgForeignState::new::<Self>(&dh);
        let foreign_toplevel_list_state = ForeignToplevelListState::new::<Self>(&dh);
        let single_pixel_buffer_state = SinglePixelBufferState::new::<Self>(&dh);
        let fifo_manager_state = FifoManagerState::new::<Self>(&dh);
        let commit_timing_manager_state = CommitTimingManagerState::new::<Self>(&dh);
        TextInputManagerState::new::<Self>(&dh);
        InputMethodManagerState::new::<Self, _>(&dh, |_client| true);
        VirtualKeyboardManagerState::new::<Self, _>(&dh, |_client| true);
        // Expose global only if backend supports relative motion events
        if BackendData::HAS_RELATIVE_MOTION {
            RelativePointerManagerState::new::<Self>(&dh);
        }
        PointerConstraintsState::new::<Self>(&dh);
        if BackendData::HAS_GESTURES {
            PointerGesturesState::new::<Self>(&dh);
        }
        TabletManagerState::new::<Self>(&dh);
        SecurityContextState::new::<Self, _>(&dh, |client| {
            client
                .get_data::<ClientState>()
                .is_none_or(|client_state| client_state.security_context.is_none())
        });
        FixesState::new::<Self>(&dh);

        // Image capture protocols (screencopy)
        let image_capture_source_state = ImageCaptureSourceState::new();
        let output_capture_source_state = OutputCaptureSourceState::new::<Self>(&dh);
        let image_copy_capture_state = ImageCopyCaptureState::new::<Self>(&dh);

        // Cursor-shape, idle, session-lock, and pointer-warp globals.
        // Cursor-shape and pointer-warp state objects aren't needed post-init
        // (they only hold a GlobalId we don't otherwise reference); let them drop.
        let _ = CursorShapeManagerState::new::<Self>(&dh);
        let _ = PointerWarpManager::new::<Self>(&dh);
        let idle_inhibit_manager_state = IdleInhibitManagerState::new::<Self>(&dh);
        let idle_notifier_state = IdleNotifierState::<Self>::new(&dh, handle.clone());
        let tearing_control_manager_state = TearingControlManagerState::new::<Self>(&dh);
        let gamma_control_manager_state = GammaControlManagerState::new::<Self>(&dh);
        let output_management_manager_state = OutputManagementManagerState::new::<Self>(&dh);
        // Gate session-lock behind the same security-context filter that the rest
        // of the privileged globals use.
        let session_lock_manager_state = SessionLockManagerState::new::<Self, _>(&dh, |client| {
            client
                .get_data::<ClientState>()
                .is_none_or(|client_state| client_state.security_context.is_none())
        });

        // init input
        let seat_name = backend_data.seat_name();
        let mut seat = seat_state.new_wl_seat(&dh, seat_name.clone());

        let pointer = seat.add_pointer();
        seat.add_keyboard(XkbConfig::default(), 200, 25)
            .expect("Failed to initialize the keyboard");

        let keyboard_shortcuts_inhibit_state = KeyboardShortcutsInhibitState::new::<Self>(&dh);

        #[cfg(feature = "xwayland")]
        let xwayland_shell_state = xwayland_shell::XWaylandShellState::new::<Self>(&dh.clone());

        #[cfg(feature = "xwayland")]
        XWaylandKeyboardGrabState::new::<Self>(&dh.clone());

        // Build the compile-time extension registry. Register everything we
        // want running in-process before the first frame, then run init_all
        // so each extension can prepare any state it needs. LogFrameCount is
        // the canonical example/template; real extensions (animations, layout
        // hooks, etc.) get registered alongside it here.
        let mut extension_registry = crate::extension::ExtensionRegistry::new();
        extension_registry.register(Box::new(crate::extension::LogFrameCount::new()));
        extension_registry.init_all();

        AnvilState {
            backend_data,
            display_handle: dh,
            socket_name,
            running: Arc::new(AtomicBool::new(true)),
            handle,
            space: Space::default(),
            popups: PopupManager::default(),
            compositor_state,
            data_device_state,
            layer_shell_state,
            output_manager_state,
            primary_selection_state,
            data_control_state,
            seat_state,
            keyboard_shortcuts_inhibit_state,
            shm_state,
            viewporter_state,
            xdg_activation_state,
            xdg_decoration_state,
            kde_decoration_state,
            xdg_shell_state,
            presentation_state,
            fractional_scale_manager_state,
            xdg_foreign_state,
            foreign_toplevel_list_state,
            single_pixel_buffer_state,
            fifo_manager_state,
            commit_timing_manager_state,
            image_capture_source_state,
            output_capture_source_state,
            image_copy_capture_state,
            pending_screencopy: Vec::new(),
            idle_inhibit_manager_state,
            idle_notifier_state,
            idle_inhibitors: HashSet::new(),
            tearing_control_manager_state,
            gamma_control_manager_state,
            output_management_manager_state,
            session_lock_manager_state,
            is_locked: false,
            dnd_icon: None,
            suppressed_keys: Vec::new(),
            cursor_status: CursorImageStatus::default_named(),
            seat_name,
            seat,
            pointer,
            clock,

            #[cfg(feature = "xwayland")]
            xwayland_shell_state,
            #[cfg(feature = "xwayland")]
            xwm: None,
            #[cfg(feature = "xwayland")]
            xdisplay: None,
            #[cfg(feature = "debug")]
            renderdoc: renderdoc::RenderDoc::new().ok(),
            show_window_preview: false,

            // Phase 3: per-output workspaces / focus tracking / parking lot.
            outputs: HashMap::new(),
            focused_output: None,
            workspace_history: Vec::new(),
            parking_lot: Vec::new(),
            focus_mode: FocusMode::default(),

            // Phase 3 input dispatch: bind table + suppression sets.
            bindings: crate::binds::default_bindings(),
            suppressed_keycodes: HashSet::new(),
            suppressed_buttons: HashSet::new(),

            // Phase 4 animations: registry + per-property config defaults,
            // overlaid with `~/.config/zos/animations.toml` via zos-ui's
            // loader. Missing/malformed file degrades to empty overrides.
            animation_manager: {
                let overrides = zos_ui::config::load_animations();
                crate::anim::AnimationManager::default().with_overrides(overrides)
            },

            // Phase 4 effects: 8.0 px rounded corners by default.
            corner_radius: 8.0,

            // Phase 4 effects: 16 px soft drop shadow, 4 px down, 50% black.
            shadow_radius: 16.0,
            shadow_offset: (0.0, 4.0),
            shadow_color: [0.0, 0.0, 0.0, 0.5],

            // Compile-time extension registry, already populated + init'd above.
            extension_registry,
        }
    }

    #[cfg(feature = "xwayland")]
    pub fn start_xwayland(&mut self) {
        use std::process::Stdio;

        use smithay::wayland::compositor::CompositorHandler;

        let (xwayland, client) = XWayland::spawn(
            &self.display_handle,
            None,
            std::iter::empty::<(String, String)>(),
            true,
            Stdio::null(),
            Stdio::null(),
            |_| (),
        )
        .expect("failed to start XWayland");

        let display_handle = self.display_handle.clone();
        let ret = self
            .handle
            .insert_source(xwayland, move |event, _, data| match event {
                XWaylandEvent::Ready {
                    x11_socket,
                    display_number,
                } => {
                    let xwayland_scale = std::env::var("ANVIL_XWAYLAND_SCALE")
                        .ok()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(1.);
                    data.client_compositor_state(&client)
                        .set_client_scale(xwayland_scale);
                    let mut wm =
                        X11Wm::start_wm(data.handle.clone(), &display_handle, x11_socket, client.clone())
                            .expect("Failed to attach X11 Window Manager");

                    let cursor = Cursor::load();
                    let image = cursor.get_image(1, Duration::ZERO);
                    wm.set_cursor(
                        &image.pixels_rgba,
                        Size::from((image.width as u16, image.height as u16)),
                        Point::from((image.xhot as u16, image.yhot as u16)),
                    )
                    .expect("Failed to set xwayland default cursor");
                    data.xwm = Some(wm);
                    data.xdisplay = Some(display_number);
                }
                XWaylandEvent::Error => {
                    warn!("XWayland crashed on startup");
                }
            });
        if let Err(e) = ret {
            tracing::error!("Failed to insert the XWaylandSource into the event loop: {}", e);
        }
    }
}

impl<BackendData: Backend + 'static> AnvilState<BackendData> {
    pub fn pre_repaint(&mut self, output: &Output, frame_target: impl Into<Time<Monotonic>>) {
        let frame_target = frame_target.into();

        #[allow(clippy::mutable_key_type)]
        let mut clients: HashMap<ClientId, Client> = HashMap::new();
        self.space.elements().for_each(|window| {
            window.with_surfaces(|surface, states| {
                if let Some(mut commit_timer_state) = states
                    .data_map
                    .get::<CommitTimerBarrierStateUserData>()
                    .map(|commit_timer| commit_timer.lock().unwrap())
                {
                    commit_timer_state.signal_until(frame_target);
                    let client = surface.client().unwrap();
                    clients.insert(client.id(), client);
                }
            });
        });

        let map = smithay::desktop::layer_map_for_output(output);
        for layer_surface in map.layers() {
            layer_surface.with_surfaces(|surface, states| {
                if let Some(mut commit_timer_state) = states
                    .data_map
                    .get::<CommitTimerBarrierStateUserData>()
                    .map(|commit_timer| commit_timer.lock().unwrap())
                {
                    commit_timer_state.signal_until(frame_target);
                    let client = surface.client().unwrap();
                    clients.insert(client.id(), client);
                }
            });
        }
        // Drop the lock to the layer map before calling blocker_cleared, which might end up
        // calling the commit handler which in turn again could access the layer map.
        std::mem::drop(map);

        if let CursorImageStatus::Surface(ref surface) = self.cursor_status {
            with_surfaces_surface_tree(surface, |surface, states| {
                if let Some(mut commit_timer_state) = states
                    .data_map
                    .get::<CommitTimerBarrierStateUserData>()
                    .map(|commit_timer| commit_timer.lock().unwrap())
                {
                    commit_timer_state.signal_until(frame_target);
                    let client = surface.client().unwrap();
                    clients.insert(client.id(), client);
                }
            });
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            with_surfaces_surface_tree(surface, |surface, states| {
                if let Some(mut commit_timer_state) = states
                    .data_map
                    .get::<CommitTimerBarrierStateUserData>()
                    .map(|commit_timer| commit_timer.lock().unwrap())
                {
                    commit_timer_state.signal_until(frame_target);
                    let client = surface.client().unwrap();
                    clients.insert(client.id(), client);
                }
            });
        }

        let dh = self.display_handle.clone();
        for client in clients.into_values() {
            self.client_compositor_state(&client).blocker_cleared(self, &dh);
        }
    }

    pub fn post_repaint(
        &mut self,
        output: &Output,
        time: impl Into<Duration>,
        dmabuf_feedback: Option<SurfaceDmabufFeedback>,
        render_element_states: &RenderElementStates,
    ) {
        let time = time.into();
        let throttle = Some(Duration::from_secs(1));

        #[allow(clippy::mutable_key_type)]
        let mut clients: HashMap<ClientId, Client> = HashMap::new();

        self.space.elements().for_each(|window| {
            window.with_surfaces(|surface, states| {
                let primary_scanout_output = surface_primary_scanout_output(surface, states);

                if let Some(output) = primary_scanout_output.as_ref() {
                    with_fractional_scale(states, |fraction_scale| {
                        fraction_scale.set_preferred_scale(output.current_scale().fractional_scale());
                    });
                }

                if primary_scanout_output
                    .as_ref()
                    .map(|o| o == output)
                    .unwrap_or(true)
                {
                    let fifo_barrier = states
                        .cached_state
                        .get::<FifoBarrierCachedState>()
                        .current()
                        .barrier
                        .take();

                    if let Some(fifo_barrier) = fifo_barrier {
                        fifo_barrier.signal();
                        let client = surface.client().unwrap();
                        clients.insert(client.id(), client);
                    }
                }
            });

            if self.space.outputs_for_element(window).contains(output) {
                window.send_frame(output, time, throttle, surface_primary_scanout_output);
                if let Some(dmabuf_feedback) = dmabuf_feedback.as_ref() {
                    window.send_dmabuf_feedback(output, surface_primary_scanout_output, |surface, _| {
                        select_dmabuf_feedback(
                            surface,
                            render_element_states,
                            &dmabuf_feedback.render_feedback,
                            &dmabuf_feedback.scanout_feedback,
                        )
                    });
                }
            }
        });
        let map = smithay::desktop::layer_map_for_output(output);
        for layer_surface in map.layers() {
            layer_surface.with_surfaces(|surface, states| {
                let primary_scanout_output = surface_primary_scanout_output(surface, states);

                if let Some(output) = primary_scanout_output.as_ref() {
                    with_fractional_scale(states, |fraction_scale| {
                        fraction_scale.set_preferred_scale(output.current_scale().fractional_scale());
                    });
                }

                if primary_scanout_output
                    .as_ref()
                    .map(|o| o == output)
                    .unwrap_or(true)
                {
                    let fifo_barrier = states
                        .cached_state
                        .get::<FifoBarrierCachedState>()
                        .current()
                        .barrier
                        .take();

                    if let Some(fifo_barrier) = fifo_barrier {
                        fifo_barrier.signal();
                        let client = surface.client().unwrap();
                        clients.insert(client.id(), client);
                    }
                }
            });

            layer_surface.send_frame(output, time, throttle, surface_primary_scanout_output);
            if let Some(dmabuf_feedback) = dmabuf_feedback.as_ref() {
                layer_surface.send_dmabuf_feedback(output, surface_primary_scanout_output, |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &dmabuf_feedback.render_feedback,
                        &dmabuf_feedback.scanout_feedback,
                    )
                });
            }
        }
        // Drop the lock to the layer map before calling blocker_cleared, which might end up
        // calling the commit handler which in turn again could access the layer map.
        std::mem::drop(map);

        if let CursorImageStatus::Surface(ref surface) = self.cursor_status {
            with_surfaces_surface_tree(surface, |surface, states| {
                let primary_scanout_output = surface_primary_scanout_output(surface, states);

                if let Some(output) = primary_scanout_output.as_ref() {
                    with_fractional_scale(states, |fraction_scale| {
                        fraction_scale.set_preferred_scale(output.current_scale().fractional_scale());
                    });
                }

                if primary_scanout_output
                    .as_ref()
                    .map(|o| o == output)
                    .unwrap_or(true)
                {
                    let fifo_barrier = states
                        .cached_state
                        .get::<FifoBarrierCachedState>()
                        .current()
                        .barrier
                        .take();

                    if let Some(fifo_barrier) = fifo_barrier {
                        fifo_barrier.signal();
                        let client = surface.client().unwrap();
                        clients.insert(client.id(), client);
                    }
                }
            });
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            with_surfaces_surface_tree(surface, |surface, states| {
                let primary_scanout_output = surface_primary_scanout_output(surface, states);

                if let Some(output) = primary_scanout_output.as_ref() {
                    with_fractional_scale(states, |fraction_scale| {
                        fraction_scale.set_preferred_scale(output.current_scale().fractional_scale());
                    });
                }

                if primary_scanout_output
                    .as_ref()
                    .map(|o| o == output)
                    .unwrap_or(true)
                {
                    let fifo_barrier = states
                        .cached_state
                        .get::<FifoBarrierCachedState>()
                        .current()
                        .barrier
                        .take();

                    if let Some(fifo_barrier) = fifo_barrier {
                        fifo_barrier.signal();
                        let client = surface.client().unwrap();
                        clients.insert(client.id(), client);
                    }
                }
            });
        }

        let dh = self.display_handle.clone();
        for client in clients.into_values() {
            self.client_compositor_state(&client).blocker_cleared(self, &dh);
        }
    }

    /// Advance every animation across all output states. Called once per
    /// frame at the start of render before element lists are built.
    pub fn tick_animations(&mut self, now: std::time::Instant) {
        for output_state in self.outputs.values_mut() {
            for workspace in output_state.workspaces.iter_mut() {
                workspace.tick_animations(now);
            }
        }
    }

    /// Returns true if any animation across all outputs is still in flight.
    pub fn any_animating(&self) -> bool {
        for output_state in self.outputs.values() {
            for workspace in output_state.workspaces.iter() {
                if workspace.any_animating() {
                    return true;
                }
            }
        }
        false
    }
}

pub fn update_primary_scanout_output(
    space: &Space<WindowElement>,
    output: &Output,
    dnd_icon: &Option<DndIcon>,
    cursor_status: &CursorImageStatus,
    render_element_states: &RenderElementStates,
) {
    space.elements().for_each(|window| {
        window.with_surfaces(|surface, states| {
            update_surface_primary_scanout_output(
                surface,
                output,
                states,
                None,
                render_element_states,
                default_primary_scanout_output_compare,
            );
        });
    });
    let map = smithay::desktop::layer_map_for_output(output);
    for layer_surface in map.layers() {
        layer_surface.with_surfaces(|surface, states| {
            update_surface_primary_scanout_output(
                surface,
                output,
                states,
                None,
                render_element_states,
                default_primary_scanout_output_compare,
            );
        });
    }

    if let CursorImageStatus::Surface(surface) = cursor_status {
        with_surfaces_surface_tree(surface, |surface, states| {
            update_surface_primary_scanout_output(
                surface,
                output,
                states,
                None,
                render_element_states,
                default_primary_scanout_output_compare,
            );
        });
    }

    if let Some(surface) = dnd_icon.as_ref().map(|icon| &icon.surface) {
        with_surfaces_surface_tree(surface, |surface, states| {
            update_surface_primary_scanout_output(
                surface,
                output,
                states,
                None,
                render_element_states,
                default_primary_scanout_output_compare,
            );
        });
    }
}

#[derive(Debug, Clone)]
pub struct SurfaceDmabufFeedback {
    pub render_feedback: DmabufFeedback,
    pub scanout_feedback: DmabufFeedback,
}

#[profiling::function]
pub fn take_presentation_feedback(
    output: &Output,
    space: &Space<WindowElement>,
    render_element_states: &RenderElementStates,
) -> OutputPresentationFeedback {
    let mut output_presentation_feedback = OutputPresentationFeedback::new(output);

    space.elements().for_each(|window| {
        if space.outputs_for_element(window).contains(output) {
            window.take_presentation_feedback(
                &mut output_presentation_feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(surface, None, render_element_states)
                },
            );
        }
    });
    let map = smithay::desktop::layer_map_for_output(output);
    for layer_surface in map.layers() {
        layer_surface.take_presentation_feedback(
            &mut output_presentation_feedback,
            surface_primary_scanout_output,
            |surface, _| {
                surface_presentation_feedback_flags_from_states(surface, None, render_element_states)
            },
        );
    }

    output_presentation_feedback
}

pub trait Backend {
    const HAS_RELATIVE_MOTION: bool = false;
    const HAS_GESTURES: bool = false;
    fn seat_name(&self) -> String;
    fn reset_buffers(&mut self, output: &Output);
    fn early_import(&mut self, surface: &WlSurface);
    fn update_led_state(&mut self, led_state: LedState);

    /// Apply (or test) a validated output configuration produced by a
    /// `zwlr_output_management_v1` client.
    ///
    /// The default implementation refuses every config with
    /// `OutputConfigError::NotSupported`, which is the right behaviour for
    /// backends that have a single virtual output (winit) or no output
    /// management at all (the headless/x11 test paths). The udev backend
    /// overrides this in follow-up task 2.D.3 to drive the real DRM
    /// modeset/space-remap path.
    ///
    /// `test_only = true` means the client called
    /// `zwlr_output_configuration_v1.test()` rather than `apply()`. The
    /// implementation must not commit any state changes in that case;
    /// it should only validate that the configuration is achievable.
    fn apply_output_config(
        &mut self,
        _changes: &[crate::protocols::output_management::OutputConfigChange],
        _test_only: bool,
    ) -> Result<(), crate::protocols::output_management::OutputConfigError> {
        Err(crate::protocols::output_management::OutputConfigError::NotSupported)
    }

    /// Optional dmabuf-capture format advertisement for ext-image-copy-capture-v1.
    ///
    /// Backends that have a render-capable GPU (winit's GLES context, udev's
    /// primary GPU) override this to advertise the dmabuf formats that
    /// `try_capture` can render directly into. Returning `None` falls back to
    /// shm-only capture (the legacy CPU memcpy path).
    ///
    /// Gated on `backend_drm` (via the same `udev`/`winit`/`x11` feature
    /// envelope as `BufferConstraints::dma`) because `DmabufConstraints`
    /// only exists when smithay's `backend_drm` is enabled.
    #[cfg(any(feature = "udev", feature = "winit", feature = "x11"))]
    fn screencopy_dma_constraints(
        &mut self,
    ) -> Option<smithay::wayland::image_copy_capture::DmabufConstraints> {
        None
    }
}

// ---------------------------------------------------------------------------
// IPC integration (Phase 7)
//
// The IPC server runs on its own thread (Unix socket accept loop). Per-client
// reader threads call back into the user-supplied handler closure. Since
// AnvilState is not `Send`, the handler can't touch it directly; instead we
// bridge via a calloop channel: handler enqueues `(Request, oneshot::Sender)`
// onto the compositor's event loop, the source dispatches the request inline
// against AnvilState, then sends the response back through the per-request
// oneshot. The handler closure blocks on the oneshot (with a 2s timeout) and
// returns the response to the IPC reader thread.
// ---------------------------------------------------------------------------
impl<BackendData: Backend + 'static> AnvilState<BackendData> {
    /// Start the zos-wm IPC server and wire it into the compositor's event
    /// loop. Returns the server handle, which **must** be kept alive for the
    /// duration of the main loop (its `Drop` impl removes the socket file).
    /// Returns `None` if either the calloop source or the listener fails to
    /// register; in that case the compositor continues without IPC.
    pub fn start_ipc_server(&self) -> Option<crate::ipc::IpcServer> {
        let socket_path = crate::ipc::IpcServer::default_socket_path();
        let handle = self.handle.clone();

        // Calloop channel from IPC threads -> compositor event loop.
        let (ipc_tx, ipc_rx) = smithay::reexports::calloop::channel::channel::<(
            crate::ipc::Request,
            std::sync::mpsc::SyncSender<crate::ipc::Response>,
        )>();

        if let Err(e) =
            handle.insert_source(ipc_rx, |event, _, data: &mut AnvilState<BackendData>| {
                use smithay::reexports::calloop::channel::Event;
                if let Event::Msg((req, resp_tx)) = event {
                    let response = data.handle_ipc_request(req);
                    let _ = resp_tx.send(response);
                }
            })
        {
            warn!(?e, "failed to insert ipc calloop source");
            return None;
        }

        // Handler closure consumed by the IPC server. Each invocation builds
        // a fresh oneshot pair so concurrent requests don't cross-talk.
        let handler = move |req: crate::ipc::Request| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<crate::ipc::Response>(1);
            if ipc_tx.send((req, tx)).is_err() {
                return crate::ipc::Response::Error {
                    message: "compositor channel closed".into(),
                };
            }
            rx.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap_or_else(|_| crate::ipc::Response::Error {
                    message: "compositor timeout".into(),
                })
        };

        match crate::ipc::IpcServer::start(socket_path, handler) {
            Ok(s) => {
                info!("zos-wm IPC server started");
                Some(s)
            }
            Err(e) => {
                warn!(?e, "failed to start IPC server; continuing without it");
                None
            }
        }
    }

    /// Map a single `Request` against the live compositor state and return a
    /// `Response`. Runs inline on the calloop event loop (not in an IPC
    /// thread), so it's safe to mutate `AnvilState` here.
    pub fn handle_ipc_request(&mut self, req: crate::ipc::Request) -> crate::ipc::Response {
        use crate::ipc::{
            Monitor as RMonitor, Request, Response, Window as RWindow, Workspace as RWorkspace,
        };
        use crate::shell::{WorkspaceId, ZBand};
        use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

        // Helper: pull (app_id, title) out of a WindowElement's xdg toplevel
        // role. Returns ("", "") for X11 surfaces or surfaces without the
        // role attached.
        fn class_and_title(element: &crate::shell::WindowElement) -> (String, String) {
            let Some(surface) = element.wl_surface() else {
                return (String::new(), String::new());
            };
            with_states(&surface, |states| {
                let role = states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .and_then(|d| d.lock().ok().map(|g| (g.app_id.clone(), g.title.clone())));
                match role {
                    Some((a, t)) => (a.unwrap_or_default(), t.unwrap_or_default()),
                    None => (String::new(), String::new()),
                }
            })
        }

        fn band_str(band: ZBand) -> String {
            match band {
                ZBand::Below => "Below",
                ZBand::Normal => "Normal",
                ZBand::AlwaysOnTop => "AlwaysOnTop",
                ZBand::Fullscreen => "Fullscreen",
            }
            .into()
        }

        match req {
            Request::Workspaces { output: filter } => {
                let mut all = Vec::new();
                for output_state in self.outputs.values() {
                    let output_name = output_state.output.name();
                    if let Some(ref f) = filter {
                        if &output_name != f {
                            continue;
                        }
                    }
                    let active_id = output_state.active().id;
                    for ws in &output_state.workspaces {
                        all.push(RWorkspace {
                            id: ws.id.0,
                            output: output_name.clone(),
                            windows: ws.windows.len(),
                            active: ws.id == active_id,
                        });
                    }
                }
                Response::Workspaces { workspaces: all }
            }
            Request::Windows {
                workspace: filter_ws,
            } => {
                let mut all = Vec::new();
                for output_state in self.outputs.values() {
                    let output_name = output_state.output.name();
                    let active_ws_id = output_state.active().id;
                    for ws in &output_state.workspaces {
                        if let Some(f) = filter_ws
                            && ws.id.0 != f
                        {
                            continue;
                        }
                        for entry in &ws.windows {
                            let (class, title) = class_and_title(&entry.element);
                            all.push(RWindow {
                                id: entry.id.raw(),
                                workspace_id: ws.id.0,
                                output: output_name.clone(),
                                class,
                                title,
                                focused: ws.active == Some(entry.id) && ws.id == active_ws_id,
                                band: band_str(entry.band),
                            });
                        }
                    }
                }
                Response::Windows { windows: all }
            }
            Request::Monitors => {
                let mut all = Vec::new();
                for output_state in self.outputs.values() {
                    let mode = output_state.output.current_mode();
                    let (width, height, refresh_mhz) = mode
                        .map(|m| (m.size.w as u32, m.size.h as u32, m.refresh as u32))
                        .unwrap_or((0, 0, 0));
                    all.push(RMonitor {
                        id: output_state.id.0,
                        name: output_state.output.name(),
                        width,
                        height,
                        refresh_mhz,
                        active_workspace: Some(output_state.active().id.0),
                    });
                }
                Response::Monitors { monitors: all }
            }
            Request::ActiveWindow => {
                // First focused window across all outputs.
                let win = self.outputs.values().find_map(|os| {
                    let ws = os.active();
                    let active_id = ws.active?;
                    let entry = ws.windows.iter().find(|e| e.id == active_id)?;
                    Some((os.output.name(), ws.id, entry.clone()))
                });
                let response_win = win.map(|(out_name, ws_id, entry)| {
                    let (class, title) = class_and_title(&entry.element);
                    RWindow {
                        id: entry.id.raw(),
                        workspace_id: ws_id.0,
                        output: out_name,
                        class,
                        title,
                        focused: true,
                        band: band_str(entry.band),
                    }
                });
                Response::ActiveWindow {
                    window: response_win,
                }
            }
            Request::SwitchToWorkspace { id } => {
                if let Some(out_id) = self.focused_output {
                    if let Some(out_state) = self.outputs.get_mut(&out_id) {
                        out_state.switch_to(WorkspaceId(id));
                    }
                    crate::shell::workspace::sync_active_workspaces_to_space(
                        &self.outputs,
                        &mut self.space,
                    );
                }
                Response::Ok
            }
            Request::MoveWindowToWorkspace { id } => {
                let target_id = WorkspaceId(id);
                let Some(out_id) = self.focused_output else {
                    return Response::Error {
                        message: "no focused output".into(),
                    };
                };
                let Some(out_state) = self.outputs.get_mut(&out_id) else {
                    return Response::Error {
                        message: "focused output not found".into(),
                    };
                };
                let Some(focused_id) = out_state.active().active else {
                    return Response::Error {
                        message: "no focused window".into(),
                    };
                };
                if out_state.active().id == target_id {
                    // Already on the target workspace — nothing to do.
                    return Response::Ok;
                }
                let Some(mut entry) = out_state.active_mut().remove(focused_id) else {
                    return Response::Error {
                        message: "failed to remove focused window".into(),
                    };
                };
                entry.workspace_id = target_id;
                if out_state.workspace(target_id).is_some() {
                    out_state.workspace_mut(target_id).unwrap().add(entry);
                } else {
                    // Lazy-create the target workspace without leaving it
                    // active; mirrors action_move_window_to_workspace.
                    let prev_active_id = out_state.active().id;
                    out_state.switch_to(target_id);
                    out_state.active_mut().add(entry);
                    out_state.switch_to(prev_active_id);
                }
                crate::shell::workspace::sync_active_workspaces_to_space(
                    &self.outputs,
                    &mut self.space,
                );
                Response::Ok
            }
            Request::FocusWindow { id } => {
                // WindowId has no `from_raw`; compare via raw() instead.
                for out_state in self.outputs.values_mut() {
                    let workspace = out_state.active_mut();
                    let target = workspace
                        .windows
                        .iter()
                        .find(|e| e.id.raw() == id)
                        .map(|e| e.id);
                    if let Some(target) = target {
                        workspace.focus(target, true);
                        crate::shell::workspace::sync_active_workspaces_to_space(
                            &self.outputs,
                            &mut self.space,
                        );
                        return Response::Ok;
                    }
                }
                Response::Error {
                    message: format!("window {} not found on any active workspace", id),
                }
            }
            Request::CloseFocused => {
                // Find focused window across all outputs and send close.
                let target = self.outputs.values().find_map(|os| {
                    let ws = os.active();
                    let active_id = ws.active?;
                    ws.windows
                        .iter()
                        .find(|e| e.id == active_id)
                        .map(|e| e.element.clone())
                });
                if let Some(elem) = target {
                    match elem.0.underlying_surface() {
                        smithay::desktop::WindowSurface::Wayland(w) => {
                            w.send_close();
                            return Response::Ok;
                        }
                        #[cfg(feature = "xwayland")]
                        smithay::desktop::WindowSurface::X11(w) => {
                            let _ = w.close();
                            return Response::Ok;
                        }
                    }
                }
                Response::Error {
                    message: "no focused window".into(),
                }
            }
            Request::Version => Response::Version {
                ipc: crate::ipc::PROTOCOL_VERSION.into(),
                build: env!("CARGO_PKG_VERSION").into(),
            },
            Request::Quit => Response::Ok,
        }
    }
}
