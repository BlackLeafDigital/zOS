//! Rounded-corners effect.
//!
//! Implements rounded window corners by drawing a `PixelShaderElement`
//! over the window. The fragment shader computes the signed distance
//! from the nearest corner and uses smoothstep for a 1-pixel anti-aliased
//! edge; pixels outside the rounded shape get alpha=0.
//!
//! ## Smithay pixel shader contract (recap)
//!
//! From `GlesRenderer::compile_custom_pixel_shader` (smithay 27af99e,
//! `src/backend/renderer/gles/mod.rs:1945`):
//!
//! - Shader source must NOT contain a `#version` directive (smithay
//!   prepends `#version 100`).
//! - Smithay provides:
//!     - `varying vec2 v_coords` — normalized 0..1 across the rendered rect.
//!     - `uniform vec2  size`    — viewport size in buffer pixels.
//!     - `uniform float alpha`   — alpha passed to `render_pixel_shader_to`.
//!     - `uniform float tint`    — only when `DEBUG_FLAGS` is defined.
//! - Additional uniforms are declared via `UniformName`s and then bound
//!   per-frame via `Uniform::new(name, value)` on the
//!   `PixelShaderElement`.
//!
//! ## Usage (future, once shader is threaded through backends)
//!
//! ```ignore
//! // Backend init (udev.rs / winit.rs):
//! let rounded = RoundedCornersEffect::new(&mut gles_renderer)?;
//! state.effects.rounded = Some(rounded);
//!
//! // Per-frame in the render path:
//! let shader_element = rounded.pixel_shader_element(window_geometry, 8.0);
//! // ...wrap and append to the element list above the window content.
//! ```
//!
//! TODO(P4-effect-init): wire `RoundedCornersEffect::new` into backend
//! init in `udev.rs` and `winit.rs`. Holding pattern: each backend
//! constructs one effect per `GlesRenderer` (so per-GPU under the udev
//! `MultiRenderer`) and stashes the program on `BackendData`. The render
//! path then borrows the program to build per-window shader elements.
//!
//! TODO(P4-rounded-corners-config): replace the hardcoded radius arg with
//! a config-driven `state.corner_radius: f32` once we add a config field.
//! Default 8.0 px matches the reference Catppuccin theme.

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

/// GLSL source for the rounded-corners pixel shader.
///
/// Smithay requires no `#version` directive (it prepends `#version 100`
/// itself). The smithay pixel-shader vertex stage emits a varying
/// `v_coords` in normalized 0..1 across the rect; multiplying by
/// `size` (also injected by smithay) gives buffer-pixel coordinates.
///
/// We compute distance to the nearest corner: outside the rounded
/// quadrant, alpha is 0 (transparent corner cut); a one-pixel smoothstep
/// band on the edge gives anti-aliasing.
pub const ROUNDED_CORNERS_FRAGMENT_SHADER: &str = r#"
//_DEFINES_
precision mediump float;

varying vec2 v_coords;

uniform vec2  size;
uniform float alpha;

uniform float v_radius;
uniform vec4  v_color;

void main() {
    // v_coords is 0..1 across the element's rect; convert to pixels.
    vec2 px = v_coords * size;

    // Distance from the nearest corner of the rect.
    vec2 dist_from_edge = min(px, size - px);
    float corner_dist = length(max(vec2(v_radius) - dist_from_edge, vec2(0.0)));

    // Smoothstep over one pixel for anti-aliased rounded corners.
    float a = 1.0 - smoothstep(v_radius - 1.0, v_radius, corner_dist);

    gl_FragColor = v_color * alpha * a;
}
"#;

/// Compiled rounded-corners shader holder.
///
/// Holds one compiled program per `GlesRenderer`. For udev's
/// `MultiRenderer` (one `GlesRenderer` per GPU) the caller stashes one
/// per backing renderer.
#[derive(Debug, Clone)]
pub struct RoundedCornersEffect {
    /// The compiled GLES pixel-shader program. Cheap to clone (`Arc`
    /// inside).
    pub program: GlesPixelProgram,
}

impl RoundedCornersEffect {
    /// Compile the shader. Call once per `GlesRenderer` at backend init.
    ///
    /// Returns `GlesError` from `compile_custom_pixel_shader` if the
    /// shader fails to compile or link (driver-dependent — should not
    /// happen with a valid driver and the bundled GLSL source).
    pub fn new(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let program = renderer.compile_custom_pixel_shader(
            ROUNDED_CORNERS_FRAGMENT_SHADER,
            &[
                UniformName::new("v_radius", UniformType::_1f),
                UniformName::new("v_color", UniformType::_4f),
            ],
        )?;
        Ok(Self { program })
    }

    /// Build a `PixelShaderElement` that draws the rounded-corner mask
    /// for a window at `geometry` with the given corner `radius` in
    /// logical pixels.
    ///
    /// For v1 this draws an opaque white tint over the window, with the
    /// corners cut to alpha=0. The caller is responsible for layering
    /// this above the window content; the final composite blends
    /// according to the element's alpha and `v_color`.
    ///
    /// `geometry` is the window's logical rect (top-left + size).
    /// `radius` is in logical pixels (typically 8.0 to match
    /// Catppuccin/zOS defaults).
    ///
    /// We deliberately pass `None` for opaque regions: the rounded mask
    /// is *not* opaque (the corner pixels are transparent), and lying
    /// to the damage tracker would produce visual artifacts. See the
    /// "Opaque-region tracking" note in
    /// `docs/research/phase-4-smithay-effects.md` §4.1.
    pub fn pixel_shader_element(
        &self,
        geometry: Rectangle<i32, Logical>,
        radius: f32,
    ) -> PixelShaderElement {
        PixelShaderElement::new(
            self.program.clone(),
            geometry,
            None,
            1.0,
            vec![
                Uniform::new("v_radius", radius),
                Uniform::new("v_color", [1.0_f32, 1.0, 1.0, 1.0]),
            ],
            // `Kind::Unspecified` is the right default for a per-window
            // overlay; `Kind::Cursor` would mark it as rarely-changing
            // (wrong) and `Kind::ScanoutCandidate` would hint the DRM
            // backend to try plane scanout (also wrong for a mask).
            Kind::Unspecified,
        )
    }
}
