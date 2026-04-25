# Phase 2.FIX — Qt CSD window-control investigation

Date: 2026-04-24
Scope: zos-wm (Smithay/anvil fork) vs. Hyprland, Qt 6 Dolphin client

## TL;DR

- **Root cause is not H1 (missing KDE decoration protocol).** Qt 6's QtWayland plugin (in `qtbase`) does **not** use `org_kde_kwin_server_decoration_manager` at all; Qt only uses `zxdg_decoration_manager_v1`. plasma-integration's Qt6 `KWaylandIntegration` only binds `org_kde_kwin_server_decoration_palette` (for color schemes), not the decoration-mode protocol itself. See `/tmp/kwayland.cpp` (cloned from KDE/plasma-integration@master, qt6/src/platformtheme/kwaylandintegration.cpp).
- **Root cause is most likely H3 (Qt's own SSD request races past anvil's CSD default).** On startup Qt **unconditionally calls `set_mode(server_side)`** on every toplevel that has an `xdg_decoration` object attached (qtbase `qwaylandxdgshell.cpp:210`). anvil's `request_mode` handler in `src/state.rs:496-508` honours that request: it flips the toplevel's `decoration_mode` to `Mode::ServerSide` and sends a fresh configure. That means Dolphin on zos-wm is actually negotiating **SSD**, not CSD, and Qt's `wantsDecorations()` then returns `false` — no bradient titlebar is ever created. The "Mode::ClientSide default" in `new_decoration` is irrelevant because Qt overrides it immediately.
- **H4 (SSD-render bleed-through) is secondary.** Even after the `decoration_state().is_ssd` gate is restored, Dolphin's `wantsDecorations()` will still be `false` because negotiation landed on SSD; you'll just stop painting anvil's placeholder rectangles. You will **not** get Qt bradient back.
- Recommended fix: stop honouring `request_mode(ServerSide)` from clients until zos-wm has a real SSD renderer. Force the reply to always be `Mode::ClientSide`. Qt will then skip SSD, fall through to bradient, and Dolphin gets visible min/max/close buttons.
- Bonus: wiring up `org_kde_kwin_server_decoration_manager` (Smithay has built-in support) is cheap and polite (some niche Qt5/Plasma helpers still poke it), but it will **not** fix Dolphin's controls on its own.

## Symptom

zos-wm nested inside a host compositor (NVIDIA+AMD box, 3× 1080p60). Launch Dolphin (Qt 6, KDE Frameworks 6). Result:

- No Qt-drawn titlebar, no min/max/close buttons.
- Only visible "frame" is anvil's SSD placeholder — two solid-colored rectangles painted unconditionally (separate bug, being fixed in parallel by re-gating on `decoration_state().is_ssd`).

Comparison on the same machine: Hyprland + same Dolphin binary. Visible result is either (a) a thin coloured Hyprland border with no titlebar buttons, or (b) a full Breeze-style titlebar when the `hyprbars` plugin is loaded. The user's report of "Dolphin renders its own visible titlebar with window controls" likely corresponds to `hyprbars` being active or to Dolphin-specific toolbar (not true CSD buttons) — Hyprland's upstream default decoration flow is strictly SSD (see below), so unmodified Hyprland should **not** show Qt bradient either. This reframes the question: the goal is to make zos-wm show **either** a zos-wm-drawn SSD titlebar **or** Qt's bradient CSD fallback; right now it shows neither.

## Hypothesis testing

### H1 — KDE server-decoration protocol (`org_kde_kwin_server_decoration_manager`)

**Status: mostly rejected.**

- Smithay **does** provide it: `src/wayland/shell/kde/decoration.rs` (module doc explicitly calls it "KDE's legacy server-side decorations"). anvil/zos-wm does **not** wire it up — no `KdeDecorationHandler`, no `KdeDecorationState`, no `delegate_kde_decoration!` anywhere in `/var/home/zach/github/zOS/zos-wm/src/` (confirmed by grep).
- Hyprland **does** wire it up: `src/protocols/ServerDecorationKDE.cpp` advertises the global and always replies `SERVER` mode.
- **However:** Qt 6's QtWayland platform plugin (source tree: `qtbase/src/plugins/platforms/wayland/plugins/shellintegration/xdg-shell/`) does **not** reference `org_kde_kwin_server_decoration` at all. I pulled the full file list via `api.github.com/repos/qt/qtbase/git/trees/dev?recursive=1` and filtered for `kde` + `wayland`: zero hits. It only uses `zxdg_decoration_manager_v1` (see `qwaylandxdgdecorationv1.cpp`).
- The plasma-integration platform theme plugin (`KDE/plasma-integration` master, `qt6/src/platformtheme/kwaylandintegration.cpp`, 166 lines total) only binds `org_kde_kwin_server_decoration_palette_manager` to ship the Breeze palette to server-side decorators. It never binds `org_kde_kwin_server_decoration_manager`.
- Conclusion: wiring up KDE server-decoration in zos-wm is good hygiene (it's a two-line delegate-macro call) but will not move Dolphin's bradient one pixel. The protocol's real audience in 2026 is GTK apps and a few legacy Plasma bits, not modern Qt6.

### H2 — Qt decoration plugin env var / selection

**Status: relevant, not root cause.**

Qt's decoration plugin selection is in `qtbase/src/plugins/platforms/wayland/qwaylandwindow.cpp::createDecoration()` (browsed via codebrowser.dev, lines 1084-1210). Decision tree:

1. `mShellSurface->wantsDecorations()` must be true, else **no plugin is loaded** (line 1113).
2. If `QT_WAYLAND_DECORATION` env is set and matches a registered plugin, use it.
3. Otherwise: on GNOME DEs pick `adwaita`/`gnome`; on other DEs remove those and pick the first remaining key. In a stock Qt6 install the surviving key is **bradient** (ships in `qtbase/src/plugins/platforms/wayland/plugins/decorations/bradient/`, confirmed present on qt/qtbase@dev). Note the bradient plugin has been *removed* from `qt/qtwayland@dev` but that's the compositor-side tree; it's **still alive on the client side in qtbase**.
4. bradient's `main.cpp` draws a full titlebar with minimize/maximize/close, see `codebrowser.dev/qt5/qtwayland/src/plugins/decorations/bradient/main.cpp.html` (Qt5 layout; Qt6 equivalent unchanged in spirit).

So Qt CAN draw CSD, but only if `wantsDecorations()` returns `true`. That leads us to H3.

`QT_WAYLAND_DECORATION=bradient` **can** be used as a diagnostic override, but only if `wantsDecorations()` is already true — the env var selects among plugins, it does not bypass the gate at step 1.

### H3 — Smithay's xdg-shell / xdg-decoration configure flow

**Status: confirmed root cause.**

`QWaylandXdgSurface::Toplevel::wantsDecorations()` (qtbase `qwaylandxdgshell.cpp:133`):

```cpp
bool QWaylandXdgSurface::Toplevel::wantsDecorations()
{
    if (m_decoration && (m_decoration->pending() == QWaylandXdgToplevelDecorationV1::mode_server_side
                         || !m_decoration->isConfigured()))
        return false;
    return !(m_pending.states & Qt::WindowFullScreen);
}
```

It returns **false** whenever (a) the decoration object has been configured to `server_side`, or (b) the decoration object exists but has not yet received any configure. The Toplevel creates `m_decoration` eagerly in its constructor whenever the compositor advertises `zxdg_decoration_manager_v1` (line 43-45), and immediately after binding it calls `requestMode(mode_server_side)` (line 210).

zos-wm's `XdgDecorationHandler::request_mode` (`/var/home/zach/github/zOS/zos-wm/src/state.rs:496`):

```rust
fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecorationMode) {
    toplevel.with_pending_state(|state| {
        state.decoration_mode = Some(match mode {
            DecorationMode::ServerSide => Mode::ServerSide,
            _ => Mode::ClientSide,
        });
    });
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}
```

It dutifully grants `ServerSide` when the client asks for it. Qt always asks for it. So every Qt toplevel on zos-wm lands on `ServerSide`, `wantsDecorations()` returns `false`, bradient is never instantiated, and the only thing ever painted "around" the window is anvil's own SSD placeholder.

The `new_decoration` handler that sets `Mode::ClientSide` as a default is a red herring: it fires when the client first creates the decoration object but is immediately overwritten by the `request_mode` call that Qt sends on the same round-trip. By the time the initial configure ship-sails, `decoration_mode` is `ServerSide`.

### H4 — anvil's SSD render bleed-through

**Status: a real bug, but not the cause of the missing buttons.**

The SSD placeholder is drawn in `zos-wm/src/shell/element.rs:432` gated on `decoration_state().is_ssd`. `is_ssd` is set from the configure ACK in `zos-wm/src/shell/xdg.rs:254-259`:

```rust
let is_ssd = configure.state.decoration_mode.map(|m| m == Mode::ServerSide).unwrap_or(false);
window.set_ssd(is_ssd);
```

After the gate is restored (the parallel fix), if Qt ended up on `ServerSide` (which it does today), `is_ssd` will be **true** and anvil will still paint the placeholder. Dolphin will still have no buttons. Fixing only H4 makes Dolphin look *emptier*, not *better*: a bare rectangle with no frame at all.

## Recommended fix path

**One-line change + one-function policy flip in `zos-wm/src/state.rs`:**

```rust
impl<BackendData: Backend> XdgDecorationHandler for AnvilState<BackendData> {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        use xdg_decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        toplevel.with_pending_state(|state| { state.decoration_mode = Some(Mode::ClientSide); });
    }
    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        // Until zos-wm has a real SSD renderer (titlebar widget, buttons, hit-testing),
        // always refuse SSD requests. Clients that can draw their own decorations
        // (Qt/bradient, GTK, Firefox, Electron) will. Clients that can't will
        // draw nothing, which is no worse than anvil's placeholder today.
        use xdg_decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        toplevel.with_pending_state(|state| { state.decoration_mode = Some(Mode::ClientSide); });
        if toplevel.is_initial_configure_sent() { toplevel.send_pending_configure(); }
    }
    fn unset_mode(&mut self, toplevel: ToplevelSurface) { /* unchanged: ClientSide */ }
}
```

Expected behaviour after this change (with H4 also fixed):

1. Qt asks SSD, zos-wm says "nope, CSD". Qt's `m_decoration->pending() == mode_client_side`, `wantsDecorations()` → **true**.
2. `createDecoration()` runs, picks `bradient` (no GNOME detected in zos-wm), Dolphin now has a blue-gradient titlebar with min/max/close buttons.
3. `is_ssd` stays `false`, anvil does not paint its placeholder — no visual double-draw.

Follow-up work (not blocking Phase 2.FIX):

- Add `KdeDecorationState::new::<Self>(&dh, DefaultMode::Client)` + `delegate_kde_decoration!` so GTK apps that probe the KDE protocol also end up on CSD. ~5 LoC, file `src/state.rs`. See Smithay doc at `/tmp/smithay-peek/src/wayland/shell/kde/decoration.rs:1-80` for the exact pattern.
- When zos-wm grows a real titlebar renderer, flip the policy: default to SSD, respect client requests for CSD. At that point the Hyprland-style "always SERVER" stance becomes viable.

## Testing instructions for the user

After applying the fix, in a nested zos-wm session:

```sh
QT_LOGGING_RULES='qt.qpa.wayland.*=true' WAYLAND_DEBUG=1 dolphin 2>&1 | tee /tmp/dolphin-deco.log
```

Look for these lines in `/tmp/dolphin-deco.log`:

- `qt.qpa.wayland: Using decoration plugin "bradient"` — confirms step 1 above. If you see `"adwaita"` or `"gnome"`, something set `XDG_CURRENT_DESKTOP=GNOME`, which affects plugin selection (`qwaylandwindow.cpp:1144`).
- `WAYLAND_DEBUG`: `-> zxdg_toplevel_decoration_v1@<id>.set_mode(2)` — `2` means `server_side`, which Qt still sends.
- Next line: `<- zxdg_toplevel_decoration_v1@<id>.configure(1)` — `1` means `client_side`, which is what our patched `request_mode` now forces. This is the key handshake.
- Dolphin should paint a bradient titlebar: flat blue/grey gradient, window title centred, three buttons top-right.

If bradient still doesn't appear, check:

- `echo $QT_WAYLAND_DECORATION` — if set to something invalid, Qt warns and falls back but may skip.
- `dnf list installed qt6-qtwayland` — the plugin ships in `qt6-qtwayland` on Fedora (bradient binary at `/usr/lib64/qt6/plugins/wayland-decoration-client/libbradient.so`). If the package is missing the plugin list is empty and `createDecoration` sets `decorationPluginFailed = true`.
- `Qt::FramelessWindowHint` on the window (rare for Dolphin).

Secondary verification: run `weston-terminal` (uses libweston, also asks SSD by default via xdg-decoration). After the fix it should also fall back to its own CSD.

## Sources

Retrieved 2026-04-24.

Local (this repo):
- `/var/home/zach/github/zOS/zos-wm/src/state.rs:488-521` — current `XdgDecorationHandler` impl.
- `/var/home/zach/github/zOS/zos-wm/src/shell/xdg.rs:248-261` — configure ACK → `is_ssd` mirror.
- `/var/home/zach/github/zOS/zos-wm/src/shell/element.rs:432-437` — SSD placeholder render path.
- `/var/home/zach/github/zOS/zos-wm/src/shell/ssd.rs:270-293` — `decoration_state()` / `set_ssd()`.
- `/var/home/zach/github/zOS/zos-wm/src/input_handler.rs:1293,1325` — `ToggleDecorations` keybind (debug only).

Smithay (cloned at `/tmp/smithay-peek`, Smithay/smithay@main):
- `src/wayland/shell/kde/decoration.rs:1-80` — full KDE decoration handler trait and module doc.
- `src/wayland/shell/xdg/` — xdg-shell + xdg-decoration implementation.

Hyprland (cloned at `/tmp/hyprland-peek`, hyprwm/Hyprland@main):
- `src/protocols/XDGDecoration.cpp:39-49` — `xdgDefaultModeCSD()` returns `SERVER_SIDE`.
- `src/protocols/ServerDecorationKDE.cpp:31-59` — KDE default mode also `SERVER`.

Qt 6 (qt/qtbase@dev, via api.github.com tree listing + codebrowser.dev):
- `src/plugins/platforms/wayland/qwaylandwindow.cpp:1084-1200` — `createDecoration()` body. Key gate: line 1113 `if (!mShellSurface || !mShellSurface->wantsDecorations()) decoration = false;`. Plugin selection: line 1131-1157 (bradient fallback on non-GNOME). `https://codebrowser.dev/qt6/qtbase/src/plugins/platforms/wayland/qwaylandwindow.cpp.html`.
- `src/plugins/platforms/wayland/plugins/shellintegration/xdg-shell/qwaylandxdgshell.cpp:133-140` — `wantsDecorations()` returns false when pending mode is `server_side` or decoration not yet configured.
- `src/plugins/platforms/wayland/plugins/shellintegration/xdg-shell/qwaylandxdgshell.cpp:210` — `m_decoration->requestMode(mode_server_side)` sent unconditionally at toplevel creation.
- `src/plugins/platforms/wayland/plugins/shellintegration/xdg-shell/qwaylandxdgdecorationv1.cpp:40-55` — `requestMode` implementation.
- `src/plugins/platforms/wayland/plugins/decorations/bradient/main.cpp` — bradient plugin (still present on Qt6 client side).

plasma-integration (KDE/plasma-integration@master, fetched via raw.githubusercontent):
- `qt6/src/platformtheme/kwaylandintegration.cpp:1-166` — only binds `org_kde_kwin_server_decoration_palette`, **not** `_manager`. Confirms Qt6 KDE theme does not intercept decoration mode selection.

Qt docs:
- `https://doc.qt.io/qt-6/qwaylandxdgdecorationmanagerv1.html` — compositor-side companion docs; explains that `preferredMode` is only a hint.

Perplexity (Sonar Pro) web grounding on Qt decoration selection, Fedora/Arch package naming for plasma-integration (2025 dates). Accessed 2026-04-24.
