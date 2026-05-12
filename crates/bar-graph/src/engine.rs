use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

use crate::node::{Node, NodeId};
use crate::port::PortId;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("node not found: {0:?}")]
    NodeNotFound(NodeId),

    #[error("cycle detected in graph")]
    CycleDetected,

    #[error("incompatible port types on connection")]
    IncompatiblePorts,

    #[error("port not found: {0:?}")]
    PortNotFound(PortId),
}

/// A connection between two ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub from: PortId,
    pub to: PortId,
}

/// The graph engine: manages nodes, connections, and evaluation order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEngine {
    nodes: HashMap<NodeId, Node>,
    connections: Vec<Connection>,
    next_id: u64,
    revision: u64,
}

impl Default for GraphEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphEngine {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            connections: Vec::new(),
            next_id: 1,
            revision: 0,
        }
    }

    /// Get the current revision number (incremented on each mutation).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Add a node to the graph. Returns its assigned ID.
    pub fn add_node(&mut self, mut node: Node) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        node.id = id;
        self.nodes.insert(id, node);
        self.revision += 1;
        id
    }

    /// Remove a node and all its connections.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(), GraphError> {
        if self.nodes.remove(&id).is_none() {
            return Err(GraphError::NodeNotFound(id));
        }
        self.connections
            .retain(|c| c.from.node_id != id && c.to.node_id != id);
        self.revision += 1;
        Ok(())
    }

    /// Connect an output port to an input port.
    pub fn connect(&mut self, from: PortId, to: PortId) -> Result<(), GraphError> {
        // Validate nodes exist
        let from_node = self
            .nodes
            .get(&from.node_id)
            .ok_or(GraphError::NodeNotFound(from.node_id))?;
        let to_node = self
            .nodes
            .get(&to.node_id)
            .ok_or(GraphError::NodeNotFound(to.node_id))?;

        // Validate source port is an output
        let source_port = from_node
            .outputs
            .iter()
            .find(|p| p.name == from.port_name)
            .ok_or_else(|| GraphError::PortNotFound(from.clone()))?;

        // Validate dest port is an input
        let dest_port = to_node
            .inputs
            .iter()
            .find(|p| p.name == to.port_name)
            .ok_or_else(|| GraphError::PortNotFound(to.clone()))?;

        // Validate port kinds are compatible. SubgraphInput / SubgraphOutput
        // boundary nodes are kind-polymorphic on their "value" port -- the
        // kind is auto-inferred from the connection after the fact by
        // recompute_all_subgraph_io. Skip validation on both sides.
        let io_value_bypass = (matches!(
            to_node.node_type,
            crate::node::NodeType::SubgraphInput | crate::node::NodeType::SubgraphOutput
        ) && to.port_name == "value")
            || (matches!(
                from_node.node_type,
                crate::node::NodeType::SubgraphInput | crate::node::NodeType::SubgraphOutput
            ) && from.port_name == "value");
        if !io_value_bypass && !source_port.kind.compatible_with(dest_port.kind) {
            return Err(GraphError::IncompatiblePorts);
        }

        let cardinality = dest_port.cardinality;

        // For single-input ports, remove existing connection
        if cardinality == crate::port::PortCardinality::One {
            self.connections.retain(|c| c.to != to);
        }

        let target_node_id = to.node_id;
        self.connections.push(Connection { from, to });

        // Mark downstream node as dirty
        if let Some(node) = self.nodes.get_mut(&target_node_id) {
            node.mark_dirty();
        }

        self.revision += 1;
        Ok(())
    }

    /// Disconnect a specific connection.
    pub fn disconnect(&mut self, from: &PortId, to: &PortId) {
        self.connections.retain(|c| &c.from != from || &c.to != to);
        if let Some(node) = self.nodes.get_mut(&to.node_id) {
            node.mark_dirty();
        }
        self.revision += 1;
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Get a mutable node by ID.
    ///
    /// Bumps the graph's revision counter unconditionally — callers
    /// typically use this to mutate node params, and the consumers of
    /// `revision()` (the progressive preview gating, validation
    /// fingerprint, autosave) need to see the change. Skipping the
    /// bump when the caller happens to be read-only is not worth the
    /// risk of forgetting to bump on a real mutation; a redundant
    /// preview pass is far cheaper than a stale viewport.
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        if self.nodes.contains_key(&id) {
            self.revision += 1;
        }
        self.nodes.get_mut(&id)
    }

    /// Get all nodes.
    pub fn nodes(&self) -> &HashMap<NodeId, Node> {
        &self.nodes
    }

    /// Get all nodes mutably. Bumps the revision counter unconditionally
    /// because callers will use this to mutate node params.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = (&NodeId, &mut Node)> {
        self.revision += 1;
        self.nodes.iter_mut()
    }

    /// Get all connections.
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    /// Compute topological sort of the graph for evaluation order.
    /// Returns nodes in dependency order (sources first).
    pub fn topological_sort(&self) -> Result<Vec<NodeId>, GraphError> {
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // Initialize
        for &id in self.nodes.keys() {
            in_degree.insert(id, 0);
            adjacency.insert(id, Vec::new());
        }

        // Build adjacency from connections
        for conn in &self.connections {
            let from_node = conn.from.node_id;
            let to_node = conn.to.node_id;
            adjacency.get_mut(&from_node).unwrap().push(to_node);
            *in_degree.get_mut(&to_node).unwrap() += 1;
        }

        // Kahn's algorithm
        let mut queue: Vec<NodeId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();
        queue.sort_by_key(|id| id.0); // Deterministic ordering

        let mut sorted = Vec::new();
        while let Some(id) = queue.pop() {
            sorted.push(id);
            if let Some(neighbors) = adjacency.get(&id) {
                for &neighbor in neighbors {
                    let deg = in_degree.get_mut(&neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(neighbor);
                    }
                }
            }
        }

        if sorted.len() != self.nodes.len() {
            return Err(GraphError::CycleDetected);
        }

        Ok(sorted)
    }

    /// Get the list of dirty nodes that need recomputation.
    pub fn dirty_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.dirty)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Hash of the subgraph upstream of `root` (inclusive). Only nodes and
    /// connections reachable by following inputs backwards from `root` are
    /// included; nodes elsewhere in the graph don't affect the result.
    ///
    /// This lets the preview cache key stay stable when the user adds,
    /// deletes, or edits nodes that have no path to the preview target.
    pub fn upstream_content_hash(&self, root: NodeId) -> u64 {
        use crate::node::ParamValue;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // BFS backwards from root
        let mut visited: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if visited.insert(id) {
                for conn in &self.connections {
                    if conn.to.node_id == id && !visited.contains(&conn.from.node_id) {
                        stack.push(conn.from.node_id);
                    }
                }
            }
        }

        let mut h = DefaultHasher::new();

        // Hash nodes in deterministic order
        let mut node_ids: Vec<NodeId> = visited.iter().copied().collect();
        node_ids.sort_by_key(|n| n.0);
        for nid in &node_ids {
            nid.0.hash(&mut h);
            if let Some(node) = self.nodes.get(nid) {
                node.node_type.hash(&mut h);
                let mut params: Vec<_> = node.params.iter().collect();
                params.sort_by_key(|(k, _)| k.as_str());
                for (k, v) in params {
                    k.hash(&mut h);
                    match v {
                        ParamValue::Float(f) => f.to_bits().hash(&mut h),
                        ParamValue::Int(i) => i.hash(&mut h),
                        ParamValue::UInt(u) => u.hash(&mut h),
                        ParamValue::Bool(b) => b.hash(&mut h),
                        ParamValue::String(s) => s.hash(&mut h),
                        ParamValue::Vec2([a, b]) => {
                            a.to_bits().hash(&mut h);
                            b.to_bits().hash(&mut h);
                        }
                    }
                }
            }
        }

        // Hash connections between upstream nodes
        let mut conns: Vec<_> = self
            .connections
            .iter()
            .filter(|c| visited.contains(&c.from.node_id) && visited.contains(&c.to.node_id))
            .collect();
        conns.sort_by_key(|c| {
            (
                c.from.node_id.0,
                c.from.port_name.as_str(),
                c.to.node_id.0,
                c.to.port_name.as_str(),
            )
        });
        for c in conns {
            c.from.node_id.0.hash(&mut h);
            c.from.port_name.hash(&mut h);
            c.to.node_id.0.hash(&mut h);
            c.to.port_name.hash(&mut h);
        }

        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeType;

    #[test]
    fn test_add_remove_nodes() {
        let mut engine = GraphEngine::new();
        let node = Node::new(NodeId(0), NodeType::PerlinNoise, "Perlin");
        let id = engine.add_node(node);
        assert!(engine.get_node(id).is_some());

        engine.remove_node(id).unwrap();
        assert!(engine.get_node(id).is_none());
    }

    #[test]
    fn test_connect_nodes() {
        let mut engine = GraphEngine::new();

        let noise = Node::new(NodeId(0), NodeType::PerlinNoise, "Noise");
        let output = Node::new(NodeId(0), NodeType::Bundler, "Bundler");
        let noise_id = engine.add_node(noise);
        let output_id = engine.add_node(output);

        let from = PortId {
            node_id: noise_id,
            port_name: "output".to_string(),
        };
        let to = PortId {
            node_id: output_id,
            port_name: "heightmap".to_string(),
        };

        engine.connect(from, to).unwrap();
        assert_eq!(engine.connections().len(), 1);
    }

    #[test]
    fn test_topological_sort() {
        let mut engine = GraphEngine::new();

        let noise = Node::new(NodeId(0), NodeType::PerlinNoise, "Noise");
        let blur = Node::new(NodeId(0), NodeType::Blur, "Blur");
        let output = Node::new(NodeId(0), NodeType::Bundler, "Bundler");

        let noise_id = engine.add_node(noise);
        let blur_id = engine.add_node(blur);
        let output_id = engine.add_node(output);

        // Noise → Blur → Bundler
        engine
            .connect(
                PortId {
                    node_id: noise_id,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: blur_id,
                    port_name: "input".to_string(),
                },
            )
            .unwrap();
        engine
            .connect(
                PortId {
                    node_id: blur_id,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: output_id,
                    port_name: "heightmap".to_string(),
                },
            )
            .unwrap();

        let order = engine.topological_sort().unwrap();
        let noise_pos = order.iter().position(|&id| id == noise_id).unwrap();
        let blur_pos = order.iter().position(|&id| id == blur_id).unwrap();
        let output_pos = order.iter().position(|&id| id == output_id).unwrap();

        assert!(noise_pos < blur_pos);
        assert!(blur_pos < output_pos);
    }

    /// Every API surface that mutates a node must bump the revision
    /// counter so cache-invalidation consumers (the progressive
    /// preview gating, validation fingerprint, autosave dirty
    /// tracking) see the change. This test pins down each mutation
    /// path so a future refactor can't quietly skip a bump.
    #[test]
    fn revision_bumps_on_every_mutation_path() {
        let mut engine = GraphEngine::new();

        // add_node
        let r0 = engine.revision();
        let a = engine.add_node(Node::new(NodeId(0), NodeType::PerlinNoise, "A"));
        let b = engine.add_node(Node::new(NodeId(0), NodeType::Bundler, "B"));
        assert!(engine.revision() > r0, "add_node should bump revision");

        // connect
        let r_pre_connect = engine.revision();
        engine
            .connect(
                PortId {
                    node_id: a,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: b,
                    port_name: "heightmap".to_string(),
                },
            )
            .unwrap();
        assert!(
            engine.revision() > r_pre_connect,
            "connect should bump revision"
        );

        // get_node_mut — the bug we're regressing on. A param edit via
        // the mutable reference returned here must be observable as a
        // revision change.
        let r_pre_param_edit = engine.revision();
        if let Some(node) = engine.get_node_mut(a) {
            node.params
                .insert("frequency".to_string(), crate::node::ParamValue::Float(8.0));
            node.mark_dirty();
        }
        assert!(
            engine.revision() > r_pre_param_edit,
            "get_node_mut must bump revision so the preview re-evaluates after a param edit",
        );

        // get_node_mut on a missing id must NOT bump (no observable
        // change for downstream consumers to react to).
        let missing = NodeId(9_999);
        let r_pre_missing = engine.revision();
        let _ = engine.get_node_mut(missing);
        assert_eq!(
            engine.revision(),
            r_pre_missing,
            "get_node_mut on a missing id should not bump revision",
        );

        // nodes_mut
        let r_pre_nodes_mut = engine.revision();
        for (_, _) in engine.nodes_mut() {}
        assert!(
            engine.revision() > r_pre_nodes_mut,
            "nodes_mut should bump revision"
        );

        // disconnect
        let r_pre_disconnect = engine.revision();
        engine.disconnect(
            &PortId {
                node_id: a,
                port_name: "output".to_string(),
            },
            &PortId {
                node_id: b,
                port_name: "heightmap".to_string(),
            },
        );
        assert!(
            engine.revision() > r_pre_disconnect,
            "disconnect should bump revision"
        );

        // remove_node
        let r_pre_remove = engine.revision();
        engine.remove_node(a).unwrap();
        assert!(
            engine.revision() > r_pre_remove,
            "remove_node should bump revision"
        );
    }

    #[test]
    fn subgraph_io_node_default_kind_is_heightmap() {
        let n = Node::new(NodeId(0), NodeType::SubgraphInput, "io");
        assert_eq!(n.inputs.len(), 1);
        assert_eq!(n.outputs.len(), 1);
        assert_eq!(n.inputs[0].kind, crate::port::PortKind::Heightmap);
        assert_eq!(n.outputs[0].kind, crate::port::PortKind::Heightmap);
        // Both ports share the same name so consumers can read/write
        // through "value" regardless of side.
        assert_eq!(n.inputs[0].name, "value");
        assert_eq!(n.outputs[0].name, "value");
    }

    #[test]
    fn sync_subgraph_io_kind_flips_both_sides_in_lockstep() {
        let mut n = Node::new(NodeId(0), NodeType::SubgraphInput, "io");
        // Default state: Heightmap.
        assert_eq!(n.inputs[0].kind, crate::port::PortKind::Heightmap);

        // Switch the kind param to Color and re-sync. Both sides flip.
        n.params.insert(
            "kind".to_string(),
            crate::node::ParamValue::String("Color".to_string()),
        );
        n.sync_subgraph_io_kind();
        assert_eq!(n.inputs[0].kind, crate::port::PortKind::Color);
        assert_eq!(n.outputs[0].kind, crate::port::PortKind::Color);

        // Garbage strings are a no-op — we don't fail loudly because
        // legacy projects might carry an unrecognised kind. Cap stays
        // at the previous value (Color, since we last sync'd to it).
        n.params.insert(
            "kind".to_string(),
            crate::node::ParamValue::String("BogusKind".to_string()),
        );
        n.sync_subgraph_io_kind();
        assert_eq!(n.inputs[0].kind, crate::port::PortKind::Color);
        assert_eq!(n.outputs[0].kind, crate::port::PortKind::Color);
    }

    #[test]
    fn sync_subgraph_io_kind_is_noop_for_other_node_types() {
        // Calling sync on a non-IO node should leave it untouched. We
        // pick a generator (PerlinNoise) since it has 0 inputs / 1
        // output — the function shouldn't add any.
        let mut n = Node::new(NodeId(0), NodeType::PerlinNoise, "perlin");
        let before_inputs = n.inputs.clone();
        let before_outputs = n.outputs.clone();
        // Even with a `kind` param set, sync is gated on node type.
        n.params.insert(
            "kind".to_string(),
            crate::node::ParamValue::String("Color".to_string()),
        );
        n.sync_subgraph_io_kind();
        assert_eq!(n.inputs, before_inputs);
        assert_eq!(n.outputs, before_outputs);
    }

    #[test]
    fn port_kinds_compatible_f32_field_set() {
        use crate::port::PortKind;
        // Same kind always compatible
        assert!(PortKind::Heightmap.compatible_with(PortKind::Heightmap));
        assert!(PortKind::Color.compatible_with(PortKind::Color));
        // All f32-field variants interchangeable
        assert!(PortKind::Heightmap.compatible_with(PortKind::Mask));
        assert!(PortKind::Heightmap.compatible_with(PortKind::Control));
        assert!(PortKind::Heightmap.compatible_with(PortKind::Density));
        assert!(PortKind::Mask.compatible_with(PortKind::Control));
        assert!(PortKind::Control.compatible_with(PortKind::Density));
        assert!(PortKind::Density.compatible_with(PortKind::Mask));
        // Rejections
        assert!(!PortKind::Color.compatible_with(PortKind::Control));
        assert!(!PortKind::Color.compatible_with(PortKind::Heightmap));
        assert!(!PortKind::Scalar.compatible_with(PortKind::Heightmap));
        assert!(!PortKind::File.compatible_with(PortKind::Mask));
    }

    #[test]
    fn port_placement_for_input() {
        use crate::port::{PortKind, PortPlacement};
        assert_eq!(
            PortPlacement::for_input(PortKind::Control),
            PortPlacement::Top(0)
        );
        assert_eq!(
            PortPlacement::for_input(PortKind::Density),
            PortPlacement::Top(1)
        );
        assert_eq!(
            PortPlacement::for_input(PortKind::Mask),
            PortPlacement::Bottom
        );
        assert_eq!(
            PortPlacement::for_input(PortKind::Heightmap),
            PortPlacement::Left
        );
        assert_eq!(
            PortPlacement::for_input(PortKind::Color),
            PortPlacement::Left
        );
    }
}
