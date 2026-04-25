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
use std::time::Instant;

use crate::anim::AnimatedValue;
use crate::shell::element::{WindowEntry, WindowId, WorkspaceId, ZBand};
use crate::shell::output_state::OutputId;
use crate::shell::tiling::TilingAlgorithm;

/// A workspace's tiling/floating mode.
///
/// `Floating` is the default; windows open free-floating and are placed
/// by `place_new_window`. `Tiled` consults a `TilingAlgorithm` for
/// window rectangles instead.
pub enum WorkspaceMode {
    Floating,
    Tiled(Box<dyn TilingAlgorithm>),
}

impl std::fmt::Debug for WorkspaceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceMode::Floating => write!(f, "Floating"),
            WorkspaceMode::Tiled(_) => write!(f, "Tiled(<dyn>)"),
        }
    }
}

impl Default for WorkspaceMode {
    fn default() -> Self {
        Self::Floating
    }
}

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
    /// Tiling mode. Floating by default; toggled via `Action::ToggleWorkspaceTiling`.
    pub mode: WorkspaceMode,
    /// Workspace-wide render translation. Drives workspace-switch slide
    /// animations: the active workspace animates from its on-screen offset
    /// while the outgoing one animates the opposite direction.
    pub render_offset: AnimatedValue<smithay::utils::Point<f64, smithay::utils::Logical>>,
    /// Workspace-wide alpha. Used for cross-fades during switches and for
    /// fading the outgoing workspace out.
    pub alpha: AnimatedValue<f32>,
}

impl Workspace {
    pub fn new(id: WorkspaceId, output_id: OutputId) -> Self {
        Self {
            id,
            output_id,
            windows: VecDeque::new(),
            active: None,
            focus_history: Vec::new(),
            mode: WorkspaceMode::default(),
            render_offset: AnimatedValue::new((0.0, 0.0).into()),
            alpha: AnimatedValue::new(1.0),
        }
    }

    /// Advance every animation tied to this workspace + each of its
    /// windows. Single `now` is used for all ticks within a frame so
    /// per-property progress stays consistent.
    pub fn tick_animations(&mut self, now: Instant) {
        self.render_offset.tick(now);
        self.alpha.tick(now);
        for entry in self.windows.iter() {
            let anim = entry.element.anim_state();
            anim.render_offset.lock().unwrap().tick(now);
            anim.alpha.lock().unwrap().tick(now);
        }
    }

    /// Returns true if any animation tied to this workspace or its
    /// windows is still in flight. Render path uses this to decide
    /// whether to keep scheduling redraws.
    pub fn any_animating(&self) -> bool {
        if self.render_offset.is_animating() || self.alpha.is_animating() {
            return true;
        }
        for entry in self.windows.iter() {
            let anim = entry.element.anim_state();
            if anim.render_offset.lock().unwrap().is_animating() {
                return true;
            }
            if anim.alpha.lock().unwrap().is_animating() {
                return true;
            }
        }
        false
    }

    /// Switch to Tiled mode using the given algorithm. Existing window
    /// arrangement is NOT yet re-applied — that's a follow-up. The mode
    /// flips and future window-placement decisions consult the algorithm.
    pub fn switch_to_tiled(&mut self, algorithm: Box<dyn TilingAlgorithm>) {
        self.mode = WorkspaceMode::Tiled(algorithm);
        // TODO(P3-tile-relayout): walk self.windows and call
        // algorithm.insert for each. Update rects via space.map_element
        // and xdg_toplevel.with_pending_state size.
    }

    pub fn switch_to_floating(&mut self) {
        self.mode = WorkspaceMode::Floating;
        // TODO(P3-float-relayout): restore windows to their stored_size
        // and a sensible placement.
    }

    pub fn is_tiled(&self) -> bool {
        matches!(self.mode, WorkspaceMode::Tiled(_))
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
