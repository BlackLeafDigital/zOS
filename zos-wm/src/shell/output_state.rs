//! Per-output state: a list of workspaces + which is active.

use crate::shell::element::WorkspaceId;
use crate::shell::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputId(pub u32);

impl OutputId {
    pub fn alloc() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug)]
pub struct OutputState {
    pub id: OutputId,
    pub output: smithay::output::Output,
    pub workspaces: Vec<Workspace>,
    pub active_workspace: usize,
    /// On reconnect we try to restore this workspace. None means
    /// "use default workspace 0".
    pub last_seen_active: Option<WorkspaceId>,
}

impl OutputState {
    pub fn new(output: smithay::output::Output) -> Self {
        let id = OutputId::alloc();
        // Bootstrap with a single Workspace(1).
        let initial_ws = Workspace::new(WorkspaceId(1), id);
        Self {
            id,
            output,
            workspaces: vec![initial_ws],
            active_workspace: 0,
            last_seen_active: None,
        }
    }

    pub fn active(&self) -> &Workspace {
        &self.workspaces[self.active_workspace]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace]
    }

    /// Switch to (or create) the workspace numbered `target`. Workspaces
    /// are 1-indexed in user-facing terms (Super+1 = workspace 1).
    pub fn switch_to(&mut self, target: WorkspaceId) {
        if let Some(idx) = self.workspaces.iter().position(|w| w.id == target) {
            self.active_workspace = idx;
        } else {
            // Lazy-create.
            self.workspaces.push(Workspace::new(target, self.id));
            self.active_workspace = self.workspaces.len() - 1;
        }
        self.last_seen_active = Some(target);
    }

    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }
}
