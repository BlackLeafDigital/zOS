# Phase 2.A — Wayland protocol priority for zos-wm

Target compositor: `zos-wm`, a fork of Smithay's `anvil` reference compositor.
Pinned Smithay SHA: `27af99ef492ab4d7dc5cd2e625374d2beb2772f7` (April 2026).
Local checkouts used:
- zos-wm: `/var/home/zach/github/zOS/zos-wm/`
- Smithay (pinned): `/tmp/smithay-peek/`
- Niri (latest main, for comparison): `/tmp/niri-peek/`

## TL;DR

- **Good news: anvil ships with a surprisingly large protocol surface already wired up.** All the "core" Wayland protocols a daily-driver needs — `xdg_shell`, `wlr-layer-shell`, `xdg-decoration`, `viewporter`, `fractional-scale-v1`, `presentation-time`, `pointer-constraints`, `relative-pointer`, `linux-dmabuf` (+ `drm-syncobj` on udev), `xdg-activation`, `security-context`, `commit-timing`, `fifo`, `xdg-foreign`, `single-pixel-buffer`, `xwayland-shell` — are already present in the anvil copy in `zos-wm/src/state.rs`.
- **The gaps that block daily-driver quality-of-life are narrow**: `cursor-shape-v1`, `idle-inhibit-v1`, `idle-notify-v1`, `session-lock-v1`, `pointer-warp` (stable), `tearing-control-v1`, and a *real* screencopy (anvil's `image-copy-capture` handler is stubbed with `frame.fail()`).
- **Tearing-control does NOT exist in Smithay at the pinned SHA.** Neither smithay nor niri implement the `wp-tearing-control-v1` global in this tree (`rg tearing /tmp/smithay-peek/src` only finds a comment in `backend/egl/context.rs:518`). If we want `allow_tearing` for games, we roll our own protocol dispatch (Niri does not, as of its `main`). Punt to P1 with an explicit warning.
- **Order of battle (P0, ~2-5 days)**: (1) wire `cursor-shape-v1`; (2) wire `idle-inhibit-v1` + `idle-notify-v1`; (3) wire `session-lock-v1`; (4) wire `pointer-warp`; (5) replace the stubbed screencopy frame impl with a real one (lift from niri). XWayland is *compiled in* already; we just need to keep it working when we diverge.
- **P1 (follow-up, ~1 week)**: `wlr-output-management` (lifted from niri 923 LoC), `wlr-screencopy-v1` (niri 742 LoC), `gamma-control` (niri 245 LoC), `foreign-toplevel-list` + `ext-foreign-toplevel` (niri 669 LoC, needed for any panel/dock). Then decide on rolling our own `tearing-control`.

## Anvil's current protocol coverage

Source: `rg 'delegate_|Handler for' /var/home/zach/github/zOS/zos-wm/src` — all line numbers are into the zos-wm copy of anvil at the pinned SHA.

| Protocol | Module (smithay path) | Wire-up site (zos-wm) | LoC of handler | Status |
|---|---|---|---|---|
| wl_compositor / wl_subcompositor | `smithay::wayland::compositor` | `state.rs:200` (delegate); handler at `shell/mod.rs:102` | ~140 | Fully functional |
| wl_shm | `smithay::wayland::shm` | `state.rs:311`; handler `state.rs:306` | ~5 | Functional |
| wl_seat / wl_pointer / wl_keyboard / wl_touch | `smithay::wayland::seat` + `smithay::input` | `state.rs:339`; handler `state.rs:313-339` | ~26 | Functional |
| wl_output + xdg-output | `smithay::output` + `OutputManagerState::new_with_xdg_output` | `state.rs:260`; call site `state.rs:715` | ~3 | Functional |
| linux-dmabuf-v1 | `smithay::wayland::dmabuf` | `udev.rs:186`, `winit.rs:83`, `x11.rs:86` | ~20 per backend | Functional (per backend) |
| wp_presentation_time | `smithay::wayland::presentation` | `state.rs:511` | N/A (delegate-only) | Functional |
| wp_viewporter | `smithay::wayland::viewporter` | `state.rs:433` | N/A | Functional |
| wp_fractional_scale_v1 | `smithay::wayland::fractional_scale` | `state.rs:560`; handler `state.rs:513-559` | ~47 | Functional |
| wp_single_pixel_buffer_v1 | `smithay::wayland::single_pixel_buffer` | `state.rs:605` | N/A | Functional |
| wp_commit_timing_v1 | `smithay::wayland::commit_timing` | `state.rs:609` | N/A | Functional |
| wp_fifo_v1 | `smithay::wayland::fifo` | `state.rs:607` | N/A | Functional |
| wp_pointer_gestures | `smithay::wayland::pointer_gestures` | `state.rs:391` | N/A | Functional (BackendData-gated at `state.rs:739`) |
| wp_relative_pointer_manager_v1 | `smithay::wayland::relative_pointer` | `state.rs:393` | N/A | Functional (BackendData-gated at `state.rs:735`) |
| wp_pointer_constraints_v1 | `smithay::wayland::pointer_constraints` | `state.rs:431`; handler `state.rs:395-430` | ~36 | Functional (focus-change handling present) |
| xdg-shell (toplevel + popup) | `smithay::wayland::shell::xdg` | `state.rs:509`; handler `shell/xdg.rs:41` (~600 LoC of logic) | ~600 | Functional — this is the bulk of anvil's window-management surface |
| xdg-decoration | `smithay::wayland::shell::xdg::decoration` | `state.rs:507`; handler `state.rs:474-506` | ~33 | Functional, defaults to SSD |
| xdg-activation-v1 | `smithay::wayland::xdg_activation` | `state.rs:472`; handler `state.rs:435-471` | ~37 | Functional |
| xdg-foreign (v1/v2) | `smithay::wayland::xdg_foreign` | `state.rs:603`; handler `state.rs:598-602` | ~5 | Functional |
| wlr-layer-shell | `smithay::wayland::shell::wlr_layer` | `state.rs:510`; handler `shell/mod.rs:245` | ~200+ | Functional |
| wp_security_context_v1 | `smithay::wayland::security_context` | `state.rs:580`; handler `state.rs:562-579` | ~18 | Functional (restricted-client flag stored on `ClientState` at `state.rs:126`) |
| wp_primary_selection_unstable_v1 | `smithay::wayland::selection::primary_selection` | `state.rs:296`; handler `state.rs:291-295` | ~5 | Functional |
| zwlr_data_control_manager_v1 | `smithay::wayland::selection::wlr_data_control` | `state.rs:304`; handler `state.rs:298-303` | ~5 | Functional |
| wl_data_device (DnD) | `smithay::wayland::selection::data_device` | `state.rs:257`; handler `state.rs:202-256` | ~55 | Partially functional — per-CHANGELOG "Unreleased" note, the `start_dnd` API was removed and replaced by the new `DnDGrab` entry-point; anvil has been updated already (see `state.rs:36` `dnd::{DnDGrab, DndGrabHandler, ...}`). |
| input-method-v1 | `smithay::wayland::input_method` | `state.rs:374`; handler `state.rs:351-373` | ~23 | Functional |
| text-input-v3 | `smithay::wayland::text_input` | `state.rs:349` (delegate), call `state.rs:731` | N/A | Functional |
| virtual-keyboard-v1 | `smithay::wayland::virtual_keyboard` | `state.rs:389` | N/A | Functional |
| wp_keyboard_shortcuts_inhibit_v1 | `smithay::wayland::keyboard_shortcuts_inhibit` | `state.rs:387`; handler `state.rs:376-386` | ~11 | Functional (just accepts all requests) |
| tablet-v2 | `smithay::wayland::tablet_manager` | `state.rs:347`; handler `state.rs:341-346` | ~6 | Functional |
| xwayland-shell-v1 | `smithay::wayland::xwayland_shell` | `state.rs:596`; handler `shell/x11.rs:48` | ~20 | Functional (xwayland feature only) |
| zwp_xwayland_keyboard_grab_v1 | `smithay::wayland::xwayland_keyboard_grab` | `state.rs:593` | N/A | Functional (xwayland feature only) |
| drm-syncobj (linux-drm-syncobj-v1) | `smithay::wayland::drm_syncobj` | `udev.rs:623`; handler `udev.rs:618-622` | ~5 | Functional — **only on udev backend**, and per CHANGELOG 0.7.0 the handler now returns `Option<&mut DrmSyncobjState>` (see Gotchas) |
| drm-lease (wp-drm-lease-v1) | `smithay::wayland::drm_lease` | `udev.rs:616`; handler `udev.rs:546-615` | ~70 | Functional (udev only). Not needed for single-user desktop — skip/leave as-is. |
| ext-image-capture-source + ext-image-copy-capture | `smithay::wayland::image_capture_source` / `image_copy_capture` | `state.rs:618, 629, 665`; handler `state.rs:613-665` | ~53 | **WIRED UP BUT STUBBED** — `frame(&mut self, ...)` at `state.rs:660` calls `frame.fail(CaptureFailureReason::Unknown)`. Globals are advertised but no actual pixels are copied. |
| fixes (wl-fixes) | `smithay::wayland::fixes` | `state.rs:611`, call `state.rs:748` | N/A | Functional |

**Not implemented by anvil** (verified by `rg` across `zos-wm/src`):
`cursor-shape-v1`, `idle-inhibit-v1`, `idle-notify-v1`, `session-lock-v1`, `pointer-warp`, `content-type-v1`, `alpha-modifier-v1`, `background-effect` (ExtBackgroundEffect), `foreign-toplevel-list-v1`, `xdg-system-bell`, `xdg-toplevel-icon-v1`, `xdg-toplevel-tag-v1` — all available as `smithay::wayland::<name>` modules per `/tmp/smithay-peek/src/wayland/mod.rs:48-99`, but anvil has no `delegate_*!` for them.

**Also not implemented** — protocols Smithay has no module for at this pinned SHA (confirmed by `ls /tmp/smithay-peek/src/wayland/`): `wp-tearing-control-v1`, `zwlr-output-management-v1`, `zwlr-screencopy-v1`, `zwlr-gamma-control-v1`, `ext-workspace-v1`, `ext-foreign-toplevel-list-v1`. These are either niri-grown (see next section) or we own the dispatch.

## Niri's additions beyond anvil

Source: `rg 'delegate_|Handler for' /tmp/niri-peek/src` (lines below are into that tree).

| Protocol | Where niri keeps it | LoC | Notes |
|---|---|---|---|
| cursor-shape-v1 | `src/handlers/mod.rs:142` delegate; smithay module exists and handler is trivial | ~2 | Anvil does not delegate this. Free win — just add `delegate_cursor_shape!(AnvilState)`. |
| idle-inhibit-v1 | `src/handlers/mod.rs:524-533`; state at `src/niri.rs:302, 2302` | ~10 + state plumbing | Standard smithay module (`smithay::wayland::idle_inhibit`). Niri tracks inhibiting surfaces in a `HashSet<WlSurface>` at `niri.rs:333`. |
| idle-notify-v1 (ext-idle-notify-v1) | `src/handlers/mod.rs:517-522`; state `niri.rs:301, 2301` | ~6 + plumbing | `IdleNotifierState::new(&display_handle, event_loop.clone())` — needs a `LoopHandle` to schedule timers. |
| session-lock-v1 (ext-session-lock-v1) | `src/handlers/mod.rs:459-484`; state `niri.rs:279, 2285` | ~26 + plumbing | Uses `SessionLockManagerState::new::<State, _>(&display_handle, client_is_unrestricted)`. The `client_is_unrestricted` callback enforces the security-context gate. |
| ext-data-control-v1 | `src/handlers/mod.rs:421-427` | ~7 | Newer variant that replaces wlr-data-control. Anvil only wires wlr-data-control (`state.rs:304`). |
| pointer-warp-v1 (wp-pointer-warp-v1) | Not in niri main as of this clone — but smithay has the module at `src/wayland/pointer_warp.rs`. | N/A | Would need us to add handler + delegate ourselves. |
| zwlr-output-management-v1 | `src/protocols/output_management.rs` (not in smithay) | 923 | Niri rolls their own. Required for KDE-plasma-style display config tools and for `wlr-randr`. |
| zwlr-screencopy-v1 | `src/protocols/screencopy.rs` (not in smithay) | 742 | Niri rolls their own. xdg-desktop-portal-wlr uses this. |
| zwlr-gamma-control-v1 | `src/protocols/gamma_control.rs` (not in smithay); handler `handlers/mod.rs:740-769` | 245 | Needed for `wlsunset`/`gammastep` night-light. Roll our own like niri. |
| ext-foreign-toplevel-list-v1 + zwlr-foreign-toplevel-manager-v1 | `src/protocols/foreign_toplevel.rs` (not in smithay); handler `handlers/mod.rs:535-598` | 669 | Both protocols in one file. Required for panels/docks (hyprpanel, waybar task lists). |
| ext-workspace-v1 | `src/protocols/ext_workspace.rs` (not in smithay); handler `handlers/mod.rs:600-634` | 715 | Required for panel workspace indicators. |
| virtual-pointer-v1 (wlr) | `src/protocols/virtual_pointer.rs` (not in smithay); handler `handlers/mod.rs:666-689` | 563 | For automation tools (ydotool replacement). P2. |
| mutter-x11-interop | `src/protocols/mutter_x11_interop.rs` | 93 | Specific to mutter clients. Skip. |
| kde-server-decoration | `src/handlers/xdg_shell.rs:990-1017` — uses `smithay::wayland::shell::kde` | ~28 | Companion to xdg-decoration for KDE-specific clients. Worth adding as P2 polish (`smithay::wayland::shell::kde` is present at the pinned SHA — see `/tmp/smithay-peek/src/wayland/shell/kde/`). |
| ext-background-effect | `src/handlers/background_effect.rs:110-123` | 13 (handler) | Smithay module exists (`smithay::wayland::background_effect`, new per CHANGELOG "Unreleased"). For blur-behind-window support. Leave for Phase 4 (effects). |
| Explicitly: **no `tearing-control`** | `rg tearing /tmp/niri-peek/src` returns only `utils/vblank_throttle.rs:4` comment. | — | Niri does not implement `wp-tearing-control-v1` either. If we want allow-tearing we are writing it ourselves from `wayland-protocols-wlr`-style scratch. |

## Ranked priority for zos-wm

| Protocol | Status in anvil | Status in niri | Priority | Who implements | Effort | Notes |
|---|---|---|---|---|---|---|
| xdg-shell | Functional | Functional | P0 | already done | — | Keep current anvil impl. Diverge only when we add floating-first rules (Phase 3). |
| wlr-layer-shell | Functional | Functional | P0 | already done | — | Anvil's impl is fine for our panel use case. |
| xdg-decoration | Functional | Functional | P0 | already done | — | Default SSD is correct. |
| viewporter | Functional | Functional | P0 | already done | — | Prereq for fractional-scale. |
| fractional-scale-v1 | Functional | Functional | P0 | already done | — | Zach's monitors are 1080p, but XWayland still benefits. |
| presentation-time | Functional | Functional | P0 | already done | — | Clock passed in at init (state.rs:725). |
| pointer-constraints-v1 | Functional | Functional | P0 | already done | — | Games need this. |
| relative-pointer-v1 | Functional | Functional | P0 | already done | — | Games need this. |
| pointer-gestures-v1 | Functional | Functional | P0 | already done | — | Touchpad gestures (even on desktop — for mouse-wheel-tilt handling). |
| linux-dmabuf-v1 + drm-syncobj | Functional | Functional | P0 | already done | — | Required on NVIDIA proprietary 580. Explicit-sync must be active. |
| security-context-v1 | Functional | Functional | P0 | already done | — | Flatpak sandboxing relies on this. |
| xdg-activation-v1 | Functional | Functional | P0 | already done | — | Activation tokens = "please focus this new window." |
| xdg-foreign-v1/v2 | Functional | Functional | P0 | already done | — | File-chooser portals use this. |
| single-pixel-buffer / fifo / commit-timing | Functional | Functional | P0 | already done | — | Modern clients expect these. |
| xwayland-shell-v1 + xwayland-keyboard-grab | Functional (feature-gated) | Functional | P0 | already done — just keep `xwayland` feature enabled | — | Required for Electron apps, Steam, games, Firefox-wayland misbehaviors. |
| **cursor-shape-v1** | **Missing** | `handlers/mod.rs:142` | **P0** | us (trivial delegate add) | ~1 hour | Without this, cursors in GNOME/Qt apps flicker to wrong theme. |
| **pointer-warp (wp-pointer-warp-v1)** | **Missing** | Missing | P1 | us (smithay module exists) | ~2 hours | Programmatic cursor jump. Some games want this. |
| **content-type-v1** | **Missing** | Missing | P1 | us (smithay module exists) | ~2 hours | Lets clients hint "this is a game / video / photo", useful for VRR and tearing decisions. |
| **alpha-modifier-v1** | **Missing** | Missing | P2 | us (smithay module exists) | ~2 hours | Transparency hint from clients. Skip until we have blur. |
| **idle-inhibit-v1** | **Missing** | `handlers/mod.rs:524-533` | **P0** | us (smithay module exists) | ~4 hours | Video apps block screensaver via this; also DE settings expect it. |
| **idle-notify-v1 (ext-idle-notify-v1)** | **Missing** | `handlers/mod.rs:517-522` | **P0** | us (smithay module exists) | ~4 hours | `swayidle`, `hypridle` use this. We need it for auto-lock. |
| **session-lock-v1 (ext-session-lock-v1)** | **Missing** | `handlers/mod.rs:459-484` | **P0** | us (smithay module exists) | 1 day | `hyprlock`, `swaylock`, `gtklock`. Critical for daily driver. Restrict to security-context-unrestricted clients (niri pattern at `niri.rs:2285`). |
| **ext-foreign-toplevel-list-v1** + zwlr-foreign-toplevel-manager-v1 | Missing | `protocols/foreign_toplevel.rs` (669 LoC) | **P0** | lift from niri, attribution | 1 day (mostly lift + adapt struct names) | Any panel/dock task list needs this. hyprpanel uses it. |
| **ext-workspace-v1** | Missing | `protocols/ext_workspace.rs` (715 LoC) | P1 | lift from niri | 1 day | Panel workspace indicators. Phase 3 floating-workspace model affects the IPC shape. |
| **zwlr-output-management-v1** | Missing | `protocols/output_management.rs` (923 LoC) | P1 | lift from niri | 2 days | Used by `wlr-randr`, KDE display settings, zOS-settings monitor page. Niri's impl is self-contained. |
| **zwlr-screencopy-v1** | Missing | `protocols/screencopy.rs` (742 LoC) | P1 | lift from niri | 2 days | xdg-desktop-portal-wlr (screen-share in browsers, OBS). Replaces anvil's stubbed `image-copy-capture`. |
| ext-image-copy-capture | Stubbed (`state.rs:660` returns fail) | Not wired | **P1 (fix the stub)** | us; implement `Frame::success` with an `OutputDamageTracker` render pass | 1-2 days | **Or** prefer the wlr-screencopy path above — xdg-desktop-portal-wlr still prefers wlr-screencopy-v1 in 2026 builds. Keep the ext-image-copy-capture stub wired up as second fallback. |
| **zwlr-gamma-control-v1** | Missing | `protocols/gamma_control.rs` (245 LoC) | P1 | lift from niri | 1 day | `wlsunset`, `gammastep`. Required if user wants night-light. |
| **tearing-control-v1** | Not in smithay | Not in niri | P1 | us from scratch (`wayland-protocols` proto, our Dispatch) | 2 days | For `allow_tearing` games. Smithay has no module for this — we are the first Smithay-based compositor to add it. |
| kde-server-decoration | Missing | `handlers/xdg_shell.rs:990` | P2 | us (smithay module exists at `wayland/shell/kde/`) | 2 hours | Fixes a handful of KDE apps that don't speak xdg-decoration. |
| virtual-pointer-v1 (wlr) | Missing | `protocols/virtual_pointer.rs` (563 LoC) | P2 | lift from niri | 1 day | For automation (ydotool). Nice-to-have. |
| ext-data-control-v1 | Missing (only wlr-data-control is wired) | `handlers/mod.rs:421-427` | P2 | us (smithay module at `selection/ext_data_control/`) | 2 hours | Newer variant of wlr-data-control. Modern clipboard managers (`cliphist`, `clipse`) prefer it. |
| input-method-v2 | Functional (v1-style) | Functional | P2 | already done | — | Existing impl covers fcitx5. |
| text-input-v3 | Functional | Functional | P2 | already done | — | Existing. |
| virtual-keyboard | Functional | Functional | P2 | already done | — | Existing. |
| keyboard-shortcuts-inhibit | Functional | Functional | P2 | already done | — | Existing — accepts all inhibit requests, fine for single-user. |
| background-effect (ext) | Missing | `handlers/background_effect.rs` | P2 (Phase 4) | us (smithay "Unreleased") | 1 day | For blur-behind-window. Only meaningful once we have blur shaders. |
| xdg-toplevel-icon-v1 | Missing | Not visible | P2 | us (smithay module exists) | 4 hours | Client-specified icons, useful once we have zos-panel. |
| xdg-toplevel-tag-v1 | Missing | Not visible | P2 | us (smithay module per CHANGELOG 0.7.0) | 4 hours | Tagging for custom rules. P2 polish. |
| xdg-system-bell | Missing | Not visible | P2 | us (smithay module exists) | 2 hours | System-bell sound. Low urgency. |
| drm-lease | Functional (udev) | Functional | Skip | — | — | VR headsets only. Zach has no Vive. |
| tablet-v2 | Functional | Functional | Skip/leave | — | — | Zach has no tablet. Leave wired (harmless). |
| mutter-x11-interop | Missing | `protocols/mutter_x11_interop.rs` | Skip | — | — | Gnome-shell-specific. |

## Smithay module provenance

For each P0 and P1 protocol, this is the smithay module we depend on. All paths verified against `/tmp/smithay-peek/src/wayland/` at SHA `27af99ef`.

**P0 — already wired in anvil:**
- `smithay::wayland::compositor` — `src/wayland/compositor/mod.rs`
- `smithay::wayland::shell::xdg` — `src/wayland/shell/xdg/mod.rs`
- `smithay::wayland::shell::xdg::decoration` — `src/wayland/shell/xdg/decoration.rs`
- `smithay::wayland::shell::wlr_layer` — `src/wayland/shell/wlr_layer/mod.rs`
- `smithay::wayland::viewporter` — `src/wayland/viewporter/mod.rs`
- `smithay::wayland::fractional_scale` — `src/wayland/fractional_scale/mod.rs`
- `smithay::wayland::presentation` — `src/wayland/presentation/mod.rs`
- `smithay::wayland::pointer_constraints` — `src/wayland/pointer_constraints.rs`
- `smithay::wayland::relative_pointer` — `src/wayland/relative_pointer.rs`
- `smithay::wayland::pointer_gestures` — `src/wayland/pointer_gestures.rs`
- `smithay::wayland::dmabuf` — `src/wayland/dmabuf/mod.rs`
- `smithay::wayland::drm_syncobj` — `src/wayland/drm_syncobj/mod.rs` (feature `backend_drm`)
- `smithay::wayland::security_context` — `src/wayland/security_context/mod.rs`
- `smithay::wayland::xdg_activation` — `src/wayland/xdg_activation/mod.rs`
- `smithay::wayland::xdg_foreign` — `src/wayland/xdg_foreign/mod.rs`
- `smithay::wayland::single_pixel_buffer` — `src/wayland/single_pixel_buffer/mod.rs`
- `smithay::wayland::fifo` — `src/wayland/fifo/mod.rs`
- `smithay::wayland::commit_timing` — `src/wayland/commit_timing/mod.rs`
- `smithay::wayland::xwayland_shell` — `src/wayland/xwayland_shell.rs` (feature `xwayland`)
- `smithay::wayland::xwayland_keyboard_grab` — `src/wayland/xwayland_keyboard_grab.rs` (feature `xwayland`)

**P0 — needs wiring in zos-wm (smithay module present):**
- `smithay::wayland::cursor_shape` — `src/wayland/cursor_shape.rs`
- `smithay::wayland::idle_inhibit` — `src/wayland/idle_inhibit/mod.rs`
- `smithay::wayland::idle_notify` — `src/wayland/idle_notify/mod.rs` (per CHANGELOG 0.5.0 bumped to v2)
- `smithay::wayland::session_lock` — `src/wayland/session_lock/mod.rs`

**P1 — smithay modules present:**
- `smithay::wayland::pointer_warp` — `src/wayland/pointer_warp.rs`
- `smithay::wayland::content_type` — `src/wayland/content_type/mod.rs`
- `smithay::wayland::image_copy_capture` — `src/wayland/image_copy_capture/mod.rs` (needs un-stubbing)
- `smithay::wayland::selection::ext_data_control` — `src/wayland/selection/ext_data_control/`

**P1 — NO smithay module at this SHA (must lift from niri or write ourselves):**
- `zwlr-output-management-v1` — niri `src/protocols/output_management.rs` (923 LoC)
- `zwlr-screencopy-v1` — niri `src/protocols/screencopy.rs` (742 LoC)
- `zwlr-gamma-control-v1` — niri `src/protocols/gamma_control.rs` (245 LoC)
- `ext-foreign-toplevel-list-v1` + `zwlr-foreign-toplevel-manager-v1` — niri `src/protocols/foreign_toplevel.rs` (669 LoC)
- `ext-workspace-v1` — niri `src/protocols/ext_workspace.rs` (715 LoC)
- `wp-tearing-control-v1` — neither. Write from scratch against `wayland-protocols::wp::tearing_control`.

## Implementation order + dependencies

```
Phase 2.A (P0 rescope, ~1 week):

  1. cursor-shape-v1           [1h]  — trivial delegate add. No deps.
  2. idle-inhibit-v1           [4h]  — needs HashSet<WlSurface> on AnvilState. No hard deps.
  3. idle-notify-v1            [4h]  — needs LoopHandle at init (already present in anvil).
                                         Depends on nothing; completes "idle" pair with (2).
  4. pointer-warp              [2h]  — needs current pointer logic; anvil already has PointerHandle.
  5. content-type-v1           [2h]  — useful before tearing-control (routes hints).
  6. session-lock-v1           [1d]  — needs a lock-surface renderer bypass in the output render
                                         loop. Depends on render.rs understanding a "locked"
                                         draw-mask. This is the big one.
  7. Un-stub image-copy-capture [1-2d] — or punt in favor of (10).
  8. Keep xwayland working     [—]  — feature already enabled; don't regress.

Phase 2.B (P1, ~2 weeks):

  9.  ext-foreign-toplevel-list-v1  [1d]  — lift from niri. Prereq for any panel task list.
                                             Dep: needs zos-wm's window list to be enumerable
                                             (it already is via `Space`).
  10. wlr-screencopy-v1             [2d]  — lift from niri/protocols/screencopy.rs.
                                             Dep: OutputDamageTracker already present.
  11. wlr-output-management-v1      [2d]  — lift from niri. Dep: zos-wm must be able to
                                             apply mode/transform changes at runtime; anvil's
                                             udev.rs (line 1677) already has mode-set logic.
  12. wlr-gamma-control-v1          [1d]  — lift from niri. Dep: DRM connector CRTC access.
  13. tearing-control-v1            [2d]  — write from scratch. Dep: DRM atomic commit path
                                             must honor an "allow tearing" flag (smithay's
                                             drm backend supports this via DrmSurface flags).
  14. content-type routing          [1d]  — hook content-type hints into (13) + VRR decisions.
  15. ext-data-control-v1           [2h]  — parallel w/ (9). Modern clipboard managers.
  16. ext-workspace-v1              [1d]  — defer until Phase 3 (workspace model stabilizes).

Total Phase 2 budget: ~3 weeks for a developer full-time, assuming anvil's patterns hold.
```

Dependency notes:
- **fractional-scale depends on viewporter** at the protocol level; both are already wired.
- **tearing-control depends on the DRM atomic commit path understanding a per-surface "allow tearing" flag**. Smithay's `DrmSurface::queue_buffer` takes a `user_data` + we set DRM_MODE_PAGE_FLIP_ASYNC via `DrmSurface::use_mode_set_with_async` — this is already plumbable; see smithay's `backend/drm/surface/atomic.rs`. Niri does *not* do this; we'd be adding the async-page-flip support ourselves.
- **image-copy-capture depends on explicit dmabuf feedback** being correct per-client. Anvil already builds `DmabufFeedback` correctly in `udev.rs:166-186` and `winit.rs:64-83`.
- **session-lock requires the render path to paint the lock surface full-screen and *nothing else*** even while normal surfaces still exist. Cleanest pattern is niri's at `handlers/mod.rs:459-484`: track a `Locked(ExtSessionLockV1)` state and short-circuit the render loop's element collection. Plan to do this in one commit, not three.

## Per-P0 gotchas

### xdg-shell (already wired)
- **CHANGELOG "Unreleased" breaking change**: `ToplevelSurface::current_state()` is **gone**. Use `with_committed_state(|state: Option<&ToplevelState>| ...)` or `with_cached_state(|cached: &ToplevelCachedState| ...)`. Anvil at the pinned SHA has been updated already — grep `zos-wm/src/shell/xdg.rs` for `with_committed_state` to confirm before you write new code.
- **CHANGELOG "Unreleased"**: you no longer need to manually call `ToplevelSurface::reset_initial_configure_sent()` or track `LayerSurfaceAttributes::initial_configure_sent` — Smithay does it. If you copy Niri's xdg_shell handler (1561 LoC at `niri/src/handlers/xdg_shell.rs`), cross-check against the CHANGELOG before keeping any manual-tracking code.
- **CHANGELOG "Unreleased"**: xdg_shell and layer_shell now **enforce that clients ack a configure before committing a buffer**. If a client misbehaves you'll get a protocol error rather than silent breakage. This is mostly a good thing — but test with old Electron builds that may assume the lax behavior.

### wlr-layer-shell (already wired)
- Same `current_state` → `with_committed_state` migration as xdg-shell. Anvil's copy in `zos-wm/src/shell/mod.rs:245` is already updated.
- **CHANGELOG "Unreleased"**: `LayerSurfaceCachedState` is no longer `Copy`. Any pattern that passes it by value must switch to borrow.

### xdg-decoration (already wired)
- No 2025-2026 breaking changes specific to this. Default SSD in `state.rs:474-506` is correct for GTK4/Qt.

### fractional-scale + viewporter (already wired)
- **CHANGELOG 0.6.0 breaking change**: `CompositorClientState::client_scale()` now returns `f64`, not `u32` (`/tmp/smithay-peek/CHANGELOG.md:307-313`). If you lift Niri code, make sure it expects the new signature; anvil's own handler at `state.rs:513-559` uses the current API.

### presentation-time (already wired)
- Clock id is cast `clock.id() as u32` at `state.rs:725`. `Clock<Monotonic>::id()` returns a stable id — don't change without updating feedback flags in `surface_presentation_feedback_flags_from_states` (imported at `state.rs:29`).

### pointer-constraints + relative-pointer (already wired)
- Relative-pointer is gated by `BackendData::HAS_RELATIVE_MOTION` at `state.rs:735`. The udev backend sets this true; winit backend sets it false. When testing nested, expect relative-pointer to be *unavailable*. This is fine, but it means game-cursor capture won't work in nested dev. Workaround: test locking via TTY login (Phase 1's `just dev-zos-wm-tty`).
- `PointerConstraintsHandler::new_constraint` at `state.rs:395` — Niri extends this (`handlers/mod.rs:157-220`) to call `refresh_pointer_contents()` + `maybe_activate_pointer_constraint()`. Anvil's impl is more barebones; if game-capture is laggy to activate, lift Niri's extension verbatim.

### linux-dmabuf + drm-syncobj (already wired, udev only)
- **CHANGELOG 0.7.0 breaking change**: `DrmSyncobjHandler::drm_syncobj_state(&mut self)` now returns `Option<&mut DrmSyncobjState>`, not `&mut DrmSyncobjState`. Anvil at the pinned SHA (`udev.rs:619`) already returns `Option`. If you ever downgrade Smithay, watch for this.
- **CHANGELOG 0.7.0 / NVIDIA 580**: `GbmFramebufferExporter::new` now requires an `import_node: Option<DrmNode>`. Per the Unreleased section, the API now takes a `NodeFilter` instead. If you rebuild on a newer Smithay, you'll need `GbmFramebufferExporter::new(gbm, node.into())` — see CHANGELOG lines 26-41.
- **NVIDIA 580 proprietary driver specifics**: `DrmSyncobjState` must have an `import_device` set to a DRM fd that the NVIDIA driver accepts. Anvil wires this at `udev.rs:494` using the render node. Leave this alone — changing it will break explicit-sync and you'll lose 30fps.

### security-context-v1 (already wired)
- Restriction check at `state.rs:743-747` — a client with a `SecurityContext` on its `ClientState` is considered "restricted" and should not be given access to session-lock, data-control, or screencopy globals. When we add session-lock, follow niri's pattern (`client_is_unrestricted` callback in `SessionLockManagerState::new` at `niri.rs:2285`): pass the same lambda that `SecurityContextState::new` uses.

### xdg-activation-v1 (already wired)
- No 2025-2026 breaking changes. `create_external_token` signature changed in 0.5.0 to take `XdgActivationTokenData`, anvil already uses the new form.

### xwayland-shell + xwayland-keyboard-grab (feature-gated, already wired)
- **CHANGELOG "Unreleased" breaking change**: `X11WM::start_wm` now needs a `DisplayHandle`-reference AND requires the state type to implement `DndGrabHandler` + `SeatHandler::PointerFocus`/`TouchFocus` to implement the new `DndFocus` trait. Anvil's `state.rs:36` imports `DnDGrab, DndGrabHandler, DndTarget` — the migration has landed. Do not regress when editing `state.rs`.
- **CHANGELOG "Unreleased"**: The XWayland WM can now handle XDND. If you lift `X11Surface::surface_under` use — it's required for `SpaceElement::is_in_input_region` and `Window::surface_under` — don't use the old `under_from_surface_tree` path on `X11Surface::wl_surface()`. Anvil's copy in `zos-wm/src/shell/x11.rs` should already be updated; verify during Phase 2.
- **NVIDIA + XWayland**: keep `XWayland::new()` called with GPU feedback from `udev.rs` (line 546ish). If XWayland renders black on NVIDIA, it's almost always a wrong `DmabufFeedback` sent to the XWayland client — cross-reference `/tmp/smithay-peek/anvil/src/udev.rs` line-by-line to make sure your divergence didn't lose the right feedback.

### cursor-shape-v1 (missing — P0 to add)
- **CHANGELOG 0.6.0 breaking change**: `CursorShapeDeviceUserData` is now generic over `<D: SeatHandler>`. When you `delegate_cursor_shape!(AnvilState<...>)`, the macro handles this but if you manually write a `Dispatch` impl you need the generic.
- **CHANGELOG 0.7.0**: cursor-shape-v1 was bumped to version 2. Clients requesting v1 still work; v2 adds the `tablet_tool_v2` shape source. We don't need to handle tablet tools (Zach has no tablet) but the generated bindings include the v2 enum variants — don't match non-exhaustively.

### idle-inhibit-v1 (missing — P0 to add)
- Smithay's `IdleInhibitManagerState::new::<State>(&display_handle)` is straightforward. The handler (per niri `handlers/mod.rs:524-533`) just tracks a `HashSet<WlSurface>` of inhibiting surfaces and refreshes on `new_inhibitor` / `inhibitor_destroyed`. No protocol gotchas.
- **Integration gotcha**: if we also wire `idle-notify` (we should), the inhibit set needs to reset the notifier's timers on change. Niri calls `self.niri.refresh_idle_inhibit()` (see `niri.rs:815`) which walks the notifier state. Copy that pattern.

### idle-notify-v1 (missing — P0 to add)
- **CHANGELOG 0.5.0**: notify was bumped to v2 in this SHA (`/tmp/smithay-peek/CHANGELOG.md:488`). That's already the current version — no migration burden.
- Construction takes a calloop `LoopHandle`. Anvil's init already has one (`state.rs:141` + `state.rs:670`). Plumb it in via the same handler-state pattern as security-context.
- Watch out: the module ships *both* `ext-idle-notify-v1` and (deprecated) `kde-idle` registration. Prefer binding only the ext variant.

### session-lock-v1 (missing — P0 to add)
- **CHANGELOG "Unreleased" breaking change**: `LockSurface::current_state()` is gone; use `with_committed_state`. Mirror of xdg-shell's migration.
- **Render loop impact**: when locked, the compositor must still render (outputs stay lit) but only the lock surface (per output). In anvil's `src/render.rs` + `src/udev.rs` render path, we'll need an early-return that collects *only* lock elements. Niri does this at `handlers/mod.rs:459-484` by queuing redraws and swapping the output-content selection.
- **Security-context interaction**: only unrestricted clients should bind `ext_session_lock_manager_v1`. Use niri's `client_is_unrestricted` lambda at `niri.rs:2285-2286` — it queries the same `security_context` on `ClientState` that anvil already stores at `state.rs:126`.
- **Don't keep the old `SessionLocker` around after lock confirm** — niri stores a `Locking(SessionLocker)` → `Locked(ExtSessionLockV1)` state machine (`niri.rs:547-551`). Copy the state machine.

### xdg-foreign, single-pixel-buffer, fifo, commit-timing, xdg-activation (already wired)
- No 2025-2026 breaking changes that affect us. Leave as-is.

### drm-lease (already wired; not a priority but don't regress)
- Udev-only. If you refactor `udev.rs`, keep `DrmLeaseState::new::<_, _>` called so that Steam VR can request leases (even though Zach has no VR headset, leaving the plumbing intact is free).

## Sources

- zos-wm source checkout at pinned SHA: `/var/home/zach/github/zOS/zos-wm/src/`
- Smithay repository at pinned SHA `27af99ef492ab4d7dc5cd2e625374d2beb2772f7`: `/tmp/smithay-peek/`
  - Protocol module inventory: `/tmp/smithay-peek/src/wayland/mod.rs:48-99`
  - CHANGELOG: `/tmp/smithay-peek/CHANGELOG.md` (Unreleased + 0.7.0 + 0.6.0 + 0.5.0 sections, lines 1-500)
- Niri repository (latest `main` as of clone date 2026-04-21, approx niri 26.x): `/tmp/niri-peek/`
  - Handler registration: `/tmp/niri-peek/src/handlers/mod.rs:64-97`
  - Protocol implementations: `/tmp/niri-peek/src/protocols/*.rs`
  - State fields (for patterns to copy): `/tmp/niri-peek/src/niri.rs:279-334`
- Wayland protocol specs (authoritative references):
  - wayland-protocols repo: https://gitlab.freedesktop.org/wayland/wayland-protocols
  - wlr-protocols repo (screencopy, output-management, gamma-control, foreign-toplevel): https://gitlab.freedesktop.org/wlroots/wlr-protocols
  - tearing-control-v1 spec: https://wayland.app/protocols/tearing-control-v1
  - session-lock-v1 spec: https://wayland.app/protocols/ext-session-lock-v1
  - idle-notify-v1 spec: https://wayland.app/protocols/ext-idle-notify-v1
  - cursor-shape-v1 spec: https://wayland.app/protocols/cursor-shape-v1 (version 2 current)
- Smithay issue tracker, searched for "nvidia", "tearing", "syncobj": https://github.com/Smithay/smithay/issues — no filed issues at pinned SHA block NVIDIA+syncobj+xwayland daily-driver use.
- Phase-1 research brief: `/home/zach/.claude/plans/hey-how-do-i-jiggly-balloon.md` (context only, not re-researched).
- Research date: 2026-04-21.
