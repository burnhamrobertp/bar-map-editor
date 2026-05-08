//! Visual presentation state for the node graph: per-node positions,
//! group runtime, and the per-frame hit-test rect caches.
//!
//! Distinct from `editor::SelectionState`: this owns *what's drawn*
//! (positions, group bodies, headers), while selection owns *what's
//! picked*. The hit-test caches rebuild every frame as the canvas
//! draws -- they exist so the next frame's input handling can do
//! O(1) lookups instead of re-walking the graph.

use std::collections::HashMap;

use bar_graph::NodeId;
use eframe::egui;

use crate::state::{GroupRuntime, NodeVisual};

/// Grouped visuals state. See module docs.
#[derive(Default, Debug, Clone)]
pub struct VisualsState {
    /// On-canvas position and size for each node.
    pub node_visuals: HashMap<NodeId, NodeVisual>,
    /// Visual node groups keyed by stable group id. Purely
    /// organisational -- groups don't affect graph evaluation.
    pub groups: HashMap<u64, GroupRuntime>,
    /// Reverse index: which group does this node belong to (if any)?
    /// Maintained alongside `groups` so the render pass and hit-
    /// testing don't need to scan every group every frame.
    pub node_to_group: HashMap<NodeId, u64>,
    /// Monotonic group id allocator. Never reuses a freed id within
    /// one session so undo/redo can refer back to deleted groups
    /// without confusion. Resets to the highest seen id + 1 at load.
    pub next_group_id: u64,
    /// Cached on-screen rect of each group's title bar from the most
    /// recent render. Used by hit-testing to detect title-bar clicks
    /// for selection and drag.
    pub group_header_rects: HashMap<u64, egui::Rect>,
    /// Cached body rect (excluding title) per group for the same
    /// reason -- clicking the body selects the group too.
    pub group_body_rects: HashMap<u64, egui::Rect>,
    /// Cached rect of each *collapsed* SubGraph block from the most
    /// recent render. Collapsed subgraphs aren't drawn through
    /// `draw_groups`, so they have no header / body rects. The
    /// contextual Properties popup uses this to know "the cursor is
    /// over collapsed group N" and drive the hover gate against it.
    pub collapsed_subgraph_rects: HashMap<u64, egui::Rect>,
}

impl VisualsState {
    /// Allocate a fresh group id, advancing the monotonic counter.
    pub fn alloc_group_id(&mut self) -> u64 {
        let id = self.next_group_id;
        self.next_group_id = self.next_group_id.saturating_add(1);
        id
    }
}
