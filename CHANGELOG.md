# Changelog

Living notes on the zOS rebuild. Read [ZOS_DELME.md](./ZOS_DELME.md) first for orientation, then this for granular history.

Format: reverse-chronological. Each entry = one "session" or logical chunk of work. Entries are concrete (file paths + line counts + cargo-check status), not narrative.

---

## 2026-04-24 — Phase 2 complete (3 commits)

Three commits on `main`:
- `3a684da feat(zos-wm): phase 2 compositor crate (anvil fork + 4 from-scratch protocols)` — 14043 insertions, 29 files
- `283c24a feat(image): zos-wm build + greetd session entry + hyprland keep-alive banner` — 79 insertions, 6 files
- `4849f55 docs: phase 2 research artifacts + ZOS_DELME handoff + HACKING.md` — 3834 insertions, 9 files

### Compositor (`zos-wm/`)
- Smithay anvil fork pinned to `27af99ef492ab4d7dc5cd2e625374d2beb2772f7`. MIT (NOTICE preserved).
- `main.rs` strips inherited `GBM_BACKEND` env on startup.
- `shell/ssd.rs` — real Catppuccin titlebar with usvg/resvg-rasterized SVG buttons (min/max/close), hover states, click-to-unmap minimize.

### Wayland protocols wired
| Protocol | Source | File |
|---|---|---|
| cursor-shape-v1 | smithay built-in | state.rs delegate |
| idle-inhibit-v1 | smithay built-in | state.rs handler + delegate |
| idle-notify-v1 | smithay built-in | state.rs handler + delegate |
| session-lock-v1 | smithay built-in | state.rs handler + delegate |
| pointer-warp-v1 | smithay built-in | state.rs handler + delegate |
| ext-foreign-toplevel-list-v1 | smithay built-in | state.rs + shell/xdg.rs lifecycle |
| ext-image-copy-capture-v1 (screencopy) | smithay built-in (winit + udev shm) | screencopy.rs (generic over Renderer + Bind\<T\> + Offscreen\<T\> + ExportMem) |
| xdg-decoration | smithay built-in | state.rs (request_mode honors client) |
| kde-server-decoration | smithay built-in | state.rs (default Mode::Client) |
| xwayland | anvil's existing | enabled by default |
| **wp-tearing-control-v1** | **from-scratch** | `protocols/tearing_control.rs` (381 lines) |
| **wlr-gamma-control-v1** | **from-scratch** | `protocols/gamma_control.rs` (476 lines) |
| **zwlr-output-management-v1** | **from-scratch** | `protocols/output_management.rs` (1405 lines) |

Smithay at our pin lacks the 3 from-scratch protocols. Wire bindings for all 3 are re-exported via `smithay::reexports::wayland_protocols{,_wlr}`.

### udev backend
- `apply_output_config` in `udev.rs` does real DRM modeset via `DrmOutput::use_mode`. v1 caveats:
  - position/scale/transform/disable need `Backend` trait extended to take `&mut Space` (deferred 2.D.5)
  - custom modes rejected at dispatch
  - adaptive-sync stored in user_data but DRM VRR property not yet written (deferred)
  - test_only is mode-list-only, not a real `DRM_MODE_ATOMIC_TEST_ONLY`
- output-management lifecycle hooks: `add_head` after `space.map_output` in `connector_connected`, `remove_head` before `space.unmap_output` in `connector_disconnected`.
- `ZOS_RENDER_DEVICE` env var (back-compat to `ANVIL_DRM_DEVICE`).
- `ZOS_DISABLE_SYNCOBJ` escape hatch.
- Diagnostic logs: seat name + GPU enumeration on session start, primary-GPU source, render-node-add events.
- NVIDIA overlay-plane quirk inherited from anvil (`udev.rs:1006-1014`).

### winit backend
- `add_head` after virtual-output `space.map_output`; `remove_head` on `PumpStatus::Exit`.
- Tearing-hint lookup scaffolded with TODO; trace log when clients request async (no-op in nested winit).
- `WinitEvent::Resized` → `notify_changes` not yet wired (deferred 2.D.4).

### Image build
- `Containerfile` builds `zos-wm --release --features udev,xwayland` and installs to `/usr/bin/zos-wm`. Required `COPY zos-wm /zos-wm` into `rust-ctx` scratch stage.
- `libseat-devel` + `mingw64-{gcc,binutils,headers,crt}` added to Rust build layer's `dnf5 install`.
- `Justfile` recipes: `dev-wm` (nested winit, filters EGL log spam) and `build-wm-local` (matches image build).
- `build_files/system_files/usr/share/wayland-sessions/zos-wm.desktop` — regreet picks up.
- `build_files/system_files/usr/bin/start-zos-wm` — POSIX-sh launcher with env hygiene + exec `/usr/bin/zos-wm --tty-udev`.
- `build_files/scripts/install-user-configs.sh` — copies both into image with explicit `chmod +x` on launcher.
- `build_files/scripts/install-hyprland.sh` — DO-NOT-REMOVE banner (Hyprland stays until Phase 8).

### Research docs (`docs/research/`)
- `phase-2-a-protocol-priority.md` — protocol audit
- `phase-2-b-niri-reusable-code.md` — license-aware reuse (Niri = GPL-3, zos-wm = MIT)
- `phase-2-c-drm-nvidia-specifics.md` — NVIDIA 580 + DRM/GBM + Bazzite session
- `phase-2-fix-decoration-investigation.md` — Qt-on-Wayland CSD root cause
- `phase-2-tearing-control.md` — from-scratch design
- `phase-2-output-management.md` — from-scratch design
- `phase-2-udev-gaps.md` — ~80% TTY readiness audit + 8-task plan

### Verified
- `cargo check -p zos-wm` clean
- `cargo check --workspace` clean (1 pre-existing unrelated warning in zos-dock)
- `cargo check -p zos-wm --features udev` fails on `libseat-sys` link (host missing libseat-devel; image build will succeed)

### Deferred
- async-pageflip — needs upstream smithay PR
- gamma-DRM-apply — wire `DrmDevice::set_gamma` from udev once stored LUT changes
- VT-switch resilience (`activate(true)`) — wait for empirical signal

---

## 2026-04-24 — Phase 2 deferred-cleanup batch

Three follow-up tasks knocked out (all `cargo check -p zos-wm` clean):

### 2.D.5 — Backend `apply_output_config` now reaches `Space`
- `state.rs`: `OutputManagementHandler::apply_output_config` impl extended. After `backend_data.apply_output_config(...)?`, walks changes calling new helper `apply_space_change(&change)` which handles position/scale/transform via `output.change_current_state(...)` and `space.map_output(...)`. Disable case calls `space.unmap_output` + `space.refresh()`.
- `udev.rs`: `OutputConfigAction::Disable` arm replaced. Drops `device.surfaces[crtc]` (its `Drop` impl tears down `DrmOutput` + removes `wl_output` global). `UdevOutputId` now derives `Clone`.
- Net effect: wlr-output-management `apply()` now actually changes layout, not just mode. kanshi / wlr-randr / wdisplays should work end-to-end.

### 2.D.4 — winit `Resized` → `notify_changes`
- `winit.rs`: 4 lines added at the `WinitEvent::Resized` arm calling `notify_changes(&mut state.output_management_manager_state, &output)`. Clients see new dimensions when the user resizes the nested winit window.

### 2.F.2 — screencopy dmabuf zero-copy path
- `Backend` trait gets new `screencopy_dma_constraints()` method (default `None`).
  - **Winit** override: pulls render node from `EGLDevice::device_for_display(...).try_get_render_node()` and formats from `self.backend.renderer().dmabuf_formats()`.
  - **Udev** override: `self.primary_gpu` for node, `self.gpus.single_renderer(&self.primary_gpu)?.dmabuf_formats()` for formats.
- `screencopy.rs`: `try_capture` trait bound widened with `Bind<Dmabuf>`. New fast-path at top of `try_capture` — if `get_dmabuf(&wl_buffer).is_some()`, bind dmabuf as framebuffer, render directly into it, return `Ok(())` (no readback). shm path unchanged below.
- `state.rs::capture_constraints`: now passes `dma: self.backend_data.screencopy_dma_constraints()` instead of hardcoded `None`.
- xdg-desktop-portal-pipewire and OBS get GPU-side handoff; CPU memcpy only on shm fallback.

### Verified
- `cargo check -p zos-wm` clean after each agent
- `cargo check -p zos-wm --features udev` still pre-existing libseat link (host environment)

---

## 2026-04-24 — Phase 3 MVP (floating-first WM core)

Phase 3 floating-first window management — core path landed. ~6 waves of agent work, 11+ chapters. Each wave verified `cargo check -p zos-wm` clean. Tests added for the dwindle algorithm (7/7 passing).

### Research artifacts (3 new docs in docs/research/)
- `phase-3-floating-windows.md` (784 lines) — floating model: per-monitor workspaces, VecDeque + ZBand, Smithay Space as cache, drag-state-machine, focus history
- `phase-3-tiling-opt-in.md` (468 lines) — binary-tree LayoutNode + DwindleAlgorithm + workspace mode toggle + per-window override
- `phase-3-input-dispatch.md` (680 lines) — Action enum + KeyCombo dispatch + suppressed-key/button mechanism + compositor-initiated grabs

### Foundation types (`shell/element.rs`)
- `WindowId` (atomic counter), `ZBand` (Below/Normal/AlwaysOnTop/Fullscreen, Ord-derived), `WorkspaceId`, `WindowLayoutState { tiled_override: Mutex<Option<bool>> }`, `WindowEntry`. `WindowElement::id()` and `.layout_state()` accessors via threadsafe user_data.

### Workspace + OutputState data layer (NEW files)
- `shell/workspace.rs` (~190 lines) — `Workspace { windows: VecDeque<WindowEntry>, active, focus_history }` with add/remove/raise/lower/focus/iter_band/iter_z_order/find/bring_descendants_above. Plus `sync_active_workspaces_to_space(outputs, &mut Space)` brutal sync helper.
- `shell/output_state.rs` (~70 lines) — `OutputState { id, output, workspaces, active_workspace, last_seen_active }` with switch_to (lazy-creates), workspace/workspace_mut.

### Tiling subsystem (NEW)
- `shell/tiling/mod.rs` (~70 lines) — `TilingAlgorithm` trait + `WindowKey` newtype + `Direction` + `Edge`.
- `shell/tiling/dwindle.rs` (~440 lines) — `LayoutNode` enum (Tile | Split { orientation, ratio, children }) + `DwindleTree` with full insert/remove/resize_edge/focus_in_direction implementations + 7 unit tests.

### Input dispatch (NEW + REWRITE)
- `binds.rs` (193 lines, NEW) — `Modifiers` bitflags, `BindKey { Keysym | MouseButton }`, `KeyCombo`, `Action` enum (33 variants), `default_bindings()` returning a populated HashMap with anvil debug carry-overs + zOS additions (Super+1..9 ws switch, Super+V toggle floating, Super+LMB BeginMove, Super+RMB BeginResize, Alt+Tab cycle, etc.).
- `input_handler.rs` rewrite — `KeyAction` enum + `process_keyboard_shortcut` deleted. New `dispatch_action` method with Action match. Suppressed-keycodes/buttons mechanism so swallowed press releases are not forwarded. Click-to-focus pre-routing in `on_pointer_button`.

### Action handlers wired
Real impls (no longer stubs):
- `CloseWindow` (xdg/x11 send_close)
- `SwitchToWorkspace(n)` (OutputState::switch_to + sync)
- `MoveWindowToWorkspace(n)` (remove + add to target + sync)
- `ToggleFloating` (cycles WindowLayoutState.tiled_override)
- `ToggleFullscreen` / `ToggleMaximize` (xdg_toplevel state set/unset + send_pending_configure)
- `BeginMove` / `BeginResize` (compositor-initiated grabs via PointerMoveSurfaceGrab::new_from_id + edges_for_pointer)
- `FocusNext/Prev` (focus_history step)
- `FocusDirection(dir)` (closest-window-in-direction by centre distance)
- `MoveWindow(dir)` (shift entry.location 50px + re-sync)
- Spawn / Quit / VtSwitch / Screen / ScaleUp/Down / RotateOutput / ToggleTint / TogglePreview (preserved from anvil)

Stubs (deferred): `ToggleWorkspaceTiling`.

### AnvilState additions (`state.rs`)
- `outputs: HashMap<OutputId, OutputState>`
- `focused_output: Option<OutputId>`
- `workspace_history: Vec<(OutputId, WorkspaceId)>`
- `parking_lot: Vec<WindowEntry>`
- `focus_mode: FocusMode { ClickToFocus, FollowMouse, FollowMouseClickToRaise }`
- `bindings: HashMap<KeyCombo, Action>` (initialized from `default_bindings()`)
- `suppressed_keycodes: HashSet<Keycode>`
- `suppressed_buttons: HashSet<u32>`

### Backend lifecycle integration
- `udev.rs::connector_connected`: bootstrap `OutputState`, set `focused_output` if first.
- `udev.rs::connector_disconnected`: tear down OutputState, reassign focused_output.
- `winit.rs::run_winit`: bootstrap virtual-output OutputState.
- `shell/xdg.rs::new_toplevel`: backfill WindowEntry into focused output's active workspace using location chosen by `place_new_window`.

### Smart placement (`shell/mod.rs::place_new_window`)
- Replaced random placement with 3-tier algorithm: (1) center on parent if xdg parent exists, (2) cascade +(48,48) from last window with wrap, (3) horizontally-centered upper-third fallback. `clamp_to_area` helper. `rand` dep no longer used here.

### Grabs refactor (`shell/grabs.rs`)
- `PointerMoveSurfaceGrab.window: WindowElement` → `window_id: WindowId`. Two constructors: `new_from_element` (back-compat) + `new_from_id`. Lookup-via-id in motion handler so workspace switch / window destroy doesn't panic.
- New `pub fn edges_for_pointer(rect, pointer) -> ResizeEdge` — 8px threshold + quadrant fallback for SUPER+RMB. Tested.

### Deferred for future Phase 3 polish
- Workspace tiling-mode toggle (Super+Shift+T) — needs Action::ToggleWorkspaceTiling handler + DwindleAlgorithm wiring
- Super+V tile/float toggle — Action wired but doesn't actually re-tile yet
- Modal parent above (T-3.9)
- Always-on-top via xdg set_above (T-3.10)
- Snap-to-corner half/quarter tile (T-3.14)
- Floating-on-tiled rendering z-stack (3.B-T13)
- Smoke tests (3.B-T16)

### Verified
- `cargo check -p zos-wm` clean after every wave
- `cargo test -p zos-wm --lib shell::tiling::dwindle` 7/7 passing
- `cargo check -p zos-wm --features udev` still pre-existing libseat link (host environment)

---

## 2026-04-25 — Phase 3 polish + Phase 4 animations infrastructure

### Phase 3 polish (commit a5647e2)
- Modal parent (T-3.9): xdg.rs reads `surface.parent()`, links `WindowEntry.parent_id`, bumps band to AlwaysOnTop, calls `bring_descendants_above`.
- Always-on-top (T-3.10): inline with modal logic — modals automatically go to AlwaysOnTop band.
- Workspace tiling toggle (Action::ToggleWorkspaceTiling): real handler. Allocates `DwindleTree` with output mode size as work area on Floating→Tiled. Per-workspace re-tile-on-switch logic still TODO.

### Phase 4 — Animations infrastructure (no commit yet)

**Research artifacts:**
- `phase-4-hyprland-animations.md` (498 lines) — port plan for Hyprland's BezierCurve + AnimatedVariable + AnimationManager. 14 tasks.
- `phase-4-smithay-effects.md` (808 lines) — custom shader API + 4 effects (rounded corners, drop shadow, opacity, kawase blur). Smithay's `compile_custom_pixel_shader` + `PixelShaderElement` cover everything.

**`anim/` module (NEW, 672 lines, 19/19 tests passing):**
- `bezier.rs` — `BezierCurve` with 255-point baking + binary-search eval. Named curves: linear, default, overshot, smoothOut, smoothIn (matching user's existing Hyprland config).
- `animatable.rs` — `Animatable` trait with impls for f32, Point<f64, Logical>, [f32; 4].
- `value.rs` — `AnimatedValue<T>` with begun/value/goal triple, animate_to, warp_to, tick(now), is_animating.
- `manager.rs` — `AnimationManager { curves, windows_in, windows_out, fade_in, fade_out, workspaces, global_enabled }` with sane defaults matching `~/.config/hypr/defaults.conf` from the user's existing zOS image.

**State integration:**
- `WindowElement::anim_state()` accessor → `WindowAnimationState { render_offset: Mutex<AnimatedValue<Point>>, alpha: Mutex<AnimatedValue<f32>> }` lazily inserted on user_data.
- `Workspace.render_offset: AnimatedValue<Point>` + `alpha: AnimatedValue<f32>`. Plus `tick_animations(now)` and `any_animating()` walking workspace + per-window state.
- `AnvilState::tick_animations(now)` walks all outputs/workspaces. Called at start of `udev::render_surface` and `winit::run_winit` per-frame.
- `AnvilState.animation_manager: AnimationManager` with default config.

**Animation drivers:**
- `xdg.rs::new_toplevel`: warps render_offset to (0, output_height) then animates to (0,0) using windows_in curve+duration. Warps alpha to 0 then animates to 1 with fade_in. Both gated on per-property + global enabled flags.
- `input_handler.rs::action_switch_to_workspace`: animates outgoing workspace render_offset off-screen and incoming from off-screen → 0. Direction-aware (forward/backward). Lazy-creates target.

### Deferred for Phase 4 visible effects
- **RelocateRenderElement integration (P4-W3)** — smithay's `space_render_elements` collapses windows internally; injecting per-window relocate requires bypassing it and walking workspace windows manually. Render-side work pending. Without this, animations tick but don't visually translate windows.
- **Rounded corners shader** (4.B step 1) — straightforward single-shader pass via `GlesRenderer::compile_custom_pixel_shader`. Independent of relocate; quick visual win.
- **Drop shadow + blur + opacity wrap** (4.B steps 2-4)
- **Window close fade** (task 14) — needs "keep window alive in fading-out state" infra.
- **TOML config for animation params** (task 5)

### Verified
- `cargo check -p zos-wm` clean
- `cargo test -p zos-wm --lib` 28/28 passing (5 bezier + 4 animatable + 6 value + 4 manager + 7 dwindle + 2 grabs)

---

## 2026-04-25 — Phase 5 zos-ui foundation

Two new workspace crates: `zos-ui` (runtime) + `zos-ui-macros` (proc-macros).

### `zos-ui` runtime crate
- **theme.rs** (157 lines) — Catppuccin Mocha palette (full set incl. SAPPHIRE/SKY/TEAL/MAROON/etc.) + typography scale (font_size::XS..X3L) + spacing tokens (space::X1..X8) + radius tokens (radius::SM..FULL) + duration tokens (FAST/NORMAL/SLOW). `Tokens` zero-sized accessor type. `zos_theme()` returns iced Theme with the canonical palette.
- **signal/** module (~480 LoC impl, 8 unit tests + 2 doctests passing) — leptos-style fine-grained reactivity. Pure-std, no external deps:
  - `Signal<T>` — get/set/update/peek with auto-subscription tracking
  - `Memo<T>` — derived signal, PartialEq dedupe to skip downstream re-runs
  - `Effect` — auto-subscribes during run, drops cleanly
  - Thread-local Runtime with effect slab + free-list + pending queue + re-entrancy guard
- **`Component` + `View` traits** — `View` blanket-impl'd for `T: Into<iced::Element<'static, ()>>`, so users return widgets directly without `.into()`. Component::view returns `impl View`.
- **prelude.rs** — re-exports iced widgets + theme + signals + macros under one import.

### `zos-ui-macros` proc-macro crate (3 files, 208 lines)
- `#[component]` — turns `pub fn Clock(class: String, hour_format: bool) -> impl View { body }` into:
  - `pub struct Clock { class, hour_format }` with field visibility preserved
  - `Clock::new(class, hour_format)` constructor + `Clock::class(self, ...)` builder methods per field
  - `impl Component for Clock { fn view(self) -> impl View { let Self { class, hour_format } = self; body } }`
- `#[panel_module]` and `#[taskbar_icon]` — alias to `#[component]` + TODO marker (real PanelModule / TaskbarIconWidget traits land later)
- Compile-time errors via `compile_error!` for: generic params, where clauses, async, self receiver, mut/ref arg patterns, destructure args
- Pure proc-macro2 + quote + syn 2.x, no other deps

### Verified
- `cargo check --workspace` clean
- `cargo test -p zos-ui --lib` — 10 passing (8 signal + 2 component smoke)

### Deferred for Phase 5 completion
- Module trait + ModuleRegistry (panel-module discovery)
- TOML config loader for theme overrides
- Hot reload (stretch)
- Style scoping macro
- Refactor zos-settings + zos-dock onto zos-ui (sequential, post-framework)

---

## 2026-04-25 — Phase 5 polish (layer + widgets + clock demo) [commit 968ced9]

- `src/layer.rs` (89 lines) — `top_bar`, `bottom_dock`, `centered_popup` helpers returning `iced_layershell::LayerShellSettings`. Feature-gated (default-on `layer-shell`).
- `widgets/{card, pill, status_dot, section_header}.rs` — themed primitives composing iced widgets.
- `examples/clock.rs` — minimal demo. `cargo build --example clock` clean.
- Adapted to iced 0.14 API: Pixels::From<f32/u32> only, Padding::from typed array, Space::new() no args, container Style 5-field, iced::application functional.

---

## 2026-04-25 — Phase 6 shell-app scaffolds + Compositor trait [commit 06dc2e6]

Four new workspace members. Each is a minimal main.rs that prints scaffold message and exits — full implementations are downstream work.

- `zos-panel` — replaces HyprPanel
- `zos-power` — replaces wlogout
- `zos-monitors` — replaces nwg-displays
- `zos-notify` — replaces swaync (deps include zbus 5.x for DBus)

`zos-core::compositor` module
- `Compositor` trait with workspaces / windows / monitors / active_window / focus_window / switch_to_workspace methods.
- `WorkspaceInfo` / `WindowInfo` / `MonitorInfo` stable types for shell apps.
- `Hyprland` impl: shells out to `hyprctl -j`, methods stub returning empty Vec / None pending serde_json parsing.
- `ZosWm` impl: stub returning empty / NotSupported until zos-wm IPC integration lands.
- `detect()` picks impl from `XDG_CURRENT_DESKTOP` env.

### Verified
- `cargo build --workspace` clean
- All 4 scaffolded binaries link

### Deferred for Phase 6 completion
- Module impls within zos-panel (Clock, Workspaces, Window title, Tray, Audio, Network, Bluetooth, Power)
- zos-power button grid + reboot-to-Windows dropdown
- zos-monitors visual layout + per-monitor controls
- zos-notify DBus server + toast UI
- TOML config file format
- serde_json parsing in Hyprland::workspaces/windows/monitors

---

## 2026-04-25 — Phase 7 plugin architecture scaffold

Two channels for extending zos-wm:

### IPC socket (`zos-wm/src/ipc/`)
- `protocol.rs` — newline-delimited JSON. Request/Response enums. 12 Request variants (Workspaces, Windows, Monitors, ActiveWindow, SwitchToWorkspace, MoveWindowToWorkspace, FocusWindow, CloseFocused, Version, Quit). Response variants mirror.
- `server.rs` — `IpcServer::start(socket_path, handler)` spawns Unix-socket accept loop on a thread; per-connection thread reads/writes newline-delimited JSON. `default_socket_path()` = `$XDG_RUNTIME_DIR/zos-wm-$WAYLAND_DISPLAY.sock`. Drop removes socket file.
- `placeholder_handler()` — returns Version on Version request, Error on others. Real handlers wire to AnvilState in main.rs (downstream).

### In-process Extension trait (`zos-wm/src/extension.rs`)
- `Extension` trait — `name()` + `init()` + `pre_frame(now)` + `post_frame(now)` + `shutdown()`.
- `ExtensionRegistry` — Vec<Box<dyn Extension>> with init/pre/post/shutdown _all helpers.
- `LogFrameCount` example impl as template (logs every 120 frames).

Cargo.toml: added `serde = { version = "1", features = ["derive"] }` + `serde_json = "1"`.

### Verified
- `cargo test -p zos-wm --lib ipc::` 4/4 passing
- `cargo test -p zos-wm --lib extension::` 1/1 passing

### Deferred
- Wiring IpcServer into main.rs/state.rs (real handler closure consulting AnvilState)
- WASM runtime (v2 — wasmtime spike + plugin API design)
- Example out-of-process plugin (e.g., a custom keybind script via IPC)
- Example in-process Extension (e.g., wobbly-windows or a tiling layout)

---

## 2026-04-25 — Phase 8 image build prep

Containerfile now copies all 6 new crates (zos-ui, zos-ui-macros, zos-panel, zos-power, zos-monitors, zos-notify) into the rust-ctx scratch stage and builds the 4 shell apps in the Rust build layer. All install to /usr/bin/.

Phase 8 swap-over is **NOT yet triggered** — Hyprland keep-alive banner stays in `install-hyprland.sh`. zos-wm needs visible animations (RelocateRenderElement render-path rewrite) + shell-app modules (Phase 6 follow-ups) + IPC integration (Phase 7 follow-up) before zos-wm becomes daily-driver.

The wayland-sessions entry + start-zos-wm launcher landed in Phase 2 already, so picking "zOS (zos-wm)" from regreet is wired the moment the binary ships in the image (which it now does).

---

# Status snapshot — end of 2026-04-25 session

| Phase | Status |
|---|---|
| 0 | Shipped |
| 1 | Done |
| 2 | Done (4 from-scratch protocols + udev backend audit + build/install + greetd session) |
| 3 | MVP done (workspaces + dispatcher + dwindle algorithm + modal/AlwaysOnTop) |
| 4 | Infrastructure done (animations tick + drivers fire). Visible effects deferred (RelocateRenderElement render-path rewrite needed) |
| 5 | Foundation done (signals + #[component] macros + theme + layer + widgets + clock demo). Remaining: refactor zos-settings/zos-dock onto zos-ui |
| 6 | Scaffolds done (4 new app crates + Compositor trait). Remaining: actual app implementations |
| 7 | Scaffolds done (IPC socket + Extension trait). Remaining: wire to AnvilState + write example plugins |
| 8 | Image-build prep done. NOT triggering swap until 4/5/6 are visible-feature-complete |

Workspace: 12 crates. Total commits this session: 7.

---

## 2026-04-25 (continued) — second-half session

8 more commits shipping visible features. Tests across workspace: zos-ui 15/15, zos-wm 33/33.

### Phase 4 visible animations + shader (commits 03fffed, d23a364, then P4-V3)
- **Render-path rewrite (P4-V1)**: `output_elements` walks `Workspace::iter_z_order().rev()` manually, wraps each `WindowRenderElement` in `RelocateRenderElement::from_element` with combined offset (workspace + per-window) and combined alpha. Layer-shell rendering kept separate via `layer_map_for_output`. New `OutputRenderElements::AnimatedWindow` and `LayerSurface` variants. `udev.rs` reschedules every frame while `state.any_animating()`.
- **Rounded-corners shader (P4-V2 + P4-V3)**: GLSL fragment shader using smithay's v_coords + size uniforms with smoothstep AA. `RoundedCornersEffect::new(&mut GlesRenderer)` compiles via `compile_custom_pixel_shader`. Wired at backend init in udev.rs (per primary GPU) and winit.rs. Stored on BackendData. `AnvilState.corner_radius: f32 = 8.0`. Per-window `PixelShaderElement` push DEFERRED (PixelShaderElement only impls `RenderElement<GlesRenderer>`, not MultiRenderer; documented in render.rs TODO with two resolution paths).

### Phase 6 shell apps — full implementations
- **zos-power (commit 03fffed)**: 4-button centered popup. `iced_layershell::application(...)` builder + `#[to_layer_message]`. Lock/Logout/Reboot/Shutdown 2x2 grid. Reboot chevron expands "Reboot to Windows" submenu via `zos_core::commands::grub::reboot_to_windows_elevated`. Esc dismiss.
- **zos-panel (commits 874c68d + d23a364)**: 32px top bar. Modules: workspaces (click-to-switch via Compositor trait), active-window-title (truncated at char boundary), HH:MM clock, audio (pactl), network (nmcli), battery (sysfs). Tick at 1s.
- **zos-monitors (commit 874c68d)**: Regular iced window. Per-monitor card showing resolution/refresh/scale/focused. "Apply" writes `~/.config/hypr/monitors.conf`. "Refresh" re-runs Compositor::detect.
- **zos-notify toast (commit d23f8dd)**: Dual-runtime — dedicated OS thread for DBus daemon, main thread for iced layer-shell. mpsc channel. Top-right anchored toasts, click-to-dismiss + auto-dismiss respecting expire_timeout. 360×84px Catppuccin cards, max 4 visible.

### IPC ↔ AnvilState (commit 874c68d)
- `state.rs::start_ipc_server`: builds calloop channel + spawns IpcServer with closure forwarding via channel + awaiting oneshot.
- `state.rs::handle_ipc_request`: real handler running on compositor thread. Workspaces/Windows/Monitors/ActiveWindow/SwitchToWorkspace/CloseFocused/Version implemented. Class/title via with_states + XdgToplevelSurfaceData.
- udev.rs::run_udev + winit.rs::run_winit: call start_ipc_server() right after start_xwayland(). _ipc_server held for life of main loop.

### zos-core Hyprland JSON parsing (commit f399e33)
- 6 unit tests, +282 lines in hyprland.rs.
- workspaces() / windows() / monitors() / active_window() now parse hyprctl -j output via serde_json. DTO structs with #[serde(default)] for forward-compat.
- Cross-references for active workspace per monitor + focused window via /activewindow address comparison.
- Resolves monitor name from numeric or string monitor field.
- zos-panel and zos-monitors now populate with real data.

### Framework refactors (commits d23f8dd, P4-V3)
- **zos-dock onto zos-ui** (-17 lines): replaced 6 raw Color literals with theme constants. 4 direct refs + 2 FRU-syntax alpha variants. No behavior/visual change.
- **zos-settings onto zos-ui** (-32 lines): `theme.rs` collapsed to 7-line re-export shim of `zos_ui::theme`. 9 internal page files compile unchanged via `crate::theme::*` paths. Validates the framework integrates with the largest existing zOS app.

### Phase 5 timer hooks (P5-W5)
- New `signal/timer.rs` (441 lines, 5 tests).
- `use_interval(every, fn) -> Interval` (Drop cancels).
- `use_timeout(after, fn) -> Timeout` (Drop cancels if not yet fired).
- `tick_timers()` advances all from frame loop.
- Pure std, single-threaded matching existing Signal/Effect model.
- Test-only synthetic clock via `tick_at(Instant)`.

### Status snapshot — end of 2026-04-25 (combined)

| Phase | Status |
|---|---|
| 0 | Shipped pre-session |
| 1 | Done |
| 2 | Done |
| 3 | MVP done + polish (modal, always-on-top, workspace tiling toggle) |
| 4 | Animations infrastructure + drivers + visible (RelocateRenderElement integrated). Rounded-corners shader compiled at backend init; per-window apply deferred (MultiRenderer trait gap). Drop shadow + blur + opacity polish deferred. |
| 5 | Foundation + layer + widgets + clock demo + use_interval/use_timeout hooks. zos-settings + zos-dock refactored onto zos-ui (validation). Hot reload deferred. |
| 6 | All 4 apps implemented (zos-panel + zos-power + zos-monitors + zos-notify). Tray/audio popups/wifi-picker UI deferred for follow-up. |
| 7 | IPC server + AnvilState wiring done. Extension trait scaffold ready for example plugins (deferred). |
| 8 | Image-build prep done. **NOT** triggering swap. |

Workspace: 12 crates. Session total commits: 15. Tests: zos-ui 15/15, zos-wm 33/33.

### Deferred for future session
- Phase 4: per-window PixelShaderElement integration (needs PixelShaderElement RenderElement<MultiRenderer> impl or per-backend specialization), drop shadow, kawase blur, opacity wrapping
- Phase 5: hot reload, style scoping macro (currently styles are inline iced closures), TOML config loader for theme overrides
- Phase 6: zos-panel system tray (StatusNotifierItem), audio sink picker popup, wifi network picker popup, monitors visual drag layout
- Phase 7: example out-of-process plugin (CLI tool that talks to IPC), example in-process Extension (e.g., wobbly windows)
- Phase 8: Hyprland keep-alive removal — only when zos-wm has been daily-driver-validated

---

## 2026-04-25 (continued) — third-half session

12 more commits. Tests across workspace: zos-ui 25/25, zos-wm 40/40, zos-core 9/9, zos-launcher 4/4. Workspace is now warning-free.

### Phase 4 polish
- **Drop shadow shader** (effects/shadow.rs, 191 lines): closed-form gaussian SDF + smoothstep, single-pass fragment shader. DropShadowEffect::new compiles via compile_custom_pixel_shader. State.rs: shadow_radius=16, shadow_offset=(0,4), shadow_color=[0,0,0,0.5] defaults. Compiled at backend init in udev.rs and winit.rs.
- **Winit-only shadow render** (P4-V5): MultiRenderer trait gap confirmed — no blanket RenderElement impl. Took fallback path 1: ShadowParams struct, new WinitOutputElements wrapper holds PixelShaderElements only on winit's R = GlesRenderer path. udev passes None. Shadow ships visibly on winit.
- **Texture-shader rounded corners** (P4-V6): rewrote effects/rounded.rs to use compile_custom_texture_shader instead of pixel shader. GlesTexProgram. Sampler shader multiplies tex sample by smoothstep mask alpha. Handles EXTERNAL/NO_ALPHA/DEBUG_FLAGS variants. Render-side TextureShaderElement wrapping still TODO (mirrors shadow trait gap).

### Phase 3 polish
- **Auto-tile new windows** (P3-auto-tile): xdg.rs::new_toplevel checks if focused workspace is Tiled; if so, allocates WindowKey, calls algorithm.insert, reads back rect, overrides location via space.map_element + sends configure. Floating workspaces unchanged.
- **Workspace mode-switch relayout** (P3-tile-relayout): switch_to_tiled walks windows, allocates WindowKey per window, inserts, captures rects, updates entry.location + stored_size, sends configure with new size. switch_to_floating restores stored_size. tiling_keys: HashMap<WindowId, WindowKey> on Workspace.

### Phase 5 config
- **TOML config loader** (zos-ui::config): animations.toml + theme.toml parsers. AnimationOverrides + BezierCurveOverride + PropertyOverride structs. ThemeOverrides with palette/font_size/space/radius hashmaps + parse_hex_color (#RRGGBB + #RRGGBBAA). Graceful default on missing/malformed file. 10 unit tests passing.
- **AnimationManager::with_overrides** (P5-config-consume): merges TOML overrides over defaults at startup. Custom curves registered before resolving property curve-by-name. Unknown curves/styles emit warn + keep default. 7 new unit tests.

### Phase 6 — shell-app polish
- **zos-panel module clicks**: audio module → spawn pavucontrol on click. network → nm-connection-editor. Battery stays read-only.
- **zos-monitors mode picker**: per-monitor pick_list dropdown listing available modes. MonitorMode { width, height, refresh_hz } + Display impl in zos-core. hyprland.rs parses availableModes JSON array (e.g., "1920x1080@60.00Hz"). Apply writes monitors.conf with chosen WxH@RR.
- **zos-launcher** (NEW crate, 500 lines): centered-popup app launcher replacing wofi. Reads .desktop files from /usr/share/applications + ~/.local/share/applications + Flatpak exports. Substring scoring (exact name > prefix > contains-position > comment match). Up/Down/Enter selection, Esc dismiss. Terminal=true wraps in `wezterm start --`. Discovers 81 entries on test box.
- **zos-settings warning cleanup**: 22 → 0 warnings via #[allow(dead_code)] annotations + 1 visibility fix on PowerAction.
- **zos-dock warning cleanup**: 2 → 0 warnings (annotated DockPosition::is_top + is_active_window_fullscreen as retained-for-future).

### IPC + CLI
- **IPC class/title** + **MoveWindowToWorkspace** + **FocusWindow** handlers wired from stub-error to real impls in zos-wm/src/state.rs::handle_ipc_request.
- **zos compositor move-to-workspace / focus-window** subcommands in zos-cli.
- **zos compositor --watch [MS]** flag on read commands. Default 1000ms when no value given. Watch loop clears screen via ANSI codes + sleeps interval. Errors print and continue (so socket reconnects work).
- **zos doctor** (NEW, 274 lines): 8 health checks — Wayland session, NVIDIA driver (modeset/fbdev/version), logind (Active=yes via loginctl), hyprctl + version, zos-wm IPC + Version round-trip, ~/.config/zos writable, animations.toml presence, DRM outputs. Colorized ✓/⚠/✗ output via inline ANSI. Exit code 1 on any Fail.

### Phase 7
- **Example Extension wired**: LogFrameCount registered in AnvilState::init, ticks pre_frame_all + post_frame_all from udev render_surface and winit run_winit. Validates Phase 7 in-process Extension trait end-to-end.

### Phase 8 — image build
- **zos-launcher in Containerfile**: rust-ctx COPY + cargo build line for shell apps + install to /usr/bin/.
- **Super+Space binding** in zos-wm/src/binds.rs spawns zos-launcher.

### Status snapshot — end of 2026-04-25 (combined, all-day)

| Phase | Status |
|---|---|
| 0-2 | Done |
| 3 | Done MVP + polish + auto-tile + relayout. ToggleWorkspaceTiling functional. |
| 4 | Animations visible. Shadow visible on winit (udev blocked on MultiRenderer trait gap). Rounded corners shader compiles, render-side wrap deferred (same gap). |
| 5 | Foundation + macros + signals + use_interval/timeout + theme + layer + widgets + config loader + clock demo. zos-settings + zos-dock refactored onto zos-ui. |
| 6 | All 5 shell apps implemented (zos-panel + zos-power + zos-monitors + zos-notify + zos-launcher). Tray/visual-drag deferred. |
| 7 | IPC + Extension trait wired and validated end-to-end. |
| 8 | Image-build prep complete; swap not triggered. |

Workspace: 13 crates, all warning-free. Session total commits: 22 (this session pass). Tests: zos-ui 25/25, zos-wm 40/40, zos-core 9/9, zos-launcher 4/4.

### Still deferred
- Phase 4: render-time integration of texture-shader rounded corners + udev-side shadow (blocked on smithay MultiRenderer's lack of blanket RenderElement impl)
- Phase 4: kawase blur, opacity wrapping (lower priority)
- Phase 5: hot reload, style scoping macro
- Phase 6: zos-panel system tray (StatusNotifierItem), wifi picker popup, monitors visual drag layout
- Phase 6: zos-notify history panel
- Phase 6: OSD overlay (volume/brightness on-screen indicator)
- Phase 7: WASM runtime (v2), example out-of-process plugin
- Phase 8: actual swap-over (Hyprland-removal, daily-drive zos-wm)

---

## 2026-04-25 (continued, fourth-half) — final batch

### binds.toml user keybinds (commit e0e1422)
- `~/.config/zos/binds.toml` parsed via toml-0.8. UserBindEntry struct with `mods=[..]` `key="..."` (or `button=N`) `action="..."` `args=[..]`.
- Modifier names: SHIFT/CTRL/ALT/SUPER/ALTGR. Keysym names via `xkb::keysym_from_name`. Action variants enumerated string-to-enum with arg parsing per type (string/integer).
- Loaded at startup in state.rs::init, merged over default_bindings via HashMap::insert (user combos override defaults).
- 4 unit tests covering empty TOML, parse with args, error paths, full round-trip.

### zos screenshot CLI (commit e0e1422)
- New zos-cli/src/screenshot.rs (117 lines). Wraps grim + slurp + wl-copy.
- Saves to ~/Pictures/Screenshots/zos-YYYY-MM-DD_HH-MM-SS.png (via system `date`, no chrono dep).
- --region (slurp selection), --copy (wl-copy clipboard), --output OUT (specific monitor), --quiet (suppress output).
- Tool probing with helpful "install: dnf install ..." errors.

### Media keys + scroll-wheel volume (commit ab3f920)
- zos-wm: XF86Audio{Raise,Lower}Volume/Mute/MicMute/Play/Next/Prev → pactl/playerctl. XF86MonBrightness{Up,Down} → brightnessctl. Print → `zos screenshot --copy --quiet`. Super+P → region screenshot.
- zos-panel audio module wrapped in MouseArea: scroll up = +5%, scroll down = -5%, middle-click = toggle mute. Pixel-delta scaled by /20 for natural feel.

### Status snapshot — end of 2026-04-25 (final)

| Phase | Status |
|---|---|
| 0-3 | Done |
| 4 | Animations visible. Drop shadow + texture-shader rounded corners compiled and ready; render-side texture-shader wrap deferred (smithay MultiRenderer trait gap). |
| 5 | zos-ui foundation (signals + macros + theme + layer + widgets + config + use_interval) + zos-settings/zos-dock refactored onto it. |
| 6 | All 5 shell apps shipping (zos-panel + zos-power + zos-monitors + zos-notify + zos-launcher). Audio scroll-wheel + media keys wired. |
| 7 | IPC + Extension trait wired and validated (LogFrameCount + zos doctor + zos compositor CLI). |
| 8 | Image-build prep complete; swap awaits daily-driver validation. |

**Workspace: 13 crates, all warning-free. Tests: zos-ui 25/25, zos-wm 44/44, zos-core 9/9, zos-launcher 4/4 = 82 passing.**

**Session total commits: 25** (this session pass, autonomous mode after the initial "go").

### Daily-driver readiness checklist for Phase 8 swap

- [x] zos-wm scaffold + Wayland protocols + udev backend
- [x] Greetd session entry + start-zos-wm launcher in image
- [x] zos-panel (clock + workspaces + audio + network + battery + active title)
- [x] zos-power (4 actions + reboot-to-Windows)
- [x] zos-monitors (mode picker + monitors.conf write)
- [x] zos-notify (DBus daemon + toast UI)
- [x] zos-launcher (Super+Space)
- [x] Media keys + screenshot bindings
- [x] zos compositor CLI (workspaces / windows / monitors / switch / focus / move / version / --watch)
- [x] zos doctor diagnostic
- [ ] Daily-drive zos-wm for 48+ hours under real load (user task)
- [ ] Confirm visual fidelity at 144Hz on 3× 1080p NVIDIA setup
- [ ] Verify the deferred render-side shader integration isn't blocking (or implement smithay PR / per-backend specialization)
- [ ] Drop Hyprland keep-alive banner in install-hyprland.sh, remove HyprPanel/wlogout/nwg-displays from dnf install
- [ ] CI image rebuild + rebase via rpm-ostree

---
