# Phase 3 — Opt-in tiling on a floating-default base (zos-wm)

**zos-wm starting point:** Smithay `anvil` fork at `/var/home/zach/github/zOS/zos-wm/`, MIT.
**Reference inspections:**

| Project | License | Local path | Pinned commit |
|---|---|---|---|
| Hyprland | BSD-3-Clause | `/tmp/hyprland-peek` | `e3c9b64812042ade8bec47499f461f2c7d36c184` |
| niri | GPL-3.0-or-later | `/tmp/niri-peek` | `9438f59e2b9d8deb6fcec5922f8aca18162b673c` |
| Sway | MIT | `/tmp/sway-peek` (shallow) | tip of `master` at fetch time |
| cosmic-comp | GPL-3.0-or-later | live fetch from `raw.githubusercontent.com` `master` | n/a |
| bspwm | BSD-2-Clause | live fetch from `raw.githubusercontent.com` `master` | n/a |

License posture: same as Phase 2.B — patterns only from GPL projects (niri, cosmic-comp). Sway is MIT, deeper study allowed. Hyprland is BSD-3 (also non-copyleft) but for consistency we re-implement rather than copy. zos-wm stays MIT.

---

## 0. TL;DR

- **Tree of containers**, not Vec-of-windows. Leaf = `Tile(WindowKey)`, internal = `Split { orientation, ratio, children: [Box<Node>; 2] }`. Sway, bspwm, cosmic-comp, and Hyprland-dwindle all use a tree under the hood. Trees give us nested splits and (later) tabs/stacks for free.
- **One algorithm to start: binary-split / dwindle.** Cribbed semantically from Hyprland (`/tmp/hyprland-peek/src/layout/algorithm/tiled/dwindle/DwindleAlgorithm.cpp`). Future algorithms (master-stack, scrolling-columns, monocle, tabs/stacks) plug in behind a `TilingAlgorithm` trait.
- **Workspace mode is per-workspace**, encoded `WorkspaceMode { Floating, Tiled(TilingState) }`. Default is `Floating`. Plugins extend the enum with new `Tiled(...)` variants in Phase 7.
- **Per-window override** is `tiled_override: Option<bool>` on `WindowElement`. `None` → follow workspace, `Some(true)` → always tiled (only takes effect on a Tiled workspace), `Some(false)` → always floating.
- **A floating window on a Tiled workspace** lives in the workspace's `Vec<WindowKey>` floating set and renders **above** the tile grid. The tiled tree never sees it. This is niri's `Workspace { scrolling, floating, floating_is_active }` pattern (`/tmp/niri-peek/src/layout/workspace.rs:46-114`), simplified to "both spaces always co-resident, focus tracks last-clicked".

---

## 1. Tree representation choice — tree wins

| Project | Representation | Source |
|---|---|---|
| Hyprland-dwindle | Tree (binary). `SDwindleNodeData` has `children: [WP<...>; 2]`, `pParent`, `splitTop: bool`, `splitRatio: f32`, `box: CBox`. | `DwindleAlgorithm.cpp:21-72` |
| Sway | Tree (n-ary). `sway_container { layout: enum { L_NONE, L_HORIZ, L_VERT, L_STACKED, L_TABBED }, children: list_t* }`. | `sway/include/sway/tree/container.h:13-66` |
| bspwm | Tree (binary). `node_t { first_child, second_child, parent, split_type, split_ratio, client* }`. Classic textbook BSP. | `bspwm/src/types.h:36-38, 146-163` |
| cosmic-comp | Tree (n-ary, via `id_tree::Tree<Data>`). `enum Data { Group { orientation, sizes: Vec<i32>, ... }, Mapped { ... }, Placeholder { ... } }`. | `cosmic-comp/src/shell/layout/tiling/mod.rs` ~144-160 |
| niri | **Vec-of-columns** (no tree). Each `Column` is a `Vec<Tile>`. niri has no nested splits at all. | `niri/src/layout/scrolling.rs` |

### Trade-off

| | **Tree** | **Vec-of-windows + algorithm** |
|---|---|---|
| Nested splits, tabs/stacks | Free — nest a `Split` under a `Split`, add `Tabbed` variant. | Requires bolt-on bookkeeping. |
| Resize edge between siblings | Walk to common ancestor, adjust its ratio. | Re-derive sibling adjacency every time. |
| Mode switch | Insert windows one-by-one — tree builds itself. | Same. |
| Memory locality | Pointer-chasing. | Cache-friendly. |
| Borrow checker pain | Real. Solve with `Box<Node>` + `WindowKey` ids — same trick cosmic-comp uses with `id_tree`. | None. |

### Recommendation

**Tree.** Specifically:

```rust
pub enum LayoutNode {
    Tile(WindowKey),
    /// Binary split. Larger fan-outs become nested binary splits.
    /// `ratio` is the fraction occupied by `children[0]`, clamped [0.05, 0.95].
    Split {
        orientation: SplitOrientation,
        ratio: f32,
        children: [Box<LayoutNode>; 2],
    },
}

pub enum SplitOrientation { Horizontal, Vertical }
```

`WindowKey` is an opaque id from a `SlotMap<WindowKey, WindowElement>` per workspace. Storing keys instead of references sidesteps cyclic-borrow issues and keeps `LayoutNode: 'static` (cosmic-comp's `id_tree<Data>` arrangement, in spirit).

We deliberately keep the leaf variant single-window (no `Tabbed { active, windows }` yet). Tabs/stacks land later; the tree shape absorbs them without breaking the algorithm interface.

---

## 2. Binary-split (dwindle) algorithm

Cribbed semantically from Hyprland's `CDwindleAlgorithm`.

### 2.1 Data

```rust
pub struct DwindleTree {
    root: Option<LayoutNode>,
    work_area: Rectangle<i32, Logical>,
    /// Default split ratio (config-tunable, default 0.5).
    default_ratio: f32,
    /// Bias for vertical-split-on-tall-rect. Hyprland's
    /// `dwindle:split_width_multiplier`, default 1.0.
    width_multiplier: f32,
}
```

### 2.2 Insert

Hyprland flow at `DwindleAlgorithm.cpp:74-260` ("addTarget"):

1. **Pick a target leaf.** Priority: leaf under cursor → focused leaf → any leaf. If tree is empty, the new window becomes the entire tree.
2. **Choose split orientation** as the longer axis of the target leaf's rect: `if rect.w > rect.h * width_multiplier { Horizontal } else { Vertical }` (Hyprland `:160-161`).
3. **Replace the target leaf with a `Split`** containing two children: original leaf + new tile. Default to "new tile is `children[1]`"; cursor-position-aware ordering (Hyprland `:225-233`) is a follow-up.
4. **Recompute geometry** recursively from the new split.

### 2.3 Recompute geometry

Direct semantic port of Hyprland's `recalcSizePosRecursive` (`DwindleAlgorithm.cpp:38-71`):

```rust
fn recalc(node: &LayoutNode, rect: Rect) -> Vec<(WindowKey, Rect)> {
    match node {
        LayoutNode::Tile(w) => vec![(*w, rect)],
        LayoutNode::Split { orientation, ratio, children } => {
            let (r0, r1) = split_rect(rect, *orientation, *ratio);
            let mut out = recalc(&children[0], r0);
            out.extend(recalc(&children[1], r1));
            out
        }
    }
}
// split_rect: Horizontal splits along x at rect.x + width*ratio,
//             Vertical splits along y at rect.y + height*ratio.
```

Diagram of a 4-window dwindle tree:

```
          Split(H, 0.5)                     +-------+-------+
          /          \                      |       |  W2   |
       Tile(W1)    Split(V, 0.5)            |       +---+---+
                    /        \              |  W1   | W3| W4|
                Tile(W2)    Split(H, 0.5)   |       |   |   |
                              /     \       +-------+---+---+
                          Tile(W3) Tile(W4)
```

### 2.4 Close

Direct semantic port of Hyprland's `removeTarget` (`DwindleAlgorithm.cpp:279-316`):

1. Locate the leaf.
2. If it's the root, set `tree.root = None`.
3. Otherwise, **replace its parent `Split` with the surviving sibling subtree**. The split disappears; the sibling expands into the parent's full rect.
4. `recalc(root, work_area)`.

### 2.5 Resize edge

Walk from the leaf upward; find the deepest split whose orientation matches the resize axis. Adjust its `ratio` by `delta / split_size_in_axis`, clamp to `[0.05, 0.95]`, recompute. (Hyprland's `smart_resizing` walks the parent chain at `DwindleAlgorithm.cpp:347-368`.)

### 2.6 Directional focus — `Super+H/J/K/L`

Convert direction to (axis, sign). Walk from the focused leaf's path upward until you find a split whose orientation matches the axis and where the focused leaf is on the "near" side. Descend into the opposite child, picking leftmost/topmost or rightmost/bottommost leaf depending on direction. (Hyprland's `getNextCandidate`, signature at `DwindleAlgorithm.hpp:22`.)

---

## 3. Workspace data model

### 3.1 What we have today

zos-wm uses one flat `Space<WindowElement>` (`/var/home/zach/github/zOS/zos-wm/src/state.rs:159`). No workspace abstraction. New windows get random positions in the upper-left 2/3 of the output (`zos-wm/src/shell/mod.rs:394-435`). `WindowElement` is a thin `smithay::desktop::Window` wrapper (`zos-wm/src/shell/element.rs:36-37`) with per-window state attached via `UserDataMap` (`element.rs:134-136`).

### 3.2 What we need

Phase 3 introduces workspaces (sibling task 3.A). Tiling-relevant shape:

```rust
pub struct Workspace {
    pub id: WorkspaceId,
    pub mode: WorkspaceMode,
    /// Floating windows on this workspace, back-to-front. On a Tiled workspace,
    /// these overlay the tile grid.
    pub floating: Vec<WindowKey>,
    pub focus_history: VecDeque<WindowKey>,
    pub output: Option<Output>,
    pub work_area: Rectangle<i32, Logical>,
}

pub enum WorkspaceMode {
    Floating,
    Tiled(TilingState),
}

pub struct TilingState {
    pub algorithm: Box<dyn TilingAlgorithm>,
}
```

Per-window state on `WindowElement` (in its `UserDataMap`):

```rust
#[derive(Default, Clone, Copy, Debug)]
pub struct WindowLayoutState {
    /// Some(true) = pin tiled (only meaningful when ws is Tiled).
    /// Some(false) = pin floating. None = follow workspace mode.
    pub tiled_override: Option<bool>,
    /// Last floating geometry. Restored on tiled→floating.
    pub last_floating_geometry: Option<Rectangle<i32, Logical>>,
}
```

### 3.3 "Is this window currently tiled?"

```rust
pub enum WindowMode { Floating, Tiled }

fn resolve_window_mode(ws: &Workspace, w: &WindowElement) -> WindowMode {
    match (w.layout_state().tiled_override, &ws.mode) {
        (Some(true),  WorkspaceMode::Tiled(_))   => WindowMode::Tiled,
        (Some(true),  WorkspaceMode::Floating)   => WindowMode::Floating, // latent
        (Some(false), _)                         => WindowMode::Floating,
        (None,        WorkspaceMode::Floating)   => WindowMode::Floating,
        (None,        WorkspaceMode::Tiled(_))   => WindowMode::Tiled,
    }
}
```

Note: `Some(true)` on a Floating workspace stays floating. The override is *latent* — saved on the window, applied when the workspace turns Tiled.

---

## 4. Mode-switch transitions

### 4.1 Floating → Tiled

User invokes `SUPER+Shift+T` on workspace 2. All windows whose `tiled_override != Some(false)` auto-tile.

```rust
fn switch_to_tiled(ws: &mut Workspace) {
    let mut tiling = DwindleAlgorithm::new(ws.work_area);
    let mut keep_floating = Vec::new();

    for key in std::mem::take(&mut ws.floating) {
        let w = ws.window_for_key(key);
        if w.layout_state().tiled_override == Some(false) {
            keep_floating.push(key);
            continue;
        }
        // Stash floating geometry for symmetric switch-back.
        let mut st = w.layout_state();
        st.last_floating_geometry = Some(ws.geom_of(&w));
        w.set_layout_state(st);
        tiling.insert(key, /* target_hint = */ ws.focused());
    }
    ws.floating = keep_floating;
    ws.mode = WorkspaceMode::Tiled(TilingState { algorithm: Box::new(tiling) });
    apply_geometry_to_windows(ws);
}
```

**Insertion order** matters because each insert splits the focused leaf. Recommended:

1. Currently focused window first (its rect becomes the largest leaf).
2. Other windows in `focus_history` order.
3. Anything left, in geometric reading order (left-to-right, top-to-bottom by rect center).

Focus history is closer to user intent than z-order. Hyprland uses focus order for default candidate selection (`DwindleAlgorithm.cpp:106-118`).

### 4.2 Tiled → Floating

Per spec: **preserve tiled positions**.

```rust
fn switch_to_floating(ws: &mut Workspace) {
    let WorkspaceMode::Tiled(state) = std::mem::replace(&mut ws.mode, WorkspaceMode::Floating)
    else { return };
    for (key, tiled_rect) in state.algorithm.recalc_all() {
        let w = ws.window_for_key(key);
        let mut st = w.layout_state();
        st.last_floating_geometry = Some(tiled_rect);
        w.set_layout_state(st);
        w.set_geometry(tiled_rect);
        ws.floating.push(key);
    }
}
```

Alternative — restore prior floating pose from `last_floating_geometry` — is rejected for v1: it's discontinuous if the user re-arranged tiles. Document as a possible config flag later.

### 4.3 Animation hand-off

Phase-4 polish; data flow now: mode-switch emits `LayoutTransition { (key, old_rect, new_rect)... }`. Phase 4's animation system tweens. niri does this in `set_window_floating` → `tile.animate_move_from(...)` (`/tmp/niri-peek/src/layout/workspace.rs:1440-1457`).

---

## 5. Per-window override (`SUPER+V`)

```rust
fn toggle_floating_for_focused(ws: &mut Workspace, w: &WindowElement) {
    let mut st = w.layout_state();
    let current = resolve_window_mode(ws, w);
    // Toggle: if already overridden, clear; if not, override to opposite.
    st.tiled_override = match (st.tiled_override, current) {
        (Some(_), _) => None,
        (None, WindowMode::Tiled)    => Some(false),
        (None, WindowMode::Floating) => Some(true),
    };
    w.set_layout_state(st);

    // Re-resolve and act.
    match (current, resolve_window_mode(ws, w)) {
        (WindowMode::Tiled, WindowMode::Floating) => {
            // Pull from tiling tree, restore floating geometry.
            if let WorkspaceMode::Tiled(state) = &mut ws.mode {
                state.algorithm.remove(w.key());
            }
            let geo = w.layout_state().last_floating_geometry
                .unwrap_or_else(|| centred(ws.work_area, w));
            w.set_geometry(geo);
            ws.floating.push(w.key());
        }
        (WindowMode::Floating, WindowMode::Tiled) => {
            // Stash floating geometry, push into tree.
            ws.floating.retain(|k| *k != w.key());
            let mut st = w.layout_state();
            st.last_floating_geometry = Some(ws.geom_of(w));
            w.set_layout_state(st);
            if let WorkspaceMode::Tiled(state) = &mut ws.mode {
                state.algorithm.insert(w.key(), ws.focused());
            }
        }
        _ => { /* override stored but resolved mode didn't change — no-op */ }
    }
    apply_geometry_to_windows(ws);
}
```

**Spec §D answer:** toggle on a Floating workspace just stores the override; the window stays floating until the workspace becomes Tiled. This falls out of `resolve_window_mode`'s `(Some(true), WorkspaceMode::Floating) => Floating` arm. UI hint (Phase-3 polish): SSD title bar shows a "tiled-pending" indicator.

---

## 6. Floating-on-tiled coexistence

A `Tiled` workspace has both `mode: WorkspaceMode::Tiled(_)` (the binary tree, full work area) and `floating: Vec<WindowKey>` (windows with resolved mode `Floating`).

**Z-stack on a Tiled workspace, top-down:**
1. Lock screen / OSD overlays (out of scope).
2. Floating windows in `floating` order (back-to-front, last raised wins).
3. Tiled windows. Stacking is the tree, not raise-individual.
4. Wallpaper / background layers.

`floating` works exactly like on a Floating workspace — same drag-to-move, same `last_floating_geometry`, same render path. Niri's `Workspace { scrolling, floating, floating_is_active }` is the model (`/tmp/niri-peek/src/layout/workspace.rs:46-114`); we drop niri's `floating_is_active` because zos-wm focus moves per-window, not per-set.

**Focus rules:**
- Click on any window: focus it.
- `SUPER+H/J/K/L`: if focused is tiled, walk the tree (§2.6); if floating, walk geometric neighbours in `floating`, falling through to tiled grid if no floating neighbour exists in that direction.
- `SUPER+Tab` cycles MRU across both sets via `focus_history`.

**Auto-floating heuristics** (Phase-3 polish, not core): Hyprland and Sway auto-float modal dialogs (`xdg_toplevel.set_parent`), splash screens, and configured app-id allowlists. Encode as: on map into a Tiled workspace, set `tiled_override = Some(false)` if window has a parent toplevel or matches a config allowlist.

---

## 7. Future-proofing — the plugin door

Hyprland's pluggable algorithm interface (`/tmp/hyprland-peek/src/layout/algorithm/ModeAlgorithm.hpp:16-49`):

```cpp
class IModeAlgorithm {
  public:
    virtual void newTarget(SP<ITarget>) = 0;
    virtual void movedTarget(SP<ITarget>, std::optional<Vector2D>) = 0;
    virtual void removeTarget(SP<ITarget>) = 0;
    virtual void resizeTarget(const Vector2D&, SP<ITarget>, eRectCorner) = 0;
    virtual void recalculate() = 0;
    virtual void swapTargets(SP<ITarget>, SP<ITarget>) = 0;
    virtual void moveTargetInDirection(SP<ITarget>, eDirection, bool) = 0;
    /* ... */
};
```

zos-wm Rust analogue:

```rust
pub trait TilingAlgorithm: std::fmt::Debug + 'static {
    fn insert(&mut self, w: WindowKey, target_hint: Option<WindowKey>);
    fn remove(&mut self, w: WindowKey);
    fn swap(&mut self, a: WindowKey, b: WindowKey);
    fn resize_edge(&mut self, w: WindowKey, delta: Point<i32, Logical>, corner: ResizeCorner);
    fn focus_in_direction(&self, w: WindowKey, dir: Direction) -> Option<WindowKey>;
    fn recalc(&self, work_area: Rectangle<i32, Logical>) -> Vec<(WindowKey, Rectangle<i32, Logical>)>;
    fn recalc_all(&self) -> Vec<(WindowKey, Rectangle<i32, Logical>)>;
    fn set_work_area(&mut self, work_area: Rectangle<i32, Logical>);
    fn windows(&self) -> Box<dyn Iterator<Item = WindowKey> + '_>;
    fn len(&self) -> usize;
    fn predict_next(&self) -> Option<Rectangle<i32, Logical>>;
}
```

`WorkspaceMode::Tiled(TilingState { algorithm: Box<dyn TilingAlgorithm> })` accepts any impl. Phase 7 plugins will add:

| Impl | Reference |
|---|---|
| `DwindleAlgorithm` (Phase 3) | Hyprland `CDwindleAlgorithm` (BSD-3, study). |
| `MasterAlgorithm` | Hyprland `CMasterAlgorithm` (`/tmp/hyprland-peek/src/layout/algorithm/tiled/master/MasterAlgorithm.cpp`, 1309 LoC). |
| `MonocleAlgorithm` | Hyprland `CMonocleAlgorithm` (trivial: fullscreen one window). |
| `ScrollingColumnsAlgorithm` | niri (`/tmp/niri-peek/src/layout/scrolling.rs`, 5603 LoC). GPL — patterns only. |
| `TabbedSplitAlgorithm` | Sway `L_TABBED`/`L_STACKED` (`/tmp/sway-peek/include/sway/tree/container.h:13-19`). MIT, deeper study allowed. |

Plugins are dynamic Rust crates loaded at startup; they call `register_tiling_algorithm("name", fn() -> Box<dyn TilingAlgorithm>)` on a `PluginRegistry`. Out of scope for Phase 3; the trait shape needs to be right *now*.

**Stability guidance:**
- Don't expose `LayoutNode` on the trait — it's a `DwindleAlgorithm` detail. Master-stack and scrolling-columns don't have a tree.
- Pass `WindowKey` everywhere, not `WindowElement`. Algorithms shouldn't peek at smithay state.
- Return `Vec<(WindowKey, Rect)>` from `recalc` — pure data.
- Defer Hyprland's `layoutMsg` bus. Add a `cmd(&str) -> Result<()>` later if a plugin needs custom keybinds.

---

## 8. zos-wm task list — Phase 3 tiling

Sequential, each task scoped to 1-2 files. Each task is its own focused agent (per `parallel_agent_workflow` memory).

| # | Task | Files | Notes |
|---|---|---|---|
| T1 | `WindowLayoutState` UserData on `WindowElement`. | `zos-wm/src/shell/element.rs` | New struct + `layout_state()`/`set_layout_state()` via `UserDataMap`. |
| T2 | `Workspace` skeleton + `Workspaces` container on `AnvilState`. | `zos-wm/src/shell/workspace.rs` (new) | Per §3.2. Pre-req: 3.A workspace research; if not done, stub. |
| T3 | `TilingAlgorithm` trait + `WindowKey` newtype + `SlotMap` allocator. | `zos-wm/src/shell/tiling/mod.rs` (new) | Just trait + ids. No impls. |
| T4 | `LayoutNode` enum + `DwindleTree` struct + `recalc` (pure geometry). | `zos-wm/src/shell/tiling/dwindle.rs` (new, part 1) | Hand-built-tree unit tests. |
| T5 | `DwindleAlgorithm::insert`. | (extend) | §2.2 minus cursor-aware ordering (always insert as `children[1]`). |
| T6 | `DwindleAlgorithm::remove`, `swap`, `len`, `windows`. | (extend) | §2.4 plus trivial getters. |
| T7 | `DwindleAlgorithm::resize_edge`. | (extend) | §2.5 — walk to deepest matching-orientation split. |
| T8 | `DwindleAlgorithm::focus_in_direction`. | (extend) | §2.6. |
| T9 | Wire `place_new_window` to consult workspace mode. | `zos-wm/src/shell/mod.rs` | Replace random placement at `:394-435` — Tiled and not pinned-floating → `algorithm.insert`; else floating set. |
| T10 | Wire window-close to `algorithm.remove`. | `zos-wm/src/shell/xdg.rs` | On unmap, route by resolved mode. |
| T11 | `switch_to_tiled` / `switch_to_floating` + keybind. | `zos-wm/src/shell/workspace.rs` + `zos-wm/src/input_handler.rs` | §4.1, §4.2. `Super+Shift+T`. |
| T12 | `SUPER+V` per-window toggle. | same as T11 | §5. |
| T13 | Floating-on-tiled rendering / z-stack. | `zos-wm/src/render.rs` | Tiled windows first, then `ws.floating` in order. |
| T14 | Directional focus across both sets. | `zos-wm/src/input_handler.rs` | §6. |
| T15 | Output / exclusive-zone updates → `algorithm.set_work_area`. | `zos-wm/src/shell/workspace.rs` + glue in `state.rs::apply_space_change` (`state.rs:804-862`). | |
| T16 | Smoke tests. | `zos-wm/tests/tiling_smoke.rs` (new) | Headless backend; 4-window dwindle + remove + mode-toggle preservation. |

Estimated cost: 16 small agents, each ~1-3 hours of human-paced review + cargo-check.

---

## 9. Sources

### Reference projects

- **Hyprland** — BSD-3-Clause, `https://github.com/hyprwm/Hyprland`, commit `e3c9b64812042ade8bec47499f461f2c7d36c184`, local `/tmp/hyprland-peek`.
  - `src/layout/algorithm/tiled/dwindle/DwindleAlgorithm.hpp:1-58` — `CDwindleAlgorithm` declaration.
  - `src/layout/algorithm/tiled/dwindle/DwindleAlgorithm.cpp:21-72` — `SDwindleNodeData` struct + `recalcSizePosRecursive`.
  - `:74-260` — `addTarget` (place new window).
  - `:279-316` — `removeTarget` (close window, sibling promotion).
  - `:318-450` — `resizeTarget` (smart-resize across nested splits).
  - `src/layout/algorithm/ModeAlgorithm.hpp:16-49` — pluggable algorithm base interface.
  - `src/layout/algorithm/TiledAlgorithm.hpp:13-25` — `ITiledAlgorithm` (adds `getNextCandidate`).
- **niri** — GPL-3.0-or-later, `https://github.com/niri-wm/niri`, commit `9438f59e2b9d8deb6fcec5922f8aca18162b673c`, local `/tmp/niri-peek`. Patterns only.
  - `src/layout/workspace.rs:46-114` — `Workspace { scrolling, floating, floating_is_active, ... }`.
  - `src/layout/floating.rs:34-95` — `FloatingSpace<W>` with per-tile `Data`.
  - `src/layout/workspace.rs:1448-1485` — `set_window_floating`, `switch_focus_floating_tiling`.
  - `src/layout/mod.rs:404-499` — `InteractiveMoveState`, `is_floating: bool` on tile metadata.
- **Sway** — MIT, `https://github.com/swaywm/sway`, shallow tip-of-master, local `/tmp/sway-peek`.
  - `include/sway/tree/container.h:13-19` — `enum sway_container_layout { L_NONE, L_HORIZ, L_VERT, L_STACKED, L_TABBED }`.
  - `include/sway/tree/container.h:41-66` — `sway_container_state`.
  - `sway/tree/arrange.c:15-100` — `apply_horiz_layout`/`apply_vert_layout` (n-ary split arithmetic).
  - `sway/tree/container.c:1508-1564` — `container_split` ("wrap leaf in new parent split").
- **cosmic-comp** — GPL-3.0-or-later, fetched from `raw.githubusercontent.com/pop-os/cosmic-comp/master/`. Patterns only.
  - `src/shell/layout/tiling/mod.rs` ~144-160 — `Tree<Data>` via `id_tree` crate; `enum Data { Group { orientation, sizes, ... }, Mapped, Placeholder }`.
- **bspwm** — BSD-2-Clause, fetched from `raw.githubusercontent.com/baskerville/bspwm/master/`.
  - `src/types.h:36-38` — `enum split_type_t { TYPE_HORIZONTAL, TYPE_VERTICAL }`.
  - `src/types.h:146-163` — `struct node_t { first_child, second_child, parent, split_type, split_ratio, client* }`. Textbook BSP — closest cousin of our `LayoutNode`.

### zos-wm files referenced

- `/var/home/zach/github/zOS/zos-wm/src/shell/element.rs:36-137` — `WindowElement` definition + `UserDataMap` access (where `WindowLayoutState` will hang).
- `/var/home/zach/github/zOS/zos-wm/src/state.rs:151-221` — `AnvilState`; `pub space: Space<WindowElement>` at `:159`.
- `/var/home/zach/github/zOS/zos-wm/src/shell/mod.rs:394-435` — `place_new_window` (today's random placement, replaced in T9).
- `/var/home/zach/github/zOS/zos-wm/src/state.rs:804-862` — `apply_space_change` (where work-area updates hook into T15).

### Decision references

- `/home/zach/.claude/projects/-var-home-zach-github-zOS/memory/MEMORY.md` — `project_compositor_direction.md` (zos-wm is floating-first, steals patterns from Niri); `feedback_parallel_agent_workflow.md` (drives the §8 task decomposition).
- `/var/home/zach/github/zOS/docs/research/phase-2-b-niri-reusable-code.md` — the no-GPL-verbatim policy this doc follows.
