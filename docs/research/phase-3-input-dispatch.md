# Phase 3 — Input Dispatch Architecture for zos-wm

> Research only. Read-only on `zos-wm`. Niri is GPL-3 — patterns only, no copy.
> Anvil is the Smithay reference compositor (MIT) — we're forking it.

## TL;DR — input flow at a high level

1. `process_input_event(InputEvent<B>)` (in `input_handler.rs`) is the single
   entry point called by every backend (`udev`, `winit`, `x11`). It pattern-
   matches on `InputEvent::{Keyboard, PointerMotion, PointerButton,
   PointerAxis, TouchDown, …, TabletTool*, Gesture*, DeviceAdded/Removed}`.
2. Keyboard events go through a closure passed to `KeyboardHandle::input(...)`
   which sees the modified `Keysym` + live `ModifiersState`; the closure
   returns `FilterResult::Intercept(KeyAction)` (we eat the key) or
   `FilterResult::Forward` (client gets the keysym). Today, `KeyAction` is
   small and hardcoded (`Quit`, `Run`, `VtSwitch`, scale/rotate, etc.).
3. Pointer button-press first calls `update_keyboard_focus(...)`, which
   raises the window under the cursor and `keyboard.set_focus(...)`s it
   (click-to-focus is already implemented). Then `pointer.button(...)` is
   called — `PointerHandle` routes into the active grab (or default
   `DefaultGrab`) which forwards the event to the client.
4. Grabs (`PointerMoveSurfaceGrab`, `PointerResizeSurfaceGrab`,
   `TouchMove*`, `TouchResize*`) live in `shell/grabs.rs`. They are entered
   today only via `xdg_toplevel.move`/`xdg_toplevel.resize` requests
   (client-initiated) and via the SSD titlebar click.
5. zos-wm gaps: (a) no compositor-initiated move/resize from `Super+LMB` /
   `Super+RMB`, (b) no edge-of-window resize hit-test, (c) no
   `HashMap<KeyCombo, Action>` config-driven binds, (d) no rich `Action`
   enum (workspace, focus-cycle, spawn-named-app, toggle-floating).

---

## 1. Anvil's existing flow (annotated walkthrough)

### 1.1 Top-level dispatch

`zos-wm/src/input_handler.rs:574` — udev backend:

```rust
pub fn process_input_event<B: InputBackend>(
    &mut self, dh: &DisplayHandle, event: InputEvent<B>,
) {
    match event {
        InputEvent::Keyboard { event, .. } => match self.keyboard_key_to_action::<B>(event) {
            KeyAction::VtSwitch(vt) => { … session.change_vt(vt) … }
            KeyAction::Screen(num) => { /* warp pointer to output #num */ }
            KeyAction::ScaleUp => { /* per-output scale +0.25, rescale pointer */ }
            …
            action => self.process_common_key_action(action),
        },
        InputEvent::PointerMotion { event, .. } => self.on_pointer_move::<B>(dh, event),
        InputEvent::PointerMotionAbsolute { event, .. } => self.on_pointer_move_absolute::<B>(dh, event),
        InputEvent::PointerButton { event, .. } => self.on_pointer_button::<B>(event),
        InputEvent::PointerAxis { event, .. } => self.on_pointer_axis::<B>(event),
        InputEvent::TabletToolAxis | TabletToolProximity | TabletToolTip | TabletToolButton ⇒ tablet handlers,
        InputEvent::GestureSwipeBegin/Update/End | GesturePinchBegin/Update/End | GestureHoldBegin/End ⇒ pointer.gesture_*,
        InputEvent::TouchDown/Up/Motion/Frame/Cancel ⇒ touch handlers,
        InputEvent::DeviceAdded/Removed ⇒ tablet_seat add/remove + add_touch,
        _ => (), // explicit silent drop
    }
}
```

The windowed (winit/x11) variant (`input_handler.rs:443`) is a smaller subset
— no tablet, no touch, no gestures, no relative pointer motion, since the
wrapping window-system already abstracts those.

### 1.2 Keyboard path

`keyboard_key_to_action` (`input_handler.rs:138`):

1. Pulls `keycode`, `state`, `serial`, `time`.
2. **Layer-shell exclusive intercept** (`:147`): walks `layer_shell_state.layer_surfaces()`; if any Top/Overlay layer has `KeyboardInteractivity::Exclusive`, that layer surface gets focus and the key is forwarded *without* compositor binds running. Returns `KeyAction::None`.
3. **Inhibitor check** (`:167`): if the surface under the pointer has an active `keyboard_shortcuts_inhibitor`, the closure forwards everything verbatim.
4. Otherwise calls `keyboard.input(self, keycode, state, serial, time, |state, modifiers, handle| { … })`. The closure receives the live `&ModifiersState` and a `KeysymHandle` (call `handle.modified_sym()` for the post-modifier `Keysym`). It returns `FilterResult::Intercept(KeyAction)` or `FilterResult::Forward`.
5. `process_keyboard_shortcut(modifiers, keysym)` (`:1298`) is a giant `if/else if` ladder mapping a tiny set of hardcoded shortcuts to `KeyAction`:
   - `ctrl+alt+BackSpace` or `Super+q` → `Quit`
   - `XF86Switch_VT_1..12` → `VtSwitch(n)`
   - `Super+Return` → `Run("weston-terminal")`
   - `Super+1..9` → `Screen(n)` (warp pointer to output)
   - `Super+Shift+M/P` → `ScaleDown/ScaleUp`
   - `Super+Shift+W/R/T/D` → `TogglePreview/RotateOutput/ToggleTint/ToggleDecorations`
6. **Suppressed-key tracking** (`:198`, `:208`): when a press triggers an action, the keysym is pushed onto `self.suppressed_keys`. On release, if the key is in the set, it's intercepted with `KeyAction::None` (so the client never sees the orphaned release).

### 1.3 Pointer button path

`on_pointer_button` (`input_handler.rs:223`):

```rust
let serial = SCOUNTER.next_serial();
let button = evt.button_code();
let state = wl_pointer::ButtonState::from(evt.state());
if state == Pressed {
    self.update_keyboard_focus(self.pointer.current_location(), serial); // click-to-focus
}
pointer.button(self, &ButtonEvent { button, state, serial, time });
pointer.frame(self);
```

`update_keyboard_focus` (`input_handler.rs:245`) — already does click-to-focus and z-order raising. Skips if pointer/keyboard/touch is grabbed (with an exception for input-method keyboard grabs). Layer order tested:

1. Output's `FullscreenSurface` if any (`:265`).
2. `Overlay` then `Top` `WlrLayer` surfaces, only if `can_receive_keyboard_focus()` (`:282`).
3. `space.element_under(...)` — calls `space.raise_element(window, true)` and sets keyboard focus to the window (`:301`).
4. `Bottom` then `Background` `WlrLayer` surfaces (`:311`).

`surface_under` (`input_handler.rs:333`) — same hit-test but builds a `(PointerFocusTarget, Point<f64, Logical>)` for pointer routing (used by motion, not button).

### 1.4 Pointer motion path

`on_pointer_move` (`input_handler.rs:779`, udev) handles:

- Pointer constraints (lock/confine via `pointer_constraints::with_pointer_constraint`).
- Relative motion: always emit `pointer.relative_motion(...)`.
- If locked: stop (no absolute motion).
- Compute new location, clamp via `clamp_coords`, re-hit-test for `new_under`.
- If confined: refuse to move outside surface or region.
- Emit `pointer.motion(self, under, &MotionEvent { … })` then `pointer.frame(self)`.
- After moving, if the new surface has a *pending* (not yet active) constraint and the pointer is inside the region, activate it.

The winit/x11 path (`on_pointer_move_absolute_windowed` `:533`) is simpler — no constraints, just clamp + dispatch.

### 1.5 Grabs in `shell/grabs.rs`

Anvil ships **four** grab classes:

| Grab | Trait | Lines |
|---|---|---|
| `PointerMoveSurfaceGrab<B>` | `PointerGrab` | `shell/grabs.rs:27` |
| `TouchMoveSurfaceGrab<B>` | `TouchGrab` | `shell/grabs.rs:170` |
| `PointerResizeSurfaceGrab<B>` | `PointerGrab` | `shell/grabs.rs:342` |
| `TouchResizeSurfaceGrab<B>` | `TouchGrab` | `shell/grabs.rs:628` |

Both move grabs:

- In `motion`: `handle.motion(data, None, event)` (no client gets pointer focus during the grab), then compute `delta = event.location - start_data.location`, then `space.map_element(window, initial_window_location.to_f64() + delta, true)`.
- In `button` (or touch `up`): when `current_pressed().is_empty()`, call `handle.unset_grab(...)`.

Both resize grabs:

- Compute `dx, dy` from `event.location - start_data.location`, flip sign for LEFT/TOP edges, clamp to `min_size`/`max_size` from `SurfaceCachedState`, and `xdg.with_pending_state(... state.size = Some(...) ...)` then `xdg.send_pending_configure()`.
- On final button-up: re-anchor location for TOP_LEFT edges, write `ResizeState::WaitingForFinalAck` (xdg) or `WaitingForCommit` (X11) into the per-surface `RefCell<SurfaceData>`.

`ResizeEdge` is a `bitflags` (`shell/grabs.rs:271`): `NONE | TOP | BOTTOM | LEFT | TOP_LEFT | BOTTOM_LEFT | RIGHT | TOP_RIGHT | BOTTOM_RIGHT`, with `From<xdg_toplevel::ResizeEdge>` and `From<X11ResizeEdge>` conversions.

Today's only entry points to a move grab are:

- `XdgShellHandler::move_request` (`shell/xdg.rs:147`) → `move_request_xdg` (`shell/xdg.rs:496`) → builds `PointerMoveSurfaceGrab` (or touch), then `pointer.set_grab(self, grab, serial, Focus::Clear)`.
- `XdgShellHandler::resize_request` (`shell/xdg.rs:152`) → `pointer.set_grab(...)` with `PointerResizeSurfaceGrab`.
- `XwmHandler::move_request` / `resize_request` for X11 (`shell/x11.rs:255`, `:213`).
- The SSD titlebar `clicked()` handler (`shell/ssd.rs:198`) which schedules a `move_request_xdg` via `event_loop.insert_idle(...)`.

**Important constraint** (`shell/xdg.rs:561`): both move and resize require `pointer.has_grab(serial)`. The serial must be the click-grab serial — i.e. the serial of the very `Pressed` event that started the implicit grab. Without that, the request is silently dropped. We must replicate this when we initiate compositor-side grabs (`Super+LMB`).

### 1.6 SSD titlebar dispatch (relevant for "click on titlebar vs content")

`shell/element.rs:147` — `pub struct SSD(WindowElement);` is a `PointerFocusTarget` variant returned by `surface_under` when the cursor is in the header bar (`shell/element.rs:46`: `if state.is_ssd && location.y < HEADER_BAR_HEIGHT as f64 { return SSD-target }`).

`SSD::button` (`shell/element.rs:193`) calls `header_bar.clicked(seat, state, &self.0, serial)`, which (`shell/ssd.rs:161`):

- Right-edge zones → close/maximize/minimize button actions.
- Anywhere else on titlebar → schedule `move_request_xdg` via `insert_idle(...)`.

The dispatch boundary is *clean*: when the pointer is in the titlebar, the surface tree never sees the click. When the pointer is in the client surface area, the SSD wrapper isn't matched and the WlSurface gets the button.

### 1.7 SeatHandler impl

`state.rs:342`:

```rust
impl<BackendData: Backend> SeatHandler for AnvilState<BackendData> {
    type KeyboardFocus = KeyboardFocusTarget;
    type PointerFocus = PointerFocusTarget;
    type TouchFocus = PointerFocusTarget;

    fn focus_changed(&mut self, seat, target) {
        // sync data-device + primary-selection focus
        let wl_surface = target.and_then(WaylandFocus::wl_surface);
        let focus = wl_surface.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, focus.clone());
        set_primary_focus(dh, seat, focus);
    }
    fn cursor_image(&mut self, _seat, image) { self.cursor_status = image; }
    fn led_state_changed(&mut self, _seat, led) { self.backend_data.update_led_state(led) }
}
```

`focus_changed` is also where zos-wm should add side effects: bumping focus history, retitling Wayland's pretty-cursor, refreshing zos-shell focus indicator, etc.

---

## 2. Niri's flow + what's worth borrowing

Source studied: `niri/src/input/mod.rs` (~5 400 lines, GPL-3 — patterns only).
Key locations:

- `on_keyboard` `mod.rs:366`
- `on_pointer_button` `mod.rs:2736`
- `on_pointer_motion` `mod.rs:2388`
- `on_pointer_axis` `mod.rs:3058`
- `do_action` `mod.rs:650`  (~120 `Action` variants)
- `should_intercept_key` `mod.rs:4313`
- `find_bind` / `find_configured_bind` `mod.rs:4397`, `:4443`
- `modifiers_from_state` `mod.rs:4494`

### 2.1 `Action` enum is rich and uniform

Niri's `Action` is one enum with ~120 variants covering window ops
(`CloseWindow`, `FullscreenWindow`, `FocusWindow(id)`), layout
(`FocusColumnLeft`, `MoveWindowDown`, `SetColumnWidth(change)`), monitor
(`FocusMonitorLeft`), workspace (`FocusWorkspaceDown`,
`MoveWorkspaceToMonitorUp`), system (`Quit(skip_confirm)`, `Suspend`,
`ChangeVt(i32)`, `Spawn(Vec<String>)`), UI (`ShowHotkeyOverlay`,
`Screenshot`, `ToggleOverview`). Every variant carries the parameters it
needs, dispatch is one `match` in `do_action(action, allow_when_locked)`.

We adopt this. Anvil's `KeyAction` is too small and the dispatch is split
(`process_common_key_action` + the matrix of `Screen/ScaleUp/...` per backend).

### 2.2 Keyboard: input-filter closure → `Bind` → `handle_bind` → `do_action`

`on_keyboard` (`mod.rs:421`):

```rust
let Some(Some(bind)) = seat.get_keyboard().unwrap().input(
    self, key_code, state, serial, time,
    |this, mods, keysym_handle| {
        let modified = keysym_handle.modified_sym();
        let raw      = keysym_handle.raw_latin_sym_or_raw_current_sym();
        let modifiers = modifiers_from_state(*mods);
        // … cancel-grab-on-Esc …
        // … MRU close on all-mods-released …
        let bindings = make_binds_iter(&config, &mut mru_ui, modifiers);
        should_intercept_key(suppressed_keys, bindings, mod_key, key_code, modified, raw, pressed, *mods, …)
    },
) else { return; };
if pressed { self.handle_bind(bind.clone()); self.start_key_repeat(bind); }
```

Worth borrowing:

- `raw_latin_sym_or_raw_current_sym()` — works around Cyrillic/Greek/Dvorak layouts that map physical-`q` to a non-latin keysym. When matching binds we want both the modified sym (for `Shift+1` ⇒ `!`) and a raw-latin fallback (`Super+q` works on Russian QWERTY). Smithay exposes both on `KeysymHandle`.
- `find_configured_bind` (`mod.rs:4443`): the bind table is just a `Vec<Bind>` linearly scanned. Modifier comparison is a bitflag equality after canonicalising the configured `mod_key` (e.g. `Mod` → `SUPER`). For a few hundred binds that's fine.
- Suppress-on-press, intercept-on-release (`mod.rs:4376`): same as anvil but cleaner — suppression is a `HashSet<Keycode>` not a `Vec<Keysym>`.
- Cancel-grab-on-Escape (`mod.rs:494`): if pointer is in a "cancellable" grab and Escape is pressed, `pointer.unset_grab(...)`. Useful for our `Super+drag` move grabs — pressing Escape mid-drag should snap the window back.
- Hardcoded VT switching is *inside* `find_bind` (`mod.rs:4409`), not a separate match arm. That keeps `do_action` clean.
- Key repeat for binds: `start_key_repeat` (`mod.rs:561`) inserts a `Timer` source into the calliope event loop with `repeat_delay` then `repeat_rate` from config. Lets `Super+Right` (focus-window) auto-repeat.

### 2.3 Pointer button: mod+click to start grabs, with consumption

`on_pointer_button` (`mod.rs:2736`):

```rust
// 1. release of a previously-suppressed mouse bind: drop it.
if self.niri.suppressed_buttons.remove(&button_code) { return; }

if Pressed {
    let mods = keyboard.modifier_state();
    let modifiers = modifiers_from_state(mods);

    // 2. mouse-button binds (Mod+MouseLeft = some configured action)
    if mods_with_mouse_binds.contains(&modifiers) {
        if let Some(bind) = find_configured_bind(bindings, mod_key, Trigger::MouseLeft, mods) {
            self.niri.suppressed_buttons.insert(button_code);   // ← consumption
            self.handle_bind(bind.clone());
            return;                                              // ← never call pointer.button()
        }
    }

    // 3. mod+LMB on a window: start MoveGrab
    if let Some(mapped) = self.window_under_cursor() {
        if button == MouseLeft && !pointer.is_grabbed() && mod_down {
            self.layout.activate_window(&window);
            let start_data = PointerGrabStartData { focus: None, button: button_code, location };
            if let Some(grab) = MoveGrab::new(self, start_data, window.clone(), false, Some(Grabbing)) {
                pointer.set_grab(self, grab, serial, Focus::Clear);
                cursor_manager.set_cursor_image(Named(Grabbing));
            }
        }
        // 4. mod+RMB on a window: hit-test edges, start ResizeGrab
        else if button == MouseRight && !pointer.is_grabbed() && mod_down {
            let edges = layout.resize_edges_under(output, pos_within_output).unwrap_or(empty());
            if !edges.is_empty() {
                if interactive_resize_begin(window.clone(), edges) {
                    let grab = ResizeGrab::new(start_data, window.clone());
                    pointer.set_grab(self, grab, serial, Focus::Clear);
                    cursor_manager.set_cursor_image(Named(edges.cursor_icon()));
                }
            }
        }
    }
}
// 5. always end with the client-facing button event (unless we suppressed/returned earlier)
pointer.button(self, &ButtonEvent { button: button_code, state, serial, time });
pointer.frame(self);
```

Key insights to copy verbatim:

1. **Suppressed-buttons HashSet** — same pattern as suppressed-keys but for mouse buttons. When a press triggers a bind/grab-start, insert the button code so the matching *release* is silently dropped. Anvil has no equivalent today.
2. **Click-to-focus is *layout*-side, not surface-side** — Niri calls `layout.activate_window(&window)` *before* `pointer.set_grab`, so the focus side-effects fire even though the grab takes over. Anvil already does this through `update_keyboard_focus` *before* `pointer.button`, so we keep that.
3. **Cursor icon during grab** — `cursor_manager.set_cursor_image(Named(Grabbing))` on grab start. Anvil's grabs don't change the cursor at all today; a UX gap.
4. **PointerGrabStartData** is built manually (not from `pointer.grab_start_data()` which only works for client-initiated grabs). Field `button: button_code` is the button that started the grab; `focus: None` because we want no client to receive focus for the duration.
5. **Edges-under-cursor** is a separate hit-test (`resize_edges_under`). For zos-wm we need our own edge hit-test on the floating-window's geometry.

### 2.4 Niri pointer-motion is short

`on_pointer_motion` (`mod.rs:2388`) is mostly the same as anvil — clamp,
constrain, `pointer.motion(...)`, `pointer.frame(...)`. The interesting
extras are: hide cursor on idle; route the motion to the screenshot UI if
open; show pointer if invisible. None of these are blockers for Phase 3.

---

## 3. Gaps zos-wm must fill

### 3.1 Modifier+button → grab dispatch (the big one)

**Currently broken**: `Super+LMB` on a window does nothing. Only the SSD
titlebar can start a move grab.

**Needed in `on_pointer_button` (`input_handler.rs:223`), in this order**:

1. Read modifier state from `seat.get_keyboard().unwrap().modifier_state()`.
2. Drop release events whose press triggered a compositor bind (suppressed-buttons HashSet on `AnvilState`).
3. On press: find the window under the cursor (`space.element_under(location)`). If `mods.logo && button == BTN_LEFT && !pointer.is_grabbed()`:
   - Build `PointerGrabStartData { focus: None, button: button_code, location: pointer.current_location() }`.
   - Build a `PointerMoveSurfaceGrab { start_data, window, initial_window_location: space.element_location(&window).unwrap() }`.
   - `pointer.set_grab(self, grab, serial, Focus::Clear)`.
   - Insert `button_code` into suppressed-buttons. Return *before* `pointer.button(self, &ButtonEvent…)` so the client never sees the click.
4. If `mods.logo && button == BTN_RIGHT && !pointer.is_grabbed()`:
   - Compute `edges` from a hit-test (see §3.2).
   - If `edges.is_empty()` (cursor is in the centre of the window), default to `BOTTOM_RIGHT` (pyramid-style "always grow toward bottom-right") or just refuse the resize — Hyprland's behaviour is "resize from the nearest edge regardless of where you click", so we pick the closest of LEFT/RIGHT and TOP/BOTTOM relative to window centre.
   - `PointerResizeSurfaceGrab { start_data, window, edges, initial_window_location, initial_window_size, last_window_size }`.
   - `pointer.set_grab(...)`. Set `data.cursor_status = CursorImageStatus::Named(edges→CursorIcon)`. Suppress the button.
5. Otherwise fall through to existing path: `update_keyboard_focus`, `pointer.button`, `pointer.frame`.

### 3.2 Edge hit-test

Anvil today only knows about the SSD header bar (`if location.y < HEADER_BAR_HEIGHT`). It has no edge hit-test for compositor-side resize.

**Spec** (Phase 3 implementation):

```rust
const RESIZE_HANDLE: i32 = 8;   // px; matches Hyprland default

fn edges_for_pointer(window_geo: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> ResizeEdge {
    let mut e = ResizeEdge::NONE;
    let local = pos - window_geo.loc.to_f64();
    if local.x < RESIZE_HANDLE as f64 { e |= ResizeEdge::LEFT; }
    if local.x > (window_geo.size.w - RESIZE_HANDLE) as f64 { e |= ResizeEdge::RIGHT; }
    if local.y < RESIZE_HANDLE as f64 { e |= ResizeEdge::TOP; }
    if local.y > (window_geo.size.h - RESIZE_HANDLE) as f64 { e |= ResizeEdge::BOTTOM; }
    e
}
```

Used in two places:

- `Super+RMB` falls back to "nearest edges to cursor" when this returns NONE.
- The pointer cursor-icon should *follow* the edge under the cursor when the cursor is within `RESIZE_HANDLE` of an edge (Hyprland-like). That hooks into `pointer.motion`, not button — which means we need a per-frame check in `on_pointer_move(_absolute)`.

### 3.3 Configurable keybindings

Today: hardcoded ladder in `process_keyboard_shortcut` (`input_handler.rs:1298`). Six fixed shortcuts plus VT switch. Useless for users wanting `bind = SUPER, V, togglefloating`.

**Replace** with a `HashMap<KeyCombo, Action>` populated at startup from a config file.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyCombo {
    pub modifiers: Modifiers,   // bitflags: SUPER | CTRL | ALT | SHIFT
    pub key: BindKey,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BindKey {
    Keysym(Keysym),       // for letters / function keys / arrows
    MouseButton(u32),     // BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, …
}

bitflags::bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct Modifiers: u8 {
        const SUPER = 1<<0;
        const CTRL  = 1<<1;
        const ALT   = 1<<2;
        const SHIFT = 1<<3;
    }
}
```

Conversion: in the `keyboard.input` closure we already have `&ModifiersState`. Build a `Modifiers` once via a helper (mirror Niri's `modifiers_from_state` `mod.rs:4494`). For matching, treat `Caps Lock` and `Num Lock` as ignored (they're toggles).

### 3.4 The `Action` enum

Phase-3 starter set (extend incrementally; *every* variant must be implemented before merging — no `TODO!()` placeholders):

```rust
#[derive(Clone, Debug)]
pub enum Action {
    // System
    Quit,
    VtSwitch(i32),
    Spawn(Vec<String>),                      // bind = SUPER, Return, exec, wezterm
    ReloadConfig,

    // Window — operate on currently-focused window
    CloseWindow,
    ToggleFloating,
    ToggleFullscreen,
    ToggleMaximize,
    Minimize,
    CenterWindow,

    // Move/Resize via keyboard (no pointer needed)
    MoveWindow(Direction),                    // Up/Down/Left/Right
    ResizeWindow(Direction, i32 /*px*/),

    // Focus
    FocusWindow(Direction),                   // spatial focus-cycle
    FocusNext,                                // alt-tab cycle
    FocusPrev,
    SwapWindow(Direction),                    // swap focus + adjacent

    // Workspace
    SwitchToWorkspace(u32),                   // 1..=10
    MoveWindowToWorkspace(u32),
    SwitchToNextWorkspace,
    SwitchToPrevWorkspace,

    // Output
    FocusOutput(Direction),
    MoveWindowToOutput(Direction),

    // Mouse-only (in a `KeyCombo { key: MouseButton(_), … }`)
    BeginMove,
    BeginResize,

    // Decorations / debug (kept from anvil)
    ToggleDecorations,
    TogglePreview,
    ToggleTint,
    RotateOutput,
    ScaleUp,
    ScaleDown,
}

#[derive(Clone, Copy, Debug)]
pub enum Direction { Up, Down, Left, Right }
```

The `BeginMove`/`BeginResize` variants are how a *configured* mouse bind
(`bindm = SUPER, mouse:272, movewindow` à la Hyprland) plugs in: the
dispatcher matches `KeyCombo { modifiers: SUPER, key: MouseButton(BTN_LEFT) }`
against the table; the matched action is `BeginMove`; in
`on_pointer_button`, *before* the modifier-check fast path, we look up
`bindings.get(&combo)` and dispatch.

### 3.5 The dispatcher — where it slots in

#### Keyboard

Refactor `keyboard_key_to_action` (`input_handler.rs:138`):

```rust
let action = keyboard.input(self, keycode, state, serial, time, |state, mods, handle| {
    let modified = handle.modified_sym();
    let raw = handle.raw_latin_sym_or_raw_current_sym();   // borrowed from Niri
    let modifiers = Modifiers::from_state(*mods);
    let pressed = matches!(state_arg, KeyState::Pressed);

    if pressed {
        // 1. hardcoded
        if let Some(a) = hardcoded_kbd_action(modified, *mods) { /* VT, ctrl-alt-bksp */
            return FilterResult::Intercept(a);
        }
        // 2. configured
        let combo_modified = KeyCombo { modifiers, key: BindKey::Keysym(modified) };
        if let Some(a) = state.bindings.get(&combo_modified) { return FilterResult::Intercept(a.clone()); }
        if let Some(raw) = raw {
            let combo_raw = KeyCombo { modifiers, key: BindKey::Keysym(raw) };
            if let Some(a) = state.bindings.get(&combo_raw) { return FilterResult::Intercept(a.clone()); }
        }
    } else {
        // release: if this keycode is in suppressed_keycodes, eat it; else forward.
        if state.suppressed_keycodes.remove(&keycode) { return FilterResult::Intercept(Action::None); }
    }
    FilterResult::Forward
});
self.dispatch_action(action);
```

(`Action::None` becomes the new "do nothing" variant replacing `KeyAction::None`.)

#### Pointer button

Insert a fast path *before* the existing `update_keyboard_focus` call in `on_pointer_button` (`input_handler.rs:223`):

```rust
if state == Pressed {
    // 0. drop suppressed-button release? (handled below via match on state too)
    let mods = self.seat.get_keyboard().unwrap().modifier_state();
    let modifiers = Modifiers::from_state(mods);

    // 1. configured mouse bind (BindKey::MouseButton)
    let combo = KeyCombo { modifiers, key: BindKey::MouseButton(button) };
    if let Some(action) = self.bindings.get(&combo).cloned() {
        match action {
            Action::BeginMove   => { self.start_compositor_move_grab(serial, button); }
            Action::BeginResize => { self.start_compositor_resize_grab(serial, button); }
            other               => { self.dispatch_action(other); }
        }
        self.suppressed_buttons.insert(button);
        return; // ← critical: skip pointer.button so the client never sees it
    }

    // 2. existing click-to-focus
    self.update_keyboard_focus(self.pointer.current_location(), serial);
} else if self.suppressed_buttons.remove(&button) {
    return; // matching release: don't deliver
}

let pointer = self.pointer.clone();
pointer.button(self, &ButtonEvent { button, state: state.try_into().unwrap(), serial, time: evt.time_msec() });
pointer.frame(self);
```

This implements both: configurable mouse binds *and* button consumption.

### 3.6 Modifier tracking

zos-wm doesn't need its own modifier-state cache — `seat.get_keyboard().unwrap().modifier_state()` (`smithay::input::keyboard::mod.rs:1099`) returns the live `ModifiersState` at any time. Use it on every pointer-button press; it's a `Mutex` lock + struct copy so it's cheap.

For the keyboard fast path, the closure passed to `keyboard.input(...)` already receives `&ModifiersState`; use that directly.

### 3.7 Click-to-focus pre-routing

Anvil's existing `update_keyboard_focus` (`input_handler.rs:245`) already runs *before* `pointer.button` on press. That gives us the pre-routing requirement for free: the focus change happens, *then* the button is delivered to the (now-focused) client.

The only enhancement we need: when the click triggers a compositor grab (`Super+LMB`), *still* run `update_keyboard_focus` (so the activate-on-click semantic survives), but *don't* deliver the button — see the dispatcher snippet above.

### 3.8 Drag-and-drop sessions vs move grabs

D&D and move-grabs are **independent grabs** managed by the same `PointerHandle`. Smithay's `start_drag` (called from `wlr-data-control` / `wl_data_device.start_drag`) installs its own `PointerGrab` impl and sets focus per-surface as the cursor moves over D&D-aware targets. Coexistence rules:

- **Mutex**: `pointer.is_grabbed()` is true if *any* grab is active. The implementations check this to refuse starting a new grab on top.
- **Source separation**: D&D is initiated by the *client* (a Wayland request from the focused surface); move grabs are initiated by the *compositor* (`Super+LMB`) or by `xdg_toplevel.move`. They never race because (a) the client cannot send `start_drag` between the press and our grab installation (those happen synchronously inside `on_pointer_button`), and (b) once we install the move grab with `Focus::Clear`, the client surface can no longer issue `start_drag` (no focus, no surface to drag from).
- **No code change needed** beyond honoring `pointer.is_grabbed()` before installing our compositor grabs, which we already do via `!pointer.is_grabbed()` checks on the move/resize path.

### 3.9 Keyboard grabs (popups, IME)

The existing path is fine:

- Popup grabs: `XdgShellHandler::grab` (`shell/xdg.rs:447`) builds a `PopupKeyboardGrab` + `PopupPointerGrab` via `popups.grab_popup(root, kind, &seat, serial)`. Anvil already gates this on `keyboard.is_grabbed() && !keyboard.has_grab(serial)` to refuse stacking grabs.
- Input-method grabs: `update_keyboard_focus` (`input_handler.rs:259`) explicitly allows focus change `if !keyboard.is_grabbed() || input_method.keyboard_grabbed()` — IME has priority, click-to-focus is suppressed.

zos-wm doesn't need to add anything here for Phase 3 *unless* we want
"keep keyboard focus locked to the focused floating window during a
compositor-initiated move grab", which would require setting a
`KeyboardGrab` alongside the `PointerGrab` to refuse focus changes from
new layer surfaces. Defer to Phase 5+.

---

## 4. Concrete task list (each task = 1–2 files, sequential)

> Order matters: each task lands a vertical slice that compiles and runs.

| # | Files | Task |
|---|---|---|
| 1 | `src/state.rs`, `src/lib.rs` | Add `Modifiers` bitflags + `BindKey` + `KeyCombo` + `Action` enum in a new module `src/binds.rs`. No dispatcher yet. Re-export from `lib.rs`. Verify `cargo check`. |
| 2 | `src/state.rs` | Add `bindings: HashMap<KeyCombo, Action>`, `suppressed_keycodes: HashSet<Keycode>`, `suppressed_buttons: HashSet<u32>` to `AnvilState`. Initialise empty in `AnvilState::init`. |
| 3 | `src/binds.rs` | Add `default_bindings()` that returns the hardcoded set anvil has today (Quit, Run, Screen 1..9, Scale±, Rotate, Toggles) translated to `Action` variants. Plus `Super+Return → Spawn(["wezterm"])`, `Super+Q → CloseWindow`, `Super+1..9 → SwitchToWorkspace`, `Super+Shift+1..9 → MoveWindowToWorkspace`. **Do not** add mouse binds yet. |
| 4 | `src/input_handler.rs` | Replace `process_keyboard_shortcut` and the `KeyAction` enum with new dispatcher: closure returns `FilterResult::Intercept(Action)`; add a single `dispatch_action(action)` method. Move all the per-backend match arms (`ScaleUp`, `Screen(n)`, `VtSwitch`) into the unified dispatcher, gated on `cfg!(feature = "udev")` for VT/Tint. |
| 5 | `src/binds.rs` | Add a tiny TOML parser (or `serde` + `toml` crate) that reads `~/.config/zos-wm/binds.toml` shaped like `[[bind]]\nmods = ["SUPER","SHIFT"]\nkey = "1"\naction = "MoveWindowToWorkspace"\nargs = [1]`. Merge over `default_bindings()`. Surface parse errors via `tracing::error!` and continue with defaults. |
| 6 | `src/input_handler.rs` | Implement the suppressed-button release-drop path in `on_pointer_button` + the configured-mouse-bind fast path (see §3.5). Add a stub `start_compositor_move_grab` / `start_compositor_resize_grab` returning `unimplemented!()` — task 7 fills them. |
| 7 | `src/input_handler.rs`, `src/shell/grabs.rs` | Implement `start_compositor_move_grab` and `start_compositor_resize_grab` on `AnvilState`. Reuse `PointerMoveSurfaceGrab` / `PointerResizeSurfaceGrab` verbatim. For resize: implement `edges_for_pointer` (§3.2). |
| 8 | `src/binds.rs`, `src/input_handler.rs` | Wire `Action::BeginMove` / `Action::BeginResize` and add the default mouse binds: `SUPER + BTN_LEFT → BeginMove`, `SUPER + BTN_RIGHT → BeginResize`. |
| 9 | `src/cursor.rs`, `src/input_handler.rs` | Update `cursor_status` to a Named cursor (`CursorIcon::Grabbing` for move, edge-specific for resize) when a compositor grab starts; restore on grab end. Hook into `PointerMoveSurfaceGrab::unset` and `PointerResizeSurfaceGrab::unset`. |
| 10 | `src/input_handler.rs` | Add Escape-to-cancel-grab in the keyboard closure — if `pointer.with_grab(|_, grab| is_compositor_initiated(grab)).unwrap_or(false)` and `keysym == Keysym::Escape`, snap the window back to `start_data` and `pointer.unset_grab(...)`. Mirrors Niri `mod.rs:494`. |
| 11 | `src/input_handler.rs` | Add per-frame edge-cursor hint in `on_pointer_move` and `on_pointer_move_absolute`: if the pointer is over a floating window's resize handle, set `cursor_status` to the appropriate `CursorIcon::N/S/E/W/NW/NE/SW/SE/Resize`; otherwise reset to `Default`. |
| 12 | `src/binds.rs`, `src/state.rs` | Add focus-history `VecDeque<WindowElement>` and the `Action::FocusNext / FocusPrev` implementations (in-process Alt+Tab; Phase 6 will swap this to spawn `zos-switcher`). |
| 13 | (next phase) | Workspaces — out of scope for input dispatch task; needed to make `Action::SwitchToWorkspace` actually do something. Land alongside Phase 3 floating-layout work. |

After tasks 1–11, `Super+drag` floating moves and `Super+right-drag` resizes
work; `Super+1..9` is wired (no-op until workspaces land); the user's TOML
binds load and override defaults; Escape cancels active compositor grabs.

---

## 5. Sources

### zos-wm (this repo, MIT, anvil fork — read-only here)

- `zos-wm/src/input_handler.rs:67` — `impl<BackendData: Backend> AnvilState<BackendData>` input methods.
- `zos-wm/src/input_handler.rs:138` — `keyboard_key_to_action`.
- `zos-wm/src/input_handler.rs:223` — `on_pointer_button`.
- `zos-wm/src/input_handler.rs:245` — `update_keyboard_focus` (click-to-focus).
- `zos-wm/src/input_handler.rs:333` — `surface_under` (hit-test).
- `zos-wm/src/input_handler.rs:574` — `process_input_event` (udev).
- `zos-wm/src/input_handler.rs:779` — `on_pointer_move` with constraints.
- `zos-wm/src/input_handler.rs:1276` — `enum KeyAction`.
- `zos-wm/src/input_handler.rs:1298` — `process_keyboard_shortcut` (the table to replace).
- `zos-wm/src/state.rs:342` — `impl SeatHandler for AnvilState`.
- `zos-wm/src/shell/grabs.rs:27` — `PointerMoveSurfaceGrab`.
- `zos-wm/src/shell/grabs.rs:170` — `TouchMoveSurfaceGrab`.
- `zos-wm/src/shell/grabs.rs:271` — `bitflags ResizeEdge`.
- `zos-wm/src/shell/grabs.rs:342` — `PointerResizeSurfaceGrab`.
- `zos-wm/src/shell/grabs.rs:628` — `TouchResizeSurfaceGrab`.
- `zos-wm/src/shell/xdg.rs:147` — `move_request` handler.
- `zos-wm/src/shell/xdg.rs:152` — `resize_request` handler.
- `zos-wm/src/shell/xdg.rs:447` — popup `grab` handler.
- `zos-wm/src/shell/xdg.rs:496` — `move_request_xdg` (constructs `PointerMoveSurfaceGrab`).
- `zos-wm/src/shell/ssd.rs:65` — `HEADER_BAR_HEIGHT`.
- `zos-wm/src/shell/ssd.rs:161` — `HeaderBar::clicked` (titlebar dispatch).
- `zos-wm/src/shell/element.rs:46` — SSD-vs-content branching in `WindowElement::surface_under`.
- `zos-wm/src/shell/element.rs:147` — `pub struct SSD`.
- `zos-wm/src/shell/element.rs:163` — `impl PointerTarget for SSD`.
- `zos-wm/src/focus.rs:39` — `KeyboardFocusTarget`.
- `zos-wm/src/focus.rs:60` — `PointerFocusTarget` (with `SSD` variant).

### Smithay (`~/.cargo/git/checkouts/smithay-*/27af99e/`, MIT — what we build on)

- `src/input/pointer/mod.rs:168` — `PointerHandle::set_grab`.
- `src/input/pointer/mod.rs:178` — `PointerHandle::unset_grab`.
- `src/input/pointer/mod.rs:187` — `PointerHandle::has_grab`.
- `src/input/pointer/mod.rs:202` — `PointerHandle::grab_start_data` (note: only valid for client-initiated grabs; we build our own `GrabStartData` for compositor-initiated).
- `src/input/pointer/mod.rs:271` — `PointerHandle::button` routes via `inner.with_grab(...)` to the active grab.
- `src/input/pointer/mod.rs:923` — `pub enum Focus { Keep, Clear }`.
- `src/input/pointer/grab.rs:178` — `pub struct GrabStartData<D: SeatHandler>`.
- `src/input/keyboard/mod.rs:553` — `pub enum FilterResult<T> { Forward, Intercept(T) }`.
- `src/input/keyboard/mod.rs:894` — `KeyboardHandle::set_grab`.
- `src/input/keyboard/mod.rs:921` — `KeyboardHandle::is_grabbed`.
- `src/input/keyboard/mod.rs:959` — `KeyboardHandle::input` (the filtered dispatch).
- `src/input/keyboard/mod.rs:1063` — `KeyboardHandle::set_focus`.
- `src/input/keyboard/mod.rs:1099` — `KeyboardHandle::modifier_state` (live `ModifiersState`).
- `src/input/keyboard/modifiers_state.rs:14` — `pub struct ModifiersState` fields (`ctrl`, `alt`, `shift`, `caps_lock`, `logo`, `num_lock`, `iso_level3_shift`, `iso_level5_shift`).

### Niri (`github.com/YaLTeR/niri`, GPL-3 — patterns only, no copy)

- `src/input/mod.rs:366` — `on_keyboard` filter closure shape.
- `src/input/mod.rs:421` — invocation of `keyboard.input(...)` returning `Bind`.
- `src/input/mod.rs:494` — Escape-cancels-cancellable-grab.
- `src/input/mod.rs:561` — `start_key_repeat` event-loop timer.
- `src/input/mod.rs:650` — `do_action(action, allow_when_locked)` mega-match.
- `src/input/mod.rs:2388` — `on_pointer_motion`.
- `src/input/mod.rs:2736` — `on_pointer_button` (the patterns we adopt).
- `src/input/mod.rs:2750` — `suppressed_buttons.remove(&button_code)` release-drop.
- `src/input/mod.rs:2780` — `mods_with_mouse_binds.contains(&modifiers)` configured-mouse-bind path.
- `src/input/mod.rs:2880` — `Mod+LMB → MoveGrab::new + pointer.set_grab(...)`.
- `src/input/mod.rs:2915` — `Mod+RMB → ResizeGrab::new + pointer.set_grab(...)` with `resize_edges_under`.
- `src/input/mod.rs:4313` — `should_intercept_key`.
- `src/input/mod.rs:4397` — `find_bind` (hardcoded VT/power-key + dispatch to `find_configured_bind`).
- `src/input/mod.rs:4443` — `find_configured_bind` (linear scan + COMPOSITOR-mod canonicalisation).
- `src/input/mod.rs:4494` — `modifiers_from_state` (the conversion we mirror).

### Hyprland (BSD-3, dispatcher pattern reference)

- `Hyprland/src/managers/input/InputManager.cpp` — `CInputManager::onMouseButton`, `CInputManager::onMouseMoved`. The pattern of "look up keybind in `g_pKeybindManager` first, eat the button if matched, else forward" is how we model `on_pointer_button`.
- `Hyprland/src/managers/KeybindManager.cpp` — `CKeybindManager::handleKeybinds(MOD, KEY, BIND, …)`. Same `Vec<Bind>` linear scan + modifier-equality semantics as Niri's `find_configured_bind`.

---

## Final summary

**Five-bullet dispatcher architecture for zos-wm:**

1. **Single `Action` enum + single `dispatch_action(Action)` method** on `AnvilState`. All keyboard shortcuts, configured mouse binds, and SSD titlebar shortcuts route through it. Replaces `KeyAction` + `process_common_key_action` + per-backend match arms.
2. **`HashMap<KeyCombo, Action>` config table** populated from `binds.rs::default_bindings()` merged with `~/.config/zos-wm/binds.toml`. Lookups are O(1), modifiers are normalised via a small `Modifiers` bitflags type.
3. **Press-suppression sets** — `HashSet<Keycode> suppressed_keycodes` and `HashSet<u32> suppressed_buttons` on `AnvilState`. When a press matches a bind, the keycode/button is inserted; the matching release is silently dropped (so the client never sees a hanging release event).
4. **Compositor-initiated pointer grabs** — `Super+LMB` / `Super+RMB` build a `PointerGrabStartData` manually (no `pointer.grab_start_data()`, since that only works for client-initiated grabs), reuse the existing `PointerMoveSurfaceGrab` / `PointerResizeSurfaceGrab` from `shell/grabs.rs`, and call `pointer.set_grab(self, grab, serial, Focus::Clear)` *before* the button event reaches the client. Edge hit-test (8 px) for resize.
5. **Keyboard filter remains in the `keyboard.input(...)` closure** — same shape as today; the closure now consults the bind table (with both `modified_sym` and `raw_latin_sym_or_raw_current_sym` fallbacks), returns `FilterResult::Intercept(Action)` or `Forward`. Escape cancels active compositor grabs.

**Two anvil files need significant edits to land Phase 3 input dispatch:**

1. **`zos-wm/src/input_handler.rs`** — replace `KeyAction`, `process_keyboard_shortcut`, `process_common_key_action`, and the per-backend `process_input_event` matches; add the `on_pointer_button` mouse-bind fast path; add `start_compositor_move_grab` / `start_compositor_resize_grab` helpers; add Escape-cancel and edge-cursor-hint paths.
2. **A new `zos-wm/src/binds.rs`** — `Modifiers`, `BindKey`, `KeyCombo`, `Action`, `Direction`, `default_bindings()`, TOML loader. (Touched once; subsequent feature work — workspaces, focus history — extends `Action` variants.)

A third file (**`zos-wm/src/state.rs`**) gets a smaller edit: add the three new fields (`bindings`, `suppressed_keycodes`, `suppressed_buttons`) to `AnvilState` and initialise them. `SeatHandler::focus_changed` is already fine — Phase 5+ can extend it for focus history if `Action::FocusNext` ends up needing it sooner than the dedicated focus-history `VecDeque`.
