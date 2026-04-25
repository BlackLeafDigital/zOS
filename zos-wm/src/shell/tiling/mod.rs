//! Pluggable tiling algorithms. Each `TilingAlgorithm` impl owns the
//! geometry of a tiled workspace. The workspace owns the algorithm
//! (`Box<dyn TilingAlgorithm>`); a workspace in floating mode skips
//! consulting the algorithm entirely.
//!
//! Phase 3 ships `dwindle::DwindleAlgorithm` (binary-split). Phase 7 will
//! add scrolling-columns, master-stack, and tabbed-split.

pub mod dwindle;

use smithay::utils::{Logical, Rectangle};

/// Opaque per-window key the algorithm uses internally. Workspaces map
/// `WindowId` → `WindowKey` and back via a `HashMap<WindowId, WindowKey>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowKey(u32);

impl WindowKey {
    pub fn alloc() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
    pub fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// The trait every tiling layout implements. Designed so Phase 7 plugins
/// can `Box<dyn TilingAlgorithm>` new layouts without touching workspace
/// internals.
pub trait TilingAlgorithm: Send {
    /// Insert a window. The algorithm assigns the rectangle.
    fn insert(&mut self, window: WindowKey);
    /// Remove a window. Sibling promotion / re-tile happens internally.
    fn remove(&mut self, window: WindowKey);
    /// Adjust a split ratio between two siblings on a particular edge.
    fn resize_edge(&mut self, window: WindowKey, edge: Edge, delta: i32);
    /// Walk to the neighbour in a given direction. Returns its `WindowKey`
    /// or `None` if no neighbour exists.
    fn focus_in_direction(&self, from: WindowKey, dir: Direction) -> Option<WindowKey>;
    /// The current rectangle for a window. None if not in the tree.
    fn rect_for(&self, window: WindowKey) -> Option<Rectangle<i32, Logical>>;
    /// Update the work area (output minus exclusive zones). Triggers a
    /// recompute of all rectangles.
    fn set_work_area(&mut self, area: Rectangle<i32, Logical>);
    /// Iterate all currently-tiled windows in some deterministic order.
    fn windows(&self) -> Box<dyn Iterator<Item = WindowKey> + '_>;
    /// Number of currently-tiled windows.
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
