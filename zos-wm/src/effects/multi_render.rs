//! Wrappers letting GLES-only `RenderElement`s ride the udev `MultiRenderer`
//! pipeline.
//!
//! ## The gap we're filling
//!
//! Smithay's [`PixelShaderElement`] (and [`TextureShaderElement`]) implement
//! [`RenderElement<GlesRenderer>`] only — direct GLES, single-context. The
//! winit backend uses a `GlesRenderer` directly so it can render shader
//! elements without ceremony. The udev backend, however, drives DRM through a
//! [`MultiRenderer<'_, '_, GbmGlesBackend<GlesRenderer, _>, GbmGlesBackend<GlesRenderer, _>>`]
//! — the multi-GPU layer that abstracts over a render device + a target
//! device — and there's no `RenderElement<MultiRenderer<...>>` impl provided
//! by smithay. Pushing a `PixelShaderElement` straight into the udev path
//! therefore fails to compile.
//!
//! ## How smithay solves the analogous case for `GlowRenderer`
//!
//! See `~/.cargo/git/checkouts/smithay-*/27af99e/src/backend/renderer/glow.rs:629`:
//!
//! ```ignore
//! impl RenderElement<GlowRenderer> for PixelShaderElement {
//!     fn draw(&self, frame: &mut GlowFrame<'_, '_>, /* ... */) -> Result<(), GlesError> {
//!         RenderElement::<GlesRenderer>::draw(
//!             self,
//!             frame.borrow_mut(), // GlowFrame: BorrowMut<GlesFrame>
//!             /* ... */
//!         )
//!     }
//! }
//! ```
//!
//! `GlowFrame` impls `BorrowMut<GlesFrame>`, so smithay just hands the inner
//! `GlesFrame` back to the existing `GlesRenderer` impl.
//!
//! ## How `MultiFrame` exposes its inner `GlesFrame`
//!
//! `MultiFrame` impls
//!
//! ```ignore
//! impl<'frame, 'buffer, R: GraphicsApi, T: GraphicsApi>
//!     AsMut<<<R::Device as ApiDevice>::Renderer as RendererSuper>::Frame<'frame, 'buffer>>
//!     for MultiFrame<'_, '_, 'frame, 'buffer, R, T> { ... }
//! ```
//!
//! (smithay 27af99e, `src/backend/renderer/multigpu/mod.rs:1000`). For the
//! udev case where `R = GbmGlesBackend<GlesRenderer, _>`,
//! `<R::Device as ApiDevice>::Renderer` is `GlesRenderer`, so its
//! `Frame<'frame, 'buffer>` is `GlesFrame<'frame, 'buffer>`. Calling
//! `MultiFrame::as_mut()` therefore yields `&mut GlesFrame`, which is exactly
//! what `RenderElement<GlesRenderer>::draw` wants.
//!
//! ## What this module ships
//!
//! [`MultiRenderPixelShaderElement`] — newtype around [`PixelShaderElement`]
//! that impls [`RenderElement<MultiRenderer<...>>`] by delegating through
//! `MultiFrame::as_mut()`. Forwards all `Element` methods to the inner
//! element.
//!
//! [`TextureShaderElement`] would benefit from the same wrapper but is not
//! shipped here: applying it requires hooking each window's
//! `TextureRenderElement` at the surface-rendering boundary inside
//! `shell::WindowElement::render_elements`, which this module doesn't touch.
//! Documented in `effects/rounded.rs` (`P4-rounded-render-integration`).

use smithay::{
    backend::renderer::{
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        gles::{
            element::PixelShaderElement, GlesError, GlesFrame, GlesRenderer,
        },
        multigpu::{ApiDevice, Error as MultiError, GraphicsApi, MultiFrame, MultiRenderer},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer as BufferCoords, Physical, Point, Rectangle, Scale, Transform},
};

/// Newtype wrapper around [`PixelShaderElement`] that adds a
/// [`RenderElement<MultiRenderer<...>>`] impl by delegating through
/// `MultiFrame::as_mut()` to the inner [`GlesFrame`].
///
/// Pattern lifted from smithay's `glow.rs:629`
/// (`impl RenderElement<GlowRenderer> for PixelShaderElement`), adapted to
/// `MultiFrame`'s `AsMut<GlesFrame>` instead of `GlowFrame`'s `BorrowMut`.
///
/// Intended for the udev render path where the renderer is
/// `MultiRenderer<'_, '_, GbmGlesBackend<GlesRenderer, _>, GbmGlesBackend<GlesRenderer, _>>`.
/// On winit (which uses a bare `GlesRenderer`) this wrapper is unnecessary —
/// keep using the raw `PixelShaderElement` there.
#[derive(Debug)]
pub struct MultiRenderPixelShaderElement(pub PixelShaderElement);

impl Element for MultiRenderPixelShaderElement {
    #[inline]
    fn id(&self) -> &Id {
        self.0.id()
    }

    #[inline]
    fn current_commit(&self) -> CommitCounter {
        self.0.current_commit()
    }

    #[inline]
    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.0.src()
    }

    #[inline]
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.0.geometry(scale)
    }

    #[inline]
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.0.opaque_regions(scale)
    }

    #[inline]
    fn alpha(&self) -> f32 {
        self.0.alpha()
    }

    #[inline]
    fn kind(&self) -> Kind {
        self.0.kind()
    }

    #[inline]
    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.0.location(scale)
    }

    #[inline]
    fn transform(&self) -> Transform {
        self.0.transform()
    }

    #[inline]
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.0.damage_since(scale, commit)
    }
}

// `RenderElement` for any `MultiRenderer` whose render and target devices
// both produce a `GlesRenderer` (i.e., the udev `GbmGlesBackend<GlesRenderer, _>`
// configuration). The trait bounds spell out exactly the requirement we
// already verified at the type level: each side of the multi-renderer must
// be using the GLES backend, so `MultiFrame::as_mut()` resolves to
// `&mut GlesFrame`, and the inner `RenderElement<GlesRenderer>::draw` can
// run unchanged.
//
// We use trait equality bounds (`Renderer = GlesRenderer`,
// `Error = GlesError`) on the associated types rather than naming a
// concrete graphics-api type — this lets the impl cover the AMD,
// NVIDIA, and any future GLES-based GBM backend variants without
// duplication, while still constraining things tightly enough that
// `as_mut`/error coercion type-check.
impl<'render, 'target, R, T> RenderElement<MultiRenderer<'render, 'target, R, T>>
    for MultiRenderPixelShaderElement
where
    R: GraphicsApi + 'static,
    T: GraphicsApi + 'static,
    R::Error: 'static,
    T::Error: 'static,
    R::Device: ApiDevice<Renderer = GlesRenderer>,
    T::Device: ApiDevice<Renderer = GlesRenderer>,
    GlesError: Into<<MultiRenderer<'render, 'target, R, T> as smithay::backend::renderer::RendererSuper>::Error>,
{
    fn draw(
        &self,
        frame: &mut MultiFrame<'_, '_, '_, '_, R, T>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        cache: Option<&smithay::utils::user_data::UserDataMap>,
    ) -> Result<(), <MultiRenderer<'render, 'target, R, T> as smithay::backend::renderer::RendererSuper>::Error>
    {
        // Reach the inner `GlesFrame` through `MultiFrame`'s `AsMut` impl,
        // then forward to `PixelShaderElement`'s native
        // `RenderElement<GlesRenderer>::draw`. The lifetime gymnastics in
        // `MultiFrame`'s definition make the inner frame's lifetimes the
        // same `'frame, 'buffer` parameters as the `MultiFrame` itself, so
        // this is just a plain reborrow under the hood.
        let gles_frame: &mut GlesFrame<'_, '_> = frame.as_mut();
        RenderElement::<GlesRenderer>::draw(&self.0, gles_frame, src, dst, damage, opaque_regions, cache)
            .map_err(|e| MultiError::<R, T>::Render(e).into())
    }

    fn underlying_storage(
        &self,
        _renderer: &mut MultiRenderer<'render, 'target, R, T>,
    ) -> Option<UnderlyingStorage<'_>> {
        // `PixelShaderElement` returns `None` for its native
        // `underlying_storage` — there's no scanout-friendly buffer behind
        // a procedurally-generated shader. Mirror that here.
        None
    }
}

// NOTE: A `MultiRenderTextureShaderElement` wrapper for the rounded-corners
// `TextureShaderElement` is *not* provided here. Wiring rounded corners
// requires intercepting each window's `TextureRenderElement` at the surface
// boundary (inside `shell::WindowElement::render_elements`), which the
// task constraints forbid touching for this work. The udev path therefore
// continues to render without rounded corners until that follow-up lands;
// shadow alone validates the multigpu plumbing and ships a visible polish
// effect on multi-GPU systems.
