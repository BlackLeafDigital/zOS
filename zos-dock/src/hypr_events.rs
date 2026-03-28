// === hypr_events.rs — Real-time Hyprland event socket subscription ===

use iced::Subscription;
use std::path::PathBuf;

/// Events from the Hyprland event socket relevant to the dock.
#[derive(Debug, Clone)]
pub enum HyprEvent {
    /// A new window was opened.
    WindowOpened,
    /// A window was closed.
    WindowClosed,
    /// A window was moved to a workspace (includes minimize to special:minimize).
    WindowMoved { workspace: String },
    /// The active (focused) window changed.
    ActiveWindowChanged { address: String },
}

/// Returns the path to Hyprland's event socket (socket2).
fn socket2_path() -> PathBuf {
    let xdg_runtime =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
    let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default();
    PathBuf::from(xdg_runtime)
        .join("hypr")
        .join(instance)
        .join(".socket2.sock")
}

/// Parse a Hyprland event line (format: "EVENT>>DATA") into a HyprEvent.
fn parse_event(line: &str) -> Option<HyprEvent> {
    let (event_name, data) = line.split_once(">>")?;
    match event_name {
        "openwindow" => Some(HyprEvent::WindowOpened),
        "closewindow" => Some(HyprEvent::WindowClosed),
        "movewindow" => {
            // Format: ADDRESS,WORKSPACE_NAME
            let workspace = data.rsplit_once(',').map(|(_, ws)| ws).unwrap_or(data);
            Some(HyprEvent::WindowMoved {
                workspace: workspace.to_string(),
            })
        }
        "activewindowv2" => Some(HyprEvent::ActiveWindowChanged {
            address: format!("0x{}", data.trim()),
        }),
        _ => None,
    }
}

/// Create a subscription that connects to the Hyprland event socket
/// and emits HyprEvent messages in real time.
pub fn hypr_event_stream() -> impl iced::futures::Stream<Item = HyprEvent> {
    iced::stream::channel(32, async move |mut sender| {
        use iced::futures::SinkExt;
        use tokio::io::AsyncBufReadExt;

        loop {
            let path = socket2_path();
            match tokio::net::UnixStream::connect(&path).await {
                Ok(stream) => {
                    let reader = tokio::io::BufReader::new(stream);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if let Some(event) = parse_event(&line) {
                            if sender.send(event).await.is_err() {
                                return;
                            }
                        }
                    }
                    // Socket closed, reconnect after delay
                }
                Err(_) => {
                    // Connection failed, retry after delay
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    })
}

/// Create an iced Subscription for Hyprland events.
pub fn hypr_events_subscription() -> Subscription<HyprEvent> {
    Subscription::run(hypr_event_stream)
}
