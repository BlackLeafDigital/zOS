//! Rounded-corners effect — texture-shader based.
//!
//! Unlike the previous pixel-shader version (which only knew how to draw a
//! solid-colored mask *over* the window and therefore ended up tinting the
//! window content rather than rounding it), this version compiles a custom
//! **texture shader** via [`GlesRenderer::compile_custom_texture_shader`].
//! A texture shader replaces the default sampling shader for a window's
//! own content texture, so the GPU can fade-out fragments outside the
//! rounded shape and leave the un-corner pixels of the window untouched.
//!
//! ## Smithay texture-shader contract (recap)
//!
//! From `GlesRenderer::compile_custom_texture_shader` (smithay 27af99e,
//! `src/backend/renderer/gles/mod.rs:2074`):
//!
//! - Shader source must NOT contain a `#version` directive (smithay
//!   prepends `#version 100`).
//! - The shader must contain the literal line `//_DEFINES_`; smithay
//!   substitutes `#define` directives there. Three variants:
//!     - `EXTERNAL`   — sample via `samplerExternalOES` (dmabuf, XWayland).
//!     - `NO_ALPHA`   — ignore the texture's alpha (replace with 1.0).
//!     - `DEBUG_FLAGS` — `tint` uniform is in scope.
//! - Smithay always provides:
//!     - `varying  vec2 v_coords` — texture sample coords (0..1 over src).
//!     - `uniform  sampler2D tex` (or `samplerExternalOES tex` if EXTERNAL).
//!     - `uniform  float alpha`   — alpha multiplier from the renderer.
//!     - `uniform  float tint`    — only when DEBUG_FLAGS is defined.
//! - Additional uniforms are declared via `UniformName`s and bound
//!   per-element via `Uniform::new` on the `TextureShaderElement`.
//!
//! ## Why a texture shader (and not a pixel shader)
//!
//! The pixel-shader path (used for [`crate::effects::shadow`]) can only
//! produce solid output — there's no access to the window's content
//! texture. To round a window's corners we need to *clip* the window's
//! own pixels, which means hooking the texture-sampling stage. The
//! texture shader does exactly that.
//!
//! ## Render-path integration
//!
//! TODO(P4-rounded-render-integration): the render path
//! (`crate::render`) currently builds default `WaylandSurfaceRenderElement`
//! /`TextureRenderElement` items directly. To apply this effect, each
//! window's underlying `TextureRenderElement<GlesTexture>` must be wrapped
//! in a [`smithay::backend::renderer::gles::element::TextureShaderElement`]
//! using [`RoundedCornersEffect::program`] plus per-window uniforms
//! (`v_window_size`, `v_radius`). That wiring is a follow-up task; for
//! now this module just compiles + stores the program so backend init
//! (`udev.rs`, `winit.rs`) can stash it on `BackendData`.
//!
//! TODO(P4-rounded-corners-config): replace the hardcoded radius (set
//! per-element by the future render-integration code) with a config-driven
//! `state.corner_radius: f32` once the config surface gains the field.
//! Default 8.0 px matches the Catppuccin/zOS reference theme.

use smithay::backend::renderer::gles::{
    GlesError, GlesRenderer, GlesTexProgram, UniformName, UniformType,
};

/// GLSL source for the rounded-corners **texture shader**.
///
/// Smithay prepends `#version 100` and substitutes `//_DEFINES_` with
/// `#define EXTERNAL` / `#define NO_ALPHA` / `#define DEBUG_FLAGS` as
/// applicable. We mirror the structure of smithay's bundled
/// `texture.frag` (see
/// `smithay/src/backend/renderer/gles/shaders/implicit/texture.frag`)
/// so EXTERNAL textures (XWayland, EGL dmabuf import) and NO_ALPHA
/// formats are handled correctly — anything less and the shader either
/// fails to compile on the EXTERNAL variant or produces wrong output on
/// XRGB/opaque buffers.
///
/// The corner-fade math is identical to the previous pixel-shader version
/// (see git history): compute the distance from the nearest rounded
/// corner's center, then `1.0 - smoothstep(r-1, r, d)` for a 1-pixel
/// anti-aliased edge. The result is multiplied into the sampled
/// fragment's alpha so the corner pixels of the window's actual content
/// fade to transparent — *that's* the round.
pub const ROUNDED_CORNERS_TEXTURE_SHADER: &str = r#"
//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;

#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec2  v_window_size;
uniform float v_radius;

void main() {
    // v_coords is 0..1 over the source rect. Convert to a pixel
    // coordinate within the window so the corner math is in the same
    // units as `v_radius`.
    vec2 pos = v_coords * v_window_size;

    // Distance from the nearest corner of the rect.
    vec2 dist_from_edge = min(pos, v_window_size - pos);
    float corner_dist = length(max(vec2(v_radius) - dist_from_edge, vec2(0.0)));

    // 1.0 inside the rounded shape, 0.0 outside, with a 1-pixel
    // anti-aliased band on the edge.
    float mask_alpha = 1.0 - smoothstep(v_radius - 1.0, v_radius, corner_dist);

    vec4 color = texture2D(tex, v_coords);

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

    // Apply the rounded-corner mask to the final alpha. Multiplying
    // both rgb and a preserves premultiplied-alpha invariants (the
    // texture comes in premultiplied from smithay's import path).
    color = color * mask_alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Compiled rounded-corners shader holder.
///
/// Holds one compiled program per `GlesRenderer`. For udev's
/// `MultiRenderer` (one `GlesRenderer` per GPU) the caller stashes one
/// per backing renderer.
///
/// Note: unlike [`crate::effects::shadow::DropShadowEffect`] which holds
/// a [`GlesPixelProgram`], this holds a [`GlesTexProgram`] — texture
/// shaders are a different program type with a different render path.
///
/// [`GlesPixelProgram`]: smithay::backend::renderer::gles::GlesPixelProgram
#[derive(Debug, Clone)]
pub struct RoundedCornersEffect {
    /// The compiled GLES texture-shader program. Cheap to clone (`Arc`
    /// inside).
    pub program: GlesTexProgram,
}

impl RoundedCornersEffect {
    /// Compile the shader. Call once per `GlesRenderer` at backend init.
    ///
    /// Returns `GlesError` from `compile_custom_texture_shader` if the
    /// shader fails to compile or link (driver-dependent — should not
    /// happen with a valid driver and the bundled GLSL source). Backend
    /// init should treat the error as non-fatal: log + continue without
    /// rounded corners, matching the shadow-effect failure mode.
    pub fn new(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let program = renderer.compile_custom_texture_shader(
            ROUNDED_CORNERS_TEXTURE_SHADER,
            &[
                UniformName::new("v_window_size", UniformType::_2f),
                UniformName::new("v_radius", UniformType::_1f),
            ],
        )?;
        Ok(Self { program })
    }

    /// Borrow the compiled program. Used by the render path (once
    /// [`P4-rounded-render-integration`](self) is wired up) to construct
    /// per-window
    /// [`smithay::backend::renderer::gles::element::TextureShaderElement`]s,
    /// passing per-window `v_window_size` and `v_radius` uniforms.
    pub fn program(&self) -> &GlesTexProgram {
        &self.program
    }
}
