//! Binary-split (Hyprland-dwindle-style) tiling algorithm.
//!
//! Each leaf is a window; each internal node is a Split with an
//! orientation + ratio. New windows split the focused leaf along its
//! longer axis. Closing a window collapses its parent split, promoting
//! the surviving sibling.
//!
//! See docs/research/phase-3-tiling-opt-in.md §2 for full design.

use smithay::utils::{Logical, Point, Rectangle, Size};

use super::{Direction, Edge, TilingAlgorithm, WindowKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Debug)]
pub enum LayoutNode {
    /// Leaf: a single window.
    Tile(WindowKey),
    /// Internal node: two children + split orientation + ratio.
    /// `ratio` is the fraction of the parent rect occupied by `children[0]`.
    Split {
        orientation: Orientation,
        ratio: f32,
        children: Box<[LayoutNode; 2]>,
    },
}

#[derive(Debug)]
pub struct DwindleTree {
    pub root: Option<LayoutNode>,
    pub work_area: Rectangle<i32, Logical>,
    pub default_ratio: f32,
    /// Cached rect for each WindowKey, refreshed by `recalc`.
    cache: std::collections::HashMap<WindowKey, Rectangle<i32, Logical>>,
}

impl DwindleTree {
    pub fn new(work_area: Rectangle<i32, Logical>) -> Self {
        Self {
            root: None,
            work_area,
            default_ratio: 0.5,
            cache: std::collections::HashMap::new(),
        }
    }

    /// Walk the tree, assigning rectangles. Pure geometry — does not
    /// mutate the tree shape.
    pub fn recalc(&mut self) {
        self.cache.clear();
        if let Some(ref root) = self.root {
            let mut cache = std::mem::take(&mut self.cache);
            Self::recalc_node(root, self.work_area, &mut cache);
            self.cache = cache;
        }
    }

    fn recalc_node(
        node: &LayoutNode,
        rect: Rectangle<i32, Logical>,
        cache: &mut std::collections::HashMap<WindowKey, Rectangle<i32, Logical>>,
    ) {
        match node {
            LayoutNode::Tile(key) => {
                cache.insert(*key, rect);
            }
            LayoutNode::Split {
                orientation,
                ratio,
                children,
            } => {
                let (a, b) = match orientation {
                    Orientation::Horizontal => {
                        // children[0] is left half.
                        let left_w = (rect.size.w as f32 * *ratio).round() as i32;
                        let right_w = rect.size.w - left_w;
                        let left_rect = Rectangle::new(rect.loc, Size::from((left_w, rect.size.h)));
                        let right_rect = Rectangle::new(
                            Point::from((rect.loc.x + left_w, rect.loc.y)),
                            Size::from((right_w, rect.size.h)),
                        );
                        (left_rect, right_rect)
                    }
                    Orientation::Vertical => {
                        // children[0] is top half.
                        let top_h = (rect.size.h as f32 * *ratio).round() as i32;
                        let bot_h = rect.size.h - top_h;
                        let top_rect = Rectangle::new(rect.loc, Size::from((rect.size.w, top_h)));
                        let bot_rect = Rectangle::new(
                            Point::from((rect.loc.x, rect.loc.y + top_h)),
                            Size::from((rect.size.w, bot_h)),
                        );
                        (top_rect, bot_rect)
                    }
                };
                Self::recalc_node(&children[0], a, cache);
                Self::recalc_node(&children[1], b, cache);
            }
        }
    }

    /// Recursively walk to the first leaf (in-order DFS) and replace it
    /// in-place with a Split whose children[0] is that original leaf and
    /// children[1] is `Tile(new_window)`. Orientation is picked from the
    /// rect we *would* have given the leaf — but at this point we don't
    /// know that rect. Instead we recompute it on the way down.
    ///
    /// Returns `true` once the replacement has happened, so the recursion
    /// can short-circuit.
    fn insert_at_first_leaf(
        node: &mut LayoutNode,
        rect: Rectangle<i32, Logical>,
        new_window: WindowKey,
        default_ratio: f32,
    ) -> bool {
        match node {
            LayoutNode::Tile(_) => {
                // Decide orientation from the leaf's current rect:
                // wider-than-tall → horizontal split (side-by-side),
                // otherwise vertical (stacked).
                let orientation = if rect.size.w >= rect.size.h {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                };
                // Take ownership of the existing leaf so we can put it
                // inside the new Split.
                let old = std::mem::replace(node, LayoutNode::Tile(new_window));
                // Reassemble: children[0] is the original leaf, [1] is the new tile.
                *node = LayoutNode::Split {
                    orientation,
                    ratio: default_ratio,
                    children: Box::new([old, LayoutNode::Tile(new_window)]),
                };
                true
            }
            LayoutNode::Split {
                orientation,
                ratio,
                children,
            } => {
                // Compute child rects identically to recalc_node so the
                // orientation decision matches what the user will see.
                let (a, b) = split_rect(rect, *orientation, *ratio);
                if Self::insert_at_first_leaf(&mut children[0], a, new_window, default_ratio) {
                    return true;
                }
                Self::insert_at_first_leaf(&mut children[1], b, new_window, default_ratio)
            }
        }
    }

    /// Recursive helper for remove. Returns `Some(replacement)` for the
    /// new subtree, or `None` if the entire subtree was the leaf to drop.
    /// `found` is set to true once we've actually located + removed the
    /// target leaf.
    fn remove_walk(
        node: LayoutNode,
        target: WindowKey,
        found: &mut bool,
    ) -> Option<LayoutNode> {
        match node {
            LayoutNode::Tile(key) => {
                if key == target {
                    *found = true;
                    None
                } else {
                    Some(LayoutNode::Tile(key))
                }
            }
            LayoutNode::Split {
                orientation,
                ratio,
                children,
            } => {
                // Box<[LayoutNode; 2]> can't be destructured by index moves
                // directly, so unbox first then move out by position.
                let [c0, c1] = *children;
                let new_c0 = Self::remove_walk(c0, target, found);
                // If we already collapsed via c0, c1 is unchanged on the right.
                let new_c1 = Self::remove_walk(c1, target, found);
                match (new_c0, new_c1) {
                    // Target was in c0 → promote c1.
                    (None, Some(sib)) => Some(sib),
                    // Target was in c1 → promote c0.
                    (Some(sib), None) => Some(sib),
                    // Neither child collapsed → rebuild Split unchanged.
                    (Some(a), Some(b)) => Some(LayoutNode::Split {
                        orientation,
                        ratio,
                        children: Box::new([a, b]),
                    }),
                    // Both gone — only possible if both children were the
                    // same target leaf, which can't happen with unique keys.
                    (None, None) => None,
                }
            }
        }
    }

    /// Walk the tree returning the path of child indices (0 or 1) from
    /// root to the leaf with `target`, or `None` if not found.
    fn path_to(&self, target: WindowKey) -> Option<Vec<usize>> {
        fn walk(node: &LayoutNode, target: WindowKey, path: &mut Vec<usize>) -> bool {
            match node {
                LayoutNode::Tile(key) => *key == target,
                LayoutNode::Split { children, .. } => {
                    path.push(0);
                    if walk(&children[0], target, path) {
                        return true;
                    }
                    path.pop();
                    path.push(1);
                    if walk(&children[1], target, path) {
                        return true;
                    }
                    path.pop();
                    false
                }
            }
        }
        let mut path = Vec::new();
        let root = self.root.as_ref()?;
        if walk(root, target, &mut path) {
            Some(path)
        } else {
            None
        }
    }

}

/// Split a rect into two child rects per orientation + ratio. Mirrors
/// `recalc_node` so insert/resize math stays in sync with layout.
fn split_rect(
    rect: Rectangle<i32, Logical>,
    orientation: Orientation,
    ratio: f32,
) -> (Rectangle<i32, Logical>, Rectangle<i32, Logical>) {
    match orientation {
        Orientation::Horizontal => {
            let left_w = (rect.size.w as f32 * ratio).round() as i32;
            let right_w = rect.size.w - left_w;
            let left_rect = Rectangle::new(rect.loc, Size::from((left_w, rect.size.h)));
            let right_rect = Rectangle::new(
                Point::from((rect.loc.x + left_w, rect.loc.y)),
                Size::from((right_w, rect.size.h)),
            );
            (left_rect, right_rect)
        }
        Orientation::Vertical => {
            let top_h = (rect.size.h as f32 * ratio).round() as i32;
            let bot_h = rect.size.h - top_h;
            let top_rect = Rectangle::new(rect.loc, Size::from((rect.size.w, top_h)));
            let bot_rect = Rectangle::new(
                Point::from((rect.loc.x, rect.loc.y + top_h)),
                Size::from((rect.size.w, bot_h)),
            );
            (top_rect, bot_rect)
        }
    }
}

impl TilingAlgorithm for DwindleTree {
    fn insert(&mut self, window: WindowKey) {
        // Empty tree → window becomes the root.
        if self.root.is_none() {
            self.root = Some(LayoutNode::Tile(window));
            self.recalc();
            return;
        }
        // Otherwise: split the first leaf we hit. Future refinement: prefer
        // the focused leaf or the leaf under the cursor.
        let work_area = self.work_area;
        let default_ratio = self.default_ratio;
        if let Some(root) = self.root.as_mut() {
            Self::insert_at_first_leaf(root, work_area, window, default_ratio);
        }
        self.recalc();
    }

    fn remove(&mut self, window: WindowKey) {
        // Take the root so we can move nodes by value through the recursion.
        let Some(root) = self.root.take() else {
            return;
        };
        let mut found = false;
        let new_root = Self::remove_walk(root, window, &mut found);
        if !found {
            // Window wasn't in the tree at all — restore root and bail.
            self.root = new_root;
            return;
        }
        self.root = new_root;
        self.recalc();
    }

    fn resize_edge(&mut self, window: WindowKey, edge: Edge, delta: i32) {
        // Map the edge to the orientation of the Split it operates on.
        // Left/Right edges only do anything against a Horizontal split;
        // Top/Bottom against a Vertical split.
        let needed_orient = match edge {
            Edge::Left | Edge::Right => Orientation::Horizontal,
            Edge::Top | Edge::Bottom => Orientation::Vertical,
        };
        let Some(path) = self.path_to(window) else {
            return;
        };
        if path.is_empty() {
            // Single-leaf tree — nothing to resize against.
            return;
        }
        let work_area = self.work_area;
        // Walk the path and for each Split decide:
        //   - does its orientation match `needed_orient`?
        //   - if yes: this is the *deepest matching ancestor we've seen so
        //     far*. Capture a "do the mutation here" plan and keep going so
        //     deeper matches override it. (Spec §2.5 says "deepest" / "first
        //     ancestor walking up", which means closest to the leaf.)
        // We collect (rect_size_in_axis, child_index_at_split, ratio_ptr)
        // through a closure but defer the mutation until after the walk.
        //
        // Implementation: we make two passes. First pass collects the
        // path-rects + orientations (read-only). Second pass mutates the
        // chosen split. This is simpler than juggling lifetimes inside a
        // single mutable walk.

        // ---- Pass 1: read-only descent to find the deepest matching split ----
        struct Hit {
            depth: usize,
            child_idx: usize,
            split_size_in_axis: i32,
        }
        let mut hit: Option<Hit> = None;
        {
            let mut node = self.root.as_ref();
            let mut rect = work_area;
            for (depth, &idx) in path.iter().enumerate() {
                let Some(LayoutNode::Split {
                    orientation,
                    ratio,
                    children,
                }) = node
                else {
                    break;
                };
                if *orientation == needed_orient {
                    let span = match orientation {
                        Orientation::Horizontal => rect.size.w,
                        Orientation::Vertical => rect.size.h,
                    };
                    hit = Some(Hit {
                        depth,
                        child_idx: idx,
                        split_size_in_axis: span,
                    });
                }
                let (r0, r1) = split_rect(rect, *orientation, *ratio);
                rect = if idx == 0 { r0 } else { r1 };
                node = Some(&children[idx]);
            }
        }
        let Some(hit) = hit else {
            return;
        };

        // Determine whether this edge moves the boundary in the +ratio
        // direction (children[0] grows) or -ratio (children[0] shrinks).
        // For a Horizontal split:
        //   - Right edge of children[0] dragging right → ratio increases.
        //   - Right edge of children[1] dragging right → boundary doesn't
        //     touch children[1]'s right edge; that edge is the *parent*
        //     rect edge, so this resize doesn't apply at this split. Same
        //     logic for Left edge of children[0]. We detect that and bail.
        //   - Left edge of children[1] dragging right → ratio increases.
        // The clean rule: the split's interior boundary is the right edge
        // of children[0] AND the left edge of children[1]. So:
        //   - Edge::Right + child_idx 0 → +delta  (boundary moves right with delta)
        //   - Edge::Left  + child_idx 1 → +delta  (boundary moves right with delta)
        //   - Edge::Right + child_idx 1 → not this split's boundary, skip
        //   - Edge::Left  + child_idx 0 → not this split's boundary, skip
        // Same vertical: interior boundary is bottom of [0] / top of [1].
        let signed_delta = match (edge, hit.child_idx) {
            (Edge::Right, 0) | (Edge::Left, 1) => delta,
            (Edge::Bottom, 0) | (Edge::Top, 1) => delta,
            // Edge is on the outer perimeter of this split — we shouldn't
            // have selected this split. Bail rather than guess.
            _ => return,
        };
        if hit.split_size_in_axis <= 0 {
            return;
        }
        let ratio_delta = signed_delta as f32 / hit.split_size_in_axis as f32;

        // ---- Pass 2: descend with a mutable borrow and apply ----
        {
            let mut node = self.root.as_mut();
            for (depth, &idx) in path.iter().enumerate() {
                let Some(n) = node else {
                    break;
                };
                let LayoutNode::Split {
                    ratio, children, ..
                } = n
                else {
                    break;
                };
                if depth == hit.depth {
                    let new_ratio = (*ratio + ratio_delta).clamp(0.05, 0.95);
                    *ratio = new_ratio;
                    break;
                }
                node = Some(&mut children[idx]);
            }
        }

        self.recalc();
    }

    fn focus_in_direction(&self, from: WindowKey, dir: Direction) -> Option<WindowKey> {
        // Source window must exist in the tree.
        let from_rect = self.cache.get(&from).copied()?;
        let from_left = from_rect.loc.x;
        let from_top = from_rect.loc.y;
        let from_right = from_rect.loc.x + from_rect.size.w;
        let from_bottom = from_rect.loc.y + from_rect.size.h;

        // For each cached window other than `from`, ask: is it in the
        // direction we want? If so, score it by:
        //   - primary: distance to the from-edge (closer is better)
        //   - secondary: perpendicular overlap (more is better)
        // We tiebreak using overlap so siblings on the same row/column
        // win over distant outliers.
        let mut best: Option<(WindowKey, i32, i32)> = None; // (key, distance, -overlap)
        for (&key, &rect) in self.cache.iter() {
            if key == from {
                continue;
            }
            let r_left = rect.loc.x;
            let r_top = rect.loc.y;
            let r_right = rect.loc.x + rect.size.w;
            let r_bottom = rect.loc.y + rect.size.h;

            // Direction-specific predicates + scoring axes.
            let (in_dir, distance, overlap) = match dir {
                Direction::Left => {
                    // Candidate must be left of `from`: its right edge
                    // lies at or before from's left edge.
                    let in_dir = r_right <= from_left;
                    // Distance: how far left of from's left edge we have to go.
                    let distance = from_left - r_right;
                    // Vertical overlap (perpendicular axis).
                    let overlap = (from_bottom.min(r_bottom) - from_top.max(r_top)).max(0);
                    (in_dir, distance, overlap)
                }
                Direction::Right => {
                    let in_dir = r_left >= from_right;
                    let distance = r_left - from_right;
                    let overlap = (from_bottom.min(r_bottom) - from_top.max(r_top)).max(0);
                    (in_dir, distance, overlap)
                }
                Direction::Up => {
                    let in_dir = r_bottom <= from_top;
                    let distance = from_top - r_bottom;
                    let overlap = (from_right.min(r_right) - from_left.max(r_left)).max(0);
                    (in_dir, distance, overlap)
                }
                Direction::Down => {
                    let in_dir = r_top >= from_bottom;
                    let distance = r_top - from_bottom;
                    let overlap = (from_right.min(r_right) - from_left.max(r_left)).max(0);
                    (in_dir, distance, overlap)
                }
            };
            if !in_dir {
                continue;
            }
            // Skip candidates with zero perpendicular overlap — they're
            // diagonally offset and not really "in this direction".
            if overlap <= 0 {
                continue;
            }
            // Sort key: (distance ascending, -overlap ascending → overlap descending)
            let score = (distance, -overlap);
            match best {
                None => best = Some((key, score.0, score.1)),
                Some((_, bd, bo)) if (score.0, score.1) < (bd, bo) => {
                    best = Some((key, score.0, score.1));
                }
                _ => {}
            }
        }
        best.map(|(k, _, _)| k)
    }

    fn rect_for(&self, window: WindowKey) -> Option<Rectangle<i32, Logical>> {
        self.cache.get(&window).copied()
    }
    fn set_work_area(&mut self, area: Rectangle<i32, Logical>) {
        self.work_area = area;
        self.recalc();
    }
    fn windows(&self) -> Box<dyn Iterator<Item = WindowKey> + '_> {
        Box::new(self.cache.keys().copied())
    }
    fn len(&self) -> usize {
        self.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_area() -> Rectangle<i32, Logical> {
        Rectangle::new(Point::from((0, 0)), Size::from((1920, 1080)))
    }

    #[test]
    fn insert_into_empty_tree_makes_single_leaf() {
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        tree.insert(w1);

        assert_eq!(tree.len(), 1);
        let r = tree.rect_for(w1).expect("w1 should have a rect");
        assert_eq!(r, work_area(), "lone window fills the whole work area");
        assert!(matches!(tree.root, Some(LayoutNode::Tile(_))));
    }

    #[test]
    fn insert_second_window_horizontal_split() {
        // 1920x1080 → wider-than-tall, so first split is Horizontal: side-by-side halves.
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        let w2 = WindowKey::alloc();
        tree.insert(w1);
        tree.insert(w2);

        assert_eq!(tree.len(), 2);
        let r1 = tree.rect_for(w1).expect("w1 rect");
        let r2 = tree.rect_for(w2).expect("w2 rect");
        // Half-width side-by-side, full height each.
        assert_eq!(r1, Rectangle::new(Point::from((0, 0)), Size::from((960, 1080))));
        assert_eq!(r2, Rectangle::new(Point::from((960, 0)), Size::from((960, 1080))));
        // Root should now be a Split.
        assert!(matches!(
            tree.root,
            Some(LayoutNode::Split {
                orientation: Orientation::Horizontal,
                ..
            })
        ));
    }

    #[test]
    fn remove_collapses_split_back_to_single_leaf() {
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        let w2 = WindowKey::alloc();
        tree.insert(w1);
        tree.insert(w2);
        tree.remove(w2);

        assert_eq!(tree.len(), 1);
        assert!(tree.rect_for(w2).is_none());
        let r1 = tree.rect_for(w1).expect("w1 should still be there");
        assert_eq!(r1, work_area(), "w1 should expand back to full area");
        assert!(matches!(tree.root, Some(LayoutNode::Tile(_))));
    }

    #[test]
    fn remove_first_window_promotes_sibling() {
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        let w2 = WindowKey::alloc();
        tree.insert(w1);
        tree.insert(w2);
        tree.remove(w1);

        assert_eq!(tree.len(), 1);
        let r2 = tree.rect_for(w2).expect("w2 should remain");
        assert_eq!(r2, work_area());
    }

    #[test]
    fn remove_unknown_window_is_noop() {
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        let phantom = WindowKey::alloc();
        tree.insert(w1);
        tree.remove(phantom);
        // w1 still alone.
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.rect_for(w1), Some(work_area()));
    }

    #[test]
    fn focus_in_direction_finds_horizontal_neighbour() {
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        let w2 = WindowKey::alloc();
        tree.insert(w1);
        tree.insert(w2);
        // w1 is on the left, w2 on the right.
        assert_eq!(tree.focus_in_direction(w1, Direction::Right), Some(w2));
        assert_eq!(tree.focus_in_direction(w2, Direction::Left), Some(w1));
        // No neighbours up/down for either.
        assert_eq!(tree.focus_in_direction(w1, Direction::Up), None);
        assert_eq!(tree.focus_in_direction(w2, Direction::Down), None);
    }

    #[test]
    fn resize_edge_adjusts_horizontal_ratio() {
        let mut tree = DwindleTree::new(work_area());
        let w1 = WindowKey::alloc();
        let w2 = WindowKey::alloc();
        tree.insert(w1);
        tree.insert(w2);
        // Drag w1's right edge 192 px to the right → ratio goes from 0.5 to 0.6.
        tree.resize_edge(w1, Edge::Right, 192);
        let r1 = tree.rect_for(w1).expect("w1 rect");
        let r2 = tree.rect_for(w2).expect("w2 rect");
        // 1920 * 0.6 = 1152.
        assert_eq!(r1.size.w, 1152);
        assert_eq!(r2.size.w, 1920 - 1152);
        assert_eq!(r2.loc.x, 1152);
    }
}
