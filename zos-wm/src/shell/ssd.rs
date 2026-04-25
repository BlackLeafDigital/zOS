use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer,
            element::{
                AsRenderElements, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::{SolidColorBuffer, SolidColorRenderElement},
            },
        },
    },
    desktop::WindowSurface,
    input::Seat,
    utils::{Logical, Point, Rectangle, Serial, Transform},
    wayland::shell::xdg::XdgShellHandler,
};

use std::cell::{RefCell, RefMut};

use crate::{AnvilState, state::Backend};

use super::WindowElement;
use super::element::WindowRenderElement;

pub struct WindowState {
    pub is_ssd: bool,
    pub header_bar: HeaderBar,
}

#[derive(Debug, Clone)]
pub struct HeaderBar {
    pub pointer_loc: Option<Point<f64, Logical>>,
    pub width: u32,
    pub minimize_button_hover: bool,
    pub maximize_button_hover: bool,
    pub close_button_hover: bool,
    pub background: SolidColorBuffer,
    pub minimize_button_bg: SolidColorBuffer,
    pub maximize_button_bg: SolidColorBuffer,
    pub close_button_bg: SolidColorBuffer,
    pub minimize_icon: MemoryRenderBuffer,
    pub minimize_icon_hover: MemoryRenderBuffer,
    pub maximize_icon: MemoryRenderBuffer,
    pub maximize_icon_hover: MemoryRenderBuffer,
    pub close_icon: MemoryRenderBuffer,
    pub close_icon_hover: MemoryRenderBuffer,
}

// --- Catppuccin Mocha palette ---
// Titlebar background (base #1e1e2e).
const BG_COLOR: [f32; 4] = [0.118, 0.118, 0.180, 1.0];
// Resting button background (mantle #181825 — close to transparent on the bar).
const BTN_BG: [f32; 4] = [0.094, 0.094, 0.145, 1.0];
// Hover button background for minimize/maximize (surface0 #313244).
const BTN_BG_HOVER: [f32; 4] = [0.192, 0.196, 0.267, 1.0];
// Close hover uses Catppuccin red #f38ba8.
const CLOSE_BG_HOVER: [f32; 4] = [0.953, 0.545, 0.659, 1.0];

// Icon stroke colors (RGBA bytes, non-premultiplied — tiny-skia handles alpha).
const ICON_COLOR: [u8; 4] = [0x7f, 0x84, 0x9c, 0xff]; // overlay1 #7f849c
const ICON_COLOR_HOVER: [u8; 4] = [0xcd, 0xd6, 0xf4, 0xff]; // text #cdd6f4
const CLOSE_ICON_HOVER: [u8; 4] = [0x1e, 0x1e, 0x2e, 0xff]; // base #1e1e2e (dark on red)

pub const HEADER_BAR_HEIGHT: i32 = 32;
const BUTTON_HEIGHT: u32 = HEADER_BAR_HEIGHT as u32;
const BUTTON_WIDTH: u32 = 32;
const ICON_SIZE: u32 = 20;

const MINIMIZE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="5" y1="12" x2="19" y2="12"/></svg>"##;

const MAXIMIZE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="5" width="14" height="14" rx="1"/></svg>"##;

const CLOSE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="6" x2="18" y2="18"/><line x1="6" y1="18" x2="18" y2="6"/></svg>"##;

/// Rasterize an SVG string into a MemoryRenderBuffer sized `size_px` square,
/// substituting `currentColor` with the supplied RGBA color.
///
/// The resulting buffer is Argb8888 premultiplied (channel order BGRA on
/// little-endian, matching smithay's expectations). tiny-skia emits RGBA
/// premultiplied, so we swap R and B channels.
fn rasterize_icon(svg_src: &str, size_px: u32, color: [u8; 4]) -> MemoryRenderBuffer {
    let color_hex = format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]);
    let svg = svg_src.replace("currentColor", &color_hex);

    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).expect("parse embedded SVG");
    let mut pixmap = tiny_skia::Pixmap::new(size_px, size_px).expect("allocate pixmap");
    let scale = size_px as f32 / 24.0;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // tiny-skia gives us RGBA premultiplied; smithay wants Argb8888 which on
    // little-endian is BGRA in memory. Swap R<->B per pixel.
    let src = pixmap.data();
    let mut buf = vec![0u8; src.len()];
    for (i, chunk) in src.chunks_exact(4).enumerate() {
        buf[i * 4] = chunk[2]; // B
        buf[i * 4 + 1] = chunk[1]; // G
        buf[i * 4 + 2] = chunk[0]; // R
        buf[i * 4 + 3] = chunk[3]; // A
    }

    let mut mr = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (size_px as i32, size_px as i32),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = mr.render();
        let _ = ctx.draw::<_, ()>(|dst| {
            dst.copy_from_slice(&buf);
            Ok(vec![Rectangle::from_size((size_px as i32, size_px as i32).into())])
        });
    }
    mr
}

impl HeaderBar {
    pub fn new() -> Self {
        HeaderBar {
            pointer_loc: None,
            width: 0,
            minimize_button_hover: false,
            maximize_button_hover: false,
            close_button_hover: false,
            background: SolidColorBuffer::default(),
            minimize_button_bg: SolidColorBuffer::default(),
            maximize_button_bg: SolidColorBuffer::default(),
            close_button_bg: SolidColorBuffer::default(),
            minimize_icon: rasterize_icon(MINIMIZE_SVG, ICON_SIZE, ICON_COLOR),
            minimize_icon_hover: rasterize_icon(MINIMIZE_SVG, ICON_SIZE, ICON_COLOR_HOVER),
            maximize_icon: rasterize_icon(MAXIMIZE_SVG, ICON_SIZE, ICON_COLOR),
            maximize_icon_hover: rasterize_icon(MAXIMIZE_SVG, ICON_SIZE, ICON_COLOR_HOVER),
            close_icon: rasterize_icon(CLOSE_SVG, ICON_SIZE, ICON_COLOR),
            close_icon_hover: rasterize_icon(CLOSE_SVG, ICON_SIZE, CLOSE_ICON_HOVER),
        }
    }

    pub fn pointer_enter(&mut self, loc: Point<f64, Logical>) {
        self.pointer_loc = Some(loc);
    }

    pub fn pointer_leave(&mut self) {
        self.pointer_loc = None;
    }

    fn zone_close(&self, x: f64) -> bool {
        x >= (self.width - BUTTON_WIDTH) as f64
    }

    fn zone_maximize(&self, x: f64) -> bool {
        x >= (self.width - BUTTON_WIDTH * 2) as f64 && x < (self.width - BUTTON_WIDTH) as f64
    }

    fn zone_minimize(&self, x: f64) -> bool {
        x >= (self.width - BUTTON_WIDTH * 3) as f64 && x < (self.width - BUTTON_WIDTH * 2) as f64
    }

    pub fn clicked<BackendData: Backend>(
        &mut self,
        seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        serial: Serial,
    ) {
        match self.pointer_loc.as_ref() {
            Some(loc) if self.zone_close(loc.x) => {
                match window.0.underlying_surface() {
                    WindowSurface::Wayland(w) => w.send_close(),
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(w) => {
                        let _ = w.close();
                    }
                };
            }
            Some(loc) if self.zone_maximize(loc.x) => {
                match window.0.underlying_surface() {
                    WindowSurface::Wayland(w) => state.maximize_request(w.clone()),
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(w) => {
                        let surface = w.clone();
                        state
                            .handle
                            .insert_idle(move |data| data.maximize_request_x11(&surface));
                    }
                };
            }
            Some(loc) if self.zone_minimize(loc.x) => {
                // Minimize: unmap from space so the window disappears.
                // TODO(phase-3): track minimized windows so they can be
                // restored from zos-dock or a SUPER+SHIFT+M shortcut.
                tracing::info!("ssd: minimize requested — unmapping window (restore not yet implemented)");
                state.space.unmap_elem(window);
            }
            Some(_) => {
                match window.0.underlying_surface() {
                    WindowSurface::Wayland(w) => {
                        let seat = seat.clone();
                        let toplevel = w.clone();
                        state
                            .handle
                            .insert_idle(move |data| data.move_request_xdg(&toplevel, &seat, serial));
                    }
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(w) => {
                        let window = w.clone();
                        state
                            .handle
                            .insert_idle(move |data| data.move_request_x11(&window));
                    }
                };
            }
            _ => {}
        };
    }

    pub fn touch_down<BackendData: Backend>(
        &mut self,
        seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        serial: Serial,
    ) {
        match self.pointer_loc.as_ref() {
            Some(loc) if self.zone_close(loc.x) => {}
            Some(loc) if self.zone_maximize(loc.x) => {}
            Some(loc) if self.zone_minimize(loc.x) => {}
            Some(_) => {
                match window.0.underlying_surface() {
                    WindowSurface::Wayland(w) => {
                        let seat = seat.clone();
                        let toplevel = w.clone();
                        state
                            .handle
                            .insert_idle(move |data| data.move_request_xdg(&toplevel, &seat, serial));
                    }
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(w) => {
                        let window = w.clone();
                        state
                            .handle
                            .insert_idle(move |data| data.move_request_x11(&window));
                    }
                };
            }
            _ => {}
        };
    }

    pub fn touch_up<BackendData: Backend>(
        &mut self,
        _seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        _serial: Serial,
    ) {
        match self.pointer_loc.as_ref() {
            Some(loc) if self.zone_close(loc.x) => {
                match window.0.underlying_surface() {
                    WindowSurface::Wayland(w) => w.send_close(),
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(w) => {
                        let _ = w.close();
                    }
                };
            }
            Some(loc) if self.zone_maximize(loc.x) => {
                match window.0.underlying_surface() {
                    WindowSurface::Wayland(w) => state.maximize_request(w.clone()),
                    #[cfg(feature = "xwayland")]
                    WindowSurface::X11(w) => {
                        let surface = w.clone();
                        state
                            .handle
                            .insert_idle(move |data| data.maximize_request_x11(&surface));
                    }
                };
            }
            Some(loc) if self.zone_minimize(loc.x) => {
                tracing::info!("ssd: minimize requested (touch) — unmapping window");
                state.space.unmap_elem(window);
            }
            _ => {}
        };
    }

    pub fn redraw(&mut self, width: u32) {
        if width == 0 {
            self.width = 0;
            return;
        }

        self.background
            .update((width as i32, HEADER_BAR_HEIGHT), BG_COLOR);

        let needs_redraw_buttons = width != self.width;
        if needs_redraw_buttons {
            self.width = width;
        }

        let hover_close = self
            .pointer_loc
            .as_ref()
            .map(|l| self.zone_close(l.x))
            .unwrap_or(false);
        let hover_max = self
            .pointer_loc
            .as_ref()
            .map(|l| self.zone_maximize(l.x))
            .unwrap_or(false);
        let hover_min = self
            .pointer_loc
            .as_ref()
            .map(|l| self.zone_minimize(l.x))
            .unwrap_or(false);

        // Close button
        if hover_close && (needs_redraw_buttons || !self.close_button_hover) {
            self.close_button_bg
                .update((BUTTON_WIDTH as i32, BUTTON_HEIGHT as i32), CLOSE_BG_HOVER);
            self.close_button_hover = true;
        } else if !hover_close && (needs_redraw_buttons || self.close_button_hover) {
            self.close_button_bg
                .update((BUTTON_WIDTH as i32, BUTTON_HEIGHT as i32), BTN_BG);
            self.close_button_hover = false;
        }

        // Maximize button
        if hover_max && (needs_redraw_buttons || !self.maximize_button_hover) {
            self.maximize_button_bg
                .update((BUTTON_WIDTH as i32, BUTTON_HEIGHT as i32), BTN_BG_HOVER);
            self.maximize_button_hover = true;
        } else if !hover_max && (needs_redraw_buttons || self.maximize_button_hover) {
            self.maximize_button_bg
                .update((BUTTON_WIDTH as i32, BUTTON_HEIGHT as i32), BTN_BG);
            self.maximize_button_hover = false;
        }

        // Minimize button
        if hover_min && (needs_redraw_buttons || !self.minimize_button_hover) {
            self.minimize_button_bg
                .update((BUTTON_WIDTH as i32, BUTTON_HEIGHT as i32), BTN_BG_HOVER);
            self.minimize_button_hover = true;
        } else if !hover_min && (needs_redraw_buttons || self.minimize_button_hover) {
            self.minimize_button_bg
                .update((BUTTON_WIDTH as i32, BUTTON_HEIGHT as i32), BTN_BG);
            self.minimize_button_hover = false;
        }
    }
}

impl Default for HeaderBar {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> AsRenderElements<R> for HeaderBar
where
    R: Renderer + smithay::backend::renderer::ImportAll + ImportMem,
    R::TextureId: Clone + Send + 'static,
{
    type RenderElement = WindowRenderElement<R>;

    fn render_elements<C: From<Self::RenderElement>>(
        &self,
        renderer: &mut R,
        location: Point<i32, smithay::utils::Physical>,
        scale: smithay::utils::Scale<f64>,
        alpha: f32,
    ) -> Vec<C> {
        let header_end_offset: Point<i32, Logical> = Point::from((self.width as i32, 0));
        let button_offset: Point<i32, Logical> = Point::from((BUTTON_WIDTH as i32, 0));

        // Button backgrounds (right-to-left: close, maximize, minimize).
        let close_bg_loc =
            location + (header_end_offset - button_offset).to_physical_precise_round(scale);
        let max_bg_loc = location
            + (header_end_offset - button_offset.upscale(2)).to_physical_precise_round(scale);
        let min_bg_loc = location
            + (header_end_offset - button_offset.upscale(3)).to_physical_precise_round(scale);

        // Icons are ICON_SIZE square, centered within BUTTON_WIDTH × HEADER_BAR_HEIGHT.
        let icon_pad_x =
            ((BUTTON_WIDTH as i32 - ICON_SIZE as i32) / 2) as f64 * scale.x;
        let icon_pad_y =
            ((HEADER_BAR_HEIGHT - ICON_SIZE as i32) / 2) as f64 * scale.y;
        let icon_pad: Point<i32, smithay::utils::Physical> =
            Point::from((icon_pad_x.round() as i32, icon_pad_y.round() as i32));

        let close_icon_loc = close_bg_loc + icon_pad;
        let max_icon_loc = max_bg_loc + icon_pad;
        let min_icon_loc = min_bg_loc + icon_pad;

        let min_icon_buf = if self.minimize_button_hover {
            &self.minimize_icon_hover
        } else {
            &self.minimize_icon
        };
        let max_icon_buf = if self.maximize_button_hover {
            &self.maximize_icon_hover
        } else {
            &self.maximize_icon
        };
        let close_icon_buf = if self.close_button_hover {
            &self.close_icon_hover
        } else {
            &self.close_icon
        };

        let mut out: Vec<WindowRenderElement<R>> = Vec::with_capacity(7);

        // Icons on top — pushed first because the render pipeline paints in
        // reverse order (index 0 is front-most).
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            close_icon_loc.to_f64(),
            close_icon_buf,
            Some(alpha),
            None,
            None,
            Kind::Unspecified,
        ) {
            out.push(WindowRenderElement::MemoryDecoration(elem));
        }
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            max_icon_loc.to_f64(),
            max_icon_buf,
            Some(alpha),
            None,
            None,
            Kind::Unspecified,
        ) {
            out.push(WindowRenderElement::MemoryDecoration(elem));
        }
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            min_icon_loc.to_f64(),
            min_icon_buf,
            Some(alpha),
            None,
            None,
            Kind::Unspecified,
        ) {
            out.push(WindowRenderElement::MemoryDecoration(elem));
        }

        // Button backgrounds below the icons.
        out.push(WindowRenderElement::Decoration(
            SolidColorRenderElement::from_buffer(
                &self.close_button_bg,
                close_bg_loc,
                scale,
                alpha,
                Kind::Unspecified,
            ),
        ));
        out.push(WindowRenderElement::Decoration(
            SolidColorRenderElement::from_buffer(
                &self.maximize_button_bg,
                max_bg_loc,
                scale,
                alpha,
                Kind::Unspecified,
            ),
        ));
        out.push(WindowRenderElement::Decoration(
            SolidColorRenderElement::from_buffer(
                &self.minimize_button_bg,
                min_bg_loc,
                scale,
                alpha,
                Kind::Unspecified,
            ),
        ));

        // Titlebar background last — sits behind everything above.
        out.push(WindowRenderElement::Decoration(
            SolidColorRenderElement::from_buffer(
                &self.background,
                location,
                scale,
                alpha,
                Kind::Unspecified,
            ),
        ));

        out.into_iter().map(C::from).collect()
    }
}

impl WindowElement {
    pub fn decoration_state(&self) -> RefMut<'_, WindowState> {
        self.user_data().insert_if_missing(|| {
            RefCell::new(WindowState {
                is_ssd: false,
                header_bar: HeaderBar::new(),
            })
        });

        self.user_data()
            .get::<RefCell<WindowState>>()
            .unwrap()
            .borrow_mut()
    }

    pub fn set_ssd(&self, ssd: bool) {
        self.decoration_state().is_ssd = ssd;
    }
}
