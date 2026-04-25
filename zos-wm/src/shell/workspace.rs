//! Per-monitor floating-first workspace.
//!
//! A `Workspace` is a stack of windows for a specific output. The active
//! workspace's content is what `Space<WindowElement>` is rebuilt from each
//! frame; the workspace owns the truth, the Space is a render cache.
//!
//! Z-order is split across `ZBand`s: `Below` < `Normal` < `AlwaysOnTop` <
//! `Fullscreen`. Within a band, the deque order from front to back
//! determines stacking (back of deque = top of band).
//!
//! `focus_history` is a separate MRU stack. Destroying the active window
//! pops it and the next entry becomes active.

use std::collections::VecDeque;

use crate::shell::element::{WindowEntry, WindowId, WorkspaceId, ZBand};
use crate::shell::output_state::OutputId;

#[derive(Debug)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub output_id: OutputId,
    /// Bottom-to-top stacking order. Multiple bands share this deque;
    /// rendering walks bands separately to enforce z-order.
    pub windows: VecDeque<WindowEntry>,
    pub active: Option<WindowId>,
    /// MRU focus stack. Last entry = most recently focused.
    pub focus_history: Vec<WindowId>,
}

impl Workspace {
    pub fn new(id: WorkspaceId, output_id: OutputId) -> Self {
        Self {
            id,
            output_id,
            windows: VecDeque::new(),
            active: None,
            focus_history: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: WindowEntry) {
        // New windows arrive at the top of their band.
        self.windows.push_back(entry);
    }

    pub fn remove(&mut self, id: WindowId) -> Option<WindowEntry> {
        let idx = self.windows.iter().position(|e| e.id == id)?;
        let entry = self.windows.remove(idx)?;
        self.focus_history.retain(|&h| h != id);
        if self.active == Some(id) {
            self.active = self.focus_history.last().copied();
        }
        Some(entry)
    }

    /// Move `id` to the top of its band (highest within-band z).
    pub fn raise(&mut self, id: WindowId) {
        let Some(idx) = self.windows.iter().position(|e| e.id == id) else {
            return;
        };
        let Some(entry) = self.windows.remove(idx) else {
            return;
        };
        // Insert just before the first window in a higher band.
        let entry_band = entry.band;
        let insert_at = self
            .windows
            .iter()
            .position(|e| e.band > entry_band)
            .unwrap_or(self.windows.len());
        self.windows.insert(insert_at, entry);
    }

    /// Move `id` to the bottom of its band.
    pub fn lower(&mut self, id: WindowId) {
        let Some(idx) = self.windows.iter().position(|e| e.id == id) else {
            return;
        };
        let Some(entry) = self.windows.remove(idx) else {
            return;
        };
        let entry_band = entry.band;
        let insert_at = self
            .windows
            .iter()
            .position(|e| e.band >= entry_band)
            .unwrap_or(self.windows.len());
        self.windows.insert(insert_at, entry);
    }

    pub fn focus(&mut self, id: WindowId, raise: bool) {
        if !self.windows.iter().any(|e| e.id == id) {
            return;
        }
        // Update activated flag.
        for entry in self.windows.iter_mut() {
            entry.activated = entry.id == id;
        }
        self.active = Some(id);
        // Update MRU history (move-to-end semantics).
        self.focus_history.retain(|&h| h != id);
        self.focus_history.push(id);
        if raise {
            self.raise(id);
        }
    }

    pub fn next_after_destroy(&self) -> Option<WindowId> {
        self.focus_history.last().copied()
    }

    /// Move all descendants (transitively) of `id` so they appear above
    /// `id` in the within-band stack. Used after raising a parent.
    pub fn bring_descendants_above(&mut self, id: WindowId) {
        // Single-pass: collect descendants, then re-insert.
        let descendants: Vec<WindowId> = {
            let mut frontier = vec![id];
            let mut found = Vec::new();
            while let Some(parent) = frontier.pop() {
                for entry in self.windows.iter() {
                    if entry.parent_id == Some(parent) && !found.contains(&entry.id) {
                        found.push(entry.id);
                        frontier.push(entry.id);
                    }
                }
            }
            found
        };
        for d in descendants {
            self.raise(d);
        }
    }

    /// Iterate windows in a specific band, bottom-to-top.
    pub fn iter_band(&self, band: ZBand) -> impl Iterator<Item = &WindowEntry> {
        self.windows.iter().filter(move |e| e.band == band)
    }

    /// All bands, in z-order, bottom-to-top. The render path consumes this
    /// to build per-frame Space contents.
    pub fn iter_z_order(&self) -> impl Iterator<Item = &WindowEntry> {
        // Below first, then Normal, AlwaysOnTop, Fullscreen.
        self.windows
            .iter()
            .filter(|e| e.band == ZBand::Below)
            .chain(self.windows.iter().filter(|e| e.band == ZBand::Normal))
            .chain(self.windows.iter().filter(|e| e.band == ZBand::AlwaysOnTop))
            .chain(self.windows.iter().filter(|e| e.band == ZBand::Fullscreen))
    }

    pub fn find(&self, id: WindowId) -> Option<&WindowEntry> {
        self.windows.iter().find(|e| e.id == id)
    }

    pub fn find_mut(&mut self, id: WindowId) -> Option<&mut WindowEntry> {
        self.windows.iter_mut().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

/// Synchronize the live `Space<WindowElement>` to reflect the current
/// active workspaces of every `OutputState`.
///
/// Approach: clear the space and re-map every window from each output's
/// active workspace. Brutal but correct; later we can do incremental
/// diffing if perf calls for it.
pub fn sync_active_workspaces_to_space(
    outputs: &std::collections::HashMap<crate::shell::output_state::OutputId, crate::shell::output_state::OutputState>,
    space: &mut smithay::desktop::Space<crate::shell::element::WindowElement>,
) {
    let existing: Vec<_> = space.elements().cloned().collect();
    for el in existing {
        space.unmap_elem(&el);
    }
    for output_state in outputs.values() {
        let ws = output_state.active();
        for entry in ws.iter_z_order() {
            space.map_element(entry.element.clone(), entry.location, entry.activated);
        }
    }
}
