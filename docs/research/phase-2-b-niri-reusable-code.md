# Phase 2.B — Niri code worth stealing for zos-wm

**Niri commit inspected:** `9438f59e2b9d8deb6fcec5922f8aca18162b673c` ("Bump Smithay (last fix was merged)") from `https://github.com/niri-wm/niri`, cloned to `/tmp/niri-peek`.
**Niri workspace version:** `25.11.0`
**Niri Smithay pin:** `ff5fa7df392cecfba049ffed55cdaa4e98a8e7ef`
**Niri license (confirmed):** `GPL-3.0-or-later` (see `/tmp/niri-peek/Cargo.toml:14` and `/tmp/niri-peek/LICENSE`).
**zos-wm starting point:** fork of Smithay `anvil` (MIT), at `/var/home/zach/github/zOS/zos-wm/`.

## TL;DR

- Niri's backend skeleton (`src/backend/tty.rs`, 3578 LoC) is the same `UdevBackend` + `LibSeatSession` + `LibinputInputBackend` stack anvil already uses — Niri *polishes* it (ignored-nodes, resume-time re-enumeration, HDR/gamma props, non-desktop DRM-leasing, paused-libinput bootstrap) rather than rewriting it. Patterns, not verbatim code, are what we want.
- The highest-value **verbatim-safe** candidate is `src/utils/scale.rs` (120 LoC, Mutter-derived monitor-scale calculator).
- The highest-value **pattern reimplementations** are (a) resume-time device list diffing (`tty.rs:616-728`), (b) the workspace-preservation model for monitor disconnect/reconnect (`layout/mod.rs:735-900`), (c) the config-file watcher (`utils/watcher.rs`, 754 LoC), and (d) the blocking/JSON/async IPC server architecture (`ipc/server.rs`, 945 LoC + `niri-ipc` 2533 LoC crate).
- Niri does **not** use XWayland directly; it spawns `xwayland-satellite` as an external process (`utils/xwayland/satellite.rs`). That is a different architectural choice than anvil's smithay-embedded XWayland. For zos-wm we keep anvil's in-process XWayland.
- Niri does **not** use `DrmSyncobjState` at all. anvil (our base) already has explicit-sync wired up. zos-wm is *ahead of Niri* on NVIDIA 555+ explicit-sync — nothing to lift here.
- **License:** GPL-3.0-or-later is viral. See next section. Recommendation: **Option A (study Niri, re-implement in our style; lift only `utils/scale.rs` once we re-derive it from Mutter ourselves — Mutter is LGPL-2.0+, and the tiny math is public domain in spirit).**

---

## License reality check (Q11)

Niri is `GPL-3.0-or-later`. Anvil (zos-wm's starting point) is MIT. Smithay itself is MIT.

GPL-3.0 is **strongly copyleft**:
- Any file lifted verbatim from Niri forces the combined work to be GPL-3.0-or-later.
- "Combined work" here means our compiled binary: GPL-3.0 code linked with MIT code produces a GPL-3.0 binary. The MIT source files *stay* MIT but the aggregate must be distributed under GPL-3.0's terms.
- "Inspired by" or "read-then-reimplement" is **not** a license event as long as we (a) rewrite from scratch without copying expressive code, and (b) do not carry over non-trivial copyrightable structures (e.g., long distinctive data structures, function bodies verbatim).
- Function **signatures**, **algorithm descriptions in comments**, and **tiny (<10 line) idiomatic snippets** are generally not copyrightable, but this is jurisdictional and we should not rely on it for anything substantial.

### Options

- **Option A — Keep MIT, study-and-reimplement only.** No Niri source in our tree. All "lifts" are pattern-based: we read Niri, design our own approach, write our own Rust. Slower but legally clean. *Recommended.*
- **Option B — Relicense zos-wm to GPL-3.0-or-later.** Lift freely. But: (i) forces GPL on all downstream image consumers / packagers, (ii) changes zOS's distribution story (zOS today ships an MIT-ish stack), (iii) one-way door (can't un-GPL without contributor sign-off).
- **Option C — Ask YaLTeR for MIT re-license on specific files.** YaLTeR is responsive but is the sole copyright holder on most files. He might agree for small utilities (`scale.rs`, `watcher.rs`). Risky bet; cannot be planned around. Worth doing *in addition* to Option A for the utilities we really want.
- **Option D — Isolate GPL into a separate crate.** Legally does not help. GPL is viral across crate boundaries in a single binary — a Cargo workspace that produces one compositor binary linking a GPL-3.0 crate produces a GPL-3.0 binary. This is the "mere aggregation" question and it almost always loses in court when the two parts share a process.

### Recommendation

**Option A, with Option C as an optional side-quest for three specific files:**
1. `src/utils/scale.rs` — the Mutter-derived monitor-scale math. (We can also re-derive from Mutter directly; Mutter is LGPL-2.0+, also copyleft but weaker — and the algorithm itself is trivial.)
2. `src/utils/watcher.rs` — the polling file-change watcher.
3. `src/utils/transaction.rs` — the client-transaction blocker helper.

Everything else in Q2-Q9 we treat as "read and reimplement". The cost is a few weeks of engineering. The benefit is that zos-wm stays MIT, zOS stays shippable under its current license posture, and we don't get a nastygram from an upstream contributor later.

**All code examples in the rest of this document must be treated as reference only.** When we implement, we write new code; we do not paste.

---

## Niri crate layout (Q1)

Top-level at `/tmp/niri-peek/src/`:

| File / dir | LoC | 1-sentence description |
|---|---:|---|
| `niri.rs` | 6530 | God-object: the `Niri` and `State` types that hold every piece of compositor state, event-loop wiring, redraw orchestration; analogous to anvil's `state.rs` x6. |
| `main.rs` | 410 | Binary entry point: CLI parsing, config load, event loop bootstrap, backend selection (tty vs winit vs headless). |
| `lib.rs` | 29 | Re-export shell for integration tests; just `#[macro_use] extern crate tracing;` + pub mods. |
| `cli.rs` | 137 | `clap` definitions for `niri`, `niri msg`, `niri validate`, `niri panic`, etc. |
| `a11y.rs` | 369 | AccessKit integration (opt-in feature `accesskit`), bridges to the GNOME a11y bus. |
| `cursor.rs` | 318 | XCursor-theme loading + multi-scale caching; parses XDG cursor themes, emits `MemoryRenderBuffer`s. |
| `frame_clock.rs` | 98 | Per-output presentation-time predictor for pacing redraws. |
| `rubber_band.rs` | 39 | Overshoot easing curve for overview / touch interactions. |
| `animation/` (bezier.rs, clock.rs, mod.rs, spring.rs) | 837 | Keyframe / spring / bezier interpolators + a shared animation clock. |
| `backend/` (headless.rs, mod.rs, tty.rs, winit.rs) | 4303 | Three backends: tty (real DRM+udev+libseat+libinput), winit (dev on a Wayland/X11 host), headless (tests). `tty.rs` is 3578 LoC — the big one. |
| `dbus/` (10 files) | 2135 | zbus interfaces: `org.gnome.Mutter.DisplayConfig`, `org.gnome.Mutter.ScreenCast`, `org.freedesktop.ScreenSaver`, `org.freedesktop.a11y`, `org.freedesktop.login1` (inhibit), `org.gnome.Shell.Screenshot`, etc. |
| `handlers/` (background_effect.rs, compositor.rs, layer_shell.rs, mod.rs, xdg_shell.rs) | 3375 | Smithay dispatch handlers — where `CompositorHandler`, `XdgShellHandler`, `LayerShellHandler` impls live. Roughly the same role as anvil's `shell/` module. |
| `input/` (12 files) | 7555 | Libinput event processing + every interactive grab (move, resize, pick color, touch overview, etc). `mod.rs` alone is 5448 LoC. |
| `ipc/` (client.rs, server.rs, mod.rs) | 1762 | Unix-socket JSON-line IPC server and `niri msg` client. See Q9. |
| `layer/` (mapped.rs, mod.rs) | 445 | `wlr-layer-shell` surface wrapper. |
| `layout/` (13 files + tests/) | 27,187 (4000 prod + tests) | Niri's scrolling-columns-workspaces model. **Not reusable** for zos-wm — different layout philosophy. |
| `protocols/` (ext_workspace, foreign_toplevel, gamma_control, mutter_x11_interop, output_management, raw, screencopy, virtual_pointer) | 3484 | Hand-rolled Wayland protocol implementations that Smithay does not provide. |
| `render_helpers/` (22 files + `shaders/`) | 5363 | Render element abstractions: borders, shadows, blur, offscreen passes, damage tracking. |
| `screencasting/` (mod.rs, pw_utils.rs) | 2390 | PipeWire producer driven by `org.gnome.Mutter.ScreenCast` requests; feeds frames to xdp-gnome. |
| `tests/` (11 files + snapshots) | ~6000 | Integration tests (spawns a headless backend + synthetic Wayland clients). |
| `ui/` (mru, config_error, exit_confirm, hotkey_overlay, screen_transition, screenshot_ui) | 4500 | In-process rendered UIs (no GTK/Qt) — the overlays niri draws itself. |
| `utils/` (id, mod, region, scale, signals, spawning, transaction, vblank_throttle, watcher, xwayland/) | 3404 | Small standalone helpers. **The richest seam for verbatim-ish reuse.** |
| `window/` (mapped.rs, mod.rs, unmapped.rs) | 1980 | `xdg-toplevel` wrapper with niri-specific state (tile, floating, dialog parent, etc.). |

Sibling crates in the workspace:
- `niri-config/` — serde/KDL config schema (not inspected here; mirrors `Cargo.toml:3`).
- `niri-ipc/` — shared IPC types, consumed by both the compositor and the `niri msg` client. 2533 LoC across `lib.rs` (2109), `socket.rs` (101), `state.rs` (323). Intended as a public SDK.
- `niri-visual-tests/` — renders UI elements to PNGs for manual inspection.

---

## Reusable subsystems

### Q2. Backend & session integration

All the interesting work is in a single file: `src/backend/tty.rs`.

- **DRM enumeration / hot-plug** — `Tty::on_udev_event` at `src/backend/tty.rs:553-595` switches on `UdevEvent::{Added, Changed, Removed}`. Device list initial scan is in `Tty::init` at `src/backend/tty.rs:508-551`. The **`DrmScanner`** (from external crate `smithay-drm-extras`, MIT) does the actual CRTC/connector diffing at `src/backend/tty.rs:966` (`device.drm_scanner.scan_connectors(&device.drm)`).
- **libseat acquire / switch / release** — `LibSeatSession::new()` at `src/backend/tty.rs:417`. VT-switch pause/resume: `on_session_event` at `src/backend/tty.rs:589-728` — this is the *good stuff*. On resume it: (1) re-scans udev, (2) diffs current vs known device set, (3) calls `drm.activate(force_disable)` per retained device, (4) re-applies pending gamma, (5) re-runs `on_output_config_changed` if the config changed while paused.
- **libinput device config** — `apply_libinput_settings()` at `src/input/mod.rs:4674-4850` is a ~300-line per-device config applier (touchpad / mouse / trackpoint / tablet / keyboard separately). Trigger is `on_device_added` at `src/input/mod.rs:244`. Touchpad detection via `config_tap_finger_count() > 0` is directly from Mutter. There is also a plugin-system init via raw FFI at `src/backend/tty.rs:3388-3430` — we don't want this (experimental libinput feature, guarded by `#[cfg(have_libinput_plugin_system)]`).
- **udev event subscription** — `UdevBackend::new(session.seat())` at `src/backend/tty.rs:425` plus `Dispatcher::new(udev_backend, ...)` at `:426-432`. Standard Smithay pattern; anvil already does the same thing.

**Diverges from anvil meaningfully?** No. Niri uses the identical crate surface. The differences are *polish*:
- Niri starts libinput in the paused state if the session is not active at boot (`tty.rs:444-447`). Anvil does not.
- Niri re-resolves `/dev/dri/by-path/...` symlinks on resume because suspend can reassign them (`tty.rs:663`). Anvil does not.
- Niri has an `ignored_nodes: HashSet<DrmNode>` populated from config (`tty.rs:92`, `:2767`). Anvil does not.
- Niri reads gamma/HDR/VRR props per connector (`tty.rs:400`+); anvil does not.
- Niri registers `DrmLeaseState` for non-desktop displays like VR headsets (`tty.rs:909`). Anvil does not.

### Q3. Multi-monitor hot-plug

The state-preservation trick lives in `src/layout/mod.rs`:
- `Layout::last_active_workspace_id: HashMap<OutputName, WorkspaceId>` — when a monitor disconnects, the active workspace *ID* is stashed keyed on output name (`layout/mod.rs:856-859`).
- `Layout::remove_output` at `src/layout/mod.rs:843-899`: (1) stash last-active-ws, (2) `Monitor::into_workspaces()` harvests all workspaces, (3) if no monitors left, transition to `MonitorSet::NoOutputs { workspaces }` and keep them, else `primary.append_workspaces(workspaces)` moves them all to the primary monitor.
- `Layout::add_output` at `src/layout/mod.rs:735-841`: iterates the primary's workspaces in reverse, pulls out any whose `original_output.matches(&output)` matches the re-connecting monitor's name, then creates a new `Monitor` with those workspaces. The new monitor activates the previously-active workspace via `self.last_active_workspace_id.remove(&output.name())`.

Key insight: **workspaces carry an `original_output: OutputName` field** (not a pointer). This is what makes round-trip reconnect work — the name survives disconnect. For zos-wm this pattern generalizes even if our "workspace" concept is totally different (we're floating-first, so our "preserved state" would be window lists / positions).

**Reuse verdict:** pattern-only. The scrolling-columns structure doesn't match our floating model, but the `OutputName`-keyed-stash and the "orphan workspaces land on primary" fallback are solid.

Output removal higher in the stack: `Niri::remove_output` at `src/niri.rs:2900-2925` calls `layout.remove_output`, `screencopy_state.remove_output`, and cleans cursor-last-output fields. `Niri::add_output` at `src/niri.rs:2806-2855` calls `layout.add_output` with a preserved `LayoutPart` config.

### Q4. XWayland rootless integration

**Niri does not run XWayland in-process.** Instead it launches [`xwayland-satellite`](https://github.com/Supreeeme/xwayland-satellite) as an external process.

- `src/utils/xwayland/mod.rs:1-160` creates the X11 socket, lock file, and abstract-socket pair (the directory-permission dance is lifted from Mutter, see the comment at `:34`).
- `src/utils/xwayland/satellite.rs:1-324` spawns the satellite binary with `fcntl_setfd(..., FdFlags::empty())` to clear CLOEXEC on the sockets, sets env vars for `DISPLAY` and `WAYLAND_DISPLAY`, and registers the spawn-watch with calloop.
- `src/niri.rs:1692` calls `xwayland::satellite::setup(self)` on startup or on config-change.

**Implication for zos-wm:** anvil's existing in-process XWayland (`src/x11.rs`, 487 LoC) is a *different* architectural choice. Niri's approach gives us process isolation (if Xwayland segfaults, the compositor survives) and lets us get out of the Xwayland-integration maintenance tax. But it adds a runtime dependency. **Recommend: stay with anvil's in-process approach for Phase 2 and revisit later.** The satellite pattern is a Phase 4+ consideration.

There is also `src/protocols/mutter_x11_interop.rs` (93 LoC) — a custom protocol that lets xwayland-satellite tell the compositor about X-specific things like `XWAYLAND_SURFACE_HAS_X11_PARENT`. Not useful unless we go the satellite route.

### Q5. Fractional scaling math

Niri uses Smithay's `FractionalScaleManagerState` + `ViewporterState` directly (`src/niri.rs:81, 109, 2294, 2344`). The Wayland-protocol side is all in Smithay.

**Niri's own value-add is two files, totaling ~135 LoC:**
- `src/utils/scale.rs` (120 LoC) — `guess_monitor_scale(size_mm, resolution) -> f64` derived from Mutter's `meta-monitor.c` (see comment at `src/utils/scale.rs:1-4` citing `https://gitlab.gnome.org/GNOME/mutter/-/blob/gnome-46/src/backends/meta-monitor.c`). Plus `closest_representable_scale(f64) -> f64` at `:53-58` that rounds to `N/120` (the fractional-scale-v1 wire denominator) and `supported_scales(resolution)` at `:41-45`.
- Per-output scale lives on the Smithay `Output`; Niri reads `output.current_scale().fractional_scale()` in ~20 places throughout `niri.rs` (see `src/niri.rs:1746, 3008, 3706, 3873, 4076, ...`). Surface-coordinate math uses `Scale::from(fractional_scale)`.

**Window-coordinate-to-physical:** pure Smithay (`Scale<f64>` + `to_logical`/`to_physical`). No niri-specific code here.
**Cursor scaling:** `src/niri.rs:3873` picks `f64::max(cursor_scale, output.current_scale().fractional_scale())` for the cursor buffer scale — trivial pattern.

**Reuse verdict:** `utils/scale.rs` is the single best verbatim-lift candidate in the entire tree, *if* license permits. Under Option A we re-derive from Mutter ourselves (Mutter is LGPL-2.0+; the math is ~20 lines of DPI arithmetic).

### Q6. Screencast / xdg-desktop-portal-gnome integration

Niri targets **xdg-desktop-portal-gnome**, not xdg-desktop-portal-wlr. The design:

1. `src/dbus/mutter_screen_cast.rs` (406 LoC) — implements `org.gnome.Mutter.ScreenCast` (the private GNOME API xdp-gnome uses instead of going through wlroots-style protocols). Exposes `RecordMonitor`, `RecordWindow`, `RecordVirtual` with PipeWire stream negotiation.
2. `src/dbus/mutter_display_config.rs` (354 LoC) — implements `org.gnome.Mutter.DisplayConfig`, which xdp-gnome calls to enumerate monitors before presenting a picker.
3. `src/dbus/gnome_shell_screenshot.rs` (106 LoC) — one-shot screenshot via dbus; bridges to niri's internal screenshot machinery.
4. `src/screencasting/mod.rs` (804 LoC) + `src/screencasting/pw_utils.rs` (1586 LoC) — PipeWire producer. Feeds `Dmabuf`s (or `Shm` buffers as fallback) into pipewire streams with metadata (cursor position, resize events).
5. `src/protocols/screencopy.rs` (742 LoC) — ALSO implements `wlr-screencopy-v1` as a separate path, for OBS etc. that speak wlroots protocol directly.

For portability, `mutter_screen_cast` is gated `#[cfg(feature = "xdp-gnome-screencast")]` (`src/dbus/mod.rs:15`).

**Reuse verdict:**
- `src/protocols/screencopy.rs` — pattern-based reimplementation. 742 LoC is too much to lift verbatim, and we'll want slightly different semantics (we might not want `wlr-screencopy` if we go xdp-gnome-only). **Effort: 2-3 days.**
- `src/dbus/mutter_screen_cast.rs` — pattern-based. Straightforward zbus interface, but bound tightly to Niri's internal `CastSessionId` / `CastStreamId`. **Effort: 2 days.**
- `src/screencasting/pw_utils.rs` — pipewire FFI wrangling; very tedious (dmabuf + SPA + format negotiation). **This is the most valuable 500-800 LoC in the tree for us** but also the most expensive to re-write. Pattern-based reimplementation: **3-5 days.**

### Q7. Surface sync / explicit-sync / drm-syncobj

Niri **does not** call Smithay's `DrmSyncobjState`. There's zero reference to `drm_syncobj`, `DrmSyncobjState`, `DrmSyncobjHandler`, or `delegate_drm_syncobj!` in the entire Niri source tree (verified via grep).

This means Niri relies on Smithay's internal implicit-sync fence import path. For NVIDIA 555+ where explicit-sync is the *only* working path, Niri users have presumably been hitting the protocol-negotiation fallback in Smithay itself.

**Anvil (our starting point) already has explicit-sync wired up** — `zos-wm/src/udev.rs:94,135,494,618-623` uses `DrmSyncobjState::new(...)` + `DrmSyncobjHandler` impl + `delegate_drm_syncobj!`.

**Reuse verdict:** zos-wm is *ahead of* Niri here. Nothing to lift; we keep what anvil has. If we want to improve, the reference is in Smithay itself (MIT) and in `cosmic-comp` (also GPL, same license hazard).

### Q8. Config hot-reload

`src/utils/watcher.rs` (754 LoC) — spawns a background thread that polls the config file's mtime every 500ms (`POLLING_INTERVAL`, `:14`). Uses a canonical-path check in addition to mtime so symlinked configs on nix (where mtime is 1970-01-01 in /nix/store) still trigger reload (see comment at `:40-44`). Sends the parsed result through a `calloop::channel::SyncSender` into the main loop.

- `Watcher::new` at `src/utils/watcher.rs:54-85` spawns the thread.
- Tracks `includes: Vec<PathBuf>` separately — each included file is stat'd independently (`:27, :77`).
- Main-loop side: `src/niri.rs:177` imports `Watcher`, `:204` holds `config_file_watcher: Option<Watcher>`, `:1421` is `State::reload_config` which is the apply side. `reload_config` is ~600 lines (`:1421-2010`) because it diffs every config subtree (keybinds, input, outputs, layout options, animations, xwayland-satellite on/off, etc.) and applies each delta atomically.

**Reuse verdict:**
- The watcher thread itself (`watcher.rs:1-200`) is a clean, general-purpose utility. **Pattern-based reimplementation — 1 day.** Alternatively, use the `notify` crate directly (inotify-based, no polling) — arguably better than Niri's approach.
- The `reload_config` diff-and-apply logic (`niri.rs:1421-2010`) is tightly bound to Niri's config schema. **Pattern reference only.** When we implement our own, we follow the same "group changes by subsystem, apply each group with its own invariants" approach.

### Q9. IPC / msg protocol

Niri's IPC is a **JSON-lines over Unix-domain socket** design.

- **Server:** `src/ipc/server.rs` (945 LoC). Socket path is `$XDG_RUNTIME_DIR/niri.$WAYLAND_DISPLAY.$PID.sock`, exported in `$NIRI_SOCKET` (`:82, src/niri-ipc/socket.rs:12`). Uses calloop's `Generic` source for the listener (`:91-102`) and `calloop::futures::Scheduler` to run per-client async tasks (`:177-184`). Each client sends one `Request` per line, gets one `Reply` per line. If a client sends `Request::EventStream`, the server stops reading requests from that socket and starts continuously pushing `Event` values (`:213-247`).
- **Event stream:** bounded `async_channel` of size 64 (`src/ipc/server.rs:41`); slow clients get kicked (`:122-126`).
- **Client:** `src/ipc/client.rs` (815 LoC) — but this is the `niri msg` *CLI presentation* code. The actual library client is in `niri-ipc/src/socket.rs` (101 LoC) which is a clean, simple blocking-IO helper.
- **Shared types:** `niri-ipc/src/lib.rs` (2109 LoC) — the serde-serialized `Request`, `Reply`, `Response`, `Event`, `Action`, `Window`, `Output`, `Mode`, `Workspace`, etc. This crate is published to crates.io so external clients (status bars, automation scripts) can link it. Has an opt-in `json-schema` feature to emit a JSON schema.
- **Event state replay:** `niri-ipc/src/state.rs` (323 LoC) — `EventStreamState` trait that lets a fresh connection replay "current state as a synthetic initial event sequence" so clients don't have to special-case the initial snapshot.

**Reuse verdict: yes, this architecture works perfectly for zos-wm.** In fact it's near-ideal. We get:
- Synchronous ergonomic client (blocking Socket helper, 101 LoC).
- Async server that doesn't block the compositor main loop (uses calloop's scheduler).
- Replay-on-connect semantics that mean an external "zos-wm status bar" can just connect and get full state without bespoke handshake.

**Effort:**
- Define zos-wm's own `Request` / `Response` / `Event` enums (our actions will be different — floating-window focused). **2 days.**
- Server plumbing. **2 days.**
- Blocking client helper + publish a `zos-wm-ipc` crate. **1 day.**

Total **~5 days** of engineering to stand up a Niri-quality IPC surface. No Niri source needed; this is all architectural.

---

## Reusable-code shopping list

| Subsystem | Niri file:lines | Category | Effort | Blocker |
|---|---|---|---|---|
| Monitor-scale DPI math | `src/utils/scale.rs:17-58` | Pattern (GPL blocks verbatim) | <1 day | Re-derive from Mutter instead |
| libinput settings applier | `src/input/mod.rs:4674-4850` | Pattern | 1 day | None — trivial property mapping |
| Resume-time device diff | `src/backend/tty.rs:610-728` | Pattern | 2 days | None; anvil has the skeleton already |
| Monitor disconnect/reconnect workspace preservation | `src/layout/mod.rs:735-899` | Pattern | 2-3 days | Our layout model differs; only the *idea* transfers |
| Config file watcher | `src/utils/watcher.rs:1-200` | Pattern | 1 day | Consider `notify` crate instead of polling |
| Config diff-and-apply orchestrator | `src/niri.rs:1421-2010` | Pattern reference only | n/a | Tightly bound to Niri schema |
| JSON-lines Unix-socket IPC server | `src/ipc/server.rs:1-300` | Pattern | 2 days | None |
| IPC shared-types crate design | `niri-ipc/src/lib.rs` whole | Pattern | 2 days | None |
| Blocking IPC client helper | `niri-ipc/src/socket.rs:1-101` | Pattern | <1 day | None |
| EventStream state-replay pattern | `niri-ipc/src/state.rs:1-323` | Pattern | 1 day | None |
| PipeWire screencast producer | `src/screencasting/pw_utils.rs` whole | Pattern reference | 3-5 days | Pipewire FFI is painful — budget accordingly |
| Mutter.ScreenCast dbus iface | `src/dbus/mutter_screen_cast.rs` whole | Pattern | 2 days | None |
| Mutter.DisplayConfig dbus iface | `src/dbus/mutter_display_config.rs` whole | Pattern | 2 days | None |
| wlr-screencopy-v1 | `src/protocols/screencopy.rs` whole | Pattern or skip | 2-3 days | Consider skipping if xdp-gnome-only |
| wlr-gamma-control-v1 | `src/protocols/gamma_control.rs` whole | Pattern | 1 day | None |
| DRM-leasing for VR headsets | `src/backend/tty.rs` (non-desktop path at `:1246-1259, :1580-1588, :2538-2547`) | Pattern reference | 1 day | Low priority |
| Signal-mask hygiene for spawned children | `src/utils/signals.rs` whole (105 LoC) | Pattern | <1 day | None |
| Transaction/blocker helper | `src/utils/transaction.rs` whole (193 LoC) | Pattern | 1 day | None |
| XWayland satellite integration | `src/utils/xwayland/*` | Skip | — | anvil's in-process XWayland is fine for now |
| explicit-sync (DrmSyncobj) | n/a | Skip | — | anvil already has this; Niri doesn't |

---

## Attribution template

If/when Option C is pursued and a file *is* lifted verbatim with upstream permission, prepend this block:

```rust
// SPDX-License-Identifier: <whatever YaLTeR agrees to>
//
// Adapted from niri by Ivan Molodetskikh (YaLTeR)
//   Upstream:  https://github.com/niri-wm/niri
//   Commit:    9438f59e2b9d8deb6fcec5922f8aca18162b673c  (v25.11.0)
//   File:      src/utils/scale.rs
//   License:   GPL-3.0-or-later upstream; re-licensed to <new license>
//              with explicit permission from the author on <date>.
//
// Changes from upstream:
//   - <bullet list>
```

For *pattern-inspired* (Option A) reimplementations, a lighter attribution is sufficient and is not a legal requirement, but good practice:

```rust
// Pattern reference: niri src/utils/watcher.rs
// (https://github.com/niri-wm/niri @ 9438f59e)
// Implementation is independent; no code copied.
```

For algorithms derived from Mutter (e.g. the monitor-scale math), cite Mutter directly:

```rust
// Algorithm from GNOME mutter
// https://gitlab.gnome.org/GNOME/mutter/-/blob/gnome-46/src/backends/meta-monitor.c
// (LGPL-2.0-or-later). The math is trivial and we believe it is not itself
// copyrightable; comments retained for traceability.
```

---

## Sources

- Niri repo: `https://github.com/niri-wm/niri` @ `9438f59e2b9d8deb6fcec5922f8aca18162b673c`. Cloned at `/tmp/niri-peek`.
- Niri Cargo.toml (license, Smithay pin): `/tmp/niri-peek/Cargo.toml`.
- zos-wm (anvil fork) at `/var/home/zach/github/zOS/zos-wm/`.
- Smithay (for reference on what Niri reuses vs implements): `https://github.com/Smithay/smithay` @ `ff5fa7df392cecfba049ffed55cdaa4e98a8e7ef`.
- xwayland-satellite (invoked by Niri's satellite integration): `https://github.com/Supreeeme/xwayland-satellite`.
- Mutter (origin of `guess_monitor_scale` and the X11 socket-permissions dance): `https://gitlab.gnome.org/GNOME/mutter`.
- GPL-3.0 text: `/tmp/niri-peek/LICENSE`.
- License-compatibility analysis: author's reading of GPL-3.0 §5 (conveying modified source) and §6 (conveying non-source). No legal counsel consulted; if zos-wm distribution posture changes, consult counsel before committing to Option B or D.
