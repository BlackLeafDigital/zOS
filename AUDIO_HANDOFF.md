# zos-settings Audio Panel Rewrite — Handoff

**Status:** Foundation laid, implementation not yet started.
**Plan file:** `/home/zach/.claude/plans/breezy-stargazing-ember.md`
**Branch:** `main` (work directly on it per project conventions; create commits per task)

---

## Why this exists

The user has iterated on the zos-settings audio panel many times and each iteration produced a worse result. The goals never got met:

1. **Multi-output routing.** The user wants a Voicemeeter-style mixer where one input can be bound to multiple outputs and vice versa.
2. **Don't break audio output.** Current `Apply` flow kills sound for every running app.
3. **Icons must render.** Currently the EQ icon renders as a missing-image placeholder.
4. **Stop the iteration spiral.** Three generations of config schemas coexist; "fix audio" commits keep landing on top of broken layers.

The user explicitly asked "why GTK4? Hyprpanel has an infinitely better UI." Answer: **the toolkit is not the problem**. The problem is the implementation chose single-select widgets and a scalar `String` data model for something that needs multi-select and a `Vec`. Switching frameworks would mean rewriting all 9 settings panels (Display/Network/Power/Boot/etc. all work fine) and would not fix any of the actual bugs. We are staying on Relm4 + libadwaita + GTK4.

---

## Root cause findings (verified by reading source)

| # | File | Issue |
|---|---|---|
| 1 | `zos-settings/src/services/pipewire.rs:595` | `AudioBusConfig.physical_device: String` — scalar field. The data model literally cannot represent multi-output routing. |
| 2 | `zos-settings/src/pages/audio/mod.rs:311` | Per-bus output picker is `adw::ComboRow` (single-select). |
| 3 | `zos-settings/src/pages/audio/mod.rs:473` | Per-app routing picker is `gtk::DropDown` (single-select). |
| 4 | `zos-settings/src/services/pipewire.rs:848` | `apply_audio_bus_config` calls `systemctl --user restart pipewire`. This drops audio for every running app. |
| 5 | `zos-settings/src/services/pipewire.rs:852` | Hardcoded `sleep(500ms)` after the restart. Races the daemon's node publication AND freezes the GTK main thread. |
| 6 | `zos-settings/src/services/pipewire.rs:300-440` | `route_stream_to_sink` parses `pw-cli info <id>` text for `node.name` and substring-matches port names. Silently fails when `node.name` is missing. Has dead `stream_ports` block (lines 306-326) that's built and dropped without use. |
| 7 | `zos-settings/src/services/pipewire.rs:371-374` + `audio/mod.rs:497` | `wpctl set-default <stream_id>` is misused as a fallback. `set-default` operates on **sinks**, not streams — passing a stream id is a no-op at best. |
| 8 | `zos-settings/src/pages/audio/mod.rs:341` | Icon name `multimedia-equalizer-symbolic` is **not** in the Adwaita symbolic icon set. Renders as missing-image placeholder. |
| 9 | `zos-settings/src/services/pipewire.rs:537-600` | Three coexisting config schemas: `BusConfig`/`BusTarget`, `InputConfig`/`OutputConfig`, `AudioBusConfig`. Migration at lines 642-664 silently drops information by only keeping `inp.outputs.first()`. |

---

## The plan (approved)

User chose **(a)** full Voicemeeter strip-mixer rewrite and **(b)** migration to native `pipewire-rs` bindings. Both decisions are locked in.

### Scope

| In scope | Out of scope |
|---|---|
| `zos-settings/Cargo.toml` — add `pipewire = "0.8"` | The other 8 settings panels (Display, Network, Power, Boot, Dock, Input, Appearance, Overview) |
| `Containerfile` — add `clang-devel` and `pipewire-devel` to build deps | JACK / non-PipeWire audio stacks |
| **NEW:** `zos-settings/src/services/pipewire_native.rs` | |
| `zos-settings/src/services/mod.rs` — register the new module | |
| `zos-settings/src/services/pipewire.rs` — data model rewrite, dead code purge, apply path rewrite | |
| `zos-settings/src/pages/audio/mod.rs` — full UI rewrite as Voicemeeter strip mixer | |

### Visual reference

Pulsemeeter is the only Linux project that has actually cloned Voicemeeter's strip-and-bus-button UX. It's Python + GTK3, but the layout we want is identical:
https://github.com/theRealCarneiro/pulsemeeter

### UI layout (the user-approved mockup)

```
┌─────────────────────────────────────────────────┐
│ APP ROUTING                                     │
│ Firefox    [Main][Chat][Game][Music]            │
│ Discord    [Main][Chat][Game][Music]            │
│ Spotify    [Main][Chat][Game][Music]            │
├─────────────────────────────────────────────────┤
│  MAIN      CHAT       GAME       MUSIC          │
│  ▓          ▓          ▓          ▓             │
│  ▓          ▓          ▓          ▓             │  ← gain Scale
│  ▓          ▓          ▓          ▓             │
│  =          =          =          =             │
│  =          =          =          =             │  ← LevelBar (peak)
│  =          =          =          =             │
│ [HDMI][SPK][HDMI][SPK][HDMI][SPK][HDMI][SPK]    │  ← toggle buttons,
│ [HP] [USB][HP] [USB][HP] [USB][HP] [USB]        │     one per physical sink
│ [Mute]     [Mute]    [Mute]     [Mute]          │
└─────────────────────────────────────────────────┘
      [+ Add Bus]            [Apply]
```

The whole point: **toggle buttons, not dropdowns**. Each strip can route to many sinks simultaneously by clicking multiple buttons.

---

## What's been done

1. **`zos-settings/Cargo.toml`** — `pipewire = "0.8"` added (line 16). Verified pulls in `pipewire-sys 0.8.0`, `libspa 0.8.0`, `libspa-sys 0.8.0`, `bindgen 0.69.5`, `nix 0.27.1`, etc.
2. **`Containerfile`** — added `clang-devel` to the main `dnf5 install` line and `dnf5 remove` line for the Rust workspace build stage. **The user manually edited the Containerfile** to install `pipewire-devel` from the `copr:copr.fedorainfracloud.org:ublue-os:bazzite-multilib` COPR repo as a separate dnf5 call (line 24), and remove it separately (line 49). This is the bazzite workaround — see the gotcha section below.
3. **`/home/zach/.claude/plans/breezy-stargazing-ember.md`** — full plan written and approved.
4. **Verified `cargo check` passes on plain Fedora 43** in a podman container (took ~28s, no errors, only pre-existing dead-code warnings that will be deleted as part of this work).
5. **Task list created** — see "Task list" section below.

## What's NOT done

Everything else. No new code has been written. `pipewire_native.rs` doesn't exist yet. `pipewire.rs` and `audio/mod.rs` are unchanged. The icon is still wrong.

---

## The Bazzite `pipewire-libs` gotcha (CRITICAL — read this before touching the Containerfile)

Bazzite ships its own pinned PipeWire build:

```
pipewire-libs-1.4.10-1.fc43.bazzite.0.0.git.6949.3eccf0ec.x86_64
pipewire-1.4.10-1.fc43.bazzite.0.0.git.6949.3eccf0ec.x86_64
```

The upstream Fedora `pipewire-devel-1.4.X-1.fc43` package has an exact version dependency on `pipewire-libs(x86-64) = 1.4.X-1.fc43`, which the bazzite-suffixed package does NOT satisfy. dnf5 refuses the install with:

```
package pipewire-devel-1.4.10-1.fc43.x86_64 from updates-archive requires
  pipewire-libs(x86-64) = 1.4.10-1.fc43, but none of the providers can be installed
package pipewire-libs-1.4.10-1.fc43.x86_64 from updates-archive is filtered out by exclude filtering
```

**The user's workaround** (already applied in Containerfile line 24): install `pipewire-devel` from the `ublue-os/bazzite-multilib` COPR, which presumably ships a `pipewire-devel` rebuilt against the bazzite `pipewire-libs`. **The next agent should verify this COPR actually has `pipewire-devel` available** before assuming `just build` works:

```bash
podman run --rm ghcr.io/ublue-os/bazzite:stable bash -c \
  'dnf5 repoquery --repo=copr:copr.fedorainfracloud.org:ublue-os:bazzite-multilib pipewire-devel'
```

If the COPR doesn't have it, fallback options in priority order:

1. **rpm2cpio extract approach** — download upstream pipewire-devel rpm, extract just the headers and `.pc` files into `/usr/include` and `/usr/lib64/pkgconfig`, build, don't remove (they're only ~1.5 MB). Verified working in a podman test session.
2. **Override the bazzite repo priority** — use `--setopt=installonly_limit=0` plus repo priority manipulation.
3. **`rpm-ostree override replace`** — too invasive, modifies the final image.

### Local development (not Containerfile)

The user is on Fedora atomic so dev headers aren't installed by default. To build locally without layering anything, use podman:

```bash
podman run --rm \
  -v "$PWD":/work:Z \
  -w /work \
  registry.fedoraproject.org/fedora:43 \
  bash -c 'dnf install -y rust cargo gtk4-devel libadwaita-devel pipewire-devel clang-devel pkgconf && \
           CARGO_HOME=/tmp/cargo-home CARGO_TARGET_DIR=/tmp/cargo-target \
           cargo check -p zos-settings'
```

Plain Fedora 43 has none of the bazzite exclusion problems and is ~50% faster to set up than bazzite. Use it for fast iteration. Use `just build` for the full CI-equivalent verification before commit.

**The user said they were going to install pipewire-devel directly on the host** — check `pkg-config --modversion libpipewire-0.3` first. If that returns a version, just run `cargo check -p zos-settings` directly on the host without podman.

---

## Task list (matches in-session TaskList)

| ID | Status | Subject |
|---|---|---|
| 1 | in_progress | Add pipewire crate dep + Containerfile build deps |
| 2 | pending | Write `pipewire_native.rs` service module |
| 3 | pending | Add peak meter helper to `pipewire_native.rs` |
| 4 | pending | Rewrite data model in `pipewire.rs` (Vec outputs, delete dead structs) |
| 5 | pending | Rewrite `apply_audio_bus_config` to use `pipewire_native` |
| 6 | pending | Rewrite `audio/mod.rs` as Voicemeeter strip mixer |
| 7 | pending | Final cargo check + clippy + just settings smoke test |

Task #1 is functionally complete pending the bazzite-multilib COPR verification. The next agent should confirm `cargo check -p zos-settings` succeeds in either the host or a podman container, mark task #1 complete, and move to task #2.

---

## Step-by-step implementation guide

### Task 2: Write `pipewire_native.rs`

**File:** `zos-settings/src/services/pipewire_native.rs` (new)
**Reference:** Helvum's `src/pipewire_connection.rs` — https://gitlab.freedesktop.org/pipewire/helvum

PipeWire's `MainLoop` is `!Send` and must be pinned to one OS thread. The bridge to the GTK main thread is via `async_channel`.

**Architecture:**

```rust
// services/pipewire_native.rs

use pipewire as pw;
use pw::{prelude::*, MainLoop, Context, Core, Registry};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::thread;

#[derive(Debug, Clone)]
pub enum PwEvent {
    NodeAdded { id: u32, name: String, description: String, media_class: MediaClass },
    NodeRemoved { id: u32 },
    LinkAdded { id: u32, src_node: u32, dst_node: u32 },
    LinkRemoved { id: u32 },
    PeakLevel { node_id: u32, level: f32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaClass {
    AudioSink,
    AudioSource,
    StreamOutputAudio,  // playback streams (apps producing audio)
    StreamInputAudio,   // capture streams (apps consuming audio)
    Other,
}

pub struct PwState {
    pub nodes: HashMap<u32, NodeInfo>,
    pub links: HashMap<u32, LinkInfo>,
}

pub struct NodeInfo {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub media_class: MediaClass,
    pub port_ids: Vec<u32>,
}

pub struct LinkInfo {
    pub id: u32,
    pub output_node: u32,
    pub input_node: u32,
    pub output_port: u32,
    pub input_port: u32,
}

pub struct PwClient {
    state: Arc<Mutex<PwState>>,
    event_rx: async_channel::Receiver<PwEvent>,
    cmd_tx: std::sync::mpsc::Sender<Cmd>,
}

enum Cmd {
    CreateLink { src_node: u32, dst_node: u32, reply: oneshot::Sender<Result<u32, String>> },
    RemoveLink { id: u32 },
    CreateNullSink { name: String, description: String, reply: oneshot::Sender<Result<u32, String>> },
    DestroyNode { id: u32 },
    SetVolume { node_id: u32, linear: f32 },
    Shutdown,
}

impl PwClient {
    pub fn start() -> Result<Self, pw::Error> {
        let (event_tx, event_rx) = async_channel::unbounded();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let state = Arc::new(Mutex::new(PwState {
            nodes: HashMap::new(),
            links: HashMap::new(),
        }));
        let state_thread = state.clone();

        thread::Builder::new()
            .name("pipewire-mainloop".into())
            .spawn(move || {
                pw::init();
                let mainloop = MainLoop::new(None).expect("pw mainloop");
                let context = Context::new(&mainloop).expect("pw context");
                let core = context.connect(None).expect("pw core");
                let registry = core.get_registry().expect("pw registry");

                // Listen for global add/remove
                let _listener = registry.add_listener_local()
                    .global({
                        let state = state_thread.clone();
                        let tx = event_tx.clone();
                        move |global| {
                            // Match on global.type_, parse properties from global.props,
                            // build NodeInfo / LinkInfo, push to state, send PwEvent
                        }
                    })
                    .global_remove({
                        let state = state_thread.clone();
                        let tx = event_tx.clone();
                        move |id| {
                            // Remove from state, send PwEvent
                        }
                    })
                    .register();

                // Process commands from the GTK thread
                let cmd_source = mainloop.add_event(move |_| {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            Cmd::CreateLink { .. } => { /* use core.create_object::<pw::link::Link>() */ }
                            // ...
                            Cmd::Shutdown => { return; }
                        }
                    }
                });

                mainloop.run();
            })
            .expect("spawn pipewire thread");

        Ok(Self { state, event_rx, cmd_tx })
    }

    pub fn subscribe(&self) -> async_channel::Receiver<PwEvent> {
        self.event_rx.clone()
    }

    pub fn create_link(&self, src_node: u32, dst_node: u32) -> Result<u32, String> {
        // pair ports by channel name (output_FL→input_FL, output_FR→input_FR)
        // for each pair, send CreateLink command and await reply via oneshot
    }

    // ... remove_link, create_null_sink, destroy_node, set_volume
}

impl Drop for PwClient {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Shutdown);
    }
}
```

**Critical PipeWire API notes:**

- Use `core.create_object::<pw::link::Link, _>("link-factory", &props)` to make links — props need `link.input.port`, `link.output.port`, `link.input.node`, `link.output.node`, all as strings.
- Use `core.create_object::<pw::node::Node, _>("adapter", &props)` with `factory.name = "support.null-audio-sink"` to make null sinks. Set `media.class = "Audio/Sink/Virtual"`.
- Volume changes go through `pw::node::Node::set_param` with `Props { channel_volumes: ..., mute: ... }` or via the more convenient `pw::registry::Registry::bind` to get a `Node` proxy and call props on it.
- Channel pairing: enumerate the source node's output ports and the destination's input ports, parse the `audio.channel` property from each port, then pair `FL→FL`, `FR→FR`, etc. NEVER substring-match port names.
- Media class strings to recognise:
  - `Audio/Sink` — physical playback device
  - `Audio/Source` — physical capture device
  - `Audio/Sink/Virtual` — null sink (our buses)
  - `Stream/Output/Audio` — application producing sound (Firefox, Spotify)
  - `Stream/Input/Audio` — application consuming sound (OBS, recording apps)

**Don't try to implement peak meters in this task — defer to task 3.**

After writing, add to `services/mod.rs`:
```rust
pub mod pipewire_native;
```

Verify with `cargo check -p zos-settings`.

### Task 3: Peak meter helper

Add a `start_peak_meter(node_id: u32) -> Result<PeakMeterHandle>` method to `PwClient`. This:

1. Creates a `pw::stream::Stream` with media class `Stream/Input/Audio`, format `S16` mono or stereo, target the given node id.
2. In the `process` callback, read the buffer, compute max abs sample value, normalise to 0.0..1.0.
3. Throttle to ~30 Hz (track last emit time, skip if < 33ms ago).
4. Send `PwEvent::PeakLevel { node_id, level }`.
5. Returns a handle whose Drop disconnects the stream.

Reference: coppwr (https://github.com/dimtpap/coppwr) does this. Helvum does NOT.

### Task 4: Rewrite `pipewire.rs` data model

**File:** `zos-settings/src/services/pipewire.rs`

Changes:

1. **Change** `AudioBusConfig.physical_device: String` (line 595) → `outputs: Vec<String>`.
2. **Fix** the migration at lines 642-664 to keep the full `inp.outputs` Vec instead of `.first()`. Map the outputs vec into the new `outputs` field directly.
3. **Delete entirely:**
   - `BusConfig` and `BusTarget` structs (lines 537-560)
   - Their loaders/savers: `load_bus_configs`, `save_bus_configs`, `bus_configs_path`
   - `InputConfig` (lines 565-570) and `OutputConfig` (lines 572-583)
   - Their loaders/savers: `load_input_configs`, `save_input_configs`, `load_output_configs`, `save_output_configs`, `input_configs_path`, `output_configs_path`
   - `route_stream_to_sink` (lines 300-440) entirely — including the dead `stream_ports` block
   - The dead `set_default(stream_id)` fallback at lines 371-374
   - All `#[allow(dead_code)]` annotations
4. **Keep:**
   - `AudioBusConfig`, `AudioUiState`, `EqBand`, `default_eq`
   - `audio_bus_configs_path`, `audio_ui_state_path`, `pipewire_bus_input_path`, `pipewire_bus_output_path`
   - `load_audio_bus_configs`, `save_audio_bus_configs`, `load_audio_ui_state`, `save_audio_ui_state`
   - `default_audio_bus_configs`
   - `cleanup_all_zos_pipewire_configs`
   - `create_bus_pipewire_nodes` (the `.conf` writer for persistence)
   - `list_sinks`, `list_physical_sinks`, `list_streams` — the read-only enumerators (still useful for the bus config defaults)
   - `set_volume`, `set_mute` for default-sink control on the UI side
5. **Update** `default_audio_bus_configs` to populate `outputs: vec![default_device]` instead of `physical_device: default_device`.

After this, run `cargo check -p zos-settings` and fix every compile error introduced by the field rename. Most will be in `pages/audio/mod.rs` — that's expected, the next task rewrites the whole file anyway, but get it compiling first.

### Task 5: Rewrite `apply_audio_bus_config`

**File:** `zos-settings/src/services/pipewire.rs:839`

New version:

```rust
pub fn apply_audio_bus_config(buses: &[AudioBusConfig], pw: &pipewire_native::PwClient) -> Result<(), String> {
    // 1. Persistence: write the .conf fragments so buses survive a reboot
    cleanup_all_zos_pipewire_configs();
    for bus in buses {
        create_bus_pipewire_nodes(bus);
    }
    save_audio_bus_configs(buses);

    // 2. Live: reconcile current PipeWire state against the desired config
    //    - For each bus that doesn't exist as a node yet, pw.create_null_sink(...)
    //    - For each existing bus node that's no longer in `buses`, pw.destroy_node(...)
    //    - For each bus, look up the bus's monitor port and each output device's
    //      input ports, and reconcile links via pw.create_link / pw.remove_link
    // 3. Apply gain via pw.set_volume per bus

    Ok(())
}
```

**Forbidden:**
- `systemctl --user restart pipewire` — never call this again
- `std::thread::sleep` — no hardcoded waits anywhere
- `wpctl set-default <stream_id>` for stream routing

**Required:**
- Wrap the call site in `gio::spawn_blocking` (in `audio/mod.rs`) so the GTK main thread doesn't freeze.
- Disable the Apply button while running, re-enable on completion via `glib::MainContext::spawn_local` from the spawn_blocking completion.

### Task 6: Rewrite `audio/mod.rs` as Voicemeeter strip mixer

**File:** `zos-settings/src/pages/audio/mod.rs`

This is the biggest task — full file rewrite. Use Relm4 `FactoryComponent` for the per-bus strip widget. Reference: https://relm4.org/book/stable/factory.html

**Module layout:**

```rust
// pages/audio/mod.rs
mod strip;       // FactoryComponent for one bus strip
mod app_routing; // Top section: app rows with toggle-button-per-bus
mod add_dialog;  // Add bus dialog (port the existing one)

use strip::AudioStrip;
use app_routing::AppRoutingList;
```

**Top-level page model:**

```rust
pub struct AudioPage {
    pw: Rc<pipewire_native::PwClient>,
    bus_configs: Vec<AudioBusConfig>,
    physical_sinks: Vec<AudioDevice>,
    strips: relm4::factory::FactoryVecDeque<AudioStrip>,
    app_routing: relm4::Controller<AppRoutingList>,
}

pub enum AudioPageMsg {
    PwEvent(pipewire_native::PwEvent),
    AddBus,
    BusAdded(AudioBusConfig),
    Apply,
    ApplyDone(Result<(), String>),
}
```

**Strip FactoryComponent:**

```rust
pub struct AudioStrip {
    config: AudioBusConfig,
    physical_sinks: Vec<AudioDevice>,
    peak_level: f32,
}

pub enum AudioStripMsg {
    GainChanged(f32),
    MuteToggled(bool),
    OutputToggled { sink_name: String, enabled: bool },
    PeakLevel(f32),
}

#[relm4::factory]
impl FactoryComponent for AudioStrip {
    type Init = (AudioBusConfig, Vec<AudioDevice>);
    type Input = AudioStripMsg;
    type Output = AudioStripOutput;
    type ParentWidget = gtk::Box;
    type CommandOutput = ();

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 8,
            set_margin_all: 8,
            add_css_class: "card",

            gtk::Label {
                set_label: &self.config.description,
                add_css_class: "heading",
            },

            gtk::Scale {
                set_orientation: gtk::Orientation::Vertical,
                set_inverted: true,
                set_range: (-12.0, 12.0),
                set_value: self.config.gain as f64,
                set_height_request: 200,
                connect_value_changed[sender] => move |s| {
                    sender.input(AudioStripMsg::GainChanged(s.value() as f32));
                },
            },

            gtk::LevelBar {
                set_orientation: gtk::Orientation::Vertical,
                set_min_value: 0.0,
                set_max_value: 1.0,
                #[watch]
                set_value: self.peak_level as f64,
                set_height_request: 100,
            },

            // Routing toggle buttons — one per physical sink
            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 4,
                #[iterate]
                append: &physical_sink_toggles(&self.physical_sinks, &self.config.outputs, sender.clone()),
            },

            gtk::ToggleButton {
                set_label: "Mute",
                #[watch]
                set_active: self.config.mute,
                connect_toggled[sender] => move |b| {
                    sender.input(AudioStripMsg::MuteToggled(b.is_active()));
                },
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        let (config, physical_sinks) = init;
        Self { config, physical_sinks, peak_level: 0.0 }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            AudioStripMsg::GainChanged(g) => self.config.gain = g,
            AudioStripMsg::MuteToggled(m) => self.config.mute = m,
            AudioStripMsg::OutputToggled { sink_name, enabled } => {
                if enabled {
                    if !self.config.outputs.contains(&sink_name) {
                        self.config.outputs.push(sink_name);
                    }
                } else {
                    self.config.outputs.retain(|s| s != &sink_name);
                }
            }
            AudioStripMsg::PeakLevel(level) => self.peak_level = level,
        }
    }
}
```

**App routing list:**

Each app row is a `gtk::Box` with an app name `gtk::Label` and a horizontal `gtk::Box` of `gtk::ToggleButton`s, one per bus. The Component subscribes to `PwEvent::NodeAdded`/`NodeRemoved` filtered to `MediaClass::StreamOutputAudio` and rebuilds the row list on each change.

**Note:** `AudioBusConfig` needs a `mute: bool` field added in task 4 — it's not there currently. Add it to the struct definition AND default to `false` in `default_audio_bus_configs`.

**Icon fix:** at the EQ ExpanderRow construction, replace `multimedia-equalizer-symbolic` with `audio-x-generic-symbolic`. Audit all other icon names in the file at the same time and replace any non-stock-Adwaita names.

### Task 7: Verification

```bash
# Local
cargo check -p zos-settings
cargo clippy -p zos-settings -- -D warnings
just dev    # NOT "just settings" — the Justfile target is `dev`

# Container
just build  # Full image build, ~10 minutes first time
```

**Smoke tests** (manual, with the binary running):

1. Audio page renders horizontal strip widgets, not stacked combo rows.
2. Each strip has multiple toggle buttons in the output-routing row.
3. The EQ icon renders (no missing-image placeholder).
4. **Multi-output test**: Click two output toggle buttons on the same strip, click Apply, route Firefox to that bus, play audio — confirm sound from BOTH outputs simultaneously.
5. **Audio-doesn't-break test**: While music is playing in another app, click Apply on a bus change. The other app's audio must NOT drop, and the GTK window must stay interactive.
6. **App routing test**: Route Firefox to one bus, then a different bus. Audio should follow the click within ~100ms.
7. **Live update test**: Open Discord while the audio page is already open. Discord should appear in the App Routing list automatically without re-opening the page.

---

## File reference (current state of in-flight changes)

### `zos-settings/Cargo.toml` (modified)

```toml
[package]
name = "zos-settings"
version = "0.1.0"
edition = "2021"
description = "GTK4/Adwaita settings application for zOS"

[dependencies]
zos-core = { path = "../zos-core" }
relm4 = { version = "0.10.1", features = ["macros", "libadwaita", "gnome_46"] }
zbus = "5.14.0"
tracing = "0.1.44"
tracing-subscriber = "0.3.23"
serde_json = "1.0.149"
cairo-rs = { version = "0.21", features = ["png"] }
serde = { version = "1.0.228", features = ["derive"] }
pipewire = "0.8"
```

### `Containerfile` lines 23-24 and 48-49 (modified by user)

```dockerfile
    dnf5 install -y rust cargo gtk4-devel libadwaita-devel gtk3-devel libayatana-appindicator-gtk3-devel gtk4-layer-shell-devel clang-devel git && \
    dnf5 install -y --repo=copr:copr.fedorainfracloud.org:ublue-os:bazzite-multilib pipewire-devel && \
    ...
    dnf5 remove -y rust cargo gtk4-devel libadwaita-devel gtk3-devel libayatana-appindicator-gtk3-devel gtk4-layer-shell-devel clang-devel git && \
    dnf5 remove -y pipewire-devel && \
```

---

## Key references

| URL | What |
|---|---|
| https://github.com/theRealCarneiro/pulsemeeter | Visual reference — the Linux Voicemeeter clone |
| https://gitlab.freedesktop.org/pipewire/helvum | Reference Rust + GTK4 + pipewire-rs codebase |
| https://github.com/dimtpap/coppwr | Reference for peak metering with pipewire-rs streams |
| https://relm4.org/book/stable/factory.html | Relm4 FactoryComponent docs (for the strip widget) |
| https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/ | pipewire-rs API docs |
| https://wiki.hyprland.org/ | Hyprland — relevant for understanding the broader desktop |
| https://docs.bazzite.gg/ | Bazzite docs — base image |
| https://vb-audio.com/Voicemeeter/VoicemeeterBanana_UserManual.pdf | Original Voicemeeter Banana manual (UI spec) |

---

## Hard rules from the global CLAUDE.md (re-stated for the next agent)

- **Never use background agents.** Always foreground, always model: opus.
- **Each agent gets ONE small task** — one file or one function. Never give an agent a giant multi-file rewrite. Tasks 4, 5, and 6 should each be a separate agent invocation, not all rolled into one.
- **After every agent**, READ the files it claims to have changed and VERIFY. Don't trust agent output blindly.
- **After every agent**, run `cargo check -p zos-settings` to verify before moving on.
- **If an agent fails or produces bad output, FIX IT YOURSELF** — don't re-launch the same task.
- **No TODO comments. No FUTURE placeholders. No stub implementations.** Finish what you start.
- **No "graceful fallbacks" or backwards-compat shims** for the deleted config schemas. Just delete the code and run the migration once.
- **Trust internal code.** Don't add validation/error handling for cases that can't happen. Validate only at boundaries (PipeWire results, file IO, user input).
- **Don't add features beyond scope.** Stay in `zos-settings/src/services/pipewire.rs`, `zos-settings/src/services/pipewire_native.rs` (new), `zos-settings/src/pages/audio/mod.rs`, `zos-settings/src/services/mod.rs`, `Cargo.toml`, and `Containerfile`. Nothing else.
- **`/etc/skel/` and `/ctx/` patterns** apply to the Containerfile only — not to anything in `zos-settings/`.

---

## Conversation flavour notes

The user is **frustrated**. This panel has been broken across many iterations and they're tired of "this iteration will be the one." They want to see actual working multi-select routing with no audio breakage. Be terse, do the work, verify each step before moving on, and don't ship anything you haven't actually compiled. Don't tell them you've fixed something unless you've actually run `cargo check` against it.

The user explicitly wanted the Voicemeeter UX, the pipewire-rs migration, AND the dead code purge. All three are non-negotiable scope. Don't try to descope to "just fix the bug" — that was already offered as an alternative and rejected.
