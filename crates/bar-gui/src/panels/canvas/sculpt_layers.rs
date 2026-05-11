//! Derive the sculpt layer panel contents from the live node graph.
//!
//! `compute_sculpt_layers` walks backward from the Bundler node, collects
//! every source node reachable from each Bundler input port, and returns a
//! grouped, ordered list for the layer panel UI.

use std::collections::{HashMap, HashSet};

use bar_graph::{GraphEngine, NodeId, NodeType};

/// A channel group in the sculpt layer panel (one per Bundler input port).
pub struct SculptLayerGroup {
    /// Bundler port name: "heightmap", "texture", "metalmap", etc.
    pub channel: String,
    /// Source nodes reachable from this port, bottom-to-top order.
    pub entries: Vec<SculptLayerEntry>,
}

/// One row in a `SculptLayerGroup`.
pub struct SculptLayerEntry {
    pub node_id: NodeId,
    pub label: String,
    /// True when the user can paint on this node (PaintedHeightmap /
    /// PaintedTexture / Sculpt).
    pub is_paintable: bool,
    /// False when the node is not actually wired into the Bundler path
    /// (shown in the panel but greyed with a "!" warning).
    pub is_connected: bool,
}

/// Derive the layer panel contents from the current graph state.
/// Returns an empty Vec when no Bundler node exists.
pub fn compute_sculpt_layers(graph: &GraphEngine) -> Vec<SculptLayerGroup> {
    // Find the Bundler.
    let Some(bundler_id) = graph
        .nodes()
        .iter()
        .find(|(_, n)| n.node_type == NodeType::Bundler)
        .map(|(&id, _)| id)
    else {
        return vec![];
    };

    // Assign a topological index to every node (lower = closer to sources).
    let topo = graph.topological_sort().unwrap_or_default();
    let depth: HashMap<NodeId, usize> = topo.iter().enumerate().map(|(i, &id)| (id, i)).collect();

    let port_names = [
        "heightmap",
        "texture",
        "metalmap",
        "typemap",
        "grassmap",
        "specular",
        "normalmap",
    ];

    let mut groups = vec![];

    for &port_name in &port_names {
        let mut entries = collect_upstream_sources(graph, bundler_id, port_name, &depth);
        if entries.is_empty() {
            continue;
        }
        // bottom-to-top = lower topo depth first
        entries.sort_by_key(|e| depth.get(&e.node_id).copied().unwrap_or(0));
        groups.push(SculptLayerGroup {
            channel: port_name.to_string(),
            entries,
        });
    }

    groups
}

/// Walk backward from `bundler_id`'s `port_name` input, traversing through
/// compositor/filter nodes and collecting every source node encountered.
fn collect_upstream_sources(
    graph: &GraphEngine,
    bundler_id: NodeId,
    port_name: &str,
    _depth: &HashMap<NodeId, usize>,
) -> Vec<SculptLayerEntry> {
    // Find the node directly connected to this Bundler input port.
    let root_id = graph
        .connections()
        .iter()
        .find(|c| c.to.node_id == bundler_id && c.to.port_name == port_name)
        .map(|c| c.from.node_id);

    let Some(root_id) = root_id else {
        return vec![];
    };

    let mut entries = vec![];
    let mut to_visit = vec![root_id];
    let mut visited: HashSet<NodeId> = HashSet::new();

    while let Some(node_id) = to_visit.pop() {
        if !visited.insert(node_id) {
            continue;
        }
        let Some(node) = graph.get_node(node_id) else {
            continue;
        };
        if is_source_node(&node.node_type) {
            entries.push(SculptLayerEntry {
                node_id,
                label: node.label.clone(),
                is_paintable: is_paintable_node(&node.node_type),
                is_connected: true,
            });
        } else {
            // Compositor/filter: recurse into its inputs.
            for conn in graph.connections() {
                if conn.to.node_id == node_id {
                    to_visit.push(conn.from.node_id);
                }
            }
        }
    }

    entries
}

fn is_source_node(t: &NodeType) -> bool {
    matches!(
        t,
        NodeType::PerlinNoise
            | NodeType::SimplexNoise
            | NodeType::WorleyNoise
            | NodeType::RidgedNoise
            | NodeType::Constant
            | NodeType::FileInput
            | NodeType::Voronoi
            | NodeType::Gradient
            | NodeType::SmfImport
            | NodeType::SmtImport
            | NodeType::PaintedHeightmap
            | NodeType::PaintedTexture
    )
}

fn is_paintable_node(t: &NodeType) -> bool {
    matches!(
        t,
        NodeType::PaintedHeightmap | NodeType::PaintedTexture | NodeType::Sculpt
    )
}
