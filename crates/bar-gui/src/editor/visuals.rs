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

use bar_graph::{NodeType, ParamValue, PortKind};

use crate::app::BarEditorApp;

impl BarEditorApp {
    /// Walk every subgraph in the project and rebuild its
    /// `subgraph_inputs/outputs` from the `SubgraphInput` /
    /// `SubgraphOutput` nodes inside it. Each IO node contributes one
    /// external port to the collapsed block:
    ///
    /// - `SubgraphInput` → an entry in `subgraph_inputs`, bound to its
    ///   `value` *input* port (so an outer wire connects directly to
    ///   the IO node from the outside).
    /// - `SubgraphOutput` → an entry in `subgraph_outputs`, bound to
    ///   its `value` *output* port (so the outer graph reads from the
    ///   IO node).
    ///
    /// Idempotent and cheap; safe to call every frame. Replaces the
    /// previous "subgraph ports are edited via a modal form" model.
    pub(crate) fn recompute_all_subgraph_io(&mut self) {
        // Snapshot member sets and node descriptors first so we can
        // iterate without holding a borrow on `self.graph`.
        let groups: Vec<(u64, Vec<NodeId>)> = self
            .visuals
            .groups
            .iter()
            .filter(|(_, g)| g.is_subgraph)
            .map(|(gid, g)| (*gid, g.member_ids.iter().copied().collect()))
            .collect();
        for (gid, members) in groups {
            // Sort members by NodeId so the per-kind fallback
            // suffix (when an IO node has no explicit name) is
            // stable across save/load.
            let mut sorted = members;
            sorted.sort_by_key(|nid| nid.0);

            // Pre-pass: collect IO nodes with their kinds so we
            // can disambiguate same-kind ports with a numeric
            // suffix. Names that the user has explicitly set
            // bypass the numbering — they win.
            #[derive(Clone)]
            struct IoEntry {
                nid: NodeId,
                is_input: bool,
                /// Display label inferred from the connected port (e.g. "Slope").
                /// Empty string means nothing is connected ("Unknown" display).
                kind_display: String,
                /// Underlying PortKind for type enforcement, inferred from the
                /// connected port's actual kind.
                port_kind: PortKind,
                explicit_name: Option<String>,
            }
            // Track all SubgraphOutput nids so we can reset disconnected ones.
            let all_output_nids: Vec<NodeId> = sorted
                .iter()
                .filter(|&&nid| {
                    self.graph
                        .get_node(nid)
                        .map(|n| n.node_type == NodeType::SubgraphOutput)
                        .unwrap_or(false)
                })
                .copied()
                .collect();

            let mut entries: Vec<IoEntry> = Vec::new();
            for nid in sorted {
                let Some(node) = self.graph.get_node(nid) else {
                    continue;
                };
                let (is_input, is_output) = match node.node_type {
                    NodeType::SubgraphInput => (true, false),
                    NodeType::SubgraphOutput => (false, true),
                    _ => (false, false),
                };
                if !(is_input || is_output) {
                    continue;
                }
                // Both input and output: infer from whatever is wired into
                // the "value" input port. For outputs, nothing connected means
                // no external port. For inputs, nothing connected is valid but
                // shows as "Unknown".
                let conn_info = self
                    .graph
                    .connections()
                    .iter()
                    .find(|c| c.to.node_id == nid && c.to.port_name == "value")
                    .and_then(|c| {
                        self.graph
                            .get_node(c.from.node_id)?
                            .outputs
                            .iter()
                            .find(|p| p.name == c.from.port_name)
                            .map(|p| (p.label.clone(), p.kind))
                    });
                let (kind_display, port_kind) = match conn_info {
                    Some((label, pk)) => (label, pk),
                    None if is_output => continue, // no connection: no external port
                    None => (String::new(), PortKind::Heightmap), // input: Unknown
                };
                let explicit_name = match node.params.get("name") {
                    Some(ParamValue::String(s)) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                };
                entries.push(IoEntry {
                    nid,
                    is_input,
                    kind_display,
                    port_kind,
                    explicit_name,
                });
            }

            // Count auto-named ports per (role, kind) so we know
            // which need a "2", "3", … suffix. Empty kind ("Unknown")
            // is counted separately. Explicit names don't contribute.
            let mut auto_counts: std::collections::HashMap<(bool, String), usize> =
                std::collections::HashMap::new();
            for e in &entries {
                if e.explicit_name.is_none() {
                    *auto_counts
                        .entry((e.is_input, e.kind_display.clone()))
                        .or_insert(0) += 1;
                }
            }

            // Collect (nid, kind_display, port_kind) for all IO nodes so
            // we can sync their node params and port kinds after the loop.
            let io_kind_syncs: Vec<(NodeId, String, PortKind)> = entries
                .iter()
                .map(|e| (e.nid, e.kind_display.clone(), e.port_kind))
                .collect();

            let mut inputs: Vec<crate::state::SubgraphPortRuntime> = Vec::new();
            let mut outputs: Vec<crate::state::SubgraphPortRuntime> = Vec::new();
            let mut auto_seen: std::collections::HashMap<(bool, String), usize> =
                std::collections::HashMap::new();
            for e in entries {
                let display_kind = if e.kind_display.is_empty() {
                    "Unknown"
                } else {
                    &e.kind_display
                };
                let label = if let Some(ref n) = e.explicit_name {
                    n.clone()
                } else {
                    let total = *auto_counts
                        .get(&(e.is_input, e.kind_display.clone()))
                        .unwrap_or(&1);
                    let idx = auto_seen
                        .entry((e.is_input, e.kind_display.clone()))
                        .or_insert(0);
                    *idx += 1;
                    if total > 1 {
                        format!("{} {}", display_kind, idx)
                    } else {
                        display_kind.to_string()
                    }
                };
                let port = crate::state::SubgraphPortRuntime {
                    name: label.clone(),
                    label,
                    kind: format!("{:?}", e.port_kind),
                    binding: Some((e.nid, "value".to_string())),
                };
                if e.is_input {
                    inputs.push(port);
                } else {
                    outputs.push(port);
                }
            }
            if let Some(g) = self.visuals.groups.get_mut(&gid) {
                g.subgraph_inputs = inputs;
                g.subgraph_outputs = outputs;
            }
            // Sync kind_display param and port kind for all IO nodes
            // that had a connection this frame.
            let synced_nids: std::collections::HashSet<NodeId> =
                io_kind_syncs.iter().map(|(nid, _, _)| *nid).collect();
            for (nid, _kind_display, port_kind) in io_kind_syncs {
                if let Some(node) = self.graph.get_node_mut(nid) {
                    node.params.insert(
                        "kind".to_string(),
                        ParamValue::String(format!("{:?}", port_kind)),
                    );
                    node.set_io_port_kind(port_kind);
                }
            }
            // Reset disconnected SubgraphOutput nodes to Unknown state.
            for nid in all_output_nids {
                if !synced_nids.contains(&nid) {
                    if let Some(node) = self.graph.get_node_mut(nid) {
                        node.params
                            .insert("kind".to_string(), ParamValue::String(String::new()));
                        node.set_io_port_kind(PortKind::Heightmap);
                    }
                }
            }
        }
    }
}
