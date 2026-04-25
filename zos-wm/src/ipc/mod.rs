//! zos-wm IPC — Unix socket for external query / control.

pub mod protocol;
pub mod server;

pub use protocol::{Monitor, PROTOCOL_VERSION, Request, Response, Window, Workspace};
pub use server::{IpcServer, placeholder_handler};
