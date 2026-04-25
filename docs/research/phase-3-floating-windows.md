# Phase 3 — Floating-first window management

Research distilling how mature compositors implement floating-first window
management, with a concrete data-structure / state-machine / algorithm proposal
for `zos-wm`. zos-wm is MIT-licensed; everything below is **patterns only** —
no expressive code is lifted from GPL sources.

Reference projects studied:

- **cosmic-comp** (GPL-3.0) — `src/shell/layout/floating/mod.rs`,
  `src/shell/grabs/moving.rs`, `src/shell/grabs/mod.rs`
- **niri** (GPL-3.0) — `src/layout/mod.rs`, `src/layout/floating.rs`,
  `src/window/mod.rs`
- **Hyprland** (BSD-3) — `src/desktop/Workspace.{cpp,hpp}`,
  `src/desktop/state/Float­State.{cpp,hpp}`, `src/desktop/state/FocusState.{cpp,hpp}`
- **KWin** (GPL-2.0) — `src/window.h`, `src/workspace.h`, `src/focuschain.h`
- **Wayfire** (GPL-3.0) — `src/output/promotion-manager.hpp`, `src/view/view-impl.cpp`

zos-wm pin: anvil fork at commit `27af99ef492ab4d7dc5cd2e625374d2beb2772f7`.
Current state: a single `Space<WindowElement>` plus `PopupManager` in
`AnvilState` (`zos-wm/src/state.rs:159-160`), random initial placement in
`shell/mod.rs:394-435`, and Wayland-only move/resize grabs in
`shell/grabs.rs`. No focus history, no per-monitor stacks, no always-on-top,
no edge-snap, no workspaces.

---

## 1. TL;DR

The recommended zos-wm floating model:

1. **Per-monitor `Workspace` owns its floating stack.** Top-level state is a
   `Vec<Output>` plus a `HashMap<OutputId, Vec<Workspace>>`; each `Workspace`
   carries `windows: VecDeque<WindowEntry>` ordered bottom-to-top
   (last-element-is-top), an explicit `active_window: Option<WindowId>`, and a
   small `focus_history: Vec<WindowId>` MRU stack. Smithay's `Space` is
   demoted to a per-output cache and rebuilt from the workspace on workspace
   switch / output (un)plug.
2. **Z-order is the workspace `windows` order plus a `ZBand` enum**
   (`Below | Normal | AlwaysOnTop | Fullscreen`). Layer-shell stays in
   Smithay's separate layer map (kept in `LayerSurface` already); only
   toplevels use the workspace stack. Modals attach to parent `WindowId` and
   render immediately above the parent.
3. **Move/resize grabs continuously update `WindowEntry.location`**
   (low-latency, Hyprland-style) without round-tripping `configure` for
   position. Resize sends `configure` per edge-motion (clients expect it).
   The grab also tracks "current output", "snap zone hint", and
   "stacking-indicator hint" — finalisation runs in the grab's `unset`/`Drop`
   path, not on every motion event.
4. **New floating windows are placed by a 3-tier strategy**: explicit
   xdg-positioner / xdg-rule → cascade-from-last → pointer-following-centered
   fallback, all clamped to the focused output's non-exclusive zone. No more
   randomised location.
5. **Focus is decoupled from z-order**: the active window is the `top` of
   `focus_history`, but z-order is the `windows` deque. Click-to-focus
   reorders both by default; "focus follows mouse" only updates focus, not
   z-order. Window destroy pops from history and re-activates the next
   surviving id.

---

## 2. Z-ordering

### 2.1 Where the stacks live

| Compositor | Stack scope | Storage |
|---|---|---|
| KWin | global, per-output filter at render | `QList<Window*> stacking_order` (workspace.h:1044-1046) |
| Wayfire | per-output, per-layer | scenegraph `node_for_layer(layer::*)` |
| cosmic-comp | per-workspace floating | `Space<CosmicMapped>` order + `spawn_order: Vec<CosmicMapped>` (`floating/mod.rs:48-57`) |
| Hyprland | global window list, queried with `m_workspace == w` | `g_pCompositor->m_windows` (`Workspace.cpp:408-415`) |
| niri | per-monitor → per-workspace → per-layout | `Vec<Tile<W>>` in `floating.rs:33`, `monitor_set: MonitorSet<W>` |

**Two things agree across all five:** the active/focused window is **not
necessarily** the top of the z-stack (niri's `floating.rs:39` comment makes
this explicit), and there is exactly one ordered list per workspace/output
that drives both render order and pick order.

### 2.2 Recommended zos-wm layout

```
AnvilState
└── outputs:        Vec<OutputState>          (in workspace order)
    ├── workspaces: Vec<Workspace>            (per-output)
    │   ├── windows: VecDeque<WindowEntry>    (idx 0 = bottom, len-1 = top)
    │   ├── active:  Option<WindowId>         (focus, NOT z-position)
    │   └── focus_history: Vec<WindowId>      (MRU, no duplicates)
    └── active_workspace_idx: usize

ZBand (per WindowEntry):
   Below       → painted before Normal (e.g. desktop helpers)
   Normal      → standard floating tier
   AlwaysOnTop → painted after Normal (xdg-toplevel `set_above`, KWin keepAbove)
   Fullscreen  → painted last in toplevel range, suppresses panel layer
                 (Wayfire-style "promotion", promotion-manager.hpp:89-99)
```

Render iteration produces:

```
for output in outputs:
   layer-shell BACKGROUND
   layer-shell BOTTOM
   for band in [Below, Normal, AlwaysOnTop]:
      for w in workspace.windows where w.band == band:
         draw(w)
   layer-shell TOP                ← suppressed if any Fullscreen present
   workspace.windows where band == Fullscreen
   layer-shell OVERLAY
   cursor / dnd icon
```

This mirrors Wayfire's promotion pattern (`promotion-manager.hpp:24-46`) but
expressed as an explicit band rather than scenegraph node-toggling.

### 2.3 Modal / transient parent rule

Pattern from KWin (`window.h` modal/transient) and niri's "child above
parent" invariant (`floating.rs:1192,1197-1201`):

> When inserting or raising a window, its descendants in the `xdg_toplevel`
> parent chain must end up immediately above it. If `raise(W)` puts W at
> idx N, scan windows[N+1..] and move every descendant of W to N+1, N+2, …
> preserving their relative order.

zos-wm `bring_descendants_above(window_id)` runs after every raise.

### 2.4 ASCII of stack mutation

```
Initial workspace.windows (bottom→top):
  [ A   B   C   D ]                   active = B
  raise(B), no children → no-op on B's children:
  [ A   C   D   B ]                   active = B
  click on A:
  [ C   D   B   A ]                   active = A; focus_history MRU push
  destroy(A):
  [ C   D   B ]                       active = pop(focus_history) = B
```

---

## 3. Smart placement

### 3.1 Survey

- **anvil-current** (`shell/mod.rs:394-435`): random within
  `[origin, origin + 2/3 of output)`. Bad UX, no overlap avoidance.
- **cosmic-comp** (`floating/mod.rs:381-429`): cascade from `spawn_order`
  with +48 px offset, alternating side, vertical-overflow → cycle
  horizontally, 16 px output padding; on cold start, "centre horizontally,
  upper third vertically".
- **niri** (`floating.rs:1160-1200, 664`): rule-driven anchor
  (`RelativeTo`) → stored last position → fall back to
  `center_preferring_top_left_in_area`. Never cursor-following.
- **Hyprland** (`FloatState.cpp:4-21`): caches per-(class, title) size; on
  re-open of same app, restores the last user-set size at the centre of the
  focused output.
- **KWin**: similar (per-rule placement engine), too complex for v1.

### 3.2 Recommended algorithm

```
fn place_new_floating(ws, win, hints, pointer_loc):
    output_zone = ws.non_exclusive_zone()
    size = clamp(win.preferred_size(), MIN_SIZE, output_zone.size * 2/3)

    # (1) explicit hint from xdg rule / dialog-with-parent
    if let Some(p) = hints.explicit_position:
        return clamp_into(p, output_zone, size)
    if win.parent.is_some():
        # dialog: centre over parent
        return centre_over(parent_geo, size)

    # (2) cascade from most-recent floating window on this workspace
    if let Some(prev) = ws.windows.iter().rev().find(|w| w.band == Normal):
        candidate = prev.loc + (CASCADE_DX, CASCADE_DY)   # 48,48 px
        if fits(candidate, size, output_zone):
            return candidate
        # vertical overflow: walk back to top, shift right
        candidate = (output_zone.loc.x + (n_cycles * CASCADE_DX),
                     output_zone.loc.y + PADDING)
        if fits(candidate, size, output_zone):
            return candidate

    # (3) fallback: centre horizontally, upper third vertically
    return (output_zone.loc.x + (output_zone.size.w - size.w) / 2,
            output_zone.loc.y + (output_zone.size.h - size.h) / 3)
```

Constants (start with cosmic-comp's): `CASCADE_DX=CASCADE_DY=48`,
`PADDING=16`, `MIN_SIZE=(320, 240)`. Pointer location is **only** used to
pick the focused output, never as the literal new-window position — that
behaviour surprises users.

### 3.3 Oversize windows

If `win.preferred_size().h > output_zone.size.h`:

1. Clamp size to `min(preferred, output_zone.size - 2*PADDING)`.
2. Send `configure` with the clamped size; client decides whether to add
   scrollbars.
3. Anchor at `(centre_x, output_zone.loc.y + PADDING)` so the title bar is
   reachable.

Cosmic-comp does this in `floating/mod.rs:358-377` (lines `clamp to
non-exclusive zone`).

---

## 4. Drag / move / resize state machine

### 4.1 Trigger surface

| Source | Move | Resize |
|---|---|---|
| SSD title-bar drag | left-button-down on title-bar (`shell/element.rs:163-203` already wires `header_bar.clicked`) | left-button-down on edge ≤ `RESIZE_EDGE_PX` |
| `xdg_toplevel.move` request | `start_move` in shell/xdg.rs | n/a |
| `xdg_toplevel.resize` request | n/a | `start_resize(edges)` |
| Mod-key chord | `Super+LMB` anywhere | `Super+RMB` anywhere |

zos-wm should add the chord trigger; Hyprland's killer feature is that you
never have to aim at a 4 px window border.

### 4.2 State machine

```
                  Super+LMB  | xdg.move | titlebar-drag (>4px)
                  ───────────▼
       ┌──────────► MoveStart{anchor, win_loc} ──┐
Idle   │                                         │ pointer.motion
       │                                         ▼
       │           Moving{delta, snap_hint, current_output}
       │                                         │
       │                                         │ pointer.motion (still)
       │                                         │ (no buttons pressed)
       │                                         ▼
       └──────── ReleaseMove{commit snap, finalize} ── back to Idle

                  Super+RMB  | xdg.resize | edge-drag
                  ───────────▼
       ┌──────────► ResizeStart{edges, init_size, init_loc} ─┐
Idle ──┤                                                     │ pointer.motion
       │                                                     ▼
       │            Resizing{last_size}  (sends configure each motion)
       │                                                     │
       │                                                     │ release
       │                                                     ▼
       └──────── WaitingForFinalAck{serial} ─── ack → Idle
                                              ↑
                                              └── on commit; re-anchors
                                                  if TOP/LEFT edges
```

### 4.3 Move semantics — low-latency vs configure-driven

- **cosmic-comp** (`grabs/moving.rs:311-424`): updates an in-memory
  `MoveGrabState.location` per motion, draws preview, commits via `Drop`
  (lines 533-652). No `configure` for position changes during the grab.
- **Hyprland**: same — `m_realPosition` is animated locally, the wl_surface
  is moved via the compositor's own scenegraph; clients are oblivious.
- **anvil-current** (`shell/grabs.rs:34-49`): `space.map_element(...)` per
  motion. This works because Smithay's `Space` doesn't round-trip the client
  for a position change. Keep this; it is functionally identical to
  cosmic-comp.

**Decision**: keep Smithay `Space::map_element` per `motion()` event. It is
already low-latency. Only add a `MoveGrabHints` struct that records:

```rust
struct MoveGrabHints {
    initial_window_location: Point<i32, Logical>,
    initial_pointer:         Point<f64, Logical>,
    snap_target:             Option<SnapTarget>,   // recomputed per motion
    crossed_outputs:         SmallVec<[Output; 2]>,// for output_enter/leave
}
```

### 4.4 Resize semantics — configure-driven

Clients (especially GTK) expect a `configure` for every size change so they
can repaint with the right buffer size. Keep the current pattern in
`shell/grabs.rs:412-426` (sets `Resizing` state, `state.size = Some(new)`,
`xdg.send_pending_configure()`). Match cosmic-comp's two-phase ack
(`grabs/floating/resize.rs:32-44`):

```
Resizing(ResizeData)
  ↓ button release
WaitingForFinalAck(ResizeData, Serial)
  ↓ client ack_configure with that serial
WaitingForCommit(ResizeData)
  ↓ commit happens
NotResizing  (and re-anchor if TOP|LEFT)
```

This is already in `shell/grabs.rs:329-340`; it stays.

### 4.5 Snap-to-edge

Add to `Moving` motion, after pos update:

```
EDGE_SNAP_THRESHOLD = 16 px
zone = active_output.non_exclusive_zone()
if |pos.x - zone.left|   < EDGE_SNAP_THRESHOLD : pos.x = zone.left
if |pos.x+size - zone.right|  < EDGE_SNAP_THRESHOLD : pos.x = zone.right - size.w
(symmetric for y)
```

Tile-snap (Aero-style: drag to top → maximize, drag to corner → quarter)
follows cosmic-comp `floating/mod.rs:1196-1206` (`snap_to_corner` +
`TiledCorners` enum). Defer to phase 3.5; gate behind a config flag.

---

## 5. Focus model

### 5.1 Survey

- **KWin** (`focuschain.h:26-58`, lines 40-53, 97-100): two structures —
  a global `m_mostRecentlyUsed: QList<Window*>` (last item = most recent)
  and a per-virtual-desktop `QHash<VirtualDesktop*, Chain> m_desktopFocusChains`.
  `update(Change::MakeLast)` on focus, `remove()` on close. Alt+Tab
  consumes the MRU chain.
- **Hyprland** (`FocusState.hpp:45-49`): only stores **current**
  focused-window/surface/monitor as weak pointers. History is in the
  workspace as `m_lastFocusedWindow` (`Workspace.cpp:308-313`).
- **niri** (`floating.rs:39`): explicit `active_window_id: Option<W::Id>`,
  separated from z-order. `Layout` keeps a workspace-id MRU map
  (`layout/mod.rs:391`).
- **cosmic-comp** (`floating/mod.rs:816-844`): no internal focus history;
  queries seat `get_keyboard().current_focus()` at click time.

### 5.2 Recommended zos-wm focus structures

```rust
// Per-workspace, on the Workspace struct
pub struct Workspace {
    pub windows:        VecDeque<WindowEntry>,   // z-order
    pub active:         Option<WindowId>,        // currently focused
    pub focus_history:  Vec<WindowId>,           // MRU, last = most recent
    ...
}

// Global, on AnvilState
pub struct GlobalFocus {
    pub focused_output:    Option<OutputId>,
    pub workspace_history: Vec<(OutputId, WorkspaceId)>,  // for "focus prev workspace"
}
```

### 5.3 Transitions

```
focus(win):
    workspace.active = Some(win)
    workspace.focus_history.retain(|w| *w != win)
    workspace.focus_history.push(win)
    if click_to_raise:
        workspace.raise(win)              // moves to top of z
        bring_descendants_above(win)
    seat.keyboard.set_focus(win.surface)

destroy(win):
    workspace.windows.retain(|w| w.id != win)
    workspace.focus_history.retain(|w| *w != win)
    workspace.active = workspace.focus_history.last().copied()
    if let Some(next) = workspace.active:
        seat.keyboard.set_focus(next.surface)

workspace_switch(new_ws):
    let prev = current_workspace()
    prev.deactivate_active()                 // sends activated=false
    current_workspace = new_ws
    if let Some(w) = new_ws.active:
        seat.keyboard.set_focus(w.surface)   // restore via active, not z-top
```

### 5.4 Click-to-focus vs sloppy focus

Two configs:

```rust
enum FocusMode { ClickToFocus, FollowMouse, FollowMouseClickToRaise }
```

- `ClickToFocus`: only `pointer.button(Press)` triggers the focus()
  transition above.
- `FollowMouse`: `pointer.motion` triggers `focus()` **without** raising
  (`raise` skipped) — niri's "focus-follows-mouse should activate, not
  raise" rule (`floating.rs:39`).
- `FollowMouseClickToRaise`: motion → focus, button-press → raise.

Per-workspace override is straightforward — store on `Workspace`, fall back
to global config.

### 5.5 Alt+Tab cycling

Walks `focus_history` backwards (KWin pattern, `focuschain.h:127-137`).
While the chord is held, snapshot the history into a transient `cycle_idx`;
release commits the picked window and re-orders history. Wraparound at
both ends.

---

## 6. Multi-monitor behaviour

### 6.1 Window-travel during drag

Pattern from cosmic-comp (`grabs/moving.rs:350-370`):

> While `Moving`, on each motion: compute new bbox; for each output, if
> bbox overlaps and was not in `current_outputs` → call
> `window.output_enter(o, overlap)`; if previously overlapped and no longer
> → `window.output_leave(o)`. Maintain a `SmallVec<[Output; 2]>` of current
> outputs.

Smithay's `SpaceElement::output_enter`/`output_leave` are already called by
`Space::map_element(_, _, true)` if you remap on each motion (which we
already do). Verify by reading
`smithay/src/desktop/space/element.rs::Space::map_element` — it scans
outputs and emits enter/leave automatically. So **no new code needed for
travel**, just don't fight Smithay.

### 6.2 "Move to next output" keybind

```
fn move_window_to_next_output(state):
    let win = workspace.active?;
    let from = state.focused_output;
    let to   = state.outputs.next_after(from);
    let from_ws = workspace_for(from);
    let to_ws   = workspace_for(to);
    let entry = from_ws.windows.remove_by_id(win)?;
    // re-anchor: position relative to to.zone using ratio
    let ratio = (entry.loc - from.zone.loc) / from.zone.size;
    let new_loc = to.zone.loc + ratio * to.zone.size;
    to_ws.windows.push_back(WindowEntry { loc: new_loc, ..entry });
    to_ws.activate(win);
```

This is cosmic-comp's `set_output` proportional-scale pattern
(`floating/mod.rs:340-346`).

### 6.3 Workspace model

| WM | Model |
|---|---|
| KWin / Plasma | per-output independent workspaces |
| GNOME / Mutter | global "primary" + per-output |
| Hyprland | per-monitor by default, configurable |
| niri | per-output (`MonitorSet`) |
| cosmic-comp | per-output |

**Recommend per-output for zos-wm.** Matches the user's 3-monitor box, lets
each monitor switch workspaces independently, matches niri's
`last_active_workspace_id: HashMap<String, WorkspaceId>` recovery
(`layout/mod.rs:391`) — when a monitor is reconnected, restore its previous
active workspace.

### 6.4 xdg-output advertisement

Use Smithay's existing `xdg_output_manager_state`. No new work for this
phase; floating window x/y are global compositor coords, clients only see
their own surface coords.

---

## 7. Outside-screen rescue

### 7.1 Monitor disconnect

Pattern from cosmic-comp (`floating/mod.rs:1185` "fixup any out of bounds
elements") and niri's `MonitorSet::Normal` → `NoOutputs` transition
(`layout/mod.rs:411-414`):

```
fn handle_output_unplug(state, output):
    let ws = state.workspaces[output].drain_all();   // takes windows
    let target = state.outputs.first()?              // first surviving
        .or(NoOutputs);
    for (entry, was_active) in ws:
        target.workspaces[active].windows.push_back(entry);
        // re-anchor via centre, since old coords are meaningless
        entry.loc = recompute_centred(target.zone, entry.size);
    state.workspaces.remove(output);
    if state.focused_output == Some(output):
        state.focused_output = state.outputs.first();
```

If no outputs remain (laptop lid close, KVM swap), keep windows in a
detached `parking_lot: Vec<WindowEntry>` (niri pattern,
`MonitorSet::NoOutputs`) and re-attach when an output appears.

### 7.2 Off-screen drag rescue

niri does this implicitly in `Data::recompute_logical_pos`
(`floating.rs:109-134`) with a comment "Make sure the window doesn't go too
much off-screen. Numbers taken from Mutter."

zos-wm rule (per-motion in the move grab):

```
MIN_ON_SCREEN = 75     # px of titlebar that must remain visible
zone = active_output.zone  # union of overlapping output zones if crossing
visible_h_band = (zone.left - (size.w - MIN_ON_SCREEN), zone.right - MIN_ON_SCREEN)
visible_v_band = (zone.top, zone.bottom - MIN_ON_SCREEN)         # never above top
loc.x = clamp(loc.x, visible_h_band)
loc.y = clamp(loc.y, visible_v_band)
```

This runs *after* edge-snap, before `space.map_element`. 75 px chosen so
the SSD header bar (`HEADER_BAR_HEIGHT` in `shell/ssd.rs`) stays
clickable.

### 7.3 Manual rescue keybind

`Super+Home` → `recentre_active()`:

```
let ws = current_workspace_mut();
let win = ws.active?;
let zone = focused_output.zone;
ws.set_loc(win, centre(zone, win.size()));
```

---

## 8. zos-wm data structures (concrete Rust)

These extend the existing types. Existing `WindowElement` in
`zos-wm/src/shell/element.rs:36-37` is a unit struct around
`smithay::desktop::Window`; we wrap it not replace it.

```rust
// zos-wm/src/shell/element.rs

pub type WindowId = u32;        // monotonic from atomic counter

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZBand { Below, Normal, AlwaysOnTop, Fullscreen }

#[derive(Debug, Clone)]
pub struct WindowEntry {
    pub id:              WindowId,
    pub element:         WindowElement,        // existing wrapper
    pub location:        Point<i32, Logical>,  // global compositor coords
    pub band:            ZBand,
    pub workspace_id:    WorkspaceId,
    pub parent_id:       Option<WindowId>,     // for modals
    // last user-set size; restored on un-fullscreen / un-maximise
    pub stored_size:     Option<Size<i32, Logical>>,
    // active state mirrored to xdg_toplevel
    pub activated:       bool,
}
```

```rust
// zos-wm/src/shell/workspace.rs (new file)

pub type WorkspaceId = u32;

pub struct Workspace {
    pub id:             WorkspaceId,
    pub output_id:      OutputId,
    pub windows:        VecDeque<WindowEntry>,   // bottom -> top
    pub active:         Option<WindowId>,
    pub focus_history:  Vec<WindowId>,           // MRU, end = most recent
}

impl Workspace {
    pub fn raise(&mut self, id: WindowId);
    pub fn lower(&mut self, id: WindowId);
    pub fn focus(&mut self, id: WindowId, raise: bool);
    pub fn add(&mut self, entry: WindowEntry);
    pub fn remove(&mut self, id: WindowId) -> Option<WindowEntry>;
    pub fn bring_descendants_above(&mut self, id: WindowId);
    pub fn next_after_destroy(&self) -> Option<WindowId>;  // = focus_history.last
    /// Iterate bottom-to-top, splitting by ZBand for render passes.
    pub fn iter_band(&self, band: ZBand) -> impl Iterator<Item = &WindowEntry>;
}
```

```rust
// zos-wm/src/shell/output_state.rs (new file)

pub type OutputId = u32;

pub struct OutputState {
    pub id:                OutputId,
    pub output:            smithay::output::Output,
    pub workspaces:        Vec<Workspace>,
    pub active_workspace:  usize,
    pub last_seen_active:  Option<WorkspaceId>,  // restore on reconnect
}
```

```rust
// zos-wm/src/state.rs additions

pub struct AnvilState<BackendData: Backend + 'static> {
    // ... existing fields ...

    // demote `space: Space<WindowElement>` to a per-frame cache, owned by
    // the workspace render path. Keep `space` for now to minimise diff;
    // make Workspace::sync_to_space(&Space) the single writer.
    pub space:               Space<WindowElement>,

    pub outputs:             Vec<OutputState>,        // NEW
    pub focused_output:      Option<OutputId>,        // NEW
    pub workspace_history:   Vec<(OutputId, WorkspaceId)>,
    pub parking_lot:         Vec<WindowEntry>,        // outputs unplugged
    pub focus_mode:          FocusMode,               // NEW
    pub next_window_id:      AtomicU32,               // NEW
}

pub enum FocusMode { ClickToFocus, FollowMouse, FollowMouseClickToRaise }
```

```rust
// zos-wm/src/shell/grabs.rs additions to existing PointerMoveSurfaceGrab

pub struct PointerMoveSurfaceGrab<B: Backend + 'static> {
    pub start_data:               PointerGrabStartData<AnvilState<B>>,
    pub window_id:                WindowId,                  // CHANGED from WindowElement
    pub initial_window_location:  Point<i32, Logical>,
    pub initial_pointer:          Point<f64, Logical>,
    pub snap_hint:                Option<SnapTarget>,        // NEW
    pub current_outputs:          smallvec::SmallVec<[OutputId; 2]>, // NEW
}

#[derive(Debug, Clone, Copy)]
pub enum SnapTarget {
    Edge(EdgeSet),         // L|R|T|B for half-tile
    Corner(CornerSet),     // TL|TR|BL|BR for quarter-tile
    Maximize,              // top-edge full-width hover
    None,
}
```

---

## 9. Implementation tasks (ordered)

Each task is 1-2 files in scope, suitable for a single rust-expert agent.

1. **T-3.1 — `WindowEntry` & `WindowId`** (`shell/element.rs`)
   Add `WindowId` (atomic counter), `ZBand`, `WindowEntry`. Do **not**
   touch existing `WindowElement` API. Add `WindowEntry::new(element)` and
   a `user_data` slot on `WindowElement` storing the `WindowId` for
   reverse-lookup from a `WlSurface`.

2. **T-3.2 — `Workspace` skeleton** (`shell/workspace.rs` new)
   Implement `add`, `remove`, `raise`, `lower`, `iter_band`. No focus, no
   sync-to-Space yet. Unit-tested with mock entries.

3. **T-3.3 — `Workspace` focus model** (`shell/workspace.rs`)
   Add `active`, `focus_history`, `focus`, `next_after_destroy`,
   `bring_descendants_above`. Property test: `destroy(active)` always
   leaves `active = focus_history.last()`.

4. **T-3.4 — `OutputState` + `AnvilState` wiring** (`state.rs`,
   `shell/output_state.rs` new)
   Replace direct `space.elements()` callers in `state.rs` with workspace
   queries. Keep `Space` as a write-only render cache; add
   `OutputState::sync_to_space(&mut Space)` rebuilding the `Space` from
   the active workspace.

5. **T-3.5 — `place_new_window` rewrite** (`shell/mod.rs:394-435`)
   Replace random placement with the cascade-+-fallback algorithm in §3.2.
   Drop `rand` dependency for placement.

6. **T-3.6 — Move grab carries `WindowId`, edge-snap, off-screen rescue**
   (`shell/grabs.rs`, `shell/workspace.rs`)
   Refactor `PointerMoveSurfaceGrab.window: WindowElement` → `window_id:
   WindowId`. In `motion()`: edge-snap (§4.5), off-screen rescue (§7.2),
   then `space.map_element` (unchanged).

7. **T-3.7 — Resize grab keeps existing two-phase ack** (`shell/grabs.rs`)
   Audit only — no behaviour change. Confirm `WaitingForFinalAck` →
   `WaitingForCommit` transitions still match cosmic-comp pattern. Add
   tests against synthetic `ack_configure` serials.

8. **T-3.8 — Click-to-focus path** (`focus.rs`, `input_handler.rs`)
   On pointer-button press over a `WindowElement`, call
   `workspace.focus(id, raise=match focus_mode {ClickToFocus|FMCR => true,
   FM => false})`.

9. **T-3.9 — Modal/transient parent above parent**
   (`shell/xdg.rs:new_toplevel`)
   On `new_toplevel`, read `xdg_toplevel.parent`; if present, set
   `WindowEntry.parent_id` and call `bring_descendants_above` after
   adding.

10. **T-3.10 — Always-on-top via `set_above`** (`shell/xdg.rs`)
    Honour `xdg_toplevel.set_parent` + KDE plasma "keep above"
    (kde-decoration extension). Mutate `WindowEntry.band` between
    `Normal` ↔ `AlwaysOnTop`. Re-render.

11. **T-3.11 — Multi-monitor unplug rescue** (`state.rs`,
    `shell/workspace.rs`)
    Implement §7.1: drain workspaces of unplugged output to first
    surviving output, recentre. Implement parking_lot for last-output.

12. **T-3.12 — `Super+LMB` move chord** (`input_handler.rs`)
    Bind `Super+LMB` press → start `PointerMoveSurfaceGrab` for window
    under pointer; `Super+RMB` → `PointerResizeSurfaceGrab` with edges
    derived from pointer-quadrant relative to window centre (Hyprland
    behaviour).

13. **T-3.13 — Alt+Tab MRU cycle** (`input_handler.rs`,
    `shell/workspace.rs`)
    Snapshot `focus_history` on `Alt` press, walk backwards on `Tab`,
    commit on `Alt` release.

14. **T-3.14 — Snap-to-corner half/quarter tile (gated)**
    (`shell/grabs.rs`, `shell/workspace.rs`)
    Add `SnapTarget::{Edge, Corner, Maximize}`. Trigger zone overlay
    during drag; on release, `set_geometry` to the snapped rect and mark
    `WindowEntry.band = Normal` (keep-floating, just resized). Behind a
    `tiling_snap_enabled` config flag.

15. **T-3.15 — Per-output workspace switching keybinds**
    (`input_handler.rs`)
    `Super+1..9` → switch active workspace on `focused_output`.
    `Super+Shift+1..9` → move active window to that workspace on the same
    output. `Super+Ctrl+arrow` → focus next output.

Tasks T-3.1–T-3.6 are the minimum viable floating-first model. T-3.7–T-3.11
are correctness fixes. T-3.12–T-3.15 are UX polish.

---

## Sources

All file references checked at the SHAs / branch tips below on 2026-04-24.

| Project | URL | Ref |
|---|---|---|
| cosmic-comp | https://github.com/pop-os/cosmic-comp | `master` (GPL-3.0) |
| niri | https://github.com/YaLTeR/niri | `main` (GPL-3.0) |
| Hyprland | https://github.com/hyprwm/Hyprland | `main` (BSD-3) |
| KWin | https://invent.kde.org/plasma/kwin | `master` (GPL-2.0) |
| Wayfire | https://github.com/WayfireWM/wayfire | `master` (GPL-3.0) |
| Smithay (zos-wm base) | https://github.com/Smithay/smithay | anvil at `27af99ef492ab4d7dc5cd2e625374d2beb2772f7` |

Specific files cited:

- `cosmic-comp/src/shell/layout/floating/mod.rs` — lines 33-57, 358-377,
  381-429, 816-844, 1185, 1196-1216
- `cosmic-comp/src/shell/layout/floating/grabs/resize.rs` — lines 32-44,
  75-127, 129-140
- `cosmic-comp/src/shell/grabs/moving.rs` — lines 58-65, 311-424, 350-370,
  455-468, 488-497, 533-652
- `cosmic-comp/src/shell/grabs/mod.rs` — lines 233, 271-453, 455-616
- `niri/src/layout/mod.rs` — lines 383-414, 391, 693-695, 1399-1573,
  1697, 2172
- `niri/src/layout/floating.rs` — lines 33, 36, 39, 75-90, 109-134,
  395-401, 420-445, 559-565, 664, 879-893, 1020-1027, 1048-1106,
  1160-1200, 1192-1201
- `niri/src/window/mod.rs` — lines 26-29, 32-119, 121-210, 255-273,
  282-324
- `Hyprland/src/desktop/Workspace.cpp` — lines 308-313, 379-395, 408-415
- `Hyprland/src/desktop/state/FloatState.{hpp,cpp}` — hpp:7,50; cpp:4-21
- `Hyprland/src/desktop/state/FocusState.{hpp,cpp}` — hpp:36-49; cpp:96
- `KWin/src/window.h` — lines 626, 655-663, 676, 704, 732-734, 752-758,
  900-901, 1048-1054
- `KWin/src/workspace.h` — lines 1019-1021, 1044-1046, 1073, 1087
- `KWin/src/focuschain.h` — lines 26-58, 40-53, 57-58, 97-100, 127-137
- `Wayfire/src/output/promotion-manager.hpp` — lines 10-16, 24-46, 48-67,
  89-99, 101-110

zos-wm files referenced (read-only, not modified):

- `zos-wm/src/state.rs:139-227` (AnvilState fields)
- `zos-wm/src/shell/mod.rs:394-435` (current `place_new_window`),
  `437-475` (`fixup_positions`)
- `zos-wm/src/shell/element.rs:36-401` (WindowElement, SSD, SpaceElement
  impl)
- `zos-wm/src/shell/grabs.rs` (existing pointer/touch move + resize grabs;
  ResizeState two-phase ack at 329-340; resize motion-handler at 351-427)
- `zos-wm/src/shell/xdg.rs:42-77` (`new_toplevel` calls
  `place_new_window`)
