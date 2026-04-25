//! Wire protocol for zos-wm's IPC socket.
//!
//! Newline-delimited JSON. Client sends a `Request`, server responds
//! with a `Response`. Connection closes on Quit or socket EOF.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum Request {
    /// List workspaces on a specific output, or across all outputs.
    Workspaces { output: Option<String> },
    /// List windows on a specific workspace, or all of them.
    Windows { workspace: Option<u32> },
    /// List all outputs.
    Monitors,
    /// Return the active window (if any).
    ActiveWindow,
    /// Switch active workspace on the focused output.
    SwitchToWorkspace { id: u32 },
    /// Move active window to workspace.
    MoveWindowToWorkspace { id: u32 },
    /// Focus a window by its WindowId.
    FocusWindow { id: u32 },
    /// Close the focused window.
    CloseFocused,
    /// Query the IPC version + zos-wm build version.
    Version,
    /// Disconnect.
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum Response {
    Workspaces { workspaces: Vec<Workspace> },
    Windows { windows: Vec<Window> },
    Monitors { monitors: Vec<Monitor> },
    ActiveWindow { window: Option<Window> },
    Ok,
    Error { message: String },
    Version { ipc: String, build: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: u32,
    pub output: String,
    pub windows: usize,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Window {
    pub id: u32,
    pub workspace_id: u32,
    pub output: String,
    pub class: String,
    pub title: String,
    pub focused: bool,
    pub band: String, // "Below" | "Normal" | "AlwaysOnTop" | "Fullscreen"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Monitor {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub active_workspace: Option<u32>,
}

pub const PROTOCOL_VERSION: &str = "1";
