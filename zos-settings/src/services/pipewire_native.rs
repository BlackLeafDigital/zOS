//! Native PipeWire client for the zos-settings audio panel.
//!
//! Spawns a dedicated OS thread that owns a PipeWire `MainLoop`, registry,
//! and core proxy. Other threads communicate with the loop thread via a
//! command channel; events are pushed back through an `async-channel`
//! receiver that the UI can subscribe to.
//!
//! See `services/mod.rs` for where this is wired in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use pipewire as pw;
use pw::context::ContextRc;
use pw::core::CoreRc;
use pw::main_loop::MainLoopRc;
use pw::properties::properties;
use pw::proxy::ProxyT;
use pw::registry::{GlobalObject, RegistryRc};
use pw::spa;
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;
use spa::param::ParamType;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, PropertyFlags, ValueArray};
use spa::utils::SpaTypes;

// ---------- Public API types ----------

/// Audio media class derived from a node's `media.class` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaClass {
    /// `Audio/Sink` — physical playback device.
    AudioSink,
    /// `Audio/Source` — physical capture device.
    AudioSource,
    /// `Audio/Sink/Virtual` — null sinks (our buses).
    AudioSinkVirtual,
    /// `Stream/Output/Audio` — applications producing audio.
    StreamOutputAudio,
    /// `Stream/Input/Audio` — applications consuming audio.
    StreamInputAudio,
    /// Anything else we do not care about.
    Other,
}

impl MediaClass {
    fn parse(s: &str) -> Self {
        match s {
            "Audio/Sink" => MediaClass::AudioSink,
            "Audio/Source" => MediaClass::AudioSource,
            "Audio/Sink/Virtual" => MediaClass::AudioSinkVirtual,
            "Stream/Output/Audio" => MediaClass::StreamOutputAudio,
            "Stream/Input/Audio" => MediaClass::StreamInputAudio,
            _ => MediaClass::Other,
        }
    }
}

/// Events emitted by the PipeWire client thread.
// Field-level dead-code lints fire because no subscriber consumes the events
// yet — the audio page polls via `list_app_streams` instead. The variants and
// their payloads are emitted by the loop thread today and will be consumed
// once `PwClient::subscribe` is wired into the audio page.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PwEvent {
    NodeAdded {
        id: u32,
        name: String,
        description: String,
        media_class: MediaClass,
    },
    NodeRemoved {
        id: u32,
    },
    LinkAdded {
        id: u32,
        output_node: u32,
        input_node: u32,
    },
    LinkRemoved {
        id: u32,
    },
    /// Reserved for the future peak-meter implementation (task 3).
    PeakLevel {
        node_id: u32,
        level: f32,
    },
}

/// Handle returned by `PwClient::start_peak_meter`.
///
/// Currently a no-op placeholder so downstream code compiles. When task 3
/// lands, dropping the handle will disconnect the peak-meter stream.
// Retained as a public placeholder so the peak-meter API surface is stable
// before task 3 lands.
#[allow(dead_code)]
pub struct PeakMeterHandle {
    _private: (),
}

/// A currently-active audio playback stream belonging to an application.
#[derive(Debug, Clone)]
pub struct AppStream {
    pub id: u32,
    /// Human-readable application name, e.g. "Floorp", "Spotify".
    /// Falls back to node.description, then node.name if application.name is absent.
    pub app_name: String,
    /// Optional per-stream media title, e.g. "YouTube — some video".
    pub media_name: Option<String>,
    /// Optional Freedesktop icon name from `application.icon_name`.
    pub icon_name: Option<String>,
}

// ---------- Internal command protocol ----------

type Reply<T> = mpsc::SyncSender<Result<T, String>>;

// `CreateLink` / `RemoveLink` / `CreateNullSink` / `DestroyNode` are matched by
// `process_cmd` but currently never produced because the corresponding
// `PwClient` methods (also retained) are not yet called from the audio page —
// routing today still goes through the wpctl/pw-link CLI helpers in
// `services::pipewire`. Keep the variants so the loop thread is feature-complete
// once the audio page switches over.
#[allow(dead_code)]
enum Cmd {
    CreateLink {
        src: u32,
        dst: u32,
        reply: Reply<Vec<u32>>,
    },
    RemoveLink {
        id: u32,
        reply: Reply<()>,
    },
    CreateNullSink {
        name: String,
        description: String,
        reply: Reply<u32>,
    },
    DestroyNode {
        id: u32,
        reply: Reply<()>,
    },
    SetVolume {
        node_id: u32,
        linear: f32,
        mute: bool,
        reply: Reply<()>,
    },
    ListAppStreams {
        reply: Reply<Vec<AppStream>>,
    },
    Shutdown,
}

// ---------- Internal state cached on the loop thread ----------

#[derive(Debug, Clone)]
struct NodeInfo {
    name: String,
    description: String,
    media_class: MediaClass,
    application_name: Option<String>,
    application_icon_name: Option<String>,
    media_name: Option<String>,
}

#[derive(Debug, Clone)]
struct LinkInfo {
    // Tracked so we can correlate registry global removals back to which nodes
    // were linked; consumed once the event subscriber path is wired up.
    #[allow(dead_code)]
    output_node: u32,
    #[allow(dead_code)]
    input_node: u32,
}

#[derive(Debug, Clone)]
struct PortInfo {
    node_id: u32,
    direction: String, // "in" or "out"
    channel: String,   // e.g. "FL", "FR", "MONO"
}

#[derive(Default)]
struct PwState {
    nodes: HashMap<u32, NodeInfo>,
    links: HashMap<u32, LinkInfo>,
    ports: HashMap<u32, PortInfo>,
}

// ---------- Public client handle ----------

/// Client handle for talking to the PipeWire main loop thread.
pub struct PwClient {
    cmd_tx: mpsc::Sender<Cmd>,
    // Cloned out by `subscribe()` for future event subscribers; the audio page
    // does not subscribe yet, so this currently has no readers.
    #[allow(dead_code)]
    events_rx: Receiver<PwEvent>,
    thread: Option<JoinHandle<()>>,
}

impl PwClient {
    /// Start the PipeWire main loop on a dedicated thread and begin listening
    /// for events. Returns once the thread is up and the core is connected.
    pub fn start() -> Result<Self, String> {
        // Channel from caller threads to the PW loop thread.
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        // Channel from the PW loop thread to event subscribers.
        let (events_tx, events_rx) = async_channel::unbounded::<PwEvent>();
        // One-shot to report startup success/failure back to the caller.
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let thread = thread::Builder::new()
            .name("pipewire-mainloop".into())
            .spawn(move || {
                run_mainloop(cmd_rx, events_tx, ready_tx);
            })
            .map_err(|e| format!("failed to spawn pipewire thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(PwClient {
                cmd_tx,
                events_rx,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err("pipewire thread exited before signalling readiness".into())
            }
        }
    }

    /// Subscribe to the event stream.
    // Retained for the planned reactive audio page (replaces the polling-based
    // `list_app_streams` call site).
    #[allow(dead_code)]
    pub fn subscribe(&self) -> Receiver<PwEvent> {
        self.events_rx.clone()
    }

    /// Create one or more `link-factory` links between two nodes, pairing
    /// ports by channel. Returns the new link ids (one per matched pair).
    // Retained alongside `Cmd::CreateLink` for the native routing path.
    #[allow(dead_code)]
    pub fn create_link(&self, src_node: u32, dst_node: u32) -> Result<Vec<u32>, String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(Cmd::CreateLink {
                src: src_node,
                dst: dst_node,
                reply,
            })
            .map_err(|e| format!("failed to send command: {e}"))?;
        rx.recv()
            .map_err(|e| format!("reply channel closed: {e}"))?
    }

    /// Destroy a link by id.
    // Retained alongside `Cmd::RemoveLink` for the native routing path.
    #[allow(dead_code)]
    pub fn remove_link(&self, link_id: u32) -> Result<(), String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(Cmd::RemoveLink { id: link_id, reply })
            .map_err(|e| format!("failed to send command: {e}"))?;
        rx.recv()
            .map_err(|e| format!("reply channel closed: {e}"))?
    }

    /// Create a null-audio-sink node via the `adapter` factory and wait for
    /// the registry to confirm it.
    // Retained for the planned native bus-creation flow (replaces the
    // pipewire.conf.d fragment writes in `services::pipewire`).
    #[allow(dead_code)]
    pub fn create_null_sink(&self, name: &str, description: &str) -> Result<u32, String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(Cmd::CreateNullSink {
                name: name.to_string(),
                description: description.to_string(),
                reply,
            })
            .map_err(|e| format!("failed to send command: {e}"))?;
        rx.recv()
            .map_err(|e| format!("reply channel closed: {e}"))?
    }

    /// Destroy a node by id.
    // Retained alongside `Cmd::DestroyNode` for the native bus-teardown flow.
    #[allow(dead_code)]
    pub fn destroy_node(&self, id: u32) -> Result<(), String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(Cmd::DestroyNode { id, reply })
            .map_err(|e| format!("failed to send command: {e}"))?;
        rx.recv()
            .map_err(|e| format!("reply channel closed: {e}"))?
    }

    /// Set a node's linear volume (0.0-1.5, 1.0 = unity) and mute state via
    /// SPA `Props` param.
    pub fn set_node_volume(&self, node_id: u32, linear: f32, mute: bool) -> Result<(), String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(Cmd::SetVolume {
                node_id,
                linear,
                mute,
                reply,
            })
            .map_err(|e| format!("failed to send command: {e}"))?;
        rx.recv()
            .map_err(|e| format!("reply channel closed: {e}"))?
    }

    /// Placeholder for the future peak-meter implementation (task 3).
    /// Returns an empty handle that does nothing.
    // Retained as a public placeholder so the peak-meter API surface is stable
    // before task 3 lands.
    #[allow(dead_code)]
    pub fn start_peak_meter(&self, _node_id: u32) -> Result<PeakMeterHandle, String> {
        Ok(PeakMeterHandle { _private: () })
    }

    /// Return a snapshot of all currently-active application output audio
    /// streams (`Stream/Output/Audio` nodes) known to the registry.
    pub fn list_app_streams(&self) -> Result<Vec<AppStream>, String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(Cmd::ListAppStreams { reply })
            .map_err(|e| format!("failed to send command: {e}"))?;
        rx.recv()
            .map_err(|e| format!("reply channel closed: {e}"))?
    }
}

impl Drop for PwClient {
    fn drop(&mut self) {
        // Best-effort shutdown — ignore errors, the thread may already be gone.
        let _ = self.cmd_tx.send(Cmd::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

// ---------- PW main loop thread implementation ----------

fn run_mainloop(
    cmd_rx: mpsc::Receiver<Cmd>,
    events_tx: Sender<PwEvent>,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
) {
    pw::init();

    let mainloop = match MainLoopRc::new(None) {
        Ok(ml) => ml,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("MainLoop::new: {e}")));
            return;
        }
    };

    let context = match ContextRc::new(&mainloop, None) {
        Ok(ctx) => ctx,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("Context::new: {e}")));
            return;
        }
    };

    let core = match context.connect_rc(None) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("Context::connect: {e}")));
            return;
        }
    };

    let registry = match core.get_registry_rc() {
        Ok(r) => r,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("Core::get_registry: {e}")));
            return;
        }
    };

    let state: Rc<RefCell<PwState>> = Rc::new(RefCell::new(PwState::default()));

    // ---- Registry listener ----
    let listener = {
        let state_added = state.clone();
        let events_added = events_tx.clone();
        let state_removed = state.clone();
        let events_removed = events_tx.clone();
        registry
            .add_listener_local()
            .global(move |global| {
                handle_global(&state_added, &events_added, global);
            })
            .global_remove(move |id| {
                handle_global_remove(&state_removed, &events_removed, id);
            })
            .register()
    };

    // Tell the caller we are up and running.
    if ready_tx.send(Ok(())).is_err() {
        // Caller bailed; clean up.
        drop(listener);
        return;
    }

    // ---- Main loop: drive iterate() and drain commands ----
    let loop_ref = mainloop.loop_();
    let mut shutdown = false;
    while !shutdown {
        // Iterate the PipeWire loop with a short timeout. This dispatches any
        // pending fds and returns. We then process any pending commands.
        loop_ref.iterate(Duration::from_millis(50));

        loop {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    if matches!(cmd, Cmd::Shutdown) {
                        shutdown = true;
                        break;
                    }
                    process_cmd(&core, &registry, loop_ref, &state, cmd);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    shutdown = true;
                    break;
                }
            }
        }
    }

    drop(listener);
    drop(registry);
    drop(core);
    drop(context);
    drop(mainloop);
    // Note: we deliberately do not call pw::deinit() here. It is unsafe and
    // doing so would prevent another PwClient::start() in the same process.
}

fn handle_global(
    state: &Rc<RefCell<PwState>>,
    events: &Sender<PwEvent>,
    global: &GlobalObject<&DictRef>,
) {
    let Some(props) = global.props else {
        return;
    };

    match &global.type_ {
        ObjectType::Node => {
            let name = props.get("node.name").unwrap_or("").to_string();
            let description = props
                .get("node.description")
                .or_else(|| props.get("application.name"))
                .unwrap_or("")
                .to_string();
            let media_class = props
                .get("media.class")
                .map(MediaClass::parse)
                .unwrap_or(MediaClass::Other);
            let application_name = props.get("application.name").map(|s| s.to_string());
            let application_icon_name = props.get("application.icon_name").map(|s| s.to_string());
            let media_name = props.get("media.name").map(|s| s.to_string());

            state.borrow_mut().nodes.insert(
                global.id,
                NodeInfo {
                    name: name.clone(),
                    description: description.clone(),
                    media_class,
                    application_name,
                    application_icon_name,
                    media_name,
                },
            );

            let _ = events.try_send(PwEvent::NodeAdded {
                id: global.id,
                name,
                description,
                media_class,
            });
        }
        ObjectType::Port => {
            let node_id = props
                .get("node.id")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let direction = props.get("port.direction").unwrap_or("").to_string();
            let channel = props.get("audio.channel").unwrap_or("").to_string();

            state.borrow_mut().ports.insert(
                global.id,
                PortInfo {
                    node_id,
                    direction,
                    channel,
                },
            );
        }
        ObjectType::Link => {
            let output_node = props
                .get("link.output.node")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let input_node = props
                .get("link.input.node")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);

            state.borrow_mut().links.insert(
                global.id,
                LinkInfo {
                    output_node,
                    input_node,
                },
            );

            let _ = events.try_send(PwEvent::LinkAdded {
                id: global.id,
                output_node,
                input_node,
            });
        }
        _ => {}
    }
}

fn handle_global_remove(state: &Rc<RefCell<PwState>>, events: &Sender<PwEvent>, id: u32) {
    let mut st = state.borrow_mut();
    if st.nodes.remove(&id).is_some() {
        let _ = events.try_send(PwEvent::NodeRemoved { id });
    } else if st.links.remove(&id).is_some() {
        let _ = events.try_send(PwEvent::LinkRemoved { id });
    } else {
        st.ports.remove(&id);
    }
}

fn process_cmd(
    core: &CoreRc,
    registry: &RegistryRc,
    loop_ref: &pw::loop_::Loop,
    state: &Rc<RefCell<PwState>>,
    cmd: Cmd,
) {
    match cmd {
        Cmd::CreateLink { src, dst, reply } => {
            let result = do_create_link(core, state, src, dst);
            let _ = reply.send(result);
        }
        Cmd::RemoveLink { id, reply } => {
            let res = registry.destroy_global(id);
            let _ = reply.send(spa_to_result(res, "destroy_global(link)"));
        }
        Cmd::CreateNullSink {
            name,
            description,
            reply,
        } => {
            let result = do_create_null_sink(core, loop_ref, state, &name, &description);
            let _ = reply.send(result);
        }
        Cmd::DestroyNode { id, reply } => {
            let res = registry.destroy_global(id);
            let _ = reply.send(spa_to_result(res, "destroy_global(node)"));
        }
        Cmd::SetVolume {
            node_id,
            linear,
            mute,
            reply,
        } => {
            let result = do_set_volume(registry, state, node_id, linear, mute);
            let _ = reply.send(result);
        }
        Cmd::ListAppStreams { reply } => {
            let streams: Vec<AppStream> = state
                .borrow()
                .nodes
                .iter()
                .filter(|(_, n)| n.media_class == MediaClass::StreamOutputAudio)
                .map(|(id, n)| AppStream {
                    id: *id,
                    app_name: n
                        .application_name
                        .clone()
                        .or_else(|| {
                            if n.description.is_empty() {
                                None
                            } else {
                                Some(n.description.clone())
                            }
                        })
                        .unwrap_or_else(|| n.name.clone()),
                    media_name: n.media_name.clone(),
                    icon_name: n.application_icon_name.clone(),
                })
                .collect();
            let _ = reply.send(Ok(streams));
        }
        Cmd::Shutdown => {
            // Handled in the iteration loop above.
        }
    }
}

fn spa_to_result(res: spa::utils::result::SpaResult, ctx: &str) -> Result<(), String> {
    match res.into_result() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{ctx}: {e}")),
    }
}

fn do_create_link(
    core: &CoreRc,
    state: &Rc<RefCell<PwState>>,
    src_node: u32,
    dst_node: u32,
) -> Result<Vec<u32>, String> {
    // Find candidate output ports of src_node and input ports of dst_node,
    // grouped by channel.
    let st = state.borrow();
    if !st.nodes.contains_key(&src_node) {
        return Err(format!("source node {src_node} not found in registry"));
    }
    if !st.nodes.contains_key(&dst_node) {
        return Err(format!("destination node {dst_node} not found in registry"));
    }

    let mut src_ports: HashMap<String, u32> = HashMap::new();
    let mut dst_ports: HashMap<String, u32> = HashMap::new();
    for (port_id, info) in st.ports.iter() {
        if info.channel.is_empty() {
            continue;
        }
        if info.node_id == src_node && info.direction == "out" {
            src_ports.insert(info.channel.clone(), *port_id);
        } else if info.node_id == dst_node && info.direction == "in" {
            dst_ports.insert(info.channel.clone(), *port_id);
        }
    }
    drop(st);

    if src_ports.is_empty() || dst_ports.is_empty() {
        return Err(format!(
            "no matching audio ports for src={src_node} dst={dst_node}",
        ));
    }

    let mut created: Vec<u32> = Vec::new();
    for (channel, src_port) in src_ports.iter() {
        let Some(dst_port) = dst_ports.get(channel) else {
            tracing::debug!("skipping channel {channel}: no matching dst port on node {dst_node}");
            continue;
        };

        let props = properties! {
            "link.output.node" => src_node.to_string(),
            "link.output.port" => src_port.to_string(),
            "link.input.node" => dst_node.to_string(),
            "link.input.port" => dst_port.to_string(),
            "object.linger" => "true",
        };

        let proxy_res: Result<pw::link::Link, pw::Error> =
            core.create_object::<pw::link::Link>("link-factory", &props);
        match proxy_res {
            Ok(link) => {
                // The proxy id is the local proxy id, which the server will
                // bind to a global id once it confirms creation. We forward
                // the local id; the registry listener will emit a LinkAdded
                // event with the canonical global id when it appears.
                created.push(link.upcast_ref().id());
                // Intentionally leak the proxy by forgetting it: dropping
                // the proxy here would tear the link down again.
                std::mem::forget(link);
            }
            Err(e) => {
                tracing::warn!("create_object(link) failed for channel {channel}: {e}");
            }
        }
    }

    if created.is_empty() {
        Err("no links created — all create_object calls failed".into())
    } else {
        Ok(created)
    }
}

fn do_create_null_sink(
    core: &CoreRc,
    loop_ref: &pw::loop_::Loop,
    state: &Rc<RefCell<PwState>>,
    name: &str,
    description: &str,
) -> Result<u32, String> {
    let props = properties! {
        "factory.name" => "support.null-audio-sink",
        "node.name" => name,
        "node.description" => description,
        "media.class" => "Audio/Sink/Virtual",
        "audio.position" => "[ FL FR ]",
        "object.linger" => "true",
    };

    let proxy_res: Result<pw::node::Node, pw::Error> =
        core.create_object::<pw::node::Node>("adapter", &props);
    let proxy = proxy_res.map_err(|e| format!("create_object(adapter): {e}"))?;
    // Keep the proxy alive — dropping it would destroy the node.
    std::mem::forget(proxy);

    // Wait up to 2 seconds for the registry to surface a node with this name.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // Pump the loop so the registry global() callback runs.
        loop_ref.iterate(Duration::from_millis(20));

        if let Some(id) =
            state
                .borrow()
                .nodes
                .iter()
                .find_map(|(id, info)| if info.name == name { Some(*id) } else { None })
        {
            return Ok(id);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for null-sink '{name}' to appear in registry"
            ));
        }
    }
}

fn do_set_volume(
    registry: &RegistryRc,
    state: &Rc<RefCell<PwState>>,
    node_id: u32,
    linear: f32,
    mute: bool,
) -> Result<(), String> {
    use spa::pod::Value as V;

    // Construct a temporary GlobalObject to bind via the registry. The bind
    // method only reads `id` and `type_`, so we can fabricate the rest.
    let st = state.borrow();
    if !st.nodes.contains_key(&node_id) {
        return Err(format!("node {node_id} not found in registry"));
    }
    drop(st);

    // Build a SPA Object Pod: SPA_TYPE_OBJECT_Props with channelVolumes + mute.
    // Cube-root applies the standard PipeWire perceptual volume curve so that
    // a linear UI slider behaves the way users expect; passing `linear` as-is
    // would also work but feel logarithmic to humans.
    let cube = linear.max(0.0).powf(3.0);
    let prop_volumes = Property {
        key: spa::sys::SPA_PROP_channelVolumes,
        flags: PropertyFlags::empty(),
        value: V::ValueArray(ValueArray::Float(vec![cube, cube])),
    };
    let prop_mute = Property::new(spa::sys::SPA_PROP_mute, V::Bool(mute));

    let object = Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Props,
        id: SpaTypes::ObjectParamProps.as_raw(),
        properties: vec![prop_volumes, prop_mute],
    };

    let mut buf: Vec<u8> = Vec::new();
    PodSerializer::serialize(Cursor::new(&mut buf), &V::Object(object))
        .map_err(|e| format!("serialize Props pod: {e}"))?;

    let pod = Pod::from_bytes(&buf).ok_or_else(|| "Pod::from_bytes returned None".to_string())?;

    // Bind the node so we can call set_param on it.
    let fake_global: GlobalObject<&DictRef> = GlobalObject {
        id: node_id,
        permissions: pw::permissions::PermissionFlags::empty(),
        type_: ObjectType::Node,
        version: 0,
        props: None,
    };
    let node: pw::node::Node = registry
        .bind(&fake_global)
        .map_err(|e| format!("registry.bind(node {node_id}): {e}"))?;

    node.set_param(ParamType::Props, 0, pod);
    // Drop the proxy — set_param has already been dispatched on the wire.
    drop(node);

    Ok(())
}

// ---------- Process-wide singleton ----------

static GLOBAL_CLIENT: OnceLock<PwClient> = OnceLock::new();

/// Get or lazily start the process-wide PipeWire client. Returns `None` if
/// startup failed; callers should treat that as "no PipeWire available" and
/// degrade gracefully.
pub fn global_client() -> Option<&'static PwClient> {
    if let Some(client) = GLOBAL_CLIENT.get() {
        return Some(client);
    }
    match PwClient::start() {
        Ok(client) => {
            // If another thread won the race, our client is dropped silently.
            let _ = GLOBAL_CLIENT.set(client);
            GLOBAL_CLIENT.get()
        }
        Err(e) => {
            ::tracing::warn!(error = %e, "failed to start PipeWire native client");
            None
        }
    }
}

/// Convenience: list currently-active application audio streams. Returns
/// an empty vec on any error so the UI can display an empty state.
pub fn list_app_streams() -> Vec<AppStream> {
    let Some(client) = global_client() else {
        return Vec::new();
    };
    // PipeWire needs ~50-100ms after a node appears for the registry
    // listener to populate state; we don't wait here — callers that need
    // fresh data should poll.
    client.list_app_streams().unwrap_or_default()
}
