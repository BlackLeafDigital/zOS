# Phase 4 — Smithay Visual Effects (blur, rounded corners, shadows, opacity)

Research for `zos-wm` (Smithay anvil fork pinned to `27af99e`). Companion to
`phase-4-hyprland-animations.md` — that doc covers *animation curves*, this one
covers *what we render each frame*.

Read-only. Smithay paths cited as `~/.cargo/git/checkouts/smithay-312425d48e59d8c8/27af99e/...`.

---

## 1. TL;DR — what we ship in Phase 4

Four effects, all opt-in, all default-off so the existing render path is
unchanged:

| Effect          | Cost / frame | Mechanism                                       |
| --------------- | -----------: | ----------------------------------------------- |
| Opacity fade    | ~0           | Per-element `alpha` already plumbed everywhere  |
| Rounded corners |    1 pass    | Custom texture shader with `niri_rounding_alpha`-style mask |
| Drop shadow     |    1 pass    | `PixelShaderElement` underneath the window      |
| Blur behind     |  N+M passes  | Kawase 2-pass downscale → upscale into texture pool |

All four hang off Smithay's existing
[`GlesRenderer::compile_custom_pixel_shader`](#smithay-renderer-survey) and
[`compile_custom_texture_shader`](#smithay-renderer-survey) APIs — we do **not**
need raw GL calls outside `with_context` / `with_profiled_context` blocks.

Integration point: `output_elements()` in `zos-wm/src/render.rs` already
collects render elements. We extend `CustomRenderElements` with one new variant
per effect and emit them when a per-window or per-output config flag is set.

Frame budget at 144 Hz is **6.94 ms total** (see §6). Phase-4 effects target
~2 ms worst-case so we keep ≥4 ms for window content + DRM scanout overhead on
the 4090 box.

---

## 2. Smithay renderer survey: what's exposed, what's missing

### What `GlesRenderer` already gives us

- **Custom shader compilation** — fully supported on the renderer side.
  - `GlesRenderer::compile_custom_pixel_shader(src, additional_uniforms)`
    returns a `GlesPixelProgram`. Used for "draw a quad with a generated
    pattern" (shadows, solid rounded rects).
    `~/.cargo/git/checkouts/smithay-.../src/backend/renderer/gles/mod.rs:1966`.
  - `GlesRenderer::compile_custom_texture_shader(src, additional_uniforms)`
    returns a `GlesTexProgram`. Used for "modify a sampled texture" (rounded
    corner mask on top of window contents).
    `…/gles/mod.rs:2095`.
  - Both compile **two** programs (`normal` + `debug`) so the smithay tint
    debug flag still works through your shader.
    `…/gles/mod.rs:1976-1978`.

- **Render elements that wrap a custom shader** — already done.
  - `PixelShaderElement` (in `…/gles/element.rs:14-132`) implements
    `Element` + `RenderElement<GlesRenderer>`. It owns a `GlesPixelProgram`,
    a logical area, opaque regions, alpha, and a `Vec<Uniform<'static>>`,
    and its `draw()` calls `frame.render_pixel_shader_to(...)`. **This is
    the primitive zos-wm builds shadow + rounded-rect-fill effects on top
    of.**
  - `TextureShaderElement` (in `…/gles/element.rs:136-227`) wraps a
    `TextureRenderElement<GlesTexture>` and a `GlesTexProgram`, and calls
    `frame.render_texture_from_to(..., Some(&self.program), &uniforms)`.
    This is what we use for window-content + corner mask in one pass.

- **Uniform plumbing** — `Uniform` / `UniformName` / `UniformType` /
  `UniformValue` cover scalars, vec2/3/4, ivecs, uvecs, all matrix sizes
  with row/col-major flag.
  `…/gles/uniform.rs:5-50`, `…/gles/uniform.rs:62-77`.
  `From<f32> / [f32;2] / [f32;4]` etc are already implemented
  (`…/gles/uniform.rs:370-411`).

- **Offscreen render targets** — needed for blur ping-pong.
  - `GlesRenderer: Offscreen<GlesTexture>` (`…/gles/mod.rs:1641`) and
    `Offscreen<GlesRenderbuffer>` (`…/gles/mod.rs:1681`). Both expose
    `create_buffer(format, size)`.
  - `Bind<GlesTexture>` (`…/gles/mod.rs:1593`) and `Bind<GlesRenderbuffer>`
    (`…/gles/mod.rs:1599`) take `&mut Self` and return a `GlesTarget`
    framebuffer for `Renderer::render(…, framebuffer, …)`. We use
    `GlesTexture` (not renderbuffer) because we need to *sample* the result
    in the next blur pass.

- **Mid-frame target switching** —
  `FrameContext<'a, 'frame, 'buffer, GlesRenderer> for GlesFrame<'frame, 'buffer>`
  in `…/gles/mod.rs:3284`. `Frame::renderer()` returns a `GlesFrameGuard`
  that derefs to `&mut GlesRenderer` so you can `bind` a different target,
  do passes, and on `Drop` it restores the original viewport, scissor, blend
  state, and rebinds the previous target (`…/gles/mod.rs:3300-3324`). **This
  is the linchpin for kawase blur** — without it, switching FBOs mid-frame
  would force us to call `Renderer::render` recursively which the borrow
  checker forbids.

- **Direct GL escape hatch** —
  `GlesFrame::with_context(|gl: &Gles2| …)` (`…/gles/mod.rs:2125`) and
  `with_profiled_context` (`…/gles/mod.rs:2136`) for the rare case we need
  to issue raw GL calls (e.g. `glCopyImageSubData` for cheap framebuffer
  ping-pongs without re-binding). We *should not* need this for the four
  effects above; niri uses it for its blur FBO management
  (`…/render_helpers/blur.rs` — gen-fbos via `gl.GenFramebuffers`).

- **Built-in blur protocol support** — Smithay already implements
  `ext_background_effect_v1` so clients can **request** blur regions:
  `…/src/wayland/background_effect/mod.rs`. The handler trait
  `ExtBackgroundEffectHandler` exposes `set_blur_region(wl_surface, region)`
  and stores the region in
  `BackgroundEffectSurfaceCachedState.blur_region`
  (`…/background_effect/mod.rs:91-95`). zos-wm needs to (a) advertise the
  global, (b) implement `ExtBackgroundEffectHandler`, and (c) read the
  cached blur region during render to decide which surfaces get the
  expensive kawase pass behind them.

### What's **not** in smithay (we have to build it)

1. No bundled blur shaders — niri ships its own `blur_down.frag` /
   `blur_up.frag` (kawase). We adopt the same pattern.
2. No `BorderRenderElement` / `ShadowShaderElement` — both cosmic-comp and
   niri build these themselves on top of `PixelShaderElement`.
3. No pre-built texture pool with damage tracking for offscreen blur
   results — niri does this in `render_helpers/blur.rs` and we mirror that
   structure (one ringed `Vec<GlesTexture>` per output).
4. No declarative effect language (no "scoped CSS-style customization").
   Phase 4 has a **fixed set of effects** baked in. See §8 for why we don't
   build a runtime shader pipeline yet.

---

## 3. Custom shader plumbing — how to write a GLSL shader smithay calls per-frame

Walking through the texture-shader path because that's what we hit hardest
(rounded corners on every window).

### 3.1 The shader source contract

From the doc-comment block at `…/gles/mod.rs:2074-2094`:

- **Vertex shader is fixed** — smithay always uses its built-in
  `texture.vert` (`…/gles/shaders/implicit/texture.vert`). We only write
  the fragment shader.
- The shader **must contain a literal `//_DEFINES_` line** somewhere near
  the top — smithay substitutes that with the active `#define`s for the
  current draw call (`EXTERNAL`, `NO_ALPHA`, `DEBUG_FLAGS`).
- The shader must **not** contain a `#version` directive — smithay prepends
  `#version 100` itself.
- Required varyings/uniforms the shader receives:
  - `varying vec2 v_coords` — texture coords from the vertex shader.
  - `uniform sampler2D tex` (or `samplerExternalOES tex` if `EXTERNAL` is
    defined).
  - `uniform float alpha` — the per-element alpha.
  - `uniform float tint` — only present when `DEBUG_FLAGS` is defined; the
    smithay TINT debug overlay.
- The vertex shader exposes `uniform mat3 matrix`, `uniform mat3 tex_matrix`,
  `attribute vec2 vert`, `attribute vec4 vert_position` — we don't touch any
  of those; smithay sets them.
- Any **additional** uniforms we declare via `UniformName`s become available
  by name.

A minimal "tint everything red" texture shader looks like:

```glsl
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
uniform vec2  zos_size;       // additional uniform we declared
varying vec2  v_coords;

void main() {
    vec4 c = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    c = vec4(c.rgb, 1.0);
#endif
    c.r = max(c.r, 0.5);
    gl_FragColor = c * alpha;
}
```

### 3.2 The pixel-shader source contract

`…/gles/mod.rs:1945-1965`:

- No `//_DEFINES_` line; smithay just prepends `#version 100\n` (and a
  `#define DEBUG_FLAGS\n` for the debug variant).
- No `tex` uniform — pixel shaders draw a generated pattern, not a sampled
  texture.
- Receives `uniform vec2 size` (viewport size in pixels), `uniform float
  alpha`, `varying vec2 v_coords`, optionally `uniform float tint`.

This is what we use for the drop-shadow shader and the
solid-rounded-rect-fill shader (used for cosmic-comp-style border
indicators if we add those later).

### 3.3 Compiling at startup, not per-frame

Both `compile_custom_*_shader` calls must happen *outside* a
`Renderer::render` (they `make_current` the EGL context — see
`…/gles/mod.rs:1972`, `…/gles/mod.rs:2101`).

Plan: stash compiled programs on the `Backend`'s long-lived state, so we
compile once per renderer creation. For the udev backend we have one
`MultiRenderer` and at startup we walk
`MultiRenderer::with_renderer_mut(|gles_renderer| gles_renderer.compile_*)`
for each per-GPU `GlesRenderer`. The 4090 + AMD iGPU box has two contexts
so we compile two copies of every program. Cosmic-comp does this in
`State::new` (its renderer-init path).

### 3.4 Setting per-frame uniforms

When we build a `PixelShaderElement` or `TextureShaderElement` per frame
we pass `Vec<Uniform<'_>>`. Each uniform has a `Cow<str>` name and a
`UniformValue`. For animated values (alpha that's mid-fade, blur radius
that's mid-tween) the call site already has the live tween state from
Phase 4.A's animation engine, so we just write
`Uniform::new("blur_radius", current_radius)` each frame.

`PixelShaderElement::update_uniforms` (`…/gles/element.rs:67`) bumps the
internal `commit_counter`, which feeds the damage tracker so smithay
re-renders that region — important: animated uniforms require the
element to be marked damaged *every frame* to get redrawn even though
nothing else changed. niri does the same.

---

## 4. Per-effect design

### 4.1 Rounded corners (mask shader, 1 texture pass)

**Approach**: replace the default texture shader for windows with a custom
texture shader that multiplies the sampled texel alpha by a smoothstep'd
distance-to-rounded-rect mask. This is the cheapest possible technique —
zero extra render passes, zero extra textures.

**Reference shader** (adapted from niri's `rounding_alpha.frag`,
GPL-3, pattern only):

```glsl
//_DEFINES_

precision highp float;
uniform sampler2D tex;
uniform float alpha;
uniform vec2  zos_size;          // window size in physical px
uniform vec4  zos_corner_radius; // [TL, TR, BR, BL]
uniform float zos_scale;         // output scale, for AA half-px
varying vec2  v_coords;

float rounding_alpha(vec2 px, vec2 size, vec4 r) {
    vec2 c; float radius;
    if      (px.x < r.x          && px.y < r.x)          { radius = r.x; c = vec2(r.x, r.x); }
    else if (size.x - r.y < px.x && px.y < r.y)          { radius = r.y; c = vec2(size.x - r.y, r.y); }
    else if (size.x - r.z < px.x && size.y - r.z < px.y) { radius = r.z; c = vec2(size.x - r.z, size.y - r.z); }
    else if (px.x < r.w          && size.y - r.w < px.y) { radius = r.w; c = vec2(r.w, size.y - r.w); }
    else return 1.0;
    float d = distance(px, c);
    float t = clamp((d - radius) * zos_scale + 0.5, 0.0, 1.0);
    return 1.0 - t * t * (3.0 - 2.0 * t);  // smoothstep
}

void main() {
    vec4 c = texture2D(tex, v_coords);
    vec2 px = v_coords * zos_size;
    gl_FragColor = c * alpha * rounding_alpha(px, zos_size, zos_corner_radius);
}
```

**Integration**:

- Compile once. Stash in `BackendData` as
  `Option<GlesTexProgram>` per renderer.
- In `WindowElement::render_elements()`
  (`zos-wm/src/shell/element.rs:428-462`) — when corner-radius config is
  non-zero, wrap the inner `WaylandSurfaceRenderElement`s in
  `TextureShaderElement::new(inner, program, vec![Uniform::new("zos_size",
  …), Uniform::new("zos_corner_radius", [r,r,r,r]), Uniform::new("zos_scale",
  scale)])`.
- **Sub-surface gotcha**: if we just apply this on top-level the inner
  surfaces still draw to the corners. We need to do what cosmic-comp's
  `ClippedSurfaceRenderElement` does: also clip child surfaces to the
  rounded geometry. For Phase 4 we punt: the rounded mask shader runs on
  *each* surface and the corners that would extend past the toplevel
  bbox are simply outside `v_coords` of that surface; in practice this is
  fine because most apps draw their content into one big surface. Mark
  this as a known limitation in the config docs.
- Opaque-region tracking: rounded corners are no longer opaque. We must
  *not* pass `opaque_regions` for the rounded element to the damage
  tracker (or pass a shrunken inner rect). Concrete fix: in our wrapper
  element implement `Element::opaque_regions()` to return the full rect
  inset by `max_radius` on each side; corners become transparent and the
  damage tracker correctly composites whatever is behind them.

### 4.2 Drop shadow (PixelShaderElement, 1 pass, behind window)

**Approach**: render a quad larger than the window using a pixel shader
that produces a gaussian-decayed alpha around a rounded-rect interior.
No offscreen texture needed — the math is closed-form (error-function
approximation). cosmic-comp's `ShadowShader` and niri's `shadow.frag`
both do this; the shader is ~30 lines.

**Reference uniforms**:

- `vec2 zos_window_size` — interior size in px
- `vec2 zos_shadow_offset` — px (e.g. (0, 8) for a downward shadow)
- `float zos_shadow_blur` — gaussian sigma in px
- `vec4 zos_shadow_color` — premultiplied RGBA
- `vec4 zos_corner_radius` — same as 4.1
- `float zos_scale`

**Geometry**: the `PixelShaderElement::area` is the window logical rect
inflated by `shadow_offset.abs() + 3*shadow_blur` on each side. A
`ShadowRenderElement::will_render(window)` helper computes this and the
inverse-window-coords-to-shader-coords transform.

**Integration**:

- New `ShadowRenderElement` in `zos-wm/src/drawing.rs` (or a new
  `effects.rs`) that wraps `PixelShaderElement` plus the geometry math.
- Push *before* the window in `output_elements` — Z-order is element-list
  order in smithay (later = on top), so shadow goes first.
- Element returns `opaque_regions: vec![]` (shadow is fully transparent
  in the interior cutout).
- When window opacity is mid-fade, multiply `zos_shadow_color.a` by the
  same alpha to avoid the shadow popping.

### 4.3 Blur behind transparent surfaces (kawase, 2-pass)

**Approach**: dual-kawase. Each frame, for each output that has at least
one surface with a non-empty `BackgroundEffectSurfaceCachedState.blur_region`:

1. Allocate or reuse a chain of `GlesTexture`s sized
   `[output_w/2, output_w/4, output_w/8, …]` (5 levels for radius 8 — niri
   uses similar). Stop at `min(w,h) ≥ 16` to avoid degenerate samples.
2. **Downscale chain**: for `i in 0..levels-1`:
   - `frame.renderer()` (FrameContext guard) → bind level `i+1` as target.
   - `render_pixel_shader_to(blur_down_program, src=level_i, dst=full)`
     using a `blur_down.frag` that does a 5-tap box (center×4 + 4 corners).
   - Drop guard → restores original target + viewport.
   - Repeat with the level we just wrote as src.
3. **Upscale chain**: walk back down `(levels-1..0)` with `blur_up.frag`
   doing the 8-tap weighted upsample (4 edges + 4 corners×2).
4. **Composite**: render level 0 (now the full-resolution blurred image)
   as a regular texture, masked to the `blur_region` of each requesting
   surface, *underneath* that surface's content.

Niri's exact shader source (`blur_down.frag`, `blur_up.frag` — GPL-3,
pattern only):

```glsl
// blur_down.frag — 5-tap box
vec4 sum = texture2D(tex, v_coords) * 4.0;
sum += texture2D(tex, v_coords + vec2(-o.x, -o.y));
sum += texture2D(tex, v_coords + vec2( o.x, -o.y));
sum += texture2D(tex, v_coords + vec2(-o.x,  o.y));
sum += texture2D(tex, v_coords + vec2( o.x,  o.y));
gl_FragColor = sum / 8.0;

// blur_up.frag — 12-tap weighted
sum  = texture2D(tex, v_coords + vec2(-2.0*o.x, 0.0));
sum += texture2D(tex, v_coords + vec2( 2.0*o.x, 0.0));
sum += texture2D(tex, v_coords + vec2(0.0, -2.0*o.y));
sum += texture2D(tex, v_coords + vec2(0.0,  2.0*o.y));
sum += texture2D(tex, v_coords + vec2(-o.x,  o.y)) * 2.0;
sum += texture2D(tex, v_coords + vec2( o.x,  o.y)) * 2.0;
sum += texture2D(tex, v_coords + vec2(-o.x, -o.y)) * 2.0;
sum += texture2D(tex, v_coords + vec2( o.x, -o.y)) * 2.0;
gl_FragColor = sum / 12.0;
```

Reimplement in zos-wm using the same algorithm — kawase blur is decades
old, the math is not what's GPL'd.

**State container** (`zos-wm/src/effects/blur.rs`):

```rust
pub struct BlurState {
    blur_down: GlesPixelProgram,
    blur_up:   GlesPixelProgram,
    // Per-output texture pool: index 0 is full-res, descending.
    // Re-allocated only when output mode changes.
    chains: HashMap<Output, Vec<GlesTexture>>,
}
```

**Where in render_surface**: between `output_elements()` and
`drm_output.render_frame()`. Concretely we *can't* do the blur passes inside
the frame that DRM will scan out, because we need their result *before*
window content gets drawn into the same framebuffer. Two options:

- **(A) Two-frame separation**: render blur into a per-output texture
  *outside* `render_frame`, before calling it; then add a
  `TextureRenderElement` referencing that texture as a custom
  background-effect element behind blur-region surfaces. This is what niri
  does (`render_helpers/effect_buffer.rs`, `framebuffer_effect.rs`,
  `background_effect.rs`).
- **(B) Render-to-texture inside render_frame**: only possible because
  `FrameContext::renderer()` lets us bind a different target mid-frame.
  Risk: the DRM compositor's damage tracker doesn't know about our extra
  passes. Skip.

Take (A). Add a small helper that runs *after* we've collected the element
list and *before* `surface.drm_output.render_frame(...)`:

```rust
state.blur.update_chain_for_output(output, &elements, renderer)?;
```

That helper does its own `bind` + `render` pair on each chain texture in
sequence using `FrameContext` to swap targets. Then the
`TextureRenderElement` wrapping the level-0 chain texture is pushed into
`elements` before any blur-region surfaces.

**Damage**: invalidate the entire chain whenever any element below a blur
region changed since last frame; otherwise reuse. niri's
`blur.rs::is_damage_empty` is the pattern.

### 4.4 Opacity fade (per-element alpha)

Already plumbed. Every `RenderElement::draw` call gets an `alpha` parameter
and `WindowElement::render_elements` already passes one through to
`AsRenderElements::render_elements` (`shell/element.rs:433-454`). The Phase
4.A animation engine writes the live alpha into the WindowElement's
user_data, and our wrapper in `output_elements` pulls it out and feeds it
through.

No shader changes. The only nuance: when alpha < 1 the surface is no
longer opaque, so just like rounded corners we have to clear opaque
regions for the duration of the fade. Provide a wrapper element that
returns `opaque_regions: empty` whenever live `alpha < 1.0` — Phase 3 already
has WindowEntry user_data on `WindowElement::user_data()`
(`shell/element.rs:134-137`), put the live alpha there.

---

## 5. Integration points in zos-wm

### 5.1 Render path entry points

Two backends, same elements list:

- **udev** (real DRM): `udev.rs::render_surface` at
  `zos-wm/src/udev.rs:1946-2097`. Calls `output_elements()` at line 2036
  then `surface.drm_output.render_frame(renderer, &elements, ...)` at
  2052.
- **winit** (development): `winit.rs` around line 374. Same
  `output_elements()` call at line 416.

**Insertion point for blur**: between `output_elements()` and the
`render_frame` / `damage_tracker.render_output` call. We need access to
the renderer (`MultiRenderer` for udev, `GlesRenderer` for winit) and the
output. Both already have it.

**Insertion point for shadow + rounded corners + opacity**: inside
`WindowElement::render_elements()` (`shell/element.rs:428-462`), or one
level higher in the `space_render_elements` path called from
`render.rs::output_elements:179`. The latter is cleaner because it sees
all windows uniformly. Add a `space_with_effects` adapter that maps each
yielded element through the effect wrappers.

### 5.2 Per-window effect config

zos-wm Phase 3 already has `WindowEntry` keyed by `WindowId`
(`shell/element.rs:563`). Add `EffectState` to that:

```rust
pub struct EffectState {
    pub corner_radius: [f32; 4],
    pub shadow: Option<ShadowSpec>,
    pub blur_behind: bool,
    pub alpha: f32,
}
```

Read at render time via `window.user_data().get::<EffectState>()`. Default
implementation returns `EffectState::DEFAULT` (all zeros, alpha 1.0,
disabled) so the existing render path is unchanged when nothing is
configured.

### 5.3 Per-output state

Blur texture chains live on `Output::user_data()`:

```rust
output.user_data().insert_if_missing(|| RefCell::new(BlurChain::default()));
```

This matches `FullscreenSurface` which already lives there
(`render.rs:152`).

### 5.4 Default-disabled with config opt-in

zos-wm currently has no config plumbing in-tree. For Phase 4 we ship:

```toml
# zos-wm.toml (read at startup; not hot-reloadable in v1)
[effects]
corner_radius = 0   # 0 = disabled
opacity_fade  = false
shadow        = false
blur          = false
```

When all are disabled, `compile_custom_*_shader` is *not* called
(saves ~5 ms startup) and the effect wrappers in `output_elements` are
no-ops.

### 5.5 Compile-once locations

- udev: in `init_udev` after the per-GPU `GlesRenderer` is created
  (`udev.rs:596-600`). We get a `&mut GlesRenderer` from
  `unsafe { GlesRenderer::with_capabilities(context, capabilities)? }`.
- winit: in `init_winit` once `backend.renderer()` is available
  (`winit.rs:165-170` — the FPS texture is created the same way, mirror
  that pattern).

Cache results per-renderer; `MultiRenderer` requires per-GPU compilation.

---

## 6. Frame pacing budget

Daily-driver target: 144 Hz on the RTX 4090 box → **6.94 ms/frame total**.

### 6.1 Worst-case per-frame budget at 4K @ 144 Hz

| Pass                                       |    Cost (4090) |   Cost (iGPU AMD) |
| ------------------------------------------ | -------------: | ----------------: |
| Window content (existing)                  |       ~1.5 ms |           ~3.0 ms |
| Rounded-corner shader (per-window, 1 quad) |       ~0.05 ms |          ~0.1 ms |
| Drop shadow (per-window, 1 quad, 30 px)   |       ~0.1 ms |          ~0.3 ms |
| Kawase down (5 levels @ 4K → 256 px)       |       ~0.3 ms |          ~0.8 ms |
| Kawase up (5 levels back)                  |       ~0.4 ms |          ~1.2 ms |
| Composite blur back into framebuffer       |       ~0.1 ms |          ~0.3 ms |
| DRM page-flip overhead (existing)          |       ~0.5 ms |          ~0.5 ms |
| **Subtotal w/ all effects on 5 windows**   |    **~3.5 ms** |       **~7.5 ms** |
| Headroom at 144 Hz (6.94 ms)               |       ~3.4 ms |     **NEGATIVE** |

Numbers are *theoretical estimates*, not measured. Actual measurement
goes in Phase 4 implementation, not research. The point is: blur at full
output resolution on iGPU at 144 Hz blows the budget.

### 6.2 Mitigations

- **Run blur at output_resolution / 2**. Kawase already downsamples — we
  just start the chain one level lower. Saves ~50% blur cost. niri does
  this; their config calls it the "blur_passes" knob.
- **Skip blur passes if no element under any blur region changed**.
  Already covered by the damage tracker — when the damage rect doesn't
  intersect any blur-region surface we reuse last frame's chain.
- **Disable blur on iGPU** by default. Detect `EGLDevice` GPU vendor;
  on the AMD iGPU we set `effects.blur = false` even if config requested
  it, with a warning. The 4090 always blurs.
- **Per-window opt-in for shadow/rounding** (not blanket per-output).
  cosmic-comp does this; the cost is per-window-on-screen, not
  per-pixel-on-screen.

### 6.3 Lower bound: at 60 Hz

Budget is 16.67 ms — even 5-window 4K blur on the AMD iGPU fits with 9 ms
headroom. Blur is only a problem at 144 Hz on slow GPUs.

---

## 7. zos-wm task list — concrete 1-2-file-scoped tasks

Ordered. Each is one agent-sized chunk. Most touch 1-2 files.

### 7.A — Foundation (no behavior change)

1. **Add `effects` module skeleton** (`src/effects/mod.rs`,
   `src/effects/shaders/`, `src/effects/state.rs`). Empty types,
   re-exported from `lib.rs`. `cargo check` clean.
2. **Add `EffectState` to WindowElement user_data**. New file
   `src/effects/state.rs`; touch `src/shell/element.rs:WindowElement` impl
   to add `pub fn effect_state() -> Ref<EffectState>` returning default
   when missing.
3. **Add `BlurChain` per-output user_data placeholder** in
   `src/effects/blur.rs`. Empty struct + getter on `Output`. No GL yet.

### 7.B — Rounded corners (smallest, ship first)

4. **Add `rounded_corners.frag` shader source** as a `&'static str`
   constant in `src/effects/shaders/mod.rs`. Adapt from the §4.1
   reference shader.
5. **Compile shader at backend init** — touch `src/udev.rs` (the per-GPU
   `GlesRenderer::with_capabilities` block at line 596) and
   `src/winit.rs` (around line 170) to call
   `renderer.compile_custom_texture_shader(ROUNDED_FRAG, &uniforms)` and
   stash the `GlesTexProgram` on `BackendData`.
6. **Add `RoundedWindowElement<R>` wrapper** in
   `src/effects/rounded.rs`. Wraps each surface element from
   `WindowElement::render_elements`, owns the program + uniforms, draws
   via `frame.render_texture_from_to(..., Some(&program), uniforms)`.
   Implements `Element` and `RenderElement<R>` for both `GlesRenderer`
   and `UdevRenderer<'a>` (mirror cosmic-comp's two-impl pattern).
7. **Plumb `RoundedWindowElement` into `output_elements`** in
   `src/render.rs`. When `EffectState::corner_radius != 0`, replace the
   bare surface element with the wrapped one. Verify damage tracking
   with `--debug` flag.
8. **Smoke test**: hardcode `corner_radius = [12,12,12,12]` for all
   windows, run winit backend, verify rounded corners on a `weston-terminal`.

### 7.C — Drop shadow

9. **Add `shadow.frag` shader source** (pixel shader; the math from
   `cosmic-comp/.../shaders/shadow.frag` style — implement a smoothstep'd
   distance field, NOT a texture sample). Compile alongside the rounded
   shader in step 5.
10. **Add `ShadowRenderElement`** wrapping `PixelShaderElement`. Geometry:
    window logical bbox inflated by `(blur*3 + |offset|)`. Lives in
    `src/effects/shadow.rs`.
11. **Plumb shadow into `output_elements`** before the rounded corner
    element so it Z-orders below.
12. **Smoke test**: same as above but with shadow `(blur=20, offset=(0,8))`.

### 7.D — Opacity fade (Phase 4.A integration)

13. **Wire alpha from `EffectState` into the existing
    `AsRenderElements::render_elements` alpha argument**. One file:
    `src/shell/element.rs` line 454 and 458 — replace the literal `1.0`
    with `state.effect_state.alpha * outer_alpha`.
14. **Clear opaque regions on alpha < 1.0** — wrap the surface element
    in an `AlphaWrapper` that returns empty `opaque_regions` when alpha
    is mid-fade. Add to `src/effects/alpha.rs`.

### 7.E — Blur (largest, last)

15. **Add `ext_background_effect_v1` global** — extend `state.rs` to
    `impl ExtBackgroundEffectHandler for AnvilState`. Global is
    advertised; handler stores blur_region on the surface (smithay does
    that automatically via `BackgroundEffectSurfaceCachedState`).
    `delegate_background_effect!` macro.
16. **Add `BlurChain` lifecycle**: `BlurChain::resize_for(output, size)`
    allocates the texture pyramid via `Offscreen<GlesTexture>::create_buffer`.
    Released when output mode changes. One file: `src/effects/blur.rs`.
17. **Add `blur_down.frag`, `blur_up.frag`** shader source. Compile in
    step 5's init path (gated on `effects.blur`).
18. **Add `BlurChain::run(renderer, output, frame_target_texture, damage)`**
    that does the down→up passes using `Bind` + `Renderer::render` per
    pass. Critically uses `MakeCurrent`/`bind` not `FrameContext` because
    we run *before* `render_frame`.
19. **Plumb blur in `udev::render_surface`**: between line 2036
    (`output_elements`) and 2052 (`render_frame`) call
    `state.blur.run(renderer, output, …)?`. Also push a
    `TextureRenderElement` for the level-0 result *before* the
    blur-region surface in `elements`.
20. **Mirror plumbing in `winit.rs`** (around line 416). Same calls.
21. **Skip blur on AMD iGPU** in the EGL device probe path. Touch
    `udev.rs:596` to detect vendor and override `effects.blur = false`
    with a warning. (Actual detection: `EGLDevice::vendor()` or fall back
    to checking `glGetString(VENDOR)` via `GlesRenderer::with_context`.)

### 7.F — Polish

22. **Per-window config defaults in zos config crate** — read effect
    settings at startup, populate `EffectState::default()`.
23. **Damage-tracking audit**: run `WAYLAND_DEBUG=1` + smithay damage
    debug to verify wrapped elements report damage correctly (especially
    when uniforms change without surface commit).
24. **Frame profiler integration**: smithay's `profiling` feature already
    instruments `gpu_span_location!`. Add explicit spans on each blur
    pass (`with_profiled_context(gpu_span_location!("blur_down_0"), …)`).

Total: ~24 small tasks. Steps 1-8 (rounded corners) is the smallest
viable shipping slice — that alone gets us visible Phase 4 wins.

---

## 8. Scoped CSS-style customization — defer to Phase 5

Question 6 in the prompt: can we ship effect-customization-by-config-file?

**No, not in v1.** Reasons:

- Compiling shaders at runtime per-config-rule needs a sandboxed shader
  validator (untrusted GLSL crashes the GPU driver). Smithay doesn't ship
  one.
- The four effects in §4 cover ~95% of what users actually want
  (Hyprland, KWin, cosmic-comp ship roughly this set).
- `zos-ui`'s `style!` macro is for *widget* styling, not compositor
  shaders. Mixing the two confuses the mental model.

**Phase 5 plan (out of scope here)**: a small effect-graph DSL where
users compose pre-validated primitives (blur, mask, mix, color-grade)
into per-window pipelines. Basically WGSL fragments that we paste-and-link
into a known-safe shader skeleton. Don't build until we have actual user
demand.

---

## 9. Sources

### Smithay (MIT, our base, citations are line-accurate)
- `~/.cargo/git/checkouts/smithay-312425d48e59d8c8/27af99e/src/backend/renderer/gles/mod.rs`
  - `compile_custom_pixel_shader` at line 1966
  - `compile_custom_texture_shader` at line 2095
  - `render_pixel_shader_to` at line 3032
  - `render_texture_from_to` at line 2693
  - `Bind<GlesRenderbuffer>` at line 1599
  - `Offscreen<GlesTexture>` at line 1641
  - `Offscreen<GlesRenderbuffer>` at line 1681
  - `FrameContext for GlesFrame` at line 3284
  - `GlesFrameGuard::Drop` (state restore) at line 3300
  - `with_context` at line 2125, `with_profiled_context` at line 2136
- `~/.cargo/git/checkouts/smithay-.../src/backend/renderer/gles/element.rs`
  - `PixelShaderElement` at line 14
  - `TextureShaderElement` at line 136
- `~/.cargo/git/checkouts/smithay-.../src/backend/renderer/gles/uniform.rs`
  - `UniformType`, `UniformValue`, `UniformName`
- `~/.cargo/git/checkouts/smithay-.../src/backend/renderer/gles/shaders/implicit/texture.frag`
  - Reference for the `//_DEFINES_` / `EXTERNAL` / `NO_ALPHA` /
    `DEBUG_FLAGS` shader contract
- `~/.cargo/git/checkouts/smithay-.../src/backend/renderer/gles/shaders/implicit/texture.vert`
  - The fixed vertex shader (we don't modify, but its uniform names are
    the contract)
- `~/.cargo/git/checkouts/smithay-.../src/backend/renderer/mod.rs`
  - `Bind<Target>` trait at line 216
  - `FrameContext` trait at line 367
  - `Offscreen<Target>` trait at line 454
- `~/.cargo/git/checkouts/smithay-.../src/wayland/background_effect/mod.rs`
  - `ExtBackgroundEffectHandler::set_blur_region` at line 68
  - `BackgroundEffectSurfaceCachedState.blur_region` at line 94

### zos-wm internal
- `zos-wm/src/render.rs` — `output_elements`, `CustomRenderElements`
  (insertion site)
- `zos-wm/src/udev.rs:1946-2097` — `render_surface`
- `zos-wm/src/winit.rs:374-…` — winit render path
- `zos-wm/src/shell/element.rs:428-462` — `WindowElement::render_elements`
  (wrap site)

### cosmic-comp (GPL-3, *patterns only* — do not copy code)
- `cosmic-comp/src/shell/element/window.rs` — shadow/border element
  composition pattern
- cosmic-comp's `IndicatorShader::element` and `ShadowShader::element`
  return `PixelShaderElement` instances; they are constructed per-frame
  and Z-ordered before content.
- cosmic-comp's `ClippedSurfaceRenderElement::will_clip` pattern for
  child-surface clipping.

### niri (GPL-3, *patterns only*)
- `niri/src/render_helpers/blur.rs` — kawase pyramid (5-tap down + 12-tap
  up) with persistent `Vec<GlesTexture>` per output
- `niri/src/render_helpers/border.rs` — `BorderRenderElement` wraps
  `ShaderRenderElement`; corner_radius converted to `[f32; 4]` uniform
- `niri/src/render_helpers/effect_buffer.rs`,
  `niri/src/render_helpers/framebuffer_effect.rs` — offscreen render
  pattern for effects that need to ship a result texture into the main
  pass
- `niri/src/render_helpers/shaders/blur_down.frag` and `blur_up.frag` —
  shader algorithm reference (re-implemented in zos-wm, not copied)
- `niri/src/render_helpers/shaders/rounding_alpha.frag` — algorithm for
  the rounded-corner mask function (re-implemented)
- `niri/src/render_helpers/shaders/shadow.frag` — closed-form shadow
  algorithm (re-implemented)

### Issues / discussions
- pop-os/cosmic-comp #511 — Blur/Frosted Glass support
- pop-os/cosmic-comp #673 — wobbly windows + shader plugins
- pop-os/cosmic-comp #691 — corner radius for all windows
- niri-wm/niri #164 — rounded corners without CSD
- niri-wm/niri #1741 — workspace-bounded shadow clipping

---

## Design summary (5 bullets)

- Smithay already exposes `compile_custom_pixel_shader` /
  `compile_custom_texture_shader` plus `PixelShaderElement` /
  `TextureShaderElement` — we never need raw GL except through
  `with_context`. We compile shader programs once per `GlesRenderer` at
  init and stash them on `BackendData`.
- Mid-frame target switching (needed for kawase blur) works through
  `FrameContext::renderer()` → `GlesFrameGuard`, which restores viewport,
  scissor, blend, and target on `Drop`. But for blur we go simpler: run
  the kawase passes *before* `render_frame` using
  `Bind<GlesTexture>::bind` + `Renderer::render`, then feed the resulting
  texture in as a regular `TextureRenderElement`.
- The four ship-in-Phase-4 effects (rounded corners, opacity fade, drop
  shadow, kawase blur) are **default-off, opt-in per window** via a new
  `EffectState` on `WindowElement::user_data()`. Existing render path is
  byte-identical when nothing is configured.
- Smithay already implements the `ext_background_effect_v1` protocol
  (`BackgroundEffectSurfaceCachedState.blur_region`) — we wire the
  handler in `state.rs`, read the cached region during render to decide
  which surfaces get blurred-behind, and have a working blur opt-in
  protocol with zero protocol code.
- Frame budget at 144 Hz is 6.94 ms; the 4090 box has ~3 ms headroom
  with all effects on; the AMD iGPU does **not** at 4K — we hard-disable
  blur on the iGPU at startup. Rounded corners + shadow + opacity all fit
  trivially on both. Ship rounded corners first (steps 1-8 in §7), it's
  ~one day of work and the most visible win.

Sources:
- [Smithay 27af99e source on local checkout]
- [pop-os/cosmic-comp on GitHub](https://github.com/pop-os/cosmic-comp)
- [YaLTeR/niri on GitHub](https://github.com/YaLTeR/niri)
- [Blur/Frosted Glass support · pop-os/cosmic-comp #511](https://github.com/pop-os/cosmic-comp/issues/511)
- [rounded corners without CSD · niri-wm/niri #164](https://github.com/niri-wm/niri/discussions/164)
- [How are window shadows clipped to workspace bounds in niri? · YaLTeR/niri #1741](https://github.com/YaLTeR/niri/discussions/1741)
