use std::cell::RefCell;

#[cfg(feature = "xwayland")]
use smithay::xwayland::XWaylandClientData;

#[cfg(feature = "udev")]
use smithay::wayland::drm_syncobj::DrmSyncobjCachedState;

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    desktop::{
        LayerSurface, PopupKind, PopupManager, Space, WindowSurfaceType, layer_map_for_output,
        space::SpaceElement,
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    reexports::{
        calloop::Interest,
        wayland_server::{
            Client, Resource,
            protocol::{wl_buffer::WlBuffer, wl_output, wl_surface::WlSurface},
        },
    },
    utils::{IsAlive, Logical, Point, Rectangle, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
            TraversalAction, add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states,
            with_surface_tree_upward,
        },
        dmabuf::get_dmabuf,
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::XdgToplevelSurfaceData,
        },
    },
};

use crate::{
    ClientState,
    state::{AnvilState, Backend},
};

mod element;
mod grabs;
pub mod output_state;
pub(crate) mod ssd;
pub mod tiling;
pub mod workspace;
#[cfg(feature = "xwayland")]
mod x11;
mod xdg;

pub use self::element::*;
pub use self::grabs::*;

fn fullscreen_output_geometry(
    wl_surface: &WlSurface,
    wl_output: Option<&wl_output::WlOutput>,
    space: &mut Space<WindowElement>,
) -> Option<Rectangle<i32, Logical>> {
    // First test if a specific output has been requested
    // if the requested output is not found ignore the request
    wl_output
        .and_then(Output::from_resource)
        .or_else(|| {
            let w = space
                .elements()
                .find(|window| window.wl_surface().map(|s| &*s == wl_surface).unwrap_or(false));
            w.and_then(|w| space.outputs_for_element(w).first().cloned())
        })
        .as_ref()
        .and_then(|o| space.output_geometry(o))
}

#[derive(Default)]
pub struct FullscreenSurface(RefCell<Option<WindowElement>>);

impl FullscreenSurface {
    pub fn set(&self, window: WindowElement) {
        *self.0.borrow_mut() = Some(window);
    }

    pub fn get(&self) -> Option<WindowElement> {
        let mut window = self.0.borrow_mut();
        if window.as_ref().map(|w| !w.alive()).unwrap_or(false) {
            *window = None;
        }
        window.clone()
    }

    pub fn clear(&self) -> Option<WindowElement> {
        self.0.borrow_mut().take()
    }
}

impl<BackendData: Backend> BufferHandler for AnvilState<BackendData> {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl<BackendData: Backend> CompositorHandler for AnvilState<BackendData> {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        #[cfg(feature = "xwayland")]
        if let Some(state) = client.get_data::<XWaylandClientData>() {
            return &state.compositor_state;
        }
        if let Some(state) = client.get_data::<ClientState>() {
            return &state.compositor_state;
        }
        panic!("Unknown client data type")
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        add_pre_commit_hook::<Self, _>(surface, move |state, _dh, surface| {
            #[cfg(feature = "udev")]
            let mut acquire_point = None;
            let maybe_dmabuf = with_states(surface, |surface_data| {
                #[cfg(feature = "udev")]
                acquire_point.clone_from(
                    &surface_data
                        .cached_state
                        .get::<DrmSyncobjCachedState>()
                        .pending()
                        .acquire_point,
                );
                surface_data
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .pending()
                    .buffer
                    .as_ref()
                    .and_then(|assignment| match assignment {
                        BufferAssignment::NewBuffer(buffer) => get_dmabuf(buffer).cloned().ok(),
                        _ => None,
                    })
            });
            if let Some(dmabuf) = maybe_dmabuf {
                #[cfg(feature = "udev")]
                if let Some(acquire_point) = acquire_point {
                    if let Ok((blocker, source)) = acquire_point.generate_blocker() {
                        let client = surface.client().unwrap();
                        let res = state.handle.insert_source(source, move |_, _, data| {
                            let dh = data.display_handle.clone();
                            data.client_compositor_state(&client).blocker_cleared(data, &dh);
                            Ok(())
                        });
                        if res.is_ok() {
                            add_blocker(surface, blocker);
                            return;
                        }
                    }
                }
                if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ) {
                    if let Some(client) = surface.client() {
                        let res = state.handle.insert_source(source, move |_, _, data| {
                            let dh = data.display_handle.clone();
                            data.client_compositor_state(&client).blocker_cleared(data, &dh);
                            Ok(())
                        });
                        if res.is_ok() {
                            add_blocker(surface, blocker);
                        }
                    }
                }
            }
        });
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.backend_data.early_import(surface);

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for_surface(&root) {
                window.0.on_commit();

                if &root == surface {
                    let buffer_offset = with_states(surface, |states| {
                        states
                            .cached_state
                            .get::<SurfaceAttributes>()
                            .current()
                            .buffer_delta
                            .take()
                    });

                    if let Some(buffer_offset) = buffer_offset {
                        let current_loc = self.space.element_location(&window).unwrap();
                        self.space.map_element(window, current_loc + buffer_offset, false);
                    }
                }
            }
        }
        self.popups.commit(surface);

        if matches!(&self.cursor_status, CursorImageStatus::Surface(cursor_surface) if cursor_surface == surface)
        {
            with_states(surface, |states| {
                let cursor_image_attributes = states.data_map.get::<CursorImageSurfaceData>();

                if let Some(mut cursor_image_attributes) =
                    cursor_image_attributes.map(|attrs| attrs.lock().unwrap())
                {
                    let buffer_delta = states
                        .cached_state
                        .get::<SurfaceAttributes>()
                        .current()
                        .buffer_delta
                        .take();
                    if let Some(buffer_delta) = buffer_delta {
                        tracing::trace!(hotspot = ?cursor_image_attributes.hotspot, ?buffer_delta, "decrementing cursor hotspot");
                        cursor_image_attributes.hotspot -= buffer_delta;
                    }
                }
            });
        }

        if matches!(&self.dnd_icon, Some(icon) if &icon.surface == surface) {
            let dnd_icon = self.dnd_icon.as_mut().unwrap();
            with_states(&dnd_icon.surface, |states| {
                let buffer_delta = states
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .current()
                    .buffer_delta
                    .take()
                    .unwrap_or_default();
                tracing::trace!(offset = ?dnd_icon.offset, ?buffer_delta, "moving dnd offset");
                dnd_icon.offset += buffer_delta;
            });
        }

        ensure_initial_configure(surface, &self.space, &mut self.popups)
    }
}

impl<BackendData: Backend> WlrLayerShellHandler for AnvilState<BackendData> {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .unwrap_or_else(|| self.space.outputs().next().unwrap().clone());
        let mut map = layer_map_for_output(&output);
        map.map_layer(&LayerSurface::new(surface, namespace)).unwrap();
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        if let Some((mut map, layer)) = self.space.outputs().find_map(|o| {
            let map = layer_map_for_output(o);
            let layer = map
                .layers()
                .find(|&layer| layer.layer_surface() == &surface)
                .cloned();
            layer.map(|layer| (map, layer))
        }) {
            map.unmap_layer(&layer);
        }
    }
}

impl<BackendData: Backend> AnvilState<BackendData> {
    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<WindowElement> {
        self.space
            .elements()
            .find(|window| window.wl_surface().map(|s| &*s == surface).unwrap_or(false))
            .cloned()
    }
}

#[derive(Default)]
pub struct SurfaceData {
    pub geometry: Option<Rectangle<i32, Logical>>,
    pub resize_state: ResizeState,
}

fn ensure_initial_configure(surface: &WlSurface, space: &Space<WindowElement>, popups: &mut PopupManager) {
    with_surface_tree_upward(
        surface,
        (),
        |_, _, _| TraversalAction::DoChildren(()),
        |_, states, _| {
            states
                .data_map
                .insert_if_missing(|| RefCell::new(SurfaceData::default()));
        },
        |_, _, _| true,
    );

    if let Some(window) = space
        .elements()
        .find(|window| window.wl_surface().map(|s| &*s == surface).unwrap_or(false))
        .cloned()
    {
        // send the initial configure if relevant
        #[cfg_attr(not(feature = "xwayland"), allow(irrefutable_let_patterns))]
        if let Some(toplevel) = window.0.toplevel() {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });
            if !initial_configure_sent {
                toplevel.send_configure();
            }
        }

        with_states(surface, |states| {
            let mut data = states
                .data_map
                .get::<RefCell<SurfaceData>>()
                .unwrap()
                .borrow_mut();

            // Finish resizing.
            if let ResizeState::WaitingForCommit(_) = data.resize_state {
                data.resize_state = ResizeState::NotResizing;
            }
        });

        return;
    }

    if let Some(popup) = popups.find_popup(surface) {
        let popup = match popup {
            PopupKind::Xdg(ref popup) => popup,
            // Doesn't require configure
            PopupKind::InputMethod(ref _input_popup) => {
                return;
            }
        };

        if !popup.is_initial_configure_sent() {
            // NOTE: This should never fail as the initial configure is always
            // allowed.
            popup.send_configure().expect("initial configure failed");
        }

        return;
    };

    if let Some(output) = space.outputs().find(|o| {
        let map = layer_map_for_output(o);
        map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .is_some()
    }) {
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<LayerSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .initial_configure_sent
        });

        let mut map = layer_map_for_output(output);

        // arrange the layers before sending the initial configure
        // to respect any size the client may have sent
        map.arrange();
        // send the initial configure if relevant
        if !initial_configure_sent {
            let layer = map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .unwrap();

            layer.layer_surface().send_configure();
        }
    };
}

/// Clamp a point so a window of `size` stays inside `area`.
fn clamp_to_area(
    loc: Point<i32, Logical>,
    size: Size<i32, Logical>,
    area: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let min_x = area.loc.x;
    let min_y = area.loc.y;
    let max_x = (area.loc.x + area.size.w - size.w).max(min_x);
    let max_y = (area.loc.y + area.size.h - size.h).max(min_y);
    Point::from((loc.x.clamp(min_x, max_x), loc.y.clamp(min_y, max_y)))
}

fn place_new_window(
    space: &mut Space<WindowElement>,
    pointer_location: Point<f64, Logical>,
    window: &WindowElement,
    activate: bool,
) {
    // Cascade / padding constants (cosmic-comp values).
    const CASCADE_DX: i32 = 48;
    const CASCADE_DY: i32 = 48;
    const PADDING: i32 = 16;

    // Pick the output: pointer-focused first, otherwise any. Pointer location
    // is used ONLY for output selection, never as a literal placement.
    let output = space
        .output_under(pointer_location)
        .next()
        .or_else(|| space.outputs().next())
        .cloned();

    // Working zone (output minus exclusive layer-shell areas).
    let output_zone = output
        .as_ref()
        .and_then(|o| {
            let geo = space.output_geometry(o)?;
            let map = layer_map_for_output(o);
            let zone = map.non_exclusive_zone();
            Some(Rectangle::new(geo.loc + zone.loc, zone.size))
        })
        .unwrap_or_else(|| Rectangle::from_size((800, 800).into()));

    // Set the initial toplevel bounds so clients can size themselves sensibly.
    #[allow(irrefutable_let_patterns)]
    if let Some(toplevel) = window.0.toplevel() {
        toplevel.with_pending_state(|state| {
            state.bounds = Some(output_zone.size);
        });
    }

    // Use the post-initial-configure natural geometry. Fall back to bbox if
    // the client hasn't reported geometry yet.
    let mut win_size = window.0.geometry().size;
    if win_size.w <= 0 || win_size.h <= 0 {
        win_size = window.0.bbox().size;
    }
    if win_size.w <= 0 {
        win_size.w = 1;
    }
    if win_size.h <= 0 {
        win_size.h = 1;
    }

    // ---- Tier 1: explicit hint — dialog/transient with a parent. ----
    let parent_surface = window.0.toplevel().and_then(|t| t.parent());
    let parent_match = parent_surface.as_ref().and_then(|psurf| {
        let elem = space
            .elements()
            .find(|e| e.wl_surface().map(|s| &*s == psurf).unwrap_or(false))
            .cloned()?;
        let loc = space.element_location(&elem)?;
        Some((elem, loc))
    });
    if let Some((parent_elem, parent_loc)) = parent_match {
        let parent_geom = parent_elem.0.geometry();
        let centered = Point::from((
            parent_loc.x + (parent_geom.size.w - win_size.w) / 2,
            parent_loc.y + (parent_geom.size.h - win_size.h) / 2,
        ));
        let placed = clamp_to_area(centered, win_size, output_zone);
        space.map_element(window.clone(), placed, activate);
        return;
    }

    // ---- Tier 2: cascade from the most-recently-mapped window on this output. ----
    if let Some(target_output) = output.as_ref() {
        // `space.elements()` is in map-order; the last entry is the most
        // recently mapped window. Filter to the same output.
        let last_on_output = space
            .elements()
            .filter(|e| space.outputs_for_element(e).iter().any(|o| o == target_output))
            .next_back()
            .cloned();

        if let Some(prev) = last_on_output {
            if let Some(prev_loc) = space.element_location(&prev) {
                let mut cascade = Point::from((prev_loc.x + CASCADE_DX, prev_loc.y + CASCADE_DY));

                // Wrap horizontally if the cascade would push us off the right edge.
                if cascade.x + win_size.w > output_zone.loc.x + output_zone.size.w {
                    cascade.x = output_zone.loc.x + PADDING;
                }
                // Wrap vertically if the cascade would push us off the bottom edge.
                if cascade.y + win_size.h > output_zone.loc.y + output_zone.size.h {
                    cascade.y = output_zone.loc.y + PADDING;
                }

                // Final safety clamp in case the window itself is larger than the zone.
                let placed = clamp_to_area(cascade, win_size, output_zone);
                space.map_element(window.clone(), placed, activate);
                return;
            }
        }
    }

    // ---- Tier 3: fallback — centered horizontally, upper third vertically. ----
    let centered = Point::from((
        output_zone.loc.x + (output_zone.size.w - win_size.w) / 2,
        output_zone.loc.y + (output_zone.size.h - win_size.h) / 3,
    ));
    let placed = clamp_to_area(centered, win_size, output_zone);
    space.map_element(window.clone(), placed, activate);
}

pub fn fixup_positions(space: &mut Space<WindowElement>, pointer_location: Point<f64, Logical>) {
    // fixup outputs
    let mut offset = Point::<i32, Logical>::from((0, 0));
    for output in space.outputs().cloned().collect::<Vec<_>>().into_iter() {
        let size = space
            .output_geometry(&output)
            .map(|geo| geo.size)
            .unwrap_or_else(|| Size::from((0, 0)));
        space.map_output(&output, offset);
        layer_map_for_output(&output).arrange();
        offset.x += size.w;
    }

    // fixup windows
    let mut orphaned_windows = Vec::new();
    let outputs = space
        .outputs()
        .flat_map(|o| {
            let geo = space.output_geometry(o)?;
            let map = layer_map_for_output(o);
            let zone = map.non_exclusive_zone();
            Some(Rectangle::new(geo.loc + zone.loc, zone.size))
        })
        .collect::<Vec<_>>();
    for window in space.elements() {
        let window_location = match space.element_location(window) {
            Some(loc) => loc,
            None => continue,
        };
        let geo_loc = window.bbox().loc + window_location;

        if !outputs.iter().any(|o_geo| o_geo.contains(geo_loc)) {
            orphaned_windows.push(window.clone());
        }
    }
    for window in orphaned_windows.into_iter() {
        place_new_window(space, pointer_location, &window, false);
    }
}
