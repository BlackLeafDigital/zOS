use std::{process::Command, sync::atomic::Ordering};

use crate::{
    AnvilState,
    binds::{Action, BindKey, Direction, KeyCombo, Modifiers},
    focus::PointerFocusTarget,
    shell::{
        FullscreenSurface, PointerMoveSurfaceGrab, PointerResizeSurfaceGrab, ResizeData, ResizeState,
        SurfaceData, WindowId, WorkspaceId, edges_for_pointer, output_state::OutputId,
        workspace::sync_active_workspaces_to_space,
    },
};

#[cfg(feature = "udev")]
use crate::udev::UdevData;
use smithay::backend::renderer::DebugFlags;

use smithay::{
    backend::input::{
        self, Axis, AxisSource, Event, InputBackend, InputEvent, KeyState, KeyboardKeyEvent,
        PointerAxisEvent, PointerButtonEvent,
    },
    desktop::{WindowSurfaceType, layer_map_for_output},
    input::{
        keyboard::{FilterResult, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    output::Scale,
    reexports::wayland_server::protocol::wl_pointer,
    utils::{Logical, Point, SERIAL_COUNTER as SCOUNTER, Serial, Transform},
    wayland::{
        input_method::InputMethodSeat,
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitorSeat,
        shell::wlr_layer::{KeyboardInteractivity, Layer as WlrLayer},
    },
};

#[cfg(any(feature = "winit", feature = "x11", feature = "udev"))]
use smithay::backend::input::AbsolutePositionEvent;

#[cfg(any(feature = "winit", feature = "x11"))]
use smithay::output::Output;
use tracing::{debug, error, info, warn};

use crate::state::Backend;
#[cfg(feature = "udev")]
use smithay::{
    backend::input::{
        Device, DeviceCapability, GestureBeginEvent, GestureEndEvent, GesturePinchUpdateEvent as _,
        GestureSwipeUpdateEvent as _, PointerMotionEvent, ProximityState, TabletToolButtonEvent,
        TabletToolEvent, TabletToolProximityEvent, TabletToolTipEvent, TabletToolTipState, TouchEvent,
    },
    input::{
        pointer::{
            GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent,
            GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent,
            RelativeMotionEvent,
        },
        touch::{DownEvent, UpEvent},
    },
    reexports::wayland_server::DisplayHandle,
    wayland::{
        pointer_constraints::{PointerConstraint, with_pointer_constraint},
        seat::WaylandFocus,
        tablet_manager::{TabletDescriptor, TabletSeatTrait},
    },
};

/// Translate smithay's `ModifiersState` into our compositor-relevant
/// `Modifiers` bitflag set. Caps-lock / num-lock are deliberately ignored;
/// they're keyboard state, not bindable mods.
fn current_modifiers(state: &ModifiersState) -> Modifiers {
    let mut m = Modifiers::empty();
    if state.shift {
        m |= Modifiers::SHIFT;
    }
    if state.ctrl {
        m |= Modifiers::CTRL;
    }
    if state.alt {
        m |= Modifiers::ALT;
    }
    if state.logo {
        m |= Modifiers::SUPER;
    }
    if state.iso_level3_shift {
        m |= Modifiers::ALTGR;
    }
    m
}

impl<BackendData: Backend> AnvilState<BackendData> {
    /// Decode a keyboard event into an `Action` (if any matches the bind
    /// table) and update the suppression set so the matching release is
    /// silently swallowed instead of leaking to the focused client.
    ///
    /// Returns `Some(action)` only on the *press* of a bound combo. Releases
    /// always return `None` — they're either swallowed (when the keycode is
    /// in `suppressed_keycodes`) or forwarded to the client by smithay's
    /// keyboard machinery.
    fn keyboard_key_to_action<B: InputBackend>(&mut self, evt: B::KeyboardKeyEvent) -> Option<Action> {
        let keycode = evt.key_code();
        let state = evt.state();
        debug!(?keycode, ?state, "key");
        let serial = SCOUNTER.next_serial();
        let time = Event::time_msec(&evt);
        let keyboard = self.seat.get_keyboard().unwrap();

        // Exclusive layer surface (Top/Overlay) steals the keyboard. We
        // don't run any binding lookup in that case; the layer client owns
        // the keyboard until it releases interactivity.
        for layer in self.layer_shell_state.layer_surfaces().rev() {
            let exclusive = layer.with_cached_state(|data| {
                data.keyboard_interactivity == KeyboardInteractivity::Exclusive
                    && (data.layer == WlrLayer::Top || data.layer == WlrLayer::Overlay)
            });
            if exclusive {
                let surface = self.space.outputs().find_map(|o| {
                    let map = layer_map_for_output(o);
                    map.layers().find(|l| l.layer_surface() == &layer).cloned()
                });
                if let Some(surface) = surface {
                    keyboard.set_focus(self, Some(surface.into()), serial);
                    keyboard.input::<(), _>(self, keycode, state, serial, time, |_, _, _| {
                        FilterResult::Forward
                    });
                    return None;
                };
            }
        }

        // If the focused surface holds a keyboard-shortcut inhibitor, skip
        // compositor binds entirely so the client sees the raw key.
        let inhibited = self
            .space
            .element_under(self.pointer.current_location())
            .and_then(|(window, _)| {
                let surface = window.wl_surface()?;
                self.seat.keyboard_shortcuts_inhibitor_for_surface(&surface)
            })
            .map(|inhibitor| inhibitor.is_active())
            .unwrap_or(false);

        keyboard.input(self, keycode, state, serial, time, |data, modifiers, handle| {
            let keysym = handle.modified_sym();
            debug!(
                ?state,
                mods = ?modifiers,
                keysym = ::xkbcommon::xkb::keysym_get_name(keysym),
                "keysym"
            );

            match state {
                KeyState::Pressed => {
                    if inhibited {
                        return FilterResult::Forward;
                    }
                    let combo = KeyCombo::keysym(current_modifiers(modifiers), keysym);
                    if let Some(action) = data.bindings.get(&combo).cloned() {
                        // Remember the keycode so the matching release is
                        // suppressed; otherwise the client would see a
                        // dangling release for a press it never received.
                        data.suppressed_keycodes.insert(keycode);
                        FilterResult::Intercept(action)
                    } else {
                        FilterResult::Forward
                    }
                }
                KeyState::Released => {
                    if data.suppressed_keycodes.remove(&keycode) {
                        // Release of a previously-suppressed press: swallow.
                        // We use `Action::Nop` as the "intercepted but no
                        // dispatch needed" sentinel; the call site filters
                        // it out before handing off to `dispatch_action`.
                        FilterResult::Intercept(Action::Nop)
                    } else {
                        FilterResult::Forward
                    }
                }
            }
        })
    }

    /// Single dispatcher for compositor `Action`s. Most arms are stubs that
    /// log a `warn!` until the relevant subsystem (workspace, grabs, focus
    /// machinery) lands; the arms preserved verbatim from the old
    /// `KeyAction` are: Quit, Spawn, ScaleUp/Down, RotateOutput, ToggleTint,
    /// TogglePreview, Screen, VtSwitch.
    pub fn dispatch_action(&mut self, action: Action) {
        match action {
            Action::Nop => {}

            Action::Quit => {
                info!("Quitting.");
                self.running.store(false, Ordering::SeqCst);
            }

            Action::Spawn(argv) => {
                if argv.is_empty() {
                    warn!("Action::Spawn invoked with empty argv");
                    return;
                }
                info!(cmd = ?argv, "Spawning program");

                let mut cmd = Command::new(&argv[0]);
                cmd.args(&argv[1..]);
                cmd.envs(
                    self.socket_name
                        .clone()
                        .map(|v| ("WAYLAND_DISPLAY", v))
                        .into_iter()
                        .chain(
                            #[cfg(feature = "xwayland")]
                            self.xdisplay.map(|v| ("DISPLAY", format!(":{v}"))),
                            #[cfg(not(feature = "xwayland"))]
                            None,
                        ),
                );
                if let Err(e) = cmd.spawn() {
                    error!(cmd = ?argv, err = %e, "Failed to start program");
                }
            }

            Action::VtSwitch(vt) => {
                self.dispatch_vt_switch(vt);
            }

            Action::ScaleUp => self.dispatch_scale_delta(0.25),
            Action::ScaleDown => self.dispatch_scale_delta(-0.25),
            Action::RotateOutput => self.dispatch_rotate_output(),
            Action::ToggleTint => self.dispatch_toggle_tint(),

            Action::TogglePreview => {
                self.show_window_preview = !self.show_window_preview;
            }

            Action::Screen(num) => {
                let geometry = self
                    .space
                    .outputs()
                    .nth(num)
                    .map(|o| self.space.output_geometry(o).unwrap());

                if let Some(geometry) = geometry {
                    let x = geometry.loc.x as f64 + geometry.size.w as f64 / 2.0;
                    let y = geometry.size.h as f64 / 2.0;
                    let location = (x, y).into();
                    let pointer = self.pointer.clone();
                    let under = self.surface_under(location);
                    pointer.motion(
                        self,
                        under,
                        &MotionEvent {
                            location,
                            serial: SCOUNTER.next_serial(),
                            time: self.clock.now().as_millis(),
                        },
                    );
                    pointer.frame(self);
                }
            }

            Action::CloseWindow => {
                self.action_close_window();
            }

            Action::SwitchToWorkspace(n) => {
                self.action_switch_to_workspace(n);
            }

            Action::MoveWindowToWorkspace(n) => {
                self.action_move_window_to_workspace(n);
            }

            Action::ToggleFloating => {
                self.action_toggle_floating();
            }

            Action::ToggleFullscreen => {
                self.action_toggle_fullscreen();
            }

            Action::ToggleMaximize => {
                self.action_toggle_maximize();
            }

            Action::BeginMove => {
                self.action_begin_move();
            }

            Action::BeginResize => {
                self.action_begin_resize();
            }

            Action::FocusNext => {
                self.action_focus_history_step(true);
            }

            Action::FocusPrev => {
                self.action_focus_history_step(false);
            }

            Action::FocusDirection(dir) => {
                self.action_focus_direction(dir);
            }

            Action::MoveWindow(dir) => {
                self.action_move_window(dir);
            }

            Action::ToggleWorkspaceTiling => {
                self.action_toggle_workspace_tiling();
            }
        }
    }

    // ------------------------------------------------------------------
    // Action handlers — broken out as helpers so dispatch_action stays
    // readable and so each unit can be exercised in isolation.
    // ------------------------------------------------------------------

    /// Resolve the focused window: `(window_id, output_id)` of the
    /// active window on the focused output's active workspace, if any.
    fn focused_output_window(&self) -> Option<(WindowId, OutputId)> {
        let out_id = self.focused_output?;
        let out_state = self.outputs.get(&out_id)?;
        let win_id = out_state.active().active?;
        Some((win_id, out_id))
    }

    fn action_close_window(&mut self) {
        let Some((focused_id, _)) = self.focused_output_window() else {
            debug!("CloseWindow: no focused window");
            return;
        };
        let Some(elem) = self
            .space
            .elements()
            .find(|e| e.id() == focused_id)
            .cloned()
        else {
            debug!("CloseWindow: focused window id not in space");
            return;
        };
        match elem.0.underlying_surface() {
            smithay::desktop::WindowSurface::Wayland(w) => w.send_close(),
            #[cfg(feature = "xwayland")]
            smithay::desktop::WindowSurface::X11(w) => {
                let _ = w.close();
            }
        }
    }

    fn action_switch_to_workspace(&mut self, n: u32) {
        let target_id = WorkspaceId(n);
        let Some(out_id) = self.focused_output else {
            debug!("SwitchToWorkspace: no focused output");
            return;
        };
        let Some(out_state) = self.outputs.get_mut(&out_id) else {
            return;
        };

        // Snapshot output width for the off-screen animation distance.
        let output_width = out_state
            .output
            .current_mode()
            .map(|m| m.size.w as f64)
            .unwrap_or(1920.0);

        // Get current active workspace id BEFORE switching.
        let prev_id = out_state.active().id;
        if prev_id == target_id {
            return;
        }

        // Snapshot AnimationProperty BEFORE further mutation; the animation
        // config is shared between the outgoing and incoming legs and we want
        // identical curve/duration on both.
        let workspaces_anim = self.animation_manager.workspaces.clone();
        let global_enabled = self.animation_manager.global_enabled;

        // Slide direction: forward switches push the outgoing workspace left
        // and bring the incoming in from the right; reverse for backward.
        let going_forward = target_id.0 > prev_id.0;
        let outgoing_target_x: f64 = if going_forward { -output_width } else { output_width };
        let incoming_start_x: f64 = if going_forward { output_width } else { -output_width };

        // Animate the outgoing workspace OUT.
        if workspaces_anim.enabled && global_enabled {
            if let Some(prev_ws) = out_state.workspace_mut(prev_id) {
                prev_ws.render_offset.warp_to((0.0, 0.0).into());
                prev_ws.render_offset.animate_to(
                    (outgoing_target_x, 0.0).into(),
                    workspaces_anim.curve.clone(),
                    workspaces_anim.duration(),
                );
            }
        }

        // Switch active workspace (lazy-creates target if needed).
        out_state.switch_to(target_id);

        // Animate the incoming workspace IN.
        if workspaces_anim.enabled && global_enabled {
            if let Some(new_ws) = out_state.workspace_mut(target_id) {
                new_ws.render_offset.warp_to((incoming_start_x, 0.0).into());
                new_ws.render_offset.animate_to(
                    (0.0, 0.0).into(),
                    workspaces_anim.curve.clone(),
                    workspaces_anim.duration(),
                );
            }
        }

        sync_active_workspaces_to_space(&self.outputs, &mut self.space);
    }

    fn action_move_window_to_workspace(&mut self, n: u32) {
        let target = WorkspaceId(n);
        let Some(out_id) = self.focused_output else {
            debug!("MoveWindowToWorkspace: no focused output");
            return;
        };
        let Some(out_state) = self.outputs.get_mut(&out_id) else {
            return;
        };
        let Some(focused_id) = out_state.active().active else {
            debug!("MoveWindowToWorkspace: no focused window");
            return;
        };
        if out_state.active().id == target {
            // Already on the target workspace.
            return;
        }

        let Some(mut entry) = out_state.active_mut().remove(focused_id) else {
            return;
        };
        entry.workspace_id = target;

        if out_state.workspace(target).is_some() {
            out_state.workspace_mut(target).unwrap().add(entry);
        } else {
            // Lazy-create the target without leaving it active.
            let prev_active_id = out_state.active().id;
            out_state.switch_to(target);
            out_state.active_mut().add(entry);
            out_state.switch_to(prev_active_id);
        }

        sync_active_workspaces_to_space(&self.outputs, &mut self.space);
    }

    fn action_toggle_floating(&mut self) {
        let Some((focused_id, _)) = self.focused_output_window() else {
            debug!("ToggleFloating: no focused window");
            return;
        };
        let Some(elem) = self.space.elements().find(|e| e.id() == focused_id) else {
            return;
        };
        let mut guard = elem.layout_state().tiled_override.lock().unwrap();
        *guard = match *guard {
            None => Some(false),
            Some(false) => Some(true),
            Some(true) => None,
        };
    }

    fn action_toggle_fullscreen(&mut self) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
        let Some((focused_id, _)) = self.focused_output_window() else {
            debug!("ToggleFullscreen: no focused window");
            return;
        };
        let Some(elem) = self
            .space
            .elements()
            .find(|e| e.id() == focused_id)
            .cloned()
        else {
            return;
        };
        if let Some(toplevel) = elem.0.toplevel() {
            toplevel.with_pending_state(|s| {
                if s.states.contains(xdg_toplevel::State::Fullscreen) {
                    s.states.unset(xdg_toplevel::State::Fullscreen);
                    s.size = None;
                } else {
                    s.states.set(xdg_toplevel::State::Fullscreen);
                }
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
    }

    fn action_toggle_maximize(&mut self) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
        let Some((focused_id, _)) = self.focused_output_window() else {
            debug!("ToggleMaximize: no focused window");
            return;
        };
        let Some(elem) = self
            .space
            .elements()
            .find(|e| e.id() == focused_id)
            .cloned()
        else {
            return;
        };
        if let Some(toplevel) = elem.0.toplevel() {
            toplevel.with_pending_state(|s| {
                if s.states.contains(xdg_toplevel::State::Maximized) {
                    s.states.unset(xdg_toplevel::State::Maximized);
                    s.size = None;
                } else {
                    s.states.set(xdg_toplevel::State::Maximized);
                }
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
    }

    fn action_begin_move(&mut self) {
        let pointer = self.pointer.clone();
        let pointer_loc = pointer.current_location();
        let Some((elem, win_loc)) = self
            .space
            .element_under(pointer_loc)
            .map(|(e, l)| (e.clone(), l))
        else {
            debug!("BeginMove: no window under pointer");
            return;
        };
        let win_id = elem.id();
        let serial = SCOUNTER.next_serial();
        let start_data = smithay::input::pointer::GrabStartData {
            focus: None,
            button: 0x110, // BTN_LEFT
            location: pointer_loc,
        };
        let grab: PointerMoveSurfaceGrab<BackendData> =
            PointerMoveSurfaceGrab::new_from_id(start_data, win_id, win_loc);
        pointer.set_grab(self, grab, serial, smithay::input::pointer::Focus::Clear);
    }

    fn action_begin_resize(&mut self) {
        use std::cell::RefCell;
        use smithay::wayland::compositor::with_states;
        let pointer = self.pointer.clone();
        let pointer_loc = pointer.current_location();
        let Some((elem, win_loc)) = self
            .space
            .element_under(pointer_loc)
            .map(|(e, l)| (e.clone(), l))
        else {
            debug!("BeginResize: no window under pointer");
            return;
        };
        let geometry = smithay::desktop::space::SpaceElement::geometry(&elem);
        let initial_window_size = geometry.size;
        let win_rect = smithay::utils::Rectangle::new(win_loc, geometry.size);
        let edges = edges_for_pointer(win_rect, pointer_loc);
        if edges.is_empty() {
            return;
        }

        // Record the per-surface resize state so commit handlers can
        // finish out the resize after the grab releases.
        if let Some(surface) = elem.wl_surface() {
            with_states(&surface, |states| {
                if let Some(data) = states.data_map.get::<RefCell<SurfaceData>>() {
                    data.borrow_mut().resize_state = ResizeState::Resizing(ResizeData {
                        edges,
                        initial_window_location: win_loc,
                        initial_window_size,
                    });
                }
            });
        }

        let serial = SCOUNTER.next_serial();
        let start_data = smithay::input::pointer::GrabStartData {
            focus: None,
            button: 0x111, // BTN_RIGHT
            location: pointer_loc,
        };
        let grab: PointerResizeSurfaceGrab<BackendData> = PointerResizeSurfaceGrab {
            start_data,
            window: elem,
            edges,
            initial_window_location: win_loc,
            initial_window_size,
            last_window_size: initial_window_size,
        };
        pointer.set_grab(self, grab, serial, smithay::input::pointer::Focus::Clear);
    }

    /// MRU-history step. `forward = true` corresponds to FocusNext (one
    /// step back in time, the previously-focused window); `forward =
    /// false` is FocusPrev. With only the focus stack to go on, both
    /// reduce to "the entry directly before the current top of stack".
    ///
    /// TODO(phase-3): track an Alt-Tab cursor across multiple invocations
    /// so repeated FocusNext walks deeper into history. The current
    /// implementation flips between the top two entries, which matches
    /// the behaviour most users expect from a single press.
    fn action_focus_history_step(&mut self, forward: bool) {
        let _ = forward; // both directions resolve to the same target today
        let Some(out_id) = self.focused_output else {
            return;
        };
        let Some(out_state) = self.outputs.get_mut(&out_id) else {
            return;
        };
        let history = &out_state.active().focus_history;
        let target = if history.len() >= 2 {
            history[history.len() - 2]
        } else {
            return;
        };
        out_state.active_mut().focus(target, true);
        sync_active_workspaces_to_space(&self.outputs, &mut self.space);
        self.update_keyboard_focus_to_window(target);
    }

    fn action_focus_direction(&mut self, dir: Direction) {
        let Some(out_id) = self.focused_output else {
            return;
        };
        // Snapshot focused window's rect.
        let (focused_id, focus_rect) = {
            let Some(out_state) = self.outputs.get(&out_id) else {
                return;
            };
            let Some(focused_id) = out_state.active().active else {
                return;
            };
            let Some(focused_elem) = self.space.elements().find(|e| e.id() == focused_id) else {
                return;
            };
            let loc = self
                .space
                .element_location(focused_elem)
                .unwrap_or_default();
            let geo = smithay::desktop::space::SpaceElement::geometry(focused_elem);
            (focused_id, smithay::utils::Rectangle::new(loc, geo.size))
        };

        // Pick the closest candidate strictly in `dir` from the focused
        // window's centre. Distance is squared euclidean distance
        // between centres; ties resolve by stable iteration order.
        let focus_centre = (
            focus_rect.loc.x as f64 + focus_rect.size.w as f64 / 2.0,
            focus_rect.loc.y as f64 + focus_rect.size.h as f64 / 2.0,
        );
        let mut best: Option<(i64, WindowId)> = None;
        let active_ids: std::collections::HashSet<WindowId> = self
            .outputs
            .get(&out_id)
            .map(|os| os.active().windows.iter().map(|e| e.id).collect())
            .unwrap_or_default();
        for elem in self.space.elements() {
            let id = elem.id();
            if id == focused_id {
                continue;
            }
            if !active_ids.contains(&id) {
                continue;
            }
            let loc = self.space.element_location(elem).unwrap_or_default();
            let geo = smithay::desktop::space::SpaceElement::geometry(elem);
            let cand_centre = (
                loc.x as f64 + geo.size.w as f64 / 2.0,
                loc.y as f64 + geo.size.h as f64 / 2.0,
            );
            let dx = cand_centre.0 - focus_centre.0;
            let dy = cand_centre.1 - focus_centre.1;
            let in_dir = match dir {
                Direction::Left => dx < -1.0 && dx.abs() >= dy.abs(),
                Direction::Right => dx > 1.0 && dx.abs() >= dy.abs(),
                Direction::Up => dy < -1.0 && dy.abs() >= dx.abs(),
                Direction::Down => dy > 1.0 && dy.abs() >= dx.abs(),
            };
            if !in_dir {
                continue;
            }
            let dist2 = (dx * dx + dy * dy) as i64;
            if best.map(|(d, _)| dist2 < d).unwrap_or(true) {
                best = Some((dist2, id));
            }
        }

        let Some((_, target)) = best else {
            return;
        };
        if let Some(out_state) = self.outputs.get_mut(&out_id) {
            out_state.active_mut().focus(target, true);
        }
        sync_active_workspaces_to_space(&self.outputs, &mut self.space);
        self.update_keyboard_focus_to_window(target);
    }

    fn action_move_window(&mut self, dir: Direction) {
        const STEP: i32 = 50;
        let Some((focused_id, out_id)) = self.focused_output_window() else {
            return;
        };
        let delta: smithay::utils::Point<i32, smithay::utils::Logical> = match dir {
            Direction::Left => (-STEP, 0).into(),
            Direction::Right => (STEP, 0).into(),
            Direction::Up => (0, -STEP).into(),
            Direction::Down => (0, STEP).into(),
        };

        // Update the workspace entry's location in-place, then re-sync
        // the live Space so the on-screen position follows.
        if let Some(out_state) = self.outputs.get_mut(&out_id) {
            if let Some(entry) = out_state.active_mut().find_mut(focused_id) {
                entry.location += delta;
            }
        }
        sync_active_workspaces_to_space(&self.outputs, &mut self.space);
    }

    fn action_toggle_workspace_tiling(&mut self) {
        let Some(out_id) = self.focused_output else {
            return;
        };
        let Some(out_state) = self.outputs.get_mut(&out_id) else {
            return;
        };

        // Snapshot the output's current mode size BEFORE taking the active
        // workspace mut borrow, so we don't double-borrow.
        let work_area = out_state
            .output
            .current_mode()
            .map(|m| {
                smithay::utils::Rectangle::new(
                    smithay::utils::Point::from((0, 0)),
                    m.size.to_logical(1),
                )
            })
            .unwrap_or_default();

        let workspace = out_state.active_mut();
        if workspace.is_tiled() {
            workspace.switch_to_floating();
            tracing::info!(workspace_id = workspace.id.0, "switched to floating mode");
        } else {
            let algorithm = Box::new(crate::shell::tiling::dwindle::DwindleTree::new(work_area));
            workspace.switch_to_tiled(algorithm);
            tracing::info!(workspace_id = workspace.id.0, "switched to tiled mode");
        }

        // Push the workspace's new entry locations to the live Space so the
        // render path picks them up on the next frame.
        crate::shell::workspace::sync_active_workspaces_to_space(&self.outputs, &mut self.space);
    }

    /// Forward a workspace-driven focus change to the wl_keyboard so
    /// clients see the focus event. Used by FocusNext/FocusPrev/
    /// FocusDirection where we change the active window without a
    /// pointer click.
    fn update_keyboard_focus_to_window(&mut self, id: WindowId) {
        let Some(elem) = self.space.elements().find(|e| e.id() == id).cloned() else {
            return;
        };
        let serial = SCOUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        if !keyboard.is_grabbed() {
            keyboard.set_focus(self, Some(elem.into()), serial);
        }
    }

    /// VT switching. Default `Backend::change_vt` is a no-op; UdevData
    /// overrides it to drive `LibSeatSession::change_vt`. Single code
    /// path, no cfg gates needed.
    fn dispatch_vt_switch(&mut self, vt: i32) {
        info!(to = vt, "Trying to switch vt");
        if let Err(err) = self.backend_data.change_vt(vt) {
            error!(vt, "Error switching vt: {}", err);
        }
    }

    /// Output-tint debug toggle (anvil dev knob). Routes through the
    /// `Backend::debug_flags` / `set_debug_flags` trait methods — winit
    /// defaults to no-op, udev forwards to `DrmCompositor::set_debug_flags`.
    fn dispatch_toggle_tint(&mut self) {
        let mut debug_flags = self.backend_data.debug_flags();
        debug_flags.toggle(DebugFlags::TINT);
        self.backend_data.set_debug_flags(debug_flags);
    }

    /// Adjust the fractional scale of the output under the pointer by
    /// `delta` (clamped to 1.0 minimum). Pointer is re-anchored on the
    /// rescaled output so it doesn't fly off-screen.
    fn dispatch_scale_delta(&mut self, delta: f64) {
        let pos = self.pointer.current_location().to_i32_round();
        let output = self
            .space
            .outputs()
            .find(|o| self.space.output_geometry(o).unwrap().contains(pos))
            .cloned();

        let Some(output) = output else {
            debug!("Scale change requested but pointer is on no output");
            return;
        };

        let (output_location, scale) = (
            self.space.output_geometry(&output).unwrap().loc,
            output.current_scale().fractional_scale(),
        );
        let new_scale = f64::max(1.0, scale + delta);
        if (new_scale - scale).abs() < f64::EPSILON {
            return;
        }
        output.change_current_state(None, None, Some(Scale::Fractional(new_scale)), None);

        let rescale = scale / new_scale;
        let output_location = output_location.to_f64();
        let mut pointer_output_location = self.pointer.current_location() - output_location;
        pointer_output_location.x *= rescale;
        pointer_output_location.y *= rescale;
        let pointer_location = output_location + pointer_output_location;

        crate::shell::fixup_positions(&mut self.space, pointer_location);
        let pointer = self.pointer.clone();
        let under = self.surface_under(pointer_location);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pointer_location,
                serial: SCOUNTER.next_serial(),
                time: self.clock.now().as_millis(),
            },
        );
        pointer.frame(self);
        self.backend_data.reset_buffers(&output);
    }

    /// Cycle through the eight `Transform` orientations on the output under
    /// the pointer. Anvil dev knob, preserved as-is.
    fn dispatch_rotate_output(&mut self) {
        let pos = self.pointer.current_location().to_i32_round();
        let output = self
            .space
            .outputs()
            .find(|o| self.space.output_geometry(o).unwrap().contains(pos))
            .cloned();

        let Some(output) = output else {
            debug!("RotateOutput requested but pointer is on no output");
            return;
        };

        let current_transform = output.current_transform();
        let new_transform = match current_transform {
            Transform::Normal => Transform::_90,
            Transform::_90 => Transform::_180,
            Transform::_180 => Transform::_270,
            Transform::_270 => Transform::Flipped,
            Transform::Flipped => Transform::Flipped90,
            Transform::Flipped90 => Transform::Flipped180,
            Transform::Flipped180 => Transform::Flipped270,
            Transform::Flipped270 => Transform::Normal,
        };
        info!(?current_transform, ?new_transform, output = ?output.name(), "changing output transform");
        output.change_current_state(None, Some(new_transform), None, None);
        crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
        self.backend_data.reset_buffers(&output);
    }

    fn on_pointer_button<B: InputBackend>(&mut self, evt: B::PointerButtonEvent) {
        let serial = SCOUNTER.next_serial();
        let button = evt.button_code();
        let state = wl_pointer::ButtonState::from(evt.state());

        // Bind lookup: if the press matches a (Modifiers, MouseButton) combo,
        // suppress it from the surface and dispatch the action instead.
        // Releases of previously-suppressed buttons are swallowed so the
        // client never sees an orphan release.
        if let Some(keyboard) = self.seat.get_keyboard() {
            let modifiers = current_modifiers(&keyboard.modifier_state());
            let combo = KeyCombo {
                modifiers,
                key: BindKey::MouseButton(button),
            };

            match state {
                wl_pointer::ButtonState::Pressed => {
                    if let Some(action) = self.bindings.get(&combo).cloned() {
                        self.suppressed_buttons.insert(button);
                        self.dispatch_action(action);
                        return;
                    }
                }
                wl_pointer::ButtonState::Released => {
                    if self.suppressed_buttons.remove(&button) {
                        return;
                    }
                }
                _ => {}
            }
        }

        if wl_pointer::ButtonState::Pressed == state {
            // Click-to-focus at the workspace level: if the press lands
            // on a tracked window, move it to the top of the active
            // workspace's focus history and raise it. This runs *after*
            // the bind-table lookup (so SUPER+LMB still triggers
            // BeginMove without flipping focus) but *before* the
            // surface-forward path so the client sees the click against
            // the now-focused window.
            if !self.pointer.is_grabbed() {
                let pointer_loc = self.pointer.current_location();
                if let Some((elem, _)) = self
                    .space
                    .element_under(pointer_loc)
                    .map(|(e, l)| (e.clone(), l))
                {
                    let win_id = elem.id();
                    if let Some(out_id) = self.focused_output {
                        if let Some(out_state) = self.outputs.get_mut(&out_id) {
                            if out_state.active().find(win_id).is_some() {
                                out_state.active_mut().focus(win_id, true);
                            }
                        }
                        sync_active_workspaces_to_space(&self.outputs, &mut self.space);
                    }
                }
            }
            self.update_keyboard_focus(self.pointer.current_location(), serial);
        };
        let pointer = self.pointer.clone();
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state: state.try_into().unwrap(),
                serial,
                time: evt.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn update_keyboard_focus(&mut self, location: Point<f64, Logical>, serial: Serial) {
        let keyboard = self.seat.get_keyboard().unwrap();
        let touch = self.seat.get_touch();
        let input_method = self.seat.input_method();
        // change the keyboard focus unless the pointer or keyboard is grabbed
        // We test for any matching surface type here but always use the root
        // (in case of a window the toplevel) surface for the focus.
        // So for example if a user clicks on a subsurface or popup the toplevel
        // will receive the keyboard focus. Directly assigning the focus to the
        // matching surface leads to issues with clients dismissing popups and
        // subsurface menus (for example firefox-wayland).
        // see here for a discussion about that issue:
        // https://gitlab.freedesktop.org/wayland/wayland/-/issues/294
        if !self.pointer.is_grabbed()
            && (!keyboard.is_grabbed() || input_method.keyboard_grabbed())
            && !touch.map(|touch| touch.is_grabbed()).unwrap_or(false)
        {
            let output = self.space.output_under(location).next().cloned();
            if let Some(output) = output.as_ref() {
                let output_geo = self.space.output_geometry(output).unwrap();
                if let Some(window) = output
                    .user_data()
                    .get::<FullscreenSurface>()
                    .and_then(|f| f.get())
                {
                    if let Some((_, _)) =
                        window.surface_under(location - output_geo.loc.to_f64(), WindowSurfaceType::ALL)
                    {
                        #[cfg(feature = "xwayland")]
                        if let Some(surface) = window.0.x11_surface() {
                            self.xwm.as_mut().unwrap().raise_window(surface).unwrap();
                        }
                        keyboard.set_focus(self, Some(window.into()), serial);
                        return;
                    }
                }

                let layers = layer_map_for_output(output);
                if let Some(layer) = layers
                    .layer_under(WlrLayer::Overlay, location - output_geo.loc.to_f64())
                    .or_else(|| layers.layer_under(WlrLayer::Top, location - output_geo.loc.to_f64()))
                {
                    if layer.can_receive_keyboard_focus() {
                        if let Some((_, _)) = layer.surface_under(
                            location
                                - output_geo.loc.to_f64()
                                - layers.layer_geometry(layer).unwrap().loc.to_f64(),
                            WindowSurfaceType::ALL,
                        ) {
                            keyboard.set_focus(self, Some(layer.clone().into()), serial);
                            return;
                        }
                    }
                }
            }

            if let Some((window, _)) = self.space.element_under(location).map(|(w, p)| (w.clone(), p)) {
                self.space.raise_element(&window, true);
                #[cfg(feature = "xwayland")]
                if let Some(surface) = window.0.x11_surface() {
                    self.xwm.as_mut().unwrap().raise_window(surface).unwrap();
                }
                keyboard.set_focus(self, Some(window.into()), serial);
                return;
            }

            if let Some(output) = output.as_ref() {
                let output_geo = self.space.output_geometry(output).unwrap();
                let layers = layer_map_for_output(output);
                if let Some(layer) = layers
                    .layer_under(WlrLayer::Bottom, location - output_geo.loc.to_f64())
                    .or_else(|| layers.layer_under(WlrLayer::Background, location - output_geo.loc.to_f64()))
                {
                    if layer.can_receive_keyboard_focus() {
                        if let Some((_, _)) = layer.surface_under(
                            location
                                - output_geo.loc.to_f64()
                                - layers.layer_geometry(layer).unwrap().loc.to_f64(),
                            WindowSurfaceType::ALL,
                        ) {
                            keyboard.set_focus(self, Some(layer.clone().into()), serial);
                        }
                    }
                }
            };
        }
    }

    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(PointerFocusTarget, Point<f64, Logical>)> {
        let output = self.space.outputs().find(|o| {
            let geometry = self.space.output_geometry(o).unwrap();
            geometry.contains(pos.to_i32_round())
        })?;
        let output_geo = self.space.output_geometry(output).unwrap();
        let layers = layer_map_for_output(output);

        let mut under = None;
        if let Some((surface, loc)) = output
            .user_data()
            .get::<FullscreenSurface>()
            .and_then(|f| f.get())
            .and_then(|w| w.surface_under(pos - output_geo.loc.to_f64(), WindowSurfaceType::ALL))
        {
            under = Some((surface, loc + output_geo.loc));
        } else if let Some(focus) = layers
            .layer_under(WlrLayer::Overlay, pos - output_geo.loc.to_f64())
            .or_else(|| layers.layer_under(WlrLayer::Top, pos - output_geo.loc.to_f64()))
            .and_then(|layer| {
                let layer_loc = layers.layer_geometry(layer).unwrap().loc;
                layer
                    .surface_under(
                        pos - output_geo.loc.to_f64() - layer_loc.to_f64(),
                        WindowSurfaceType::ALL,
                    )
                    .map(|(surface, loc)| {
                        (
                            PointerFocusTarget::from(surface),
                            loc + layer_loc + output_geo.loc,
                        )
                    })
            })
        {
            under = Some(focus)
        } else if let Some(focus) = self.space.element_under(pos).and_then(|(window, loc)| {
            window
                .surface_under(pos - loc.to_f64(), WindowSurfaceType::ALL)
                .map(|(surface, surf_loc)| (surface, surf_loc + loc))
        }) {
            under = Some(focus);
        } else if let Some(focus) = layers
            .layer_under(WlrLayer::Bottom, pos - output_geo.loc.to_f64())
            .or_else(|| layers.layer_under(WlrLayer::Background, pos - output_geo.loc.to_f64()))
            .and_then(|layer| {
                let layer_loc = layers.layer_geometry(layer).unwrap().loc;
                layer
                    .surface_under(
                        pos - output_geo.loc.to_f64() - layer_loc.to_f64(),
                        WindowSurfaceType::ALL,
                    )
                    .map(|(surface, loc)| {
                        (
                            PointerFocusTarget::from(surface),
                            loc + layer_loc + output_geo.loc,
                        )
                    })
            })
        {
            under = Some(focus)
        };
        under.map(|(s, l)| (s, l.to_f64()))
    }

    fn on_pointer_axis<B: InputBackend>(&mut self, evt: B::PointerAxisEvent) {
        let horizontal_amount = evt
            .amount(input::Axis::Horizontal)
            .unwrap_or_else(|| evt.amount_v120(input::Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
        let vertical_amount = evt
            .amount(input::Axis::Vertical)
            .unwrap_or_else(|| evt.amount_v120(input::Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);
        let horizontal_amount_discrete = evt.amount_v120(input::Axis::Horizontal);
        let vertical_amount_discrete = evt.amount_v120(input::Axis::Vertical);

        {
            let mut frame = AxisFrame::new(evt.time_msec()).source(evt.source());
            if horizontal_amount != 0.0 {
                frame = frame.relative_direction(Axis::Horizontal, evt.relative_direction(Axis::Horizontal));
                frame = frame.value(Axis::Horizontal, horizontal_amount);
                if let Some(discrete) = horizontal_amount_discrete {
                    frame = frame.v120(Axis::Horizontal, discrete as i32);
                }
            }
            if vertical_amount != 0.0 {
                frame = frame.relative_direction(Axis::Vertical, evt.relative_direction(Axis::Vertical));
                frame = frame.value(Axis::Vertical, vertical_amount);
                if let Some(discrete) = vertical_amount_discrete {
                    frame = frame.v120(Axis::Vertical, discrete as i32);
                }
            }
            if evt.source() == AxisSource::Finger {
                if evt.amount(Axis::Horizontal) == Some(0.0) {
                    frame = frame.stop(Axis::Horizontal);
                }
                if evt.amount(Axis::Vertical) == Some(0.0) {
                    frame = frame.stop(Axis::Vertical);
                }
            }
            let pointer = self.pointer.clone();
            pointer.axis(self, frame);
            pointer.frame(self);
        }
    }
}

#[cfg(any(feature = "winit", feature = "x11"))]
impl<BackendData: Backend> AnvilState<BackendData> {
    pub fn process_input_event_windowed<B: InputBackend>(&mut self, event: InputEvent<B>, output_name: &str) {
        match event {
            InputEvent::Keyboard { event } => {
                if let Some(action) = self.keyboard_key_to_action::<B>(event) {
                    if matches!(action, Action::Nop) {
                        return;
                    }
                    self.dispatch_action_windowed(action, output_name);
                }
            }

            InputEvent::PointerMotionAbsolute { event } => {
                let output = self
                    .space
                    .outputs()
                    .find(|o| o.name() == output_name)
                    .unwrap()
                    .clone();
                self.on_pointer_move_absolute_windowed::<B>(event, &output)
            }
            InputEvent::PointerButton { event } => self.on_pointer_button::<B>(event),
            InputEvent::PointerAxis { event } => self.on_pointer_axis::<B>(event),
            _ => (), // other events are not handled in anvil (yet)
        }
    }

    /// Windowed-backend-specific dispatcher: anvil debug knobs (ScaleUp,
    /// ScaleDown, RotateOutput) act on the *named* virtual output rather
    /// than the output under the pointer, since the windowed backends only
    /// have one output and the pointer-location lookup would still resolve
    /// to it. Everything else falls through to the generic dispatcher.
    fn dispatch_action_windowed(&mut self, action: Action, output_name: &str) {
        match action {
            Action::ScaleUp => {
                let output = self.space.outputs().find(|o| o.name() == output_name).cloned();
                if let Some(output) = output {
                    let current_scale = output.current_scale().fractional_scale();
                    let new_scale = current_scale + 0.25;
                    output.change_current_state(None, None, Some(Scale::Fractional(new_scale)), None);
                    crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
                    self.backend_data.reset_buffers(&output);
                }
            }
            Action::ScaleDown => {
                let output = self.space.outputs().find(|o| o.name() == output_name).cloned();
                if let Some(output) = output {
                    let current_scale = output.current_scale().fractional_scale();
                    let new_scale = f64::max(1.0, current_scale - 0.25);
                    output.change_current_state(None, None, Some(Scale::Fractional(new_scale)), None);
                    crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
                    self.backend_data.reset_buffers(&output);
                }
            }
            Action::RotateOutput => {
                let output = self.space.outputs().find(|o| o.name() == output_name).cloned();
                if let Some(output) = output {
                    let current_transform = output.current_transform();
                    let new_transform = match current_transform {
                        Transform::Normal => Transform::_90,
                        Transform::_90 => Transform::_180,
                        Transform::_180 => Transform::_270,
                        Transform::_270 => Transform::Flipped,
                        Transform::Flipped => Transform::Flipped90,
                        Transform::Flipped90 => Transform::Flipped180,
                        Transform::Flipped180 => Transform::Flipped270,
                        Transform::Flipped270 => Transform::Normal,
                    };
                    info!(?current_transform, ?new_transform, output = ?output.name(), "changing output transform");
                    output.change_current_state(None, Some(new_transform), None, None);
                    crate::shell::fixup_positions(&mut self.space, self.pointer.current_location());
                    self.backend_data.reset_buffers(&output);
                }
            }
            other => self.dispatch_action(other),
        }
    }

    fn on_pointer_move_absolute_windowed<B: InputBackend>(
        &mut self,
        evt: B::PointerMotionAbsoluteEvent,
        output: &Output,
    ) {
        let output_geo = self.space.output_geometry(output).unwrap();

        let pos = evt.position_transformed(output_geo.size) + output_geo.loc.to_f64();
        let serial = SCOUNTER.next_serial();

        let pointer = self.pointer.clone();
        let under = self.surface_under(pos);
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial,
                time: evt.time_msec(),
            },
        );
        pointer.frame(self);
    }

    pub fn release_all_keys(&mut self) {
        let keyboard = self.seat.get_keyboard().unwrap();
        for keycode in keyboard.pressed_keys() {
            keyboard.input(
                self,
                keycode,
                KeyState::Released,
                SCOUNTER.next_serial(),
                0,
                |_, _, _| FilterResult::Forward::<bool>,
            );
        }
    }
}

#[cfg(feature = "udev")]
impl AnvilState<UdevData> {
    pub fn process_input_event<B: InputBackend>(&mut self, dh: &DisplayHandle, event: InputEvent<B>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                if let Some(action) = self.keyboard_key_to_action::<B>(event) {
                    if !matches!(action, Action::Nop) {
                        self.dispatch_action(action);
                    }
                }
            }
            InputEvent::PointerMotion { event, .. } => self.on_pointer_move::<B>(dh, event),
            InputEvent::PointerMotionAbsolute { event, .. } => self.on_pointer_move_absolute::<B>(dh, event),
            InputEvent::PointerButton { event, .. } => self.on_pointer_button::<B>(event),
            InputEvent::PointerAxis { event, .. } => self.on_pointer_axis::<B>(event),
            InputEvent::TabletToolAxis { event, .. } => self.on_tablet_tool_axis::<B>(event),
            InputEvent::TabletToolProximity { event, .. } => self.on_tablet_tool_proximity::<B>(dh, event),
            InputEvent::TabletToolTip { event, .. } => self.on_tablet_tool_tip::<B>(event),
            InputEvent::TabletToolButton { event, .. } => self.on_tablet_button::<B>(event),
            InputEvent::GestureSwipeBegin { event, .. } => self.on_gesture_swipe_begin::<B>(event),
            InputEvent::GestureSwipeUpdate { event, .. } => self.on_gesture_swipe_update::<B>(event),
            InputEvent::GestureSwipeEnd { event, .. } => self.on_gesture_swipe_end::<B>(event),
            InputEvent::GesturePinchBegin { event, .. } => self.on_gesture_pinch_begin::<B>(event),
            InputEvent::GesturePinchUpdate { event, .. } => self.on_gesture_pinch_update::<B>(event),
            InputEvent::GesturePinchEnd { event, .. } => self.on_gesture_pinch_end::<B>(event),
            InputEvent::GestureHoldBegin { event, .. } => self.on_gesture_hold_begin::<B>(event),
            InputEvent::GestureHoldEnd { event, .. } => self.on_gesture_hold_end::<B>(event),

            InputEvent::TouchDown { event } => self.on_touch_down::<B>(event),
            InputEvent::TouchUp { event } => self.on_touch_up::<B>(event),
            InputEvent::TouchMotion { event } => self.on_touch_motion::<B>(event),
            InputEvent::TouchFrame { event } => self.on_touch_frame::<B>(event),
            InputEvent::TouchCancel { event } => self.on_touch_cancel::<B>(event),

            InputEvent::DeviceAdded { device } => {
                if device.has_capability(DeviceCapability::TabletTool) {
                    self.seat
                        .tablet_seat()
                        .add_tablet::<Self>(dh, &TabletDescriptor::from(&device));
                }
                if device.has_capability(DeviceCapability::Touch) && self.seat.get_touch().is_none() {
                    self.seat.add_touch();
                }
            }
            InputEvent::DeviceRemoved { device } => {
                if device.has_capability(DeviceCapability::TabletTool) {
                    let tablet_seat = self.seat.tablet_seat();

                    tablet_seat.remove_tablet(&TabletDescriptor::from(&device));

                    // If there are no tablets in seat we can remove all tools
                    if tablet_seat.count_tablets() == 0 {
                        tablet_seat.clear_tools();
                    }
                }
            }
            _ => {
                // other events are not handled in anvil (yet)
            }
        }
    }

    fn on_pointer_move<B: InputBackend>(&mut self, _dh: &DisplayHandle, evt: B::PointerMotionEvent) {
        let mut pointer_location = self.pointer.current_location();
        let serial = SCOUNTER.next_serial();

        let pointer = self.pointer.clone();
        let under = self.surface_under(pointer_location);

        let mut pointer_locked = false;
        let mut pointer_confined = false;
        let mut confine_region = None;
        if let Some((surface, surface_loc)) = under
            .as_ref()
            .and_then(|(target, l)| Some((target.wl_surface()?, l)))
        {
            with_pointer_constraint(&surface, &pointer, |constraint| match constraint {
                Some(constraint) if constraint.is_active() => {
                    // Constraint does not apply if not within region
                    if !constraint
                        .region()
                        .is_none_or(|x| x.contains((pointer_location - *surface_loc).to_i32_round()))
                    {
                        return;
                    }
                    match &*constraint {
                        PointerConstraint::Locked(_locked) => {
                            pointer_locked = true;
                        }
                        PointerConstraint::Confined(confine) => {
                            pointer_confined = true;
                            confine_region = confine.region().cloned();
                        }
                    }
                }
                _ => {}
            });
        }

        pointer.relative_motion(
            self,
            under.clone(),
            &RelativeMotionEvent {
                delta: evt.delta(),
                delta_unaccel: evt.delta_unaccel(),
                utime: evt.time(),
            },
        );

        // If pointer is locked, only emit relative motion
        if pointer_locked {
            pointer.frame(self);
            return;
        }

        pointer_location += evt.delta();

        // clamp to screen limits
        // this event is never generated by winit
        pointer_location = self.clamp_coords(pointer_location);

        let new_under = self.surface_under(pointer_location);

        // If confined, don't move pointer if it would go outside surface or region
        if pointer_confined {
            if let Some((surface, surface_loc)) = &under {
                if new_under.as_ref().and_then(|(under, _)| under.wl_surface()) != surface.wl_surface() {
                    pointer.frame(self);
                    return;
                }
                if let Some(region) = confine_region {
                    if !region.contains((pointer_location - *surface_loc).to_i32_round()) {
                        pointer.frame(self);
                        return;
                    }
                }
            }
        }

        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pointer_location,
                serial,
                time: evt.time_msec(),
            },
        );
        pointer.frame(self);

        // If pointer is now in a constraint region, activate it
        // TODO Anywhere else pointer is moved needs to do this
        if let Some((under, surface_location)) =
            new_under.and_then(|(target, loc)| Some((target.wl_surface()?.into_owned(), loc)))
        {
            with_pointer_constraint(&under, &pointer, |constraint| match constraint {
                Some(constraint) if !constraint.is_active() => {
                    let point = (pointer_location - surface_location).to_i32_round();
                    if constraint.region().is_none_or(|region| region.contains(point)) {
                        constraint.activate();
                    }
                }
                _ => {}
            });
        }
    }

    fn on_pointer_move_absolute<B: InputBackend>(
        &mut self,
        _dh: &DisplayHandle,
        evt: B::PointerMotionAbsoluteEvent,
    ) {
        let serial = SCOUNTER.next_serial();

        let max_x = self
            .space
            .outputs()
            .fold(0, |acc, o| acc + self.space.output_geometry(o).unwrap().size.w);

        let max_h_output = self
            .space
            .outputs()
            .max_by_key(|o| self.space.output_geometry(o).unwrap().size.h)
            .unwrap();

        let max_y = self.space.output_geometry(max_h_output).unwrap().size.h;

        let mut pointer_location = (evt.x_transformed(max_x), evt.y_transformed(max_y)).into();

        // clamp to screen limits
        pointer_location = self.clamp_coords(pointer_location);

        let pointer = self.pointer.clone();
        let under = self.surface_under(pointer_location);

        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pointer_location,
                serial,
                time: evt.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn on_tablet_tool_axis<B: InputBackend>(&mut self, evt: B::TabletToolAxisEvent) {
        let tablet_seat = self.seat.tablet_seat();

        if let Some(pointer_location) = self.touch_location_transformed(&evt) {
            let pointer = self.pointer.clone();
            let under = self.surface_under(pointer_location);
            let tablet = tablet_seat.get_tablet(&TabletDescriptor::from(&evt.device()));
            let tool = tablet_seat.get_tool(&evt.tool());

            pointer.motion(
                self,
                under.clone(),
                &MotionEvent {
                    location: pointer_location,
                    serial: SCOUNTER.next_serial(),
                    time: self.clock.now().as_millis(),
                },
            );

            if let (Some(tablet), Some(tool)) = (tablet, tool) {
                if evt.pressure_has_changed() {
                    tool.pressure(evt.pressure());
                }
                if evt.distance_has_changed() {
                    tool.distance(evt.distance());
                }
                if evt.tilt_has_changed() {
                    tool.tilt(evt.tilt());
                }
                if evt.slider_has_changed() {
                    tool.slider_position(evt.slider_position());
                }
                if evt.rotation_has_changed() {
                    tool.rotation(evt.rotation());
                }
                if evt.wheel_has_changed() {
                    tool.wheel(evt.wheel_delta(), evt.wheel_delta_discrete());
                }

                tool.motion(
                    pointer_location,
                    under.and_then(|(f, loc)| f.wl_surface().map(|s| (s.into_owned(), loc))),
                    &tablet,
                    SCOUNTER.next_serial(),
                    evt.time_msec(),
                );
            }

            pointer.frame(self);
        }
    }

    fn on_tablet_tool_proximity<B: InputBackend>(
        &mut self,
        dh: &DisplayHandle,
        evt: B::TabletToolProximityEvent,
    ) {
        let tablet_seat = self.seat.tablet_seat();

        if let Some(pointer_location) = self.touch_location_transformed(&evt) {
            let tool = evt.tool();
            tablet_seat.add_tool::<Self>(self, dh, &tool);

            let pointer = self.pointer.clone();
            let under = self.surface_under(pointer_location);
            let tablet = tablet_seat.get_tablet(&TabletDescriptor::from(&evt.device()));
            let tool = tablet_seat.get_tool(&tool);

            pointer.motion(
                self,
                under.clone(),
                &MotionEvent {
                    location: pointer_location,
                    serial: SCOUNTER.next_serial(),
                    time: evt.time_msec(),
                },
            );
            pointer.frame(self);

            if let (Some(under), Some(tablet), Some(tool)) = (
                under.and_then(|(f, loc)| f.wl_surface().map(|s| (s.into_owned(), loc))),
                tablet,
                tool,
            ) {
                match evt.state() {
                    ProximityState::In => tool.proximity_in(
                        pointer_location,
                        under,
                        &tablet,
                        SCOUNTER.next_serial(),
                        evt.time_msec(),
                    ),
                    ProximityState::Out => tool.proximity_out(evt.time_msec()),
                }
            }
        }
    }

    fn on_tablet_tool_tip<B: InputBackend>(&mut self, evt: B::TabletToolTipEvent) {
        let tool = self.seat.tablet_seat().get_tool(&evt.tool());

        if let Some(tool) = tool {
            match evt.tip_state() {
                TabletToolTipState::Down => {
                    let serial = SCOUNTER.next_serial();
                    tool.tip_down(serial, evt.time_msec());

                    // change the keyboard focus
                    self.update_keyboard_focus(self.pointer.current_location(), serial);
                }
                TabletToolTipState::Up => {
                    tool.tip_up(evt.time_msec());
                }
            }
        }
    }

    fn on_tablet_button<B: InputBackend>(&mut self, evt: B::TabletToolButtonEvent) {
        let tool = self.seat.tablet_seat().get_tool(&evt.tool());

        if let Some(tool) = tool {
            tool.button(
                evt.button(),
                evt.button_state(),
                SCOUNTER.next_serial(),
                evt.time_msec(),
            );
        }
    }

    fn on_gesture_swipe_begin<B: InputBackend>(&mut self, evt: B::GestureSwipeBeginEvent) {
        let serial = SCOUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_begin(
            self,
            &GestureSwipeBeginEvent {
                serial,
                time: evt.time_msec(),
                fingers: evt.fingers(),
            },
        );
    }

    fn on_gesture_swipe_update<B: InputBackend>(&mut self, evt: B::GestureSwipeUpdateEvent) {
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_update(
            self,
            &GestureSwipeUpdateEvent {
                time: evt.time_msec(),
                delta: evt.delta(),
            },
        );
    }

    fn on_gesture_swipe_end<B: InputBackend>(&mut self, evt: B::GestureSwipeEndEvent) {
        let serial = SCOUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.gesture_swipe_end(
            self,
            &GestureSwipeEndEvent {
                serial,
                time: evt.time_msec(),
                cancelled: evt.cancelled(),
            },
        );
    }

    fn on_gesture_pinch_begin<B: InputBackend>(&mut self, evt: B::GesturePinchBeginEvent) {
        let serial = SCOUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_begin(
            self,
            &GesturePinchBeginEvent {
                serial,
                time: evt.time_msec(),
                fingers: evt.fingers(),
            },
        );
    }

    fn on_gesture_pinch_update<B: InputBackend>(&mut self, evt: B::GesturePinchUpdateEvent) {
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_update(
            self,
            &GesturePinchUpdateEvent {
                time: evt.time_msec(),
                delta: evt.delta(),
                scale: evt.scale(),
                rotation: evt.rotation(),
            },
        );
    }

    fn on_gesture_pinch_end<B: InputBackend>(&mut self, evt: B::GesturePinchEndEvent) {
        let serial = SCOUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.gesture_pinch_end(
            self,
            &GesturePinchEndEvent {
                serial,
                time: evt.time_msec(),
                cancelled: evt.cancelled(),
            },
        );
    }

    fn on_gesture_hold_begin<B: InputBackend>(&mut self, evt: B::GestureHoldBeginEvent) {
        let serial = SCOUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.gesture_hold_begin(
            self,
            &GestureHoldBeginEvent {
                serial,
                time: evt.time_msec(),
                fingers: evt.fingers(),
            },
        );
    }

    fn on_gesture_hold_end<B: InputBackend>(&mut self, evt: B::GestureHoldEndEvent) {
        let serial = SCOUNTER.next_serial();
        let pointer = self.pointer.clone();
        pointer.gesture_hold_end(
            self,
            &GestureHoldEndEvent {
                serial,
                time: evt.time_msec(),
                cancelled: evt.cancelled(),
            },
        );
    }

    fn touch_location_transformed<B: InputBackend, E: AbsolutePositionEvent<B>>(
        &self,
        evt: &E,
    ) -> Option<Point<f64, Logical>> {
        let output = self
            .space
            .outputs()
            .find(|output| output.name().starts_with("eDP"))
            .or_else(|| self.space.outputs().next());

        let output = output?;
        let output_geometry = self.space.output_geometry(output)?;

        let transform = output.current_transform();
        let size = transform.invert().transform_size(output_geometry.size);
        Some(
            transform.transform_point_in(evt.position_transformed(size), &size.to_f64())
                + output_geometry.loc.to_f64(),
        )
    }

    fn on_touch_down<B: InputBackend>(&mut self, evt: B::TouchDownEvent) {
        let Some(handle) = self.seat.get_touch() else {
            return;
        };

        let Some(touch_location) = self.touch_location_transformed(&evt) else {
            return;
        };

        let serial = SCOUNTER.next_serial();
        self.update_keyboard_focus(touch_location, serial);

        let under = self.surface_under(touch_location);
        handle.down(
            self,
            under,
            &DownEvent {
                slot: evt.slot(),
                location: touch_location,
                serial,
                time: evt.time_msec(),
            },
        );
    }
    fn on_touch_up<B: InputBackend>(&mut self, evt: B::TouchUpEvent) {
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        let serial = SCOUNTER.next_serial();
        handle.up(
            self,
            &UpEvent {
                slot: evt.slot(),
                serial,
                time: evt.time_msec(),
            },
        )
    }
    fn on_touch_motion<B: InputBackend>(&mut self, evt: B::TouchMotionEvent) {
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        let Some(touch_location) = self.touch_location_transformed(&evt) else {
            return;
        };

        let under = self.surface_under(touch_location);
        handle.motion(
            self,
            under,
            &smithay::input::touch::MotionEvent {
                slot: evt.slot(),
                location: touch_location,
                time: evt.time_msec(),
            },
        );
    }
    fn on_touch_frame<B: InputBackend>(&mut self, _evt: B::TouchFrameEvent) {
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        handle.frame(self);
    }
    fn on_touch_cancel<B: InputBackend>(&mut self, _evt: B::TouchCancelEvent) {
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        handle.cancel(self);
    }

    fn clamp_coords(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        if self.space.outputs().next().is_none() {
            return pos;
        }

        let (pos_x, pos_y) = pos.into();
        let max_x = self
            .space
            .outputs()
            .fold(0, |acc, o| acc + self.space.output_geometry(o).unwrap().size.w);
        let clamped_x = pos_x.clamp(0.0, max_x as f64);
        let max_y = self
            .space
            .outputs()
            .find(|o| {
                let geo = self.space.output_geometry(o).unwrap();
                geo.contains((clamped_x as i32, 0))
            })
            .map(|o| self.space.output_geometry(o).unwrap().size.h);

        if let Some(max_y) = max_y {
            let clamped_y = pos_y.clamp(0.0, max_y as f64);
            (clamped_x, clamped_y).into()
        } else {
            (clamped_x, pos_y).into()
        }
    }
}
