use smithay::{
    backend::renderer::{
        Color32F, ImportAll, ImportMem, Renderer,
        damage::{Error as OutputDamageTrackerError, OutputDamageTracker, RenderOutputResult},
        element::{
            AsRenderElements, RenderElement, Wrap,
            surface::WaylandSurfaceRenderElement,
            utils::{
                ConstrainAlign, ConstrainScaleBehavior, CropRenderElement, Relocate,
                RelocateRenderElement, RescaleRenderElement,
            },
        },
        gles::element::PixelShaderElement,
    },
    desktop::{
        layer_map_for_output,
        space::{
            ConstrainBehavior, ConstrainReference, Space, SpaceRenderElements, constrain_space_element,
        },
    },
    output::Output,
    utils::{Point, Rectangle, Scale, Size},
    wayland::shell::wlr_layer::Layer as WlrLayer,
};

#[cfg(feature = "debug")]
use crate::drawing::FpsElement;
use crate::{
    drawing::{CLEAR_COLOR, CLEAR_COLOR_FULLSCREEN, PointerRenderElement},
    effects::shadow::DropShadowEffect,
    shell::{FullscreenSurface, WindowElement, WindowRenderElement, workspace::Workspace},
};

/// Per-frame shadow parameters threaded into `output_elements`.
///
/// All fields are values pulled from `AnvilState` once per frame (radius,
/// color, offset). The effect program reference is borrowed off the
/// backend (winit-only — udev passes `None` while the `MultiRenderer` /
/// `PixelShaderElement` trait gap remains).
#[derive(Clone, Copy)]
pub struct ShadowParams<'a> {
    pub effect: &'a DropShadowEffect,
    pub blur_radius: f32,
    pub offset: (f32, f32),
    pub color: [f32; 4],
}

smithay::backend::renderer::element::render_elements! {
    pub CustomRenderElements<R> where
        R: ImportAll + ImportMem;
    Pointer=PointerRenderElement<R>,
    Surface=WaylandSurfaceRenderElement<R>,
    #[cfg(feature = "debug")]
    // Note: We would like to borrow this element instead, but that would introduce
    // a feature-dependent lifetime, which introduces a lot more feature bounds
    // as the whole type changes and we can't have an unused lifetime (for when "debug" is disabled)
    // in the declaration.
    Fps=FpsElement<R::TextureId>,
}

impl<R: Renderer> std::fmt::Debug for CustomRenderElements<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pointer(arg0) => f.debug_tuple("Pointer").field(arg0).finish(),
            Self::Surface(arg0) => f.debug_tuple("Surface").field(arg0).finish(),
            #[cfg(feature = "debug")]
            Self::Fps(arg0) => f.debug_tuple("Fps").field(arg0).finish(),
            Self::_GenericCatcher(arg0) => f.debug_tuple("_GenericCatcher").field(arg0).finish(),
        }
    }
}

smithay::backend::renderer::element::render_elements! {
    pub OutputRenderElements<R, E> where R: ImportAll + ImportMem;
    Space=SpaceRenderElements<R, E>,
    Window=Wrap<E>,
    Custom=CustomRenderElements<R>,
    Preview=CropRenderElement<RelocateRenderElement<RescaleRenderElement<WindowRenderElement<R>>>>,
    AnimatedWindow=RelocateRenderElement<WindowRenderElement<R>>,
    LayerSurface=WaylandSurfaceRenderElement<R>,
}

impl<R: Renderer + ImportAll + ImportMem, E: RenderElement<R> + std::fmt::Debug> std::fmt::Debug
    for OutputRenderElements<R, E>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Space(arg0) => f.debug_tuple("Space").field(arg0).finish(),
            Self::Window(arg0) => f.debug_tuple("Window").field(arg0).finish(),
            Self::Custom(arg0) => f.debug_tuple("Custom").field(arg0).finish(),
            Self::Preview(arg0) => f.debug_tuple("Preview").field(arg0).finish(),
            Self::AnimatedWindow(arg0) => f.debug_tuple("AnimatedWindow").field(arg0).finish(),
            Self::LayerSurface(arg0) => f.debug_tuple("LayerSurface").field(arg0).finish(),
            Self::_GenericCatcher(arg0) => f.debug_tuple("_GenericCatcher").field(arg0).finish(),
        }
    }
}

pub fn space_preview_elements<'a, R, C>(
    renderer: &'a mut R,
    space: &'a Space<WindowElement>,
    output: &'a Output,
) -> impl Iterator<Item = C> + 'a
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + Send + 'static,
    C: From<CropRenderElement<RelocateRenderElement<RescaleRenderElement<WindowRenderElement<R>>>>> + 'a,
{
    let constrain_behavior = ConstrainBehavior {
        reference: ConstrainReference::BoundingBox,
        behavior: ConstrainScaleBehavior::Fit,
        align: ConstrainAlign::CENTER,
    };

    let preview_padding = 10;

    let elements_on_space = space.elements_for_output(output).count();
    let output_scale = output.current_scale().fractional_scale();
    let output_transform = output.current_transform();
    let output_size = output
        .current_mode()
        .map(|mode| {
            output_transform
                .transform_size(mode.size)
                .to_f64()
                .to_logical(output_scale)
        })
        .unwrap_or_default();

    let max_elements_per_row = 4;
    let elements_per_row = usize::min(elements_on_space, max_elements_per_row);
    let rows = f64::ceil(elements_on_space as f64 / elements_per_row as f64);

    let preview_size = Size::from((
        f64::round(output_size.w / elements_per_row as f64) as i32 - preview_padding * 2,
        f64::round(output_size.h / rows) as i32 - preview_padding * 2,
    ));

    space
        .elements_for_output(output)
        .enumerate()
        .flat_map(move |(element_index, window)| {
            let column = element_index % elements_per_row;
            let row = element_index / elements_per_row;
            let preview_location = Point::from((
                preview_padding + (preview_padding + preview_size.w) * column as i32,
                preview_padding + (preview_padding + preview_size.h) * row as i32,
            ));
            let constrain = Rectangle::new(preview_location, preview_size);
            constrain_space_element(
                renderer,
                window,
                preview_location,
                1.0,
                output_scale,
                constrain,
                constrain_behavior,
            )
        })
}

/// Build the per-frame element list for `output`.
///
/// `workspace` lets the caller drive per-window animation: when `Some`, we
/// walk the workspace's `iter_z_order` ourselves and wrap each window in a
/// `RelocateRenderElement` whose offset comes from
/// `WindowElement::anim_state().render_offset`. Workspace-level translation
/// (`Workspace::render_offset`) folds into the same per-window offset so a
/// workspace slide moves every window in lockstep. Per-window alpha and
/// workspace alpha multiply through `WindowElement::render_elements`'s alpha
/// parameter.
///
/// When `workspace` is `None` (e.g. before bootstrap completes or for outputs
/// we haven't yet wired into `OutputState`), we fall back to the smithay
/// `space_render_elements` path which renders without animation offsets but
/// keeps the compositor producing pixels.
///
/// Layer-shell (panels, lock screens, etc.) is rendered manually here in both
/// paths so that animated workspaces don't break panels. The walk mirrors
/// smithay's `space_render_elements`: Background/Bottom go below windows,
/// Top/Overlay go above.
///
/// Returns a third value, `shadow_elements`, that is a list of
/// `(insertion_index, PixelShaderElement)` pairs the caller is expected
/// to splice into the final element list AFTER its corresponding window
/// element (so the shadow draws below the window). This is split out
/// because `PixelShaderElement` only implements `RenderElement<GlesRenderer>`
/// — the udev path uses `MultiRenderer` and cannot consume them, so it
/// must pass `shadow_params: None` and ignore the returned vec. The winit
/// path passes its compiled program and per-frame params and is
/// responsible for splicing.
#[profiling::function]
pub fn output_elements<R>(
    output: &Output,
    space: &Space<WindowElement>,
    workspace: Option<&Workspace>,
    custom_elements: impl IntoIterator<Item = CustomRenderElements<R>>,
    renderer: &mut R,
    show_window_preview: bool,
    shadow_params: Option<ShadowParams<'_>>,
) -> (
    Vec<OutputRenderElements<R, WindowRenderElement<R>>>,
    Color32F,
    Vec<(usize, PixelShaderElement)>,
)
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + Send + 'static,
{
    if let Some(window) = output
        .user_data()
        .get::<FullscreenSurface>()
        .and_then(|f| f.get())
    {
        let scale = output.current_scale().fractional_scale().into();
        let window_render_elements: Vec<WindowRenderElement<R>> =
            AsRenderElements::<R>::render_elements(&window, renderer, (0, 0).into(), scale, 1.0);

        let elements = custom_elements
            .into_iter()
            .map(OutputRenderElements::from)
            .chain(
                window_render_elements
                    .into_iter()
                    .map(|e| OutputRenderElements::Window(Wrap::from(e))),
            )
            .collect::<Vec<_>>();
        // Fullscreen windows take the whole output: no shadow makes sense.
        (elements, CLEAR_COLOR_FULLSCREEN, Vec::new())
    } else {
        let mut output_render_elements = custom_elements
            .into_iter()
            .map(OutputRenderElements::from)
            .collect::<Vec<_>>();

        if show_window_preview && space.elements_for_output(output).count() > 0 {
            output_render_elements.extend(space_preview_elements(renderer, space, output));
        }

        let output_scale = output.current_scale().fractional_scale();
        let scale: Scale<f64> = Scale::from(output_scale);
        let output_geo = space.output_geometry(output);

        // ---- Layer-shell (Top + Overlay): rendered ABOVE windows. Collected
        //      here, appended at the end. Walk order mirrors smithay's
        //      `space_render_elements`: layers are iterated in reverse so the
        //      topmost (last in the LayerMap deque) renders first.
        let mut top_layer_elements: Vec<OutputRenderElements<R, WindowRenderElement<R>>> = Vec::new();
        // ---- Layer-shell (Background + Bottom): rendered BELOW windows. We
        //      append to `output_render_elements` immediately so the z-order
        //      ends up [custom, bg/bottom layers, windows, top/overlay layers].
        {
            let layer_map = layer_map_for_output(output);
            for surface in layer_map.layers().rev() {
                let Some(geo) = layer_map.layer_geometry(surface) else {
                    continue;
                };
                let physical_loc = geo.loc.to_physical_precise_round(output_scale);
                let surface_elements: Vec<WaylandSurfaceRenderElement<R>> =
                    AsRenderElements::<R>::render_elements(
                        surface,
                        renderer,
                        physical_loc,
                        scale,
                        1.0,
                    );
                match surface.layer() {
                    WlrLayer::Top | WlrLayer::Overlay => {
                        top_layer_elements.extend(
                            surface_elements
                                .into_iter()
                                .map(OutputRenderElements::LayerSurface),
                        );
                    }
                    WlrLayer::Background | WlrLayer::Bottom => {
                        output_render_elements.extend(
                            surface_elements
                                .into_iter()
                                .map(OutputRenderElements::LayerSurface),
                        );
                    }
                }
            }
        }

        // ---- Per-window drop-shadow elements (winit path only). We
        //      collect (insertion_index, PixelShaderElement) and surface
        //      them to the caller. The caller's responsibility is to
        //      splice these after the corresponding window's elements
        //      (so the shadow draws beneath the window). The udev path
        //      passes `shadow_params: None` and this stays empty.
        let mut shadow_elements: Vec<(usize, PixelShaderElement)> = Vec::new();

        // ---- Toplevel windows.
        if let Some(ws) = workspace {
            // Workspace-level translation applies to every window in the
            // workspace. Combined with each window's own render_offset.
            let ws_offset = ws.render_offset.value();
            let ws_alpha = ws.alpha.value();

            // `iter_z_order` returns bottom-to-top across bands. Render
            // elements expect top-to-bottom (front-most first); reverse so
            // the topmost window's elements come first in the list.
            let entries: Vec<_> = ws.iter_z_order().collect();
            for entry in entries.into_iter().rev() {
                let anim = entry.element.anim_state();
                let win_offset = anim.render_offset.lock().unwrap().value();
                let win_alpha = anim.alpha.lock().unwrap().value();
                let combined_alpha = (win_alpha * ws_alpha).clamp(0.0, 1.0);

                // Mirror smithay's render_elements_for_region: subtract the
                // output's logical origin so windows on output 1 don't end up
                // off-screen on output 0. `output_geo.loc` is the workspace-
                // global origin of this output's region.
                let region_origin = output_geo.map(|g| g.loc).unwrap_or_default();
                // smithay's `render_location = location - element.geometry().loc`
                // — see Space::render_elements_for_region. We mirror that so
                // SSD-decorated and bbox-offset windows render at the same
                // place they did under the smithay path.
                let element_geo_loc =
                    smithay::desktop::space::SpaceElement::geometry(&entry.element).loc;
                let logical_loc = entry.location - region_origin - element_geo_loc;
                let physical_loc = logical_loc.to_physical_precise_round(output_scale);

                // Render the window at its base location, then relocate by
                // the animation offset (workspace + per-window).
                let window_render_elements: Vec<WindowRenderElement<R>> =
                    AsRenderElements::<R>::render_elements(
                        &entry.element,
                        renderer,
                        physical_loc,
                        scale,
                        combined_alpha,
                    );

                // Convert the combined logical offset to physical and wrap.
                let combined_offset_logical = Point::<f64, smithay::utils::Logical>::from((
                    ws_offset.x + win_offset.x,
                    ws_offset.y + win_offset.y,
                ));
                let combined_offset_physical: Point<i32, smithay::utils::Physical> = (
                    (combined_offset_logical.x * output_scale).round() as i32,
                    (combined_offset_logical.y * output_scale).round() as i32,
                )
                    .into();

                // P4-V5: per-window drop-shadow `PixelShaderElement`.
                // Only emitted when `shadow_params` is `Some` (winit
                // path). The shadow rect is the window's visible
                // logical bbox (location after animation offset, sized
                // by the SpaceElement geometry). The element is staged
                // into `shadow_elements` with the index it will occupy
                // *after* this window's render elements are pushed
                // below; the caller splices at that index so the shadow
                // sits BELOW the window in the final list (later in the
                // list = farther back).
                //
                // Rounded corners are intentionally not shipped here:
                // the current `rounded.rs` shader writes a white masked
                // shape, which would tint the window rather than mask
                // it. Real rounded corners need a texture-shader rewrite
                // (sample window, discard outside-corner pixels) — see
                // `effects/rounded.rs` and the research doc. Shipping
                // shadow first validates the visual story.
                if let Some(shadow) = shadow_params {
                    let win_geo =
                        smithay::desktop::space::SpaceElement::geometry(&entry.element);
                    // Window's on-screen logical rect: the entry
                    // location (workspace-global) translated to
                    // output-local by subtracting the region origin,
                    // plus the per-window+workspace animation offset.
                    let region_origin = output_geo.map(|g| g.loc).unwrap_or_default();
                    let logical_loc_screen = entry.location - region_origin
                        + Point::<i32, smithay::utils::Logical>::from((
                            combined_offset_logical.x.round() as i32,
                            combined_offset_logical.y.round() as i32,
                        ));
                    let shadow_rect = Rectangle::new(logical_loc_screen, win_geo.size);
                    let element = shadow.effect.pixel_shader_element(
                        shadow_rect,
                        shadow.blur_radius,
                        shadow.color,
                        shadow.offset,
                    );
                    // The window's render elements are pushed
                    // immediately after this; shadow goes AT the index
                    // *after* those window elements (current_len +
                    // window_render_elements.len()).
                    let insert_at = output_render_elements.len() + window_render_elements.len();
                    shadow_elements.push((insert_at, element));
                }

                output_render_elements.extend(window_render_elements.into_iter().map(|el| {
                    OutputRenderElements::AnimatedWindow(RelocateRenderElement::from_element(
                        el,
                        combined_offset_physical,
                        Relocate::Relative,
                    ))
                }));
            }
        } else {
            // Fallback: no workspace bootstrap for this output yet. Render
            // via smithay's space helper so the screen is never blank during
            // the brief bootstrap window.
            //
            // NOTE: `space_render_elements` ALSO walks layer-shell. We've
            // already rendered layers above; to avoid double-rendering them
            // here we use `Space::render_elements_for_region` directly,
            // which by smithay's docs explicitly excludes layer surfaces.
            if let Some(geo) = output_geo {
                let region_elements: Vec<WindowRenderElement<R>> = space
                    .render_elements_for_region(renderer, &geo, scale, 1.0)
                    .into_iter()
                    .collect();
                output_render_elements.extend(
                    region_elements
                        .into_iter()
                        .map(|e| OutputRenderElements::Window(Wrap::from(e))),
                );
            }
        }

        // Append top/overlay layer-shell on top of everything else.
        // shadow_elements indices were captured before this append so
        // shadows still splice below windows (the splice index is
        // bounded by where the windows landed, not where layer-shell
        // ends).
        output_render_elements.extend(top_layer_elements);

        (output_render_elements, CLEAR_COLOR, shadow_elements)
    }
}


// Winit-specific wrapper that lets the damage tracker accept a flat
// `Vec` containing both `OutputRenderElements<GlesRenderer, _>`
// produced by `output_elements` AND the per-window shadow
// `PixelShaderElement`s. The `<=$renderer:ty>` macro form restricts the
// generated `RenderElement` impl to a specific renderer (here
// `GlesRenderer`), which is the only renderer that can host
// `PixelShaderElement`.
//
// The udev path doesn't use this — its renderer is `MultiRenderer` and
// can't host `PixelShaderElement` until smithay grows the impl.
smithay::backend::renderer::element::render_elements! {
    pub WinitOutputElements<=smithay::backend::renderer::gles::GlesRenderer>;
    Inner=OutputRenderElements<
        smithay::backend::renderer::gles::GlesRenderer,
        WindowRenderElement<smithay::backend::renderer::gles::GlesRenderer>,
    >,
    Shadow=PixelShaderElement,
}

impl std::fmt::Debug for WinitOutputElements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inner(arg0) => f.debug_tuple("Inner").field(arg0).finish(),
            Self::Shadow(arg0) => f.debug_tuple("Shadow").field(arg0).finish(),
            Self::_GenericCatcher(arg0) => f.debug_tuple("_GenericCatcher").field(arg0).finish(),
        }
    }
}

/// Splice `shadow_inserts` into `inner_elements` at their recorded
/// indices, producing a single `Vec<WinitOutputElements>` ready to hand
/// to the damage tracker. Insert indices are interpreted with respect to
/// the original (pre-splice) `inner_elements` ordering — we sort by
/// index ascending and add `i` to the i-th insert's position so each
/// later insert accounts for the shadows already spliced before it.
pub fn splice_winit_elements(
    inner_elements: Vec<OutputRenderElements<
        smithay::backend::renderer::gles::GlesRenderer,
        WindowRenderElement<smithay::backend::renderer::gles::GlesRenderer>,
    >>,
    mut shadow_inserts: Vec<(usize, PixelShaderElement)>,
) -> Vec<WinitOutputElements> {
    // Wrap the inner elements first.
    let mut out: Vec<WinitOutputElements> = inner_elements
        .into_iter()
        .map(WinitOutputElements::Inner)
        .collect();

    if shadow_inserts.is_empty() {
        return out;
    }

    // Sort ascending so each subsequent insert sees indices in the
    // already-grown vec. We add the running offset for each prior
    // insertion.
    shadow_inserts.sort_by_key(|(idx, _)| *idx);
    for (offset, (idx, element)) in shadow_inserts.into_iter().enumerate() {
        let pos = (idx + offset).min(out.len());
        out.insert(pos, WinitOutputElements::Shadow(element));
    }
    out
}


#[allow(clippy::too_many_arguments)]
pub fn render_output<'a, 'd, R>(
    output: &'a Output,
    space: &'a Space<WindowElement>,
    workspace: Option<&'a Workspace>,
    custom_elements: impl IntoIterator<Item = CustomRenderElements<R>>,
    renderer: &'a mut R,
    framebuffer: &'a mut R::Framebuffer<'_>,
    damage_tracker: &'d mut OutputDamageTracker,
    age: usize,
    show_window_preview: bool,
) -> Result<RenderOutputResult<'d>, OutputDamageTrackerError<R::Error>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + Send + 'static,
{
    // `render_output` is the smithay-style helper: it doesn't know about
    // backend-specific effects (shadow needs `GlesRenderer`). Callers
    // wanting shadow use `output_elements` directly + their own damage
    // tracker invocation. We pass `None` here.
    let (elements, clear_color, _shadow_elements) = output_elements(
        output,
        space,
        workspace,
        custom_elements,
        renderer,
        show_window_preview,
        None,
    );
    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}
