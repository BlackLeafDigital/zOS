//! zos-wm Compositor impl — talks to zos-wm's IPC socket.
//!
//! Stub for now (Phase 8 finishes this). Returns NotSupported / empty
//! values so apps gracefully fall back to placeholder UI when running
//! under zos-wm before the IPC socket is wired up.

use super::{Compositor, MonitorInfo, WindowInfo, WorkspaceInfo};
use std::error::Error;

#[derive(Debug, Default)]
pub struct ZosWm;

impl ZosWm {
    pub fn new() -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self)
    }
}

impl Compositor for ZosWm {
    fn workspaces(&self) -> Result<Vec<WorkspaceInfo>, Box<dyn Error + Send + Sync>> {
        // TODO(zos-wm-ipc): connect to socket
        Ok(Vec::new())
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, Box<dyn Error + Send + Sync>> {
        Ok(Vec::new())
    }
    fn monitors(&self) -> Result<Vec<MonitorInfo>, Box<dyn Error + Send + Sync>> {
        Ok(Vec::new())
    }
    fn active_window(&self) -> Result<Option<WindowInfo>, Box<dyn Error + Send + Sync>> {
        Ok(None)
    }
    fn focus_window(&self, _address: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        Err("zos-wm IPC not yet implemented".into())
    }
    fn switch_to_workspace(&self, _id: i64) -> Result<(), Box<dyn Error + Send + Sync>> {
        Err("zos-wm IPC not yet implemented".into())
    }
}
