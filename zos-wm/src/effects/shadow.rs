//! Drop-shadow effect.
//!
//! Closed-form gaussian-ish approximation: signed distance from the
//! window rect, then `1.0 - smoothstep(0.0, blur_radius, abs(dist))` for
//! the shadow density. Cheap on the GPU, no multi-pass blur required.
//!
//! ## Smithay pixel shader contract (recap)
//!
//! Same as `crate::effects::rounded` — see that module for the long form.
//! Quick recap:
//!
//! - No `#version` directive (smithay prepends `#version 100`).
//! - The vertex stage emits `varying vec2 v_coords` in 0..1 across the
//!   element rect.
//! - Smithay injects `uniform vec2 size`, `uniform float alpha`, and
//!   (when DEBUG_FLAGS) `uniform float tint`.
//! - Additional uniforms are declared via `UniformName` and then bound
//!   per-frame via `Uniform::new`.
//!
//! ## Geometry
//!
//! The element rect we hand to `PixelShaderElement::new` is the window
//! rect *expanded* by `blur_radius` on every side, plus the absolute
//! offset, so the shadow has room to fade out beyond the window edge
//! and offset direction. Inside the (un-offset) window rect we write
//! alpha=0 — the window draws on top there, and we don't want shadow
//! bleeding through transparent window pixels for v1.
//!
//! TODO(P4-render-integration): same render-path integration story as
//! rounded corners — the per-window `PixelShaderElement` push is blocked
//! by the `MultiRenderer` trait gap (`PixelShaderElement` only impls
//! `RenderElement<GlesRenderer>`). The shader is compiled at backend
//! init and stored on `BackendData`; `crate::render` has the TODO marker.

use smithay::{
    backend::renderer::{
        element::Kind,
        gles::{
            element::PixelShaderElement, GlesError, GlesPixelProgram, GlesRenderer, Uniform,
            UniformName, UniformType,
        },
    },
    utils::{Logical, Rectangle},
};

/// GLSL source for the drop-shadow pixel shader.
///
/// Smithay requires no `#version` directive. The fragment expands the
/// element-local 0..1 `v_coords` into a pixel coordinate relative to the
/// (logical) window's top-left, accounting for the offset (positive
/// offset → shadow to the right/down).
///
/// We then take the signed distance from the window rect; outside the
/// rect, alpha falls off via `smoothstep(0.0, blur_radius, dist)`.
/// Inside the window rect, alpha=0 (the window draws there).
pub const DROP_SHADOW_FRAGMENT_SHADER: &str = r#"
//_DEFINES_
precision mediump float;

varying vec2 v_coords;

uniform vec2  size;
uniform float alpha;

uniform vec2  v_window_size;
uniform vec2  v_offset;
uniform float v_blur_radius;
uniform vec4  v_color;

void main() {
    // v_coords is in [0,1] over the *expanded* element rect. The element
    // is (window + 2*blur on each axis + 2*|offset|), so convert v_coords
    // to a pixel coordinate relative to the window's top-left, then
    // shift by the shadow offset (positive offset → shadow to the
    // right/down, so the visible shadow center moves the *opposite*
    // direction in window-local space).
    vec2 pos = v_coords * size - vec2(v_blur_radius) - abs(v_offset) - v_offset * vec2(-1.0, -1.0);

    // Signed distance from the window rectangle edges. Negative inside,
    // positive outside; for outside, length() of the per-axis positive
    // overshoot is the Euclidean distance to the nearest edge.
    vec2 d = max(vec2(0.0) - pos, pos - v_window_size);
    float outside_dist = length(max(d, vec2(0.0)));

    // Smoothstep from full opacity at the rect edge to 0 at blur_radius.
    float a = 1.0 - smoothstep(0.0, v_blur_radius, outside_dist);

    // Inside the window rect, force alpha=0 — the window draws there
    // and we don't want a uniform tint behind it (or shadow bleeding
    // through transparent window content for v1).
    if (pos.x >= 0.0 && pos.y >= 0.0 && pos.x < v_window_size.x && pos.y < v_window_size.y) {
        a = 0.0;
    }

    gl_FragColor = v_color * alpha * a;
}
"#;

/// Compiled drop-shadow shader holder.
///
/// Holds one compiled program per `GlesRenderer`. For udev's
/// `MultiRenderer` (one `GlesRenderer` per GPU) the caller stashes one
/// per backing renderer, mirroring `RoundedCornersEffect`.
#[derive(Debug, Clone)]
pub struct DropShadowEffect {
    /// The compiled GLES pixel-shader program. Cheap to clone (`Arc`
    /// inside).
    pub program: GlesPixelProgram,
}

impl DropShadowEffect {
    /// Compile the shader. Call once per `GlesRenderer` at backend init.
    ///
    /// Returns `GlesError` from `compile_custom_pixel_shader` on driver
    /// shader-compile / link failure. Callers should treat the error as
    /// non-fatal (log + skip) — degrading gracefully to "no shadow"
    /// matches the rounded-corners pattern.
    pub fn new(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let program = renderer.compile_custom_pixel_shader(
            DROP_SHADOW_FRAGMENT_SHADER,
            &[
                UniformName::new("v_window_size", UniformType::_2f),
                UniformName::new("v_offset", UniformType::_2f),
                UniformName::new("v_blur_radius", UniformType::_1f),
                UniformName::new("v_color", UniformType::_4f),
            ],
        )?;
        Ok(Self { program })
    }

    /// Build a `PixelShaderElement` rendering the drop shadow for a
    /// window at `window_geometry`.
    ///
    /// The element rect is expanded by `blur_radius` on every side plus
    /// `|offset|` so the shadow has room to fade and shift.
    /// `offset` is the (x, y) shadow offset in logical pixels relative
    /// to the window (e.g., `(0.0, 4.0)` for a typical "drop" shadow).
    /// `color` is premultiplied RGBA in 0..1.
    ///
    /// As with `RoundedCornersEffect` we deliberately pass `None` for
    /// opaque regions: the shadow is a soft-edge alpha fade, lying to
    /// the damage tracker would smear pixels. The caller is responsible
    /// for layering this *below* the window content.
    pub fn pixel_shader_element(
        &self,
        window_geometry: Rectangle<i32, Logical>,
        blur_radius: f32,
        color: [f32; 4],
        offset: (f32, f32),
    ) -> PixelShaderElement {
        // Pad the rect by blur_radius on each side, plus enough room for
        // the absolute offset on each axis, so the shadow doesn't get
        // clipped at the element bounds.
        let pad = blur_radius.ceil() as i32;
        let off_x = offset.0.abs().ceil() as i32;
        let off_y = offset.1.abs().ceil() as i32;

        let expanded = Rectangle::new(
            (
                window_geometry.loc.x - pad - off_x,
                window_geometry.loc.y - pad - off_y,
            )
                .into(),
            (
                window_geometry.size.w + 2 * (pad + off_x),
                window_geometry.size.h + 2 * (pad + off_y),
            )
                .into(),
        );

        PixelShaderElement::new(
            self.program.clone(),
            expanded,
            None,
            1.0,
            vec![
                Uniform::new(
                    "v_window_size",
                    (window_geometry.size.w as f32, window_geometry.size.h as f32),
                ),
                Uniform::new("v_offset", offset),
                Uniform::new("v_blur_radius", blur_radius),
                Uniform::new("v_color", color),
            ],
            // Same reasoning as rounded corners: a per-window overlay
            // is `Kind::Unspecified`; nothing about a shadow makes it a
            // cursor or scanout candidate.
            Kind::Unspecified,
        )
    }
}
