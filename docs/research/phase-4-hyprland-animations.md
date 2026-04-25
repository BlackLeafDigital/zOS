# Phase 4 — Hyprland Animations: Architecture & Port to zos-wm

Research doc for Phase 4 (animations + visual polish). Studies how Hyprland's
animation system is built and proposes a Rust/Smithay port for `zos-wm`. We are
**read-only on Hyprland** — this is pattern study, not code copy. Hyprland is
BSD-3 licensed; the patterns are fair game even if we wanted to reuse code, but
zos-wm is MIT and we will reimplement from scratch.

## 1. TL;DR — Recommended zos-wm animation architecture

- **Crate-shaped:** put bezier curves + an `AnimatedValue<T>` engine in a small
  internal module (`zos-wm/src/anim/`), not pulled in from a third-party crate.
  Hyprutils' design (BezierCurve + AnimatedVariable + AnimationManager) is
  excellent and ports almost line-for-line into Rust. ~600 LoC total.
- **Time-driven, not vsync-driven:** advance every animated value by elapsed
  wall-clock time on each render pass. Hyprland uses a 1ms event-loop timer, but
  for a fork-of-anvil the simplest call site is the start of `render_surface`
  in `udev.rs`/`winit.rs`. Frame rate independence falls out for free.
- **Per-output damage opt-in:** when any animated value attached to a window on
  an output is in flight, mark that output as continuously dirty (request a
  frame) — Hyprland's `scheduleFrameForMonitor(...AQ_SCHEDULE_ANIMATION)`
  pattern. Smithay equivalent: `Output::layer_map()` gets dirty + we keep
  ticking by re-arming `frame_target` until all `AnimatedValue`s are settled.
- **Three animatable types:** `f32`, `Point<f64, Logical>`, and `Color`
  (premultiplied RGBA). Hyprland adds OkLab interpolation for color — copy that
  for borders, skip it for opacity.
- **Two animation primitives, composed:** every higher-level animation
  (window-open, window-close, workspace-switch) is built from `AnimatedValue`s
  on (a) a `render_offset: Point` and (b) an `alpha: f32` attached to either
  `Window` or `Workspace`. No "animation type" enum — just compose values.

## 2. Hyprland's flow: ConfigManager → AnimationManager → AnimatedVariable<T> → render tick

The flow has four conceptual layers, each with a clean responsibility:

### 2.1 Config side: parsing `animations { ... }`

Source: `src/config/ConfigManager.cpp` (parse) + `src/config/shared/animation/AnimationTree.cpp` (storage).

`animations.conf` lines look like:

```
bezier   = overshot, 0.05, 0.9, 0.1, 1.05
animation = windows, 1, 5, overshot, slide
animation = fade,    1, 5, smoothIn
```

These get parsed by `CConfigManager` into two parallel structures:

1. **Beziers** — `g_pAnimationManager->addBezierWithName(name, p1, p2)` adds a
   `CBezierCurve` keyed by name. The curve is *baked* once at registration
   (255 sample points, see §3).
2. **Animation property tree** — `Config::animationTree()` (`AnimationTree.cpp`)
   builds a parent/child tree of `SAnimationPropertyConfig` nodes. The tree
   nodes mirror what users can configure: `global → windows → windowsIn`,
   `global → fade → fadeIn`, `global → workspaces → workspacesIn`, etc.
   Children inherit from parents unless overridden.

The tree is the **source of truth** that animation consumers ask:

```cpp
auto cfg = Config::animationTree()->getAnimationPropertyConfig("windowsIn");
pWindow->m_realPosition->setConfig(cfg);
```

`SAnimationPropertyConfig` holds: `enabled`, `speed` (a multiplier — bigger =
slower; duration_ms ≈ 100 × speed), `bezier` name, `style` string ("slide",
"popin 80%", "fade", etc.).

### 2.2 The variable: `CAnimatedVariable<T>`

Source: `hyprutils/include/hyprutils/animation/AnimatedVariable.hpp` +
`hyprutils/src/animation/AnimatedVariable.cpp`, specialized in
`Hyprland/src/helpers/AnimatedVariable.hpp` for Hyprland's three types
(`float`, `Vector2D`, `CHyprColor`).

Each animatable property is a `CGenericAnimatedVariable<T, SAnimationContext>`
holding three values: `m_Begun`, `m_Value`, `m_Goal`, plus a config pointer
(`SAnimationPropertyConfig`), an animation-start timestamp, and an
`AnimationContext` (back-references to `pWindow` / `pWorkspace` / `pLayer` and
a damage policy enum).

Assignment is the trigger:

```cpp
*pWindow->m_realPosition = newPos;   // operator= sets m_Goal, m_Begun = m_Value, calls onAnimationBegin()
```

`onAnimationBegin()` records `animationBegin = steady_clock::now()` and
**connects** the variable to the active list via a signal
(`m_pSignals->connect.emit(self)`). The `CAnimationManager` is a pure
listener — its `m_vActiveAnimatedVariables` is populated by signal callbacks.

`getPercent()` is the time-based progress accessor:

```cpp
auto durationPassedMs = ms_since(animationBegin);
return clamp((durationPassedMs / 100.f) / speed, 0, 1);
```

That `100.f` is the magic number. With `speed = 5` you get ~500ms duration;
with `speed = 10` you get 1s. Frame-rate independent: a 60Hz monitor and a
240Hz monitor produce the same final-state arrival time.

### 2.3 The manager and the tick

Source: `Hyprland/src/managers/animation/AnimationManager.cpp` (overrides) +
hyprutils' base `CAnimationManager`.

`g_pAnimationManager->m_animationTimer` is an event-loop timer with a 500µs
initial timeout, scheduled to fire every ~1ms while there's anything to
animate (`scheduleTick` → `m_animationTimer->updateTimeout(1ms)`).

Each `frameTick()`:

1. Calls `tick()` if at least 1ms has passed since the last tick (rate-limit).
2. `tick()` iterates `m_vActiveAnimatedVariables`, computes
   `POINTY = bezier->getYForPoint(pAV->getPercent())` — that's the curve-mapped
   completion fraction in [0, 1] — then sets `value = begun + (goal-begun) * POINTY`.
3. If `getPercent() >= 1.0`, the variable is *warped* (snapped to goal) and
   disconnected from the active list.
4. Damage is batched **per owner** (window/workspace/layer): pre-damage with
   the old state, update all values, post-damage with the new state, then
   `scheduleFrameForMonitor(monitor, AQ_SCHEDULE_ANIMATION)` to wake the
   render loop.
5. If anything still animates (`shouldTickForNext()`), `scheduleTick()`
   re-arms the timer.

### 2.4 The render tick

The renderer doesn't drive the animation tick directly — it consumes the
animated values when rendering. `Window::m_realPosition->value()` and
`m_alpha->value()` are read each frame. The animation tick has already
written them on a separate timer; the render path just reads.

This decoupling is important: if the GPU stalls, the animation timer keeps
ticking, and when the GPU recovers it sees the latest interpolated value (no
"catching up" of dropped frames).

## 3. Curves: how named curves are stored, how the bezier is evaluated

Source: `hyprutils/include/hyprutils/animation/BezierCurve.hpp` +
`hyprutils/src/animation/BezierCurve.cpp`.

Each named curve is a `CBezierCurve`. Construction takes 2 control points
(P1, P2) — the cubic Bézier endpoints (P0=(0,0) and P3=(1,1)) are implicit:

```cpp
addBezierWithName("overshot", Vector2D(0.05, 0.9), Vector2D(0.1, 1.05));
```

Note that overshoot/undershoot is allowed: y can be > 1 or < 0 mid-curve,
which is how the named "overshot" curve gets that satisfying past-the-target
spring-back feel.

### Baking

On construction, `setup4()` pre-bakes 255 sample points (`BAKEDPOINTS = 255`).
At each point i in `[1..255]`, t = (i+1)/255, and stores
`(getXForT(t), getYForT(t))`. That's a single allocation, 255 Vector2D entries
per curve, evaluated once per curve at config load.

### Per-frame evaluation: `getYForPoint(x)`

Per-frame call: given progress `x ∈ [0, 1]`, return curve-mapped `y`.
Hyprutils does **binary search over the baked array** to find the bracketing
points, then linear interpolates between them. The binary search is on
`m_aPointsBaked[i].x`, which is monotone-increasing because Bézier curves
defined with monotone control point x-coordinates have monotone-increasing
sampled x.

```cpp
for (int step = (BAKEDPOINTS+1)/2; step > 0; step /= 2) {
    if (below) index += step; else index -= step;
    below = m_aPointsBaked[index].x < x;
}
// linear interp between baked[lower] and baked[lower+1]
```

Cost: ~8 iterations of the binary search loop + one linear interp = ~30
floating-point ops per evaluation. Fast enough that no caching is required.

### Default and built-in curves

Hyprutils ships exactly one default: `default` = `Vector2D(0, 0.75)` to
`Vector2D(0.15, 1.0)` (a snappy ease-out). Hyprland adds `linear` =
`(0,0) → (1,1)` in `CHyprAnimationManager`'s constructor. Everything else
("overshot", "smoothOut", "easeInOut") comes from the user's
`animations.conf` `bezier = ...` lines. Our zOS default config defines
`overshot`, `smoothOut`, and `smoothIn` (see
`build_files/system_files/usr/share/zos/hypr/defaults.conf:51-53`).

## 4. Render integration: when `tick()` runs, how damage is computed, frame-rate independence

### Two clocks

- **Animation clock** — a 1ms event-loop timer, independent of any monitor's
  vsync. This advances `m_Value` toward `m_Goal`.
- **Render clock** — DRM page-flip / vsync per monitor. Reads the latest
  `m_Value` and renders.

Because the animation clock is wall-clock-time-based (`getPercent()` uses
`steady_clock::now()`), the same animation looks the same on a 60Hz panel as
on a 144Hz panel. Skipped animation ticks (e.g. tick fires while the previous
hadn't finished) just produce larger interpolation jumps at the next tick;
the curve is sampled at fewer points but starts/ends correctly.

### Damage tracking

Each `CGenericAnimatedVariable` carries a `eDamagePolicy` (enum:
`ENTIRE` / `BORDER` / `SHADOW` / `NONE`), set when the variable is created
(see `CHyprAnimationManager::createAnimation`). On each tick, `tick()`:

1. Groups animated variables **by owner** (window/workspace/layer) into
   `SDamageOwner`s. This is the dedup pass — a single window with both
   `m_realPosition` and `m_realSize` animating only damages once.
2. **Pre-damage** with the old state (call site:
   `g_pHyprRenderer->damageWindow(w)` etc.). This invalidates the area the
   thing currently occupies.
3. Updates all `m_Value`s.
4. **Post-damage** with the new state. Invalidates the area it now occupies.
5. Calls `scheduleFrameForMonitor` with `AQ_SCHEDULE_ANIMATION` to wake the
   compositor's render loop.

Workspace switches use a special `preDamageWorkspace` because a workspace
animation translates *every* window on it — so damage is essentially the
whole monitor.

## 5. Window-open/close: source rect derivation, opacity fade

Source: `Hyprland/src/managers/animation/DesktopAnimationManager.cpp::startAnimation(PHLWINDOW, ...)`.

When a window opens (`ANIMATION_TYPE_IN`), the dispatcher does roughly:

1. Resolve the goal: `GOALPOS = m_realPosition->goal()`,
   `GOALSIZE = m_realSize->goal()` (already set by the layout to the final
   rect).
2. Choose the source rect based on the configured style (`slide`, `popin`,
   `gnome`, etc.):
   - **Slide:** find the closest monitor edge to the window's centroid, then
     compute `posOffset` = the edge-aligned off-screen position. Force overrides
     ("slide left" / "slide top") skip the centroid check. See `animationSlide`
     (lines 389-446 of DesktopAnimationManager.cpp).
   - **Popin minPerc%:** start at `(GOALSIZE * minPerc/100).clamp({5,5},
     GOALSIZE)`, centered on `GOALPOS + GOALSIZE/2`. Window grows from a small
     central rect.
   - **Gnomed:** start at zero height, full width, centered vertically inside
     the goal rect.
   - **Fade only:** position/size warp instantly to goal; only alpha animates.
3. Set the source: `m_realPosition->setValue(srcPos)` (which sets `m_Begun`
   without changing `m_Goal`, restarting the timer); then assignment is a
   no-op because goal didn't change. Internally `setValue` calls
   `onAnimationBegin()` which restarts the timer.

For **close** (`ANIMATION_TYPE_OUT`), it's flipped: source = current,
goal = off-screen / shrunk / faded. Hyprland keeps the window object alive
in `m_fadingOut` state and tears it down only after the alpha animation
reaches zero.

**Opacity fade** runs on a separate `m_alpha: float`, configured with
`fadeIn` / `fadeOut` properties. `*m_alpha = 1.0` (open) or `*m_alpha = 0.0`
(close) is the trigger; the animation system handles the rest.

Three animated variables move in lockstep on window open:
`m_realPosition`, `m_realSize`, `m_alpha`. They have **independent timers
and curves** (e.g. position uses `overshot`, alpha uses `smoothIn`), giving
the illusion of a single animation but with per-axis bezier control.

## 6. Workspace switch: geometric translation strategy

Source: `DesktopAnimationManager.cpp::startAnimation(PHLWORKSPACE, ...)` (lines 242-374).

Hyprland uses **per-workspace `m_renderOffset: Vector2D`**, NOT per-window
translation. The renderer reads `workspace.m_renderOffset->value()` and
translates the entire workspace's rendered space by it. Each window on the
workspace then has `w->onWorkspaceAnimUpdate()` invoked via update callback,
which lets per-window logic (like floating windows pinned across workspaces)
adjust if needed.

### The math

Given monitor size `(W, H)` and a "left" direction (i.e., the *new* workspace
slid in from the left edge):

```
XDISTANCE = (W + gaps_workspaces) * (movePerc / 100)

# IN animation (the workspace becoming active):
m_renderOffset.warp_to(left ? +XDISTANCE : -XDISTANCE)   // start off-screen
m_renderOffset = (0, 0)                                   // animate to centered

# OUT animation (the workspace leaving):
m_renderOffset = left ? -XDISTANCE : +XDISTANCE          // animate off-screen
```

Both old and new workspaces animate *simultaneously*, both with the same
`workspaces` config (same speed + bezier), so they appear as one continuous
slide.

### Per-monitor scope

Each monitor has its own active+pending workspace pair. Workspace animations
are **per-monitor** — switching workspace on monitor A doesn't animate
monitor B. This is critical for our 3×1080p hardware: workspace switches
shouldn't cause cross-monitor flicker.

### Style variants

- **`slide`** (default): horizontal translate, X axis only.
- **`slidevert`**: vertical translate, Y axis.
- **`slidefade`** / **`slidefadevert`**: translate + alpha fade combined
  (both `m_renderOffset` and `m_alpha` animate).
- **`fade`**: only `m_alpha` animates, no translate.

Special workspaces (Hyprland's overlay/scratchpad concept) always animate
their alpha because they overlay the active workspace; ordinary workspaces
only fade if the explicit `fade` style is set.

## 7. Port challenges + recommended Rust approach for zos-wm

### What's hard

1. **C++ `operator=` magic.** `*pWindow->m_realPosition = newPos` looks like
   assignment but is a side-effecting call that records `m_Begun = m_Value`,
   updates `m_Goal`, restarts the clock, and emits a signal to register with
   the manager. In Rust, that's a method: `window.position.animate_to(new_pos)`.
   No `Deref`-trick to fake operator=; just be explicit.
2. **Smithay's render path is element-based.** Hyprland's renderer does its
   own per-window draw call (translate by `m_realPosition->value()`, fade by
   `m_alpha->value()`). Smithay's `OutputDamageTracker::render_output` takes
   a list of `RenderElement`s. To "translate by an animated offset" we either
   (a) wrap our `WindowElement` in a `RelocateRenderElement` per frame, or
   (b) update the window's `Space` location each tick. We want (a) — `Space`
   is the persistent layout, not the per-frame transform.
3. **Damage tracking ownership.** `OutputDamageTracker` damages based on
   element geometry diff between frames. If we change an element's location
   each frame via a wrapping `RelocateRenderElement`, damage is computed
   automatically — but only if the wrapper exposes its prior location through
   the element's `id` correctly. Smithay's `RelocateRenderElement` already
   does this (it forwards `id` and computes damage from the relocated bbox
   delta). Confirmed by skimming `space_render_elements` in
   `zos-wm/src/render.rs:179`.
4. **Per-frame call site.** Hyprland uses a separate event-loop timer for
   the animation tick. We *could* do the same in `calloop`, but it's simpler
   to tick at the start of each render. Render call sites:
   - Winit backend: `winit.rs:425` (the closure inside `dispatch_new_events`
     that calls `render_output`).
   - Udev backend: `udev.rs:1771` (`fn render`) → `render_surface`
     (line 1790) → `fn render_surface` (line 1946).
5. **Frame scheduling when settling.** Hyprland's `scheduleFrameForMonitor`
   wakes the loop. Smithay equivalent: in udev we drive frames via
   `drm_compositor.queue_frame` and re-arm via the `OutputPresentationFeedback`
   path. While *any* `AnimatedValue` is in flight on outputs, we need to keep
   asking for frames until everything settles. Simplest: have the animation
   manager expose `wants_frame_for(output) -> bool`, and in
   `udev.rs::render_surface` always schedule another frame if it returns true.

### Recommended Rust shape

```rust
// zos-wm/src/anim/mod.rs

pub trait Animatable: Copy + PartialEq {
    fn lerp(start: Self, end: Self, t: f32) -> Self;
}

impl Animatable for f32 { ... }
impl Animatable for Point<f64, Logical> { ... }
impl Animatable for [f32; 4] { ... }   // RGBA premultiplied; use OkLab for borders later

pub struct BezierCurve {
    baked: [(f32, f32); 255],   // (x, y) sample table
    control_points: [Vec2; 4],   // P0..P3 for debug/serialization
}

impl BezierCurve {
    pub fn new(p1: Vec2, p2: Vec2) -> Self { ... }   // bake on construction
    pub fn y_for_x(&self, x: f32) -> f32 { ... }     // binary search + lerp
}

pub struct AnimatedValue<T: Animatable> {
    begun: T,
    value: T,
    goal: T,
    started_at: Option<Instant>,
    speed: f32,                 // duration_ms = 100 * speed (Hyprland convention)
    curve: Arc<BezierCurve>,
    enabled: bool,
}

impl<T: Animatable> AnimatedValue<T> {
    pub fn new(initial: T, speed: f32, curve: Arc<BezierCurve>) -> Self;
    pub fn animate_to(&mut self, goal: T);   // sets goal, started_at = now()
    pub fn warp_to(&mut self, value: T);     // skip animation, set immediately
    pub fn tick(&mut self, now: Instant);    // advances value toward goal
    pub fn is_animating(&self) -> bool;
    pub fn value(&self) -> T;
}

pub struct AnimationManager {
    curves: HashMap<String, Arc<BezierCurve>>,
    properties: HashMap<String, AnimationPropertyConfig>,  // mirrors AnimationTree
}

impl AnimationManager {
    pub fn from_config(cfg: &AnimationsConfig) -> Self;
    pub fn property(&self, name: &str) -> &AnimationPropertyConfig;
    pub fn curve(&self, name: &str) -> Arc<BezierCurve>;
}
```

The per-frame call site in `udev.rs::render_surface` becomes:

```rust
let now = Instant::now();
state.tick_animations(now);
let needs_more = state.has_active_animations(output);
// ... existing render ...
if needs_more {
    output_state.queue_render_in(Duration::from_millis(8));   // re-arm
}
```

For the actual translation in the render path, wrap `WindowElement`s with
`RelocateRenderElement::from_element(elem, current_offset, Relocate::Relative)`
where `current_offset = window.render_offset.value() + workspace.render_offset.value()`.

## 8. zos-wm task list

Each task is 1-2 files, scoped small per the parallel-agent workflow.

| # | Task | Files |
|---|------|-------|
| 1 | Add `BezierCurve` (with 255-point baking + binary-search eval) | `zos-wm/src/anim/bezier.rs` |
| 2 | Add `Animatable` trait + impls for `f32`, `Point<f64, Logical>`, `[f32; 4]` | `zos-wm/src/anim/animatable.rs` |
| 3 | Add `AnimatedValue<T>` with `animate_to`/`warp_to`/`tick` | `zos-wm/src/anim/value.rs` |
| 4 | Add `AnimationManager` with curve registry + property-config tree | `zos-wm/src/anim/manager.rs` + `zos-wm/src/anim/config.rs` |
| 5 | Parse `animations { bezier = ..., animation = ... }` from a TOML/KDL config (pick one zos-wm uses) into `AnimationManager` | `zos-wm/src/anim/parse.rs` |
| 6 | Add `render_offset: AnimatedValue<Point>` and `alpha: AnimatedValue<f32>` to `WindowElement` | `zos-wm/src/shell/element.rs` |
| 7 | Add `render_offset: AnimatedValue<Point>` and `alpha: AnimatedValue<f32>` to `Workspace` | `zos-wm/src/shell/workspace.rs` |
| 8 | Plumb `state.tick_animations(now)` at the start of `render_surface` (udev) | `zos-wm/src/udev.rs` |
| 9 | Plumb `state.tick_animations(now)` at the start of the winit render path | `zos-wm/src/winit.rs` |
| 10 | Wrap window elements in `RelocateRenderElement` using their `render_offset` value when building the element list | `zos-wm/src/render.rs` |
| 11 | Drive window-open animation: on `xdg_toplevel` map, set `alpha` from 0→1 and `render_offset` from edge→0 | `zos-wm/src/shell/xdg.rs` |
| 12 | Drive workspace-switch animation: on workspace activate, set old `render_offset` to off-screen and new `render_offset` from off-screen→0 | `zos-wm/src/shell/workspace.rs` |
| 13 | Schedule re-render while any `AnimatedValue` is in flight (per-output dirty flag) | `zos-wm/src/udev.rs` (and mirror in winit) |
| 14 | Plumb fade-on-close: keep window alive in fading-out state until alpha reaches 0 | `zos-wm/src/shell/xdg.rs` + `zos-wm/src/shell/element.rs` |

Curves to ship in defaults (matching the user's existing
`build_files/system_files/usr/share/zos/hypr/defaults.conf`):

- `default` = (0, 0.75) → (0.15, 1.0)
- `linear` = (0, 0) → (1, 1)
- `overshot` = (0.05, 0.9) → (0.1, 1.05)
- `smoothOut` = (0.36, 0) → (0.66, -0.56)
- `smoothIn` = (0.25, 1) → (0.5, 1)

Default property bindings:

- `windowsIn` — speed 5, curve `overshot`, style `slide`
- `windowsOut` — speed 4, curve `smoothOut`, style `slide`
- `fadeIn` — speed 5, curve `smoothIn`
- `fadeOut` — speed 5, curve `smoothIn`
- `workspaces` — speed 6, curve `default`, style `slide`

## 9. Sources

- `Hyprland/src/managers/animation/AnimationManager.cpp` — `tick()`, frame
  scheduling, damage batching.
- `Hyprland/src/managers/animation/DesktopAnimationManager.cpp` — window/
  workspace/layer animation kickoff (slide/popin/gnome/fade dispatch).
- `Hyprland/src/managers/animation/AnimationManager.hpp` — class shape,
  `m_animationTimer` (1ms event-loop timer, microseconds(500) initial).
- `Hyprland/src/helpers/AnimatedVariable.hpp` — type aliases, damage policy
  enum, `SAnimationContext` (window/workspace/layer back-references).
- `Hyprland/src/config/shared/animation/AnimationTree.cpp` — parent/child
  property tree (windowsIn → windows → global).
- `hyprutils/include/hyprutils/animation/AnimatedVariable.hpp` —
  `CGenericAnimatedVariable<T, Ctx>` core: `m_Begun`/`m_Value`/`m_Goal` triple,
  `operator=` triggers animation, `setValue`/`setValueAndWarp`/`warp` API.
- `hyprutils/src/animation/AnimatedVariable.cpp` — `getPercent()` formula:
  `(ms_since_begin / 100) / speed` clamped to [0, 1].
- `hyprutils/src/animation/BezierCurve.cpp` — 255-point bake + binary search
  evaluation in `getYForPoint(x)`.
- `hyprutils/src/animation/AnimationManager.cpp` — bezier registry +
  signal-driven active list (`m_vActiveAnimatedVariables`).
- `hyprutils/include/hyprutils/animation/AnimationConfig.hpp` —
  `SAnimationPropertyConfig` (enabled / speed / bezier / style fields) and
  `CAnimationConfigTree` parent inheritance logic.
- zos-wm render entry points: `zos-wm/src/render.rs:193` (`render_output`),
  `zos-wm/src/udev.rs:1790` (`render_surface`), `zos-wm/src/winit.rs:425-432`
  (winit dispatch render path).
- User's existing animation prefs:
  `build_files/system_files/usr/share/zos/hypr/defaults.conf:48-60`.
- Smithay's `RelocateRenderElement` (per-element render offset wrapper) —
  exists in `smithay::backend::renderer::element::utils`. Already used by
  anvil's render path; the offset is the right primitive for window/workspace
  translation.
