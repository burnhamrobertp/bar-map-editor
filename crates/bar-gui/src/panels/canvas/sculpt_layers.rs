//! Derive the sculpt layer panel contents from the live node graph.
//!
//! `compute_sculpt_layers` walks from the Bundler inward, emitting source
//! nodes as selectable layer rows and compositor/filter nodes as non-selectable
//! folder headers. The resulting flat list, ordered by DFS traversal from the
//! Bundler, maps naturally onto a Photoshop-style layer stack.

use std::collections::{HashMap, HashSet};

use bar_graph::{GraphEngine, NodeId, NodeType};

/// One row in the sculpt layer panel.
///
/// Can be either a selectable source row (`is_compositor = false`) or a
/// non-selectable compositor folder header (`is_compositor = true`).
pub struct SculptLayerEntry {
    pub node_id: NodeId,
    pub label: String,
    /// Bundler port channel: "heightmap", "texture", "metalmap", etc.
    pub channel: String,
    /// True when the user can paint directly on this node.
    pub is_paintable: bool,
    /// True when this entry is a compositor/filter "folder" header.
    pub is_compositor: bool,
    /// Nesting level inside compositor folders (0 = top-level).
    pub indent: u8,
}

/// Derive the layer panel contents from the current graph state.
///
/// Returns an empty Vec when no Bundler node exists. Compositor nodes
/// appear as folder headers (non-selectable); source nodes appear as
/// selectable rows. The order mirrors the compositing stack: Bundler-closest
/// entries come first (analogous to Photoshop's top-of-stack).
pub fn compute_sculpt_layers(graph: &GraphEngine) -> Vec<SculptLayerEntry> {
    let Some(bundler_id) = graph
        .nodes()
        .iter()
        .find(|(_, n)| n.node_type == NodeType::Bundler)
        .map(|(&id, _)| id)
    else {
        return vec![];
    };

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

    let mut out = vec![];
    // Global visited set so a node shared across channels appears only once.
    let mut visited: HashSet<NodeId> = HashSet::new();

    for &port_name in &port_names {
        let Some(root_id) = graph
            .connections()
            .iter()
            .find(|c| c.to.node_id == bundler_id && c.to.port_name == port_name)
            .map(|c| c.from.node_id)
        else {
            continue;
        };
        collect_layer_items(graph, root_id, port_name, &depth, &mut visited, 0, &mut out);
    }

    out
}

/// Recursively walk inward from `node_id`, emitting layer entries.
///
/// Source nodes emit a single row entry. Compositor/filter nodes emit a
/// folder header followed by their inputs (sorted Bundler-closest first).
fn collect_layer_items(
    graph: &GraphEngine,
    node_id: NodeId,
    channel: &str,
    depth: &HashMap<NodeId, usize>,
    visited: &mut HashSet<NodeId>,
    indent: u8,
    out: &mut Vec<SculptLayerEntry>,
) {
    if !visited.insert(node_id) {
        return;
    }
    let Some(node) = graph.get_node(node_id) else {
        return;
    };

    if is_source_node(&node.node_type) {
        out.push(SculptLayerEntry {
            node_id,
            label: node.label.clone(),
            channel: channel.to_string(),
            is_paintable: is_paintable_node(&node.node_type),
            is_compositor: false,
            indent,
        });
    } else {
        // Compositor/filter: emit as a folder header, then recurse into inputs.
        out.push(SculptLayerEntry {
            node_id,
            label: node.label.clone(),
            channel: channel.to_string(),
            is_paintable: false,
            is_compositor: true,
            indent,
        });
        // Collect inputs, sorted by descending topo depth (Bundler-closest first).
        let mut inputs: Vec<(NodeId, usize)> = graph
            .connections()
            .iter()
            .filter(|c| c.to.node_id == node_id)
            .map(|c| {
                (
                    c.from.node_id,
                    depth.get(&c.from.node_id).copied().unwrap_or(0),
                )
            })
            .collect();
        inputs.sort_by_key(|&(_, d)| std::cmp::Reverse(d));
        for (child_id, _) in inputs {
            collect_layer_items(graph, child_id, channel, depth, visited, indent + 1, out);
        }
    }
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
