# ZOS_DELME — zOS rewrite handoff

**Purpose.** Single-doc snapshot of the in-flight zOS-compositor + zos-ui-framework rewrite. A fresh agent (or me in a fresh session) reading this should know what's done, what's pending, what files matter, what NOT to touch, and how to keep moving.

**Delete this file once Phase 8 ships.** Until then, treat it as the authoritative current-state document.

---

## TL;DR — where we are

| Area | Status |
|---|---|
| Phase 0 — image fixes (NVIDIA conf, polkit, sysrq, etc.) | **Shipped to main** (commit `faf85d3`) |
| Phase 1 — `zos-wm` scaffold + dev env | **Done.** Nested winit compositor runs via `just dev-wm` |
| Phase 2 — protocols & backends | **Done.** XWayland, foreign-toplevel-list, screencopy (winit + udev shm), 5 QoL protocols, real Catppuccin titlebar, KDE decoration, plus 3 from-scratch protocols (tearing-control, gamma-control, wlr-output-management) with udev modeset apply. Build/install + greetd session entry shipped. udev backend audit at ~80% with v1 caveats noted. |
| Phase 2.FIX — SSD + decoration handling | **Done.** Real SVG-icon titlebar (min/max/close) with Catppuccin theme + click-to-unmap minimize |
| Phase 3 — floating-first window mgmt | **Next.** |
| Phase 4 — animations/polish | Not started |
| Phase 5 — `zos-ui` Vue-for-Rust framework | Not started |
| Phase 6 — shell apps on `zos-ui` | Not started |
| Phase 7 — plugin architecture | Not started |
| Phase 8 — image swap (Hyprland → zos-wm in zOS) | Not started |

**Active plan file** (canonical): `~/.claude/plans/hey-how-do-i-jiggly-balloon.md`. Kept up to date phase-by-phase.

---

## Project context — non-negotiables

These are decisions LOCKED IN by the user. Do not propose alternatives.

1. **Build our own Wayland compositor.** Forked Smithay anvil. Not switching to Niri or COSMIC.
2. **Build our own Rust UI framework, `zos-ui`** — Vue-for-Rust DX (template macro, fine-grained signals, scoped CSS, `<Component />` composition). Not adopting Slint, Astal, GTK, or any web-stack-disguised-as-native.
3. **Floating-first window model.** Tiling is opt-in per workspace/window, never forced.
4. **Develop in nested winit on the daily-driver box.** Don't switch the live session to zos-wm until Phase 8.
5. **Heavy parallel agent execution.** Per-phase: research-fan-out (general-purpose, opus, foreground) → implementation-fan-out (rust-expert, opus, foreground), each agent ONE small file/function-scoped task.
6. **Save research artifacts to `docs/research/`** so they're durable across sessions.
7. **Keep current Hyprland stack installed in the image** until Phase 8. Daily driver remains usable while we build.

---

## Repo layout (workspace crates)

| Crate | Purpose | Status |
|---|---|---|
| `zos-core` | Shared library (commands, IPC, system ops). Hosts `commands::grub::reboot_to_windows_elevated`, etc. | Stable, used in production |
| `zos-cli` | `zos` CLI + TUI | Stable |
| `zos-settings` | iced GUI for system settings | Stable; needs port to `zos-ui` in Phase 5 |
| `zos-dock` | iced layer-shell app dock | Stable; needs port in Phase 5 |
| `zos-daemon` | Tray + udev listener | Stable |
| `zos-wm` | **Wayland compositor** — fork of Smithay anvil at commit `27af99ef492ab4d7dc5cd2e625374d2beb2772f7` | **Active development** — Phase 2 in progress |

Future crates (NOT YET CREATED): `zos-ui`, `zos-ui-macros`, `zos-ui-core`, `zos-panel`, `zos-power`, `zos-monitors`, `zos-notify`. All Phase 5/6.

---

## What's been done so far in zos-wm — explicit file inventory

### Crate scaffold (Phase 1)
- `zos-wm/Cargo.toml` — fork pinned to Smithay rev `27af99ef`. Default features `["winit", "xwayland"]`. udev feature available but blocked on `libseat-devel` for local builds (now added to Containerfile, see below).
- `zos-wm/src/` — copy of anvil source, unmodified except where noted.
- `zos-wm/NOTICE` — MIT attribution to upstream Smithay.

### Init-time fixes (Phase 2 Batch 1)
- `zos-wm/src/main.rs` — `unsafe { std::env::remove_var("GBM_BACKEND"); }` as first statement of `main()`. Strips inherited NVIDIA env that breaks compositor GBM init.
- `zos-wm/src/state.rs` — 5 new Smithay protocol delegates wired up: `cursor-shape-v1`, `idle-inhibit-v1`, `idle-notify-v1`, `session-lock-v1`, `pointer-warp-v1`. New fields on `AnvilState`: `idle_inhibit_manager_state`, `idle_notifier_state`, `idle_inhibitors`, `session_lock_manager_state`, `is_locked`. Delegate count went 33 → 38 → 39 (with foreign-toplevel-list).
- `zos-wm/Cargo.toml` (`udev` feature) — `smithay/backend_vulkan` removed (unnecessary on NVIDIA GLES path, reduces compile time).

### Decoration handler (Phase 2.FIX) — full real-titlebar rebuild
- `zos-wm/src/shell/ssd.rs` — completely rewritten from anvil's pastel-rect placeholder. Now:
  - Three buttons (minimize, maximize, close) — anvil only had 2.
  - SVG icons rasterized via `usvg` + `resvg` + `tiny-skia` at HeaderBar init. Source SVGs are `const &str` in the file.
  - Catppuccin Mocha palette (BG `#1e1e2e`, button bg `#24273a`, icon `#7f849c` normal / `#cdd6f4` hover, close-hover bg `#f38ba8` red).
  - 6 icon buffers pre-rasterized (3 icons × {normal, hover}) — no per-frame raster cost.
  - Minimize click → `state.space.unmap_elem(window)` (window disappears; restoration is Phase 3 work).
  - Maximize/close click handlers unchanged from anvil.
- `zos-wm/Cargo.toml` — added `usvg = "0.45"`, `resvg = "0.45"`, `tiny-skia = "0.11"` (all `default-features = false`).
- `zos-wm/src/shell/element.rs` — `WindowRenderElement` macro extended with `MemoryDecoration` variant (so `MemoryRenderBufferRenderElement` can flow through anvil's render pipeline). `+ Send` bound added on `R::TextureId`.
- `zos-wm/src/render.rs` — `+ Send` bound cascaded through `space_preview_elements`, `output_elements`, `render_output` helpers.
- `zos-wm/src/state.rs` — `XdgDecorationHandler::request_mode` honors client's request again (default CSD; honor explicit SSD ask). User-visible note: with our real titlebar this works correctly now.

### XWayland (Phase 2)
- `zos-wm/Cargo.toml` — `default = ["winit", "xwayland"]`. Anvil's existing XWayland integration code (`shell/x11.rs`) lights up. `Xwayland` binary present on Bazzite (no system-side install needed).

### `ext-foreign-toplevel-list-v1` (Phase 2)
- `zos-wm/src/state.rs` — added `foreign_toplevel_list_state: ForeignToplevelListState` field, `ForeignToplevelListHandler` impl (just the state getter), `delegate_foreign_toplevel_list!` macro.
- `zos-wm/src/shell/xdg.rs` — lifecycle hooks in `XdgShellHandler`:
  - `new_toplevel`: creates handle, stashes in `window.user_data()`.
  - `toplevel_destroyed`: removes handle from `ForeignToplevelListState`.
  - `title_changed` + `app_id_changed`: forwards to `handle.send_*` + `send_done`.

### Screencopy / `ext-image-copy-capture-v1` (Phase 2)
- `zos-wm/src/screencopy.rs` — NEW module (290 LoC). One-shot shm capture path: enqueue request → drain at end-of-frame → render-to-texture → `copy_framebuffer` + `map_texture` + `ptr::copy_nonoverlapping` into client shm. ARGB8888 + XRGB8888.
- `zos-wm/src/state.rs` — `ImageCopyCaptureHandler::frame` un-stubbed (was `frame.fail()`); now enqueues into `pending_screencopy` Vec.
- `zos-wm/src/winit.rs` — render path refactored to extract `output_elements` from the `bind().and_then(...)` closure so the element list is reachable post-render. Added `screencopy::drain_pending_for_output` call.
- `zos-wm/src/udev.rs` — fail-all stub for udev path so pending requests don't accumulate. TODO(screencopy-udev) marked.
- `zos-wm/src/lib.rs` — `pub mod screencopy;`.

**Deferred screencopy work (TODOs in code):**
- `TODO(screencopy-udev)` — udev path needs full implementation; equivalent to winit but via `MultiRenderer`.
- `TODO(screencopy-dmabuf)` — dmabuf buffers rejected via `BufferConstraints` today.
- `TODO(screencopy-toplevel)` — toplevel-source captures (vs. output-source) not supported.
- `TODO(screencopy-streaming)` — only one-shot per request.

### Other touchpoints
- `zos-wm/src/shell/mod.rs` — `pub(crate) mod ssd;` (restored after a bad cleanup).
- `Cargo.toml` (workspace) — `members` includes `"zos-wm"`.
- `Justfile` — `dev-wm` recipe uses bash-shebang form with `RUST_LOG=info,smithay::backend::egl::ffi=off cargo run -p zos-wm -- --winit` (filters cosmetic NVIDIA EGL log line).
- `HACKING.md` — top-level dev notes.

### Image build deps (just added)
- `Containerfile` — Rust build layer's `dnf5 install` line now includes `libseat-devel mingw64-gcc mingw64-binutils mingw64-headers mingw64-crt`. libseat-devel = required to compile zos-wm with `udev` feature in CI/image builds. mingw64 = Windows cross-compile toolchain (for any future Rust → .exe targets).
- `build_files/scripts/install-hyprland.sh` — added DO-NOT-REMOVE banner at top of file. Spells out: keep hyprland, hyprpanel, hyprshell, wlogout, nwg-displays, hypridle, hyprlock, hyprpaper, hyprpolkitagent until Phase 8.

---

## Memory — important context for future sessions

`~/.claude/projects/-var-home-zach-github-zOS/memory/MEMORY.md` is the index. Key entries:

| Memory | Why it matters |
|---|---|
| `feedback_aq_drm_devices.md` | NEVER set `AQ_DRM_DEVICES` in hyprland.conf — colons in by-path values shred the env var and kill the greeter |
| `feedback_dont_rip_on_ambiguity.md` | When user says "we handle that" — investigate why the related code isn't visible/working, do NOT delete it |
| `feedback_no_compositor_swap_trial.md` | Don't propose "daily-drive Niri for a weekend to try" — zOS box is the real daily driver |
| `feedback_parallel_agent_workflow.md` | Multi-phase work pattern: parallel research agents → parallel rust-expert agents, one small task each |
| `feedback_cargo_add.md` | Use `cargo add` instead of editing Cargo.toml manually |
| `feedback_greeter.md` | Don't change greetd config to use `start-hyprland` — breaks greeter |
| `project_compositor_direction.md` | zos-wm = Smithay anvil fork, floating-first, steal-from-niri-as-needed |
| `project_zos_ui_framework.md` | zos-ui = Vue-for-Rust framework with `view!` + `style!` + `#[component]` macros + signals |
| `project_zos_ui_icons.md` | `<Icon name="mdi:home"/>` resolves via Iconify, build-time embed, usvg/resvg render |
| `project_audio_routing_model.md` | Audio settings page = Voicemeeter Potato model |
| `project_hardware_zos_box.md` | Zach's daily driver hardware (RTX 4090 + 9950X3D + 3× 1080p + Razer Naga Pro) |

---

## Research docs — durable artifacts

`/var/home/zach/github/zOS/docs/research/` is the canonical location.

| File | Topic | Lines |
|---|---|---|
| `phase-2-a-protocol-priority.md` | Wayland protocol ranked priority — what's already in anvil, what to wire next, what to skip | 309 |
| `phase-2-b-niri-reusable-code.md` | Niri code worth studying (NOT lifting — Niri is GPL, zos-wm is MIT). License analysis included | 279 |
| `phase-2-c-drm-nvidia-specifics.md` | DRM+udev backend + NVIDIA proprietary 580 specifics, Bazzite session config, pre-flight checklist | 698 |
| `phase-2-fix-decoration-investigation.md` | Why Qt-on-Wayland CSD doesn't show controls automatically; root cause analysis | 190 |

Each doc starts with a TL;DR + has a Sources section with URLs and commit SHAs. Read them before doing related work.

Earlier landscape brief is at `~/.claude/plans/hey-how-do-i-jiggly-balloon-agent-af0347fe8dea34f56.md` (Hyprland NVIDIA shipping config from Phase 0).

---

## What's still to do — concrete, ordered

### Phase 2 remainder — ALL DONE except small deferred items below

| # | Task | Status |
|---|---|---|
| 2.A | NVIDIA overlay-plane quirk | **Already in code** — `udev.rs:1006-1014` (anvil's pre-existing check, equivalent to cosmic-comp's `planes.overlay.clear()`) |
| 2.B | wp-tearing-control-v1 from-scratch | **Done.** `protocols/tearing_control.rs`. Render-path scaffold + TODO; real async pageflip blocked on smithay PR (FrameFlags lacks ALLOW_TEARING bit) |
| 2.C | wlr-gamma-control-v1 from-scratch | **Done.** `protocols/gamma_control.rs`. Reads LUT from client fd, stores per-output. DRM apply via `TODO(gamma-drm-apply)`. |
| 2.D | wlr-output-management | **Done.** `protocols/output_management.rs`. Lifecycle hooks in udev/winit. udev `apply_output_config` does real DRM modeset. v1 caveats: position/scale/transform/disable don't reach Space (Backend trait would need `&mut Space` extension — task 2.D.5). |
| 2.E | Real udev DRM backend integration | **Audit done + 4 polish items shipped.** ~80% ready for first TTY boot per `docs/research/phase-2-udev-gaps.md`. ZOS_RENDER_DEVICE / ZOS_DISABLE_SYNCOBJ env vars wired. Better diagnostics. |
| 2.F | Screencopy udev path | **Done (shm only).** `screencopy.rs` generic over `Renderer + Bind<T> + Offscreen<T> + ExportMem`. Dmabuf still rejected — task 2.F.2. |
| 2.G | KDE server-decoration | **Done.** Smithay built-in delegate wired with `Mode::Client` default. |
| U-1 to U-8 | Build/install + session integration + udev polish | **All done.** Containerfile builds zos-wm, Justfile recipe added, zos-wm.desktop + start-zos-wm landed. |

### Deferred follow-ups (all small, all tracked)

| # | Task | Effort |
|---|---|---|
| 2.B-async | Real async pageflip for tearing | Upstream smithay PR. Local shim alternative documented in `docs/research/phase-2-tearing-control.md` |
| 2.C-drm | DRM gamma LUT application | 1-2h. Wire `DrmDevice::set_gamma` from udev once stored LUT changes |
| 2.D.4 | winit `Resized` → `notify_changes` | ~5min, dev convenience |
| 2.D.5 | Backend trait extension for Space access | 1-2h. Currently apply_output_config lies about position/scale/transform success |
| 2.F.2 | Dmabuf path for screencopy | 2-3h. Add dma format list + zero-copy export |
| 2.E.U-9 | VT-switch resilience (`activate(true)`) | 5min when first VT-switch corruption shows up |

### Phase 3 — floating-first window management
Per plan file `~/.claude/plans/hey-how-do-i-jiggly-balloon.md` PHASE 3 section. ~4-6 days. Major touchpoints: `zos-wm/src/shell/element.rs`, `zos-wm/src/state.rs` (workspace + window stack data), `zos-wm/src/input_handler.rs` (Super+drag, Super+right-drag).

Includes proper minimize restoration (currently minimize button just unmaps). Workspaces, smart placement, tiling-as-opt-in.

### Phase 4 — animations + polish
Bezier curves, window slide-in/out, blur (opt-in), rounded corners (already in render path needs polish), shadows.

### Phase 5 — `zos-ui` framework — biggest single effort
Per plan PHASE 5 section. NEW crates:
- `zos-ui` (the public API)
- `zos-ui-macros` (proc-macros: `#[component]`, `view!`, `style!`)
- `zos-ui-core` (signals, reactive graph, render substrate)

Substrate: iced + `iced_layershell` for v1. Designed to allow Vello swap later.
Reactivity: either `leptos_reactive` standalone or roll our own ~500 LoC graph (research agent picks at Phase 5 start).
Template macro: built on `syn` + `rstml`.

After Phase 5 lands, refactor `zos-settings` and `zos-dock` to consume `zos-ui` (sequential edits, not agent fan-out).

### Phase 6 — shell apps
- `zos-panel` — replaces HyprPanel (clock, workspaces, tray, audio, network, bluetooth, power)
- `zos-power` — replaces wlogout
- `zos-monitors` — replaces nwg-displays
- `zos-notify` — replaces swaync (optional, deferrable to Phase 8)

### Phase 7 — plugin/extension architecture
IPC for external panels (already works via the protocols). In-process trait-based for render-loop extensions (animations, custom layouts). WASM is v2.

### Phase 8 — image swap-over
Containerfile changes, greetd config, install-zos-wm.sh, drop the `DO NOT REMOVE` banner from install-hyprland.sh, finally remove HyprPanel/wlogout/nwg-displays from the image. Daily-driver validation period before Hyprland is dropped.

---

## How to test what's currently shipped

```bash
just dev-wm                                # launches nested winit zos-wm
                                           # filters cosmetic EGL log spam

# In another terminal — substitute the socket from zos-wm's startup log:
export WAYLAND_DISPLAY=wayland-2

# Wayland-native tests
WAYLAND_DISPLAY=$WAYLAND_DISPLAY dolphin   # KDE file manager — see real titlebar
WAYLAND_DISPLAY=$WAYLAND_DISPLAY weston-terminal  # if installed; basic surface test

# XWayland tests (xwayland feature now default)
WAYLAND_DISPLAY=$WAYLAND_DISPLAY firefox &
WAYLAND_DISPLAY=$WAYLAND_DISPLAY xeyes     # classic X11 sanity

# Screencopy test
WAYLAND_DISPLAY=$WAYLAND_DISPLAY grim /tmp/zos-capture.png   # one-shot screenshot
xdg-open /tmp/zos-capture.png

# Foreign-toplevel-list test
WAYLAND_DISPLAY=$WAYLAND_DISPLAY wayland-info | grep foreign_toplevel
# Should show: interface: 'ext_foreign_toplevel_list_v1', version: 1
WAYLAND_DISPLAY=$WAYLAND_DISPLAY wlrctl toplevel list   # if wlrctl installed

# Window controls (titlebar)
# Hover over the three buttons in the dark titlebar of any window —
# they should highlight (overlay-gray icons go bright white; close-hover turns red)
# Click minimize → window disappears (no restore UI yet)
# Click maximize → toggles maximize
# Click close → window closes
```

For release-mode performance testing: `cargo run -p zos-wm --release -- --winit`. Significantly faster scrolling, no debug-build penalty.

For the full image rebuild + rebase flow: `git push` to forgejo → CI builds → `rpm-ostree rebase ostree-image-signed:docker://ghcr.io/blackleafdigital/zos-nvidia:latest` → reboot.

---

## Critical gotchas to remember

1. **Don't run `rpm-ostree usroverlay` lightly** — it makes /usr writable until next reboot but reverts on image update. Fine for testing image-side changes locally before pushing.
2. **`AQ_DRM_DEVICES` must NEVER be set in hyprland.conf** — colons in by-path values break parsing, kill greeter. Memory `feedback_aq_drm_devices.md` documents this.
3. **`hyprpolkitagent` lives at `/usr/libexec/hyprpolkitagent`**, not in `$PATH`. The autostart line was wrong before; now it uses full path. Future check: any new daemon needs full path in autostart.conf if it's in /usr/libexec.
4. **OCI layer push semantics**: only changed blobs upload. But Bazzite base `:stable` rolls daily, so first rebase after a CI run that coincided with a base refresh = always a big download. Subsequent rebases = small.
5. **Containerfile bind-mount cache**: editing any file in `build_files/` invalidates all 4 RUN steps' cache because they all bind-mount the whole tree. Future cleanup: per-script contexts. Not urgent.
6. **NVIDIA + winit nested EGL_BAD_SURFACE log line is cosmetic.** First-frame `eglQuerySurface(BUFFER_AGE)` fires before any make-current. Filtered via `RUST_LOG=...smithay::backend::egl::ffi=off` in the dev-wm Justfile recipe.
7. **Niri is GPL-3.0 — DO NOT lift code verbatim.** Patterns and architecture are fair game; copying expressive code is a license incident. See `docs/research/phase-2-b-niri-reusable-code.md` for the full analysis.
8. **`shell/ssd.rs` was deleted in a bad cleanup, then restored.** Don't delete it. The real titlebar rendering lives there now.
9. **`libseat-devel` is required to compile zos-wm with the `udev` feature.** Just added to Containerfile so CI/image builds work. Local dev = `rpm-ostree install libseat-devel` (reboot required) OR distrobox/toolbox build.

---

## Agent workflow pattern (per memory `feedback_parallel_agent_workflow.md`)

For each phase:

1. **Research fan-out** — up to 3 general-purpose agents in parallel (foreground, model `opus`). Each gets ONE focused research question with citation requirements. Output → save to `docs/research/phase-X-Y.md` (NOT returned to main context — keeps context lean).
2. **Design consolidation** — me, no agents. Reads research docs, produces concrete task list.
3. **Implementation fan-out** — up to 3 rust-expert agents in parallel (foreground, model `opus`). Each touches **1-2 files maximum** per Zach's CLAUDE.md rule. NEVER give an agent a multi-file rewrite — break it down.
4. **Verification** — me. After every agent: read the files claimed-to-be-changed AND run `cargo check --workspace`. If anything's broken, fix manually rather than re-launching another agent on the same task.
5. **Phase gate** — Zach sanity-tests in the nested winit session. Next phase only starts after gate is cleared.

---

## How to resume in a new session

1. Read this file (`ZOS_DELME.md`).
2. Read the plan file: `~/.claude/plans/hey-how-do-i-jiggly-balloon.md`.
3. Read MEMORY.md and any project_/feedback_ memory entries.
4. Pick the next task from the "What's still to do" section above. Easy starts: `2.A` (cosmic-comp NVIDIA quirk port) or `2.G` (KDE server-decoration delegate). Bigger: `2.E` (udev backend) or Phase 5 (`zos-ui`).
5. Follow the agent workflow pattern.
6. Update this file when significant work lands.

---

## Current branch state

Phase 2 work being committed in this session in 3 logical chunks:
- `feat(zos-wm): phase 2 compositor crate (anvil fork + 4 from-scratch protocols + Catppuccin titlebar)` — entire `zos-wm/` crate + workspace `Cargo.{toml,lock}`
- `feat(image): zos-wm build + greetd session + DO-NOT-REMOVE banner` — Containerfile, Justfile, build_files/* glue
- `docs: phase 2 research + ZOS_DELME handoff` — docs/research/*, ZOS_DELME.md, HACKING.md

---

## Last session's frustrations (so future-me knows)

- I twice deleted SSD code thinking the user wanted it gone when they were just reporting a visible bug. Don't repeat. See `feedback_dont_rip_on_ambiguity.md`.
- I twice tried to fix the same EGL_BAD_SURFACE error with priming `bind()` calls before learning that `WinitGraphicsBackend::bind()` doesn't actually call `eglMakeCurrent` — the real make-current happens inside `GlesRenderer::render()`. Investigate root cause before reflex-patching.
- The user wants **opus 4.7 agents for everything** — even small tasks. Doesn't trust me to do anything Rust-y solo. Respect that.
