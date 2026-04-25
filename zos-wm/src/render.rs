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
    shell::{FullscreenSurface, WindowElement, WindowRenderElement, workspace::Workspace},
};

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
#[profiling::function]
pub fn output_elements<R>(
    output: &Output,
    space: &Space<WindowElement>,
    workspace: Option<&Workspace>,
    custom_elements: impl IntoIterator<Item = CustomRenderElements<R>>,
    renderer: &mut R,
    show_window_preview: bool,
) -> (Vec<OutputRenderElements<R, WindowRenderElement<R>>>, Color32F)
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
        (elements, CLEAR_COLOR_FULLSCREEN)
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

                // TODO(P4-rounded-corners): once `RoundedCornersEffect`
                // is initialized per-renderer at backend init
                // (udev.rs / winit.rs), append a `PixelShaderElement`
                // per window here, layered above the window's content.
                // See `zos-wm/src/effects/rounded.rs`. The shape is
                // roughly:
                //
                //   if let Some(effect) = backend.rounded_corners() {
                //       let geometry = Rectangle::new(
                //           logical_loc + offset, window_size);
                //       let mask = effect.pixel_shader_element(
                //           geometry, /* radius */ 8.0);
                //       output_render_elements.push(/* wrap mask */);
                //   }
                //
                // Blocked on threading the GlesRenderer-typed shader
                // handle through `output_elements`'s renderer-generic
                // `R` parameter, which needs either a downcast helper
                // or a backend-specific render path.
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
        output_render_elements.extend(top_layer_elements);

        (output_render_elements, CLEAR_COLOR)
    }
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
    let (elements, clear_color) =
        output_elements(output, space, workspace, custom_elements, renderer, show_window_preview);
    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}
