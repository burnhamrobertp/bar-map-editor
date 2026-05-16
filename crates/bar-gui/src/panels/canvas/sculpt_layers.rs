//! Derive the sculpt layer panel contents from the live node graph.
//!
//! `compute_sculpt_layers` walks from the Bundler inward, emitting only
//! nodes that are relevant to the painter:
//!
//! - **Paintable** source nodes (PaintedHeightmap, PaintedTexture, Sculpt)
//!   become selectable layer rows.
//! - **Combiner** nodes (Blend, Add, LayerBlend, etc.) that have at least
//!   one paintable descendant become non-selectable folder headers.
//! - All other nodes (generators, single-input filters, import nodes) are
//!   transparent -- traversed silently to find paintable descendants.
//!
//! The result is a minimal, Photoshop-style list: only layers the user can
//! actually paint on, grouped under any combiners that organize them.

use std::collections::{HashMap, HashSet};

use bar_graph::{GraphEngine, NodeId, NodeType};

/// One row in the sculpt layer panel.
pub struct SculptLayerEntry {
    pub node_id: NodeId,
    pub label: String,
    /// Bundler port channel: "heightmap", "texture", "metalmap", etc.
    pub channel: String,
    /// True when the user can paint directly on this node.
    pub is_paintable: bool,
    /// True when this entry is a combiner "folder" header.
    pub is_compositor: bool,
    /// Nesting level inside combiner folders (0 = top-level).
    pub indent: u8,
}

/// Derive the layer panel contents from the current graph state.
///
/// Returns an empty Vec when no Bundler node exists or no paintable
/// nodes are reachable from it. Only paintable nodes and the combiners
/// that directly organize them are included.
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
        collect_relevant_items(graph, root_id, port_name, &depth, &mut visited, 0, &mut out);
    }

    out
}

/// Recursively walk inward from `node_id`, emitting only paintable nodes
/// and the combiners that organise them.
///
/// Returns `true` if at least one paintable entry was emitted for this
/// subtree (used by the caller to decide whether to emit a folder header).
fn collect_relevant_items(
    graph: &GraphEngine,
    node_id: NodeId,
    channel: &str,
    depth: &HashMap<NodeId, usize>,
    visited: &mut HashSet<NodeId>,
    indent: u8,
    out: &mut Vec<SculptLayerEntry>,
) -> bool {
    if !visited.insert(node_id) {
        return false;
    }
    let Some(node) = graph.get_node(node_id) else {
        return false;
    };

    // Paintable source: emit a selectable row and stop recursing.
    if is_paintable_node(&node.node_type) {
        out.push(SculptLayerEntry {
            node_id,
            label: effective_label(&node.label, &node.node_type),
            channel: channel.to_string(),
            is_paintable: true,
            is_compositor: false,
            indent,
        });
        return true;
    }

    // Sort inputs by descending topo depth (Bundler-closest first).
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

    if is_combiner_node(&node.node_type) {
        // Combiner: collect children first, emit folder header only if any are paintable.
        let mut child_out: Vec<SculptLayerEntry> = Vec::new();
        let mut had_paintable = false;
        for (child_id, _) in inputs {
            if collect_relevant_items(
                graph,
                child_id,
                channel,
                depth,
                visited,
                indent + 1,
                &mut child_out,
            ) {
                had_paintable = true;
            }
        }
        if had_paintable {
            out.push(SculptLayerEntry {
                node_id,
                label: effective_label(&node.label, &node.node_type),
                channel: channel.to_string(),
                is_paintable: false,
                is_compositor: true,
                indent,
            });
            out.extend(child_out);
        }
        had_paintable
    } else {
        // Transparent node (filter, generator, import, etc.): traverse silently.
        let mut any_found = false;
        for (child_id, _) in inputs {
            if collect_relevant_items(graph, child_id, channel, depth, visited, indent, out) {
                any_found = true;
            }
        }
        any_found
    }
}

fn is_paintable_node(t: &NodeType) -> bool {
    matches!(t, NodeType::PaintedHeightmap | NodeType::PaintedTexture)
}

fn is_combiner_node(t: &NodeType) -> bool {
    matches!(
        t,
        NodeType::Blend
            | NodeType::Add
            | NodeType::Subtract
            | NodeType::Multiply
            | NodeType::Max
            | NodeType::Min
            | NodeType::LayerBlend
            | NodeType::MaskSelect
    )
}

fn effective_label(label: &str, node_type: &NodeType) -> String {
    if !label.is_empty() {
        return label.to_string();
    }
    node_type_fallback_name(node_type).to_string()
}

fn node_type_fallback_name(t: &NodeType) -> &'static str {
    match t {
        NodeType::Blend => "Blend",
        NodeType::Add => "Add",
        NodeType::Subtract => "Subtract",
        NodeType::Multiply => "Multiply",
        NodeType::Max => "Max",
        NodeType::Min => "Min",
        NodeType::LayerBlend => "Layer Blend",
        NodeType::MaskSelect => "Mask Select",
        NodeType::PaintedHeightmap => "Painted Heightmap",
        NodeType::PaintedTexture => "Painted Texture",
        _ => "Node",
    }
}
