use std::collections::HashMap;

use bar_data::Heightmap;
use thiserror::Error;

use crate::engine::GraphEngine;
use crate::node::{NodeId, NodeType, ParamValue};
use crate::port::PortValue;

#[derive(Error, Debug)]
pub enum EvalError {
    #[error("node {0:?} has no implementation")]
    NoImplementation(NodeId),

    #[error("missing input on port {port}")]
    MissingInput { port: String },

    #[error("graph error: {0}")]
    Graph(#[from] crate::engine::GraphError),

    #[error("compute error: {0}")]
    Compute(String),
}

/// Result cache for evaluated nodes.
pub type NodeOutputs = HashMap<NodeId, HashMap<String, PortValue>>;

/// A trait for executing node operations.
/// Implemented by the application layer to bridge graph → compute.
/// `Send + Sync` allows sharing a single executor via `Arc` across threads.
pub trait NodeExecutor: Send + Sync {
    fn execute(
        &self,
        node_type: &NodeType,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
        hm_width: u32,
        hm_height: u32,
        tex_width: u32,
        tex_height: u32,
    ) -> Result<HashMap<String, PortValue>, EvalError>;
}

/// Evaluate the entire graph and produce outputs for whichever
/// nodes can run. Per-node failures (e.g. a Sculpt node with its
/// `input` port unconnected) are **not** fatal — the failing node
/// simply produces no entry in the output map, and any node
/// downstream of it that needed its output will likewise fail to
/// gather inputs and be skipped. The graph keeps evaluating
/// everywhere else.
///
/// This matters because the 3D preview, CLI export, and bundler
/// pipeline all share this entry point. The preview should reflect
/// only what's wired to its target node — not be blanked because a
/// stray disconnected node lives elsewhere on the canvas. Callers
/// that need a specific output already check `outputs.get(...)`
/// for `None`, so a partial map is the correct shape.
///
/// `EvalError` is still returned for graph-structural failures
/// (cycles via `topological_sort`).
/// Like [`evaluate_graph`] but calls `on_progress` after each node with a
/// formatted `[XX%] label` string. Lets callers surface per-node progress
/// (e.g. into a log or status channel) without blocking the background thread.
/// The callback receives a borrowed `&str`; clone/send as needed.
pub fn evaluate_graph_with_progress(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    hm_width: u32,
    hm_height: u32,
    tex_width: u32,
    tex_height: u32,
    on_progress: &dyn Fn(&str),
) -> Result<NodeOutputs, EvalError> {
    let eval_order = graph.topological_sort()?;
    let total = eval_order.len().max(1);
    let mut outputs: NodeOutputs = HashMap::new();

    for (i, &node_id) in eval_order.iter().enumerate() {
        let node = graph.get_node(node_id).unwrap();

        // Gather inputs from upstream connections
        let mut inputs: HashMap<String, PortValue> = HashMap::new();
        for conn in graph.connections() {
            if conn.to.node_id == node_id {
                if let Some(upstream_outputs) = outputs.get(&conn.from.node_id) {
                    if let Some(value) = upstream_outputs.get(&conn.from.port_name) {
                        inputs.insert(conn.to.port_name.clone(), value.clone());
                    }
                }
            }
        }

        // Execute the node. Per-node failures are localised: the
        // failing node produces nothing; everything else proceeds.
        match executor.execute(
            &node.node_type,
            &node.params,
            &inputs,
            hm_width,
            hm_height,
            tex_width,
            tex_height,
        ) {
            Ok(node_outputs) => {
                outputs.insert(node_id, node_outputs);
            }
            Err(e) => {
                tracing::debug!("Skipping {:?} ({:?}): {:?}", node.node_type, node_id, e);
            }
        }

        let pct = (i + 1) * 100 / total;
        on_progress(&format!("[{pct:3}%] {}", node.label));
    }

    Ok(outputs)
}

pub fn evaluate_graph(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    hm_width: u32,
    hm_height: u32,
    tex_width: u32,
    tex_height: u32,
) -> Result<NodeOutputs, EvalError> {
    evaluate_graph_with_progress(
        graph,
        executor,
        hm_width,
        hm_height,
        tex_width,
        tex_height,
        &|_| {},
    )
}

/// Get the heightmap wired to the Bundler's `heightmap` port.
pub fn get_heightmap_output(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    get_bundler_heightmap(graph, outputs, "heightmap")
}

/// Get a heightmap suitable for the interactive preview.
///
/// Prefers the value wired to the Bundler's `heightmap` port. Falls back to
/// the last evaluated Heightmap in topological order so the viewport stays
/// live even in simple graphs without a Bundler.
pub fn get_preview_heightmap(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    if let Some(hm) = get_bundler_heightmap(graph, outputs, "heightmap") {
        return Some(hm);
    }
    // Fallback: last Heightmap value in topo order
    if let Ok(order) = graph.topological_sort() {
        for &node_id in order.iter().rev() {
            if let Some(node_outputs) = outputs.get(&node_id) {
                for val in node_outputs.values() {
                    if let PortValue::Heightmap(hm) = val {
                        return Some(hm.clone());
                    }
                }
            }
        }
    }
    None
}

/// Get the metalmap wired to the Bundler's `metalmap` port.
pub fn get_metalmap_output(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    get_bundler_heightmap(graph, outputs, "metalmap")
}

/// Get the typemap wired to the Bundler's `typemap` port.
pub fn get_typemap_output(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    get_bundler_heightmap(graph, outputs, "typemap")
}

/// Get the color texture wired to the Bundler's `texture` port.
pub fn get_texture_output(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
) -> Option<bar_data::ColorBuffer> {
    get_bundler_color(graph, outputs, "texture")
}

/// Get the normal map wired to the Bundler's `normalmap` port.
pub fn get_normalmap_output(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
) -> Option<bar_data::ColorBuffer> {
    get_bundler_color(graph, outputs, "normalmap")
}

/// Get the grass map wired to the Bundler's `grassmap` port.
pub fn get_grassmap_output(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    get_bundler_heightmap(graph, outputs, "grassmap")
}

/// Get the heightmap wired to a specific Bundler node's `heightmap` input port.
///
/// Used to drive the 3D preview for a particular bundler when multiple exist.
pub fn get_bundler_node_heightmap(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    bundler_node: NodeId,
) -> Option<Heightmap> {
    for conn in graph.connections() {
        if conn.to.node_id == bundler_node && conn.to.port_name == "heightmap" {
            if let Some(upstream) = outputs.get(&conn.from.node_id) {
                if let Some(PortValue::Heightmap(hm)) = upstream.get(&conn.from.port_name) {
                    return Some(hm.clone());
                }
            }
        }
    }
    None
}

/// Heightmap directly off a node's own `output` port. Used for
/// Preview-style mid-pipeline taps where the node itself produces
/// a heightmap (rather than consuming one as a Bundler does).
pub fn get_node_output_heightmap(outputs: &NodeOutputs, node_id: NodeId) -> Option<Heightmap> {
    let ports = outputs.get(&node_id)?;
    if let Some(PortValue::Heightmap(hm)) = ports.get("output") {
        return Some(hm.clone());
    }
    None
}

/// Get the texture wired to a specific Bundler node's `texture` input port.
///
/// Used to drive the 3D preview for a particular bundler when multiple exist.
pub fn get_bundler_node_texture(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    bundler_node: NodeId,
) -> Option<bar_data::ColorBuffer> {
    for conn in graph.connections() {
        if conn.to.node_id == bundler_node && conn.to.port_name == "texture" {
            if let Some(upstream) = outputs.get(&conn.from.node_id) {
                if let Some(PortValue::Color(cb)) = upstream.get(&conn.from.port_name) {
                    return Some(cb.clone());
                }
            }
        }
    }
    None
}

/// Read a node's named runtime output as a Heightmap. Works for
/// any node — Preview is just a node that happens to have its
/// inputs passed through as outputs by its executor, so the
/// viewport reads from it the same way any consumer reads from
/// any other node.
pub fn get_node_output_heightmap_named(
    outputs: &NodeOutputs,
    node_id: NodeId,
    port: &str,
) -> Option<Heightmap> {
    let ports = outputs.get(&node_id)?;
    if let Some(PortValue::Heightmap(hm)) = ports.get(port) {
        return Some(hm.clone());
    }
    None
}

/// Read a node's named runtime output as a Color buffer. Same
/// shape as `get_node_output_heightmap_named` but for color
/// outputs (textures, normal maps, etc.).
pub fn get_node_output_color_named(
    outputs: &NodeOutputs,
    node_id: NodeId,
    port: &str,
) -> Option<bar_data::ColorBuffer> {
    let ports = outputs.get(&node_id)?;
    if let Some(PortValue::Color(cb)) = ports.get(port) {
        return Some(cb.clone());
    }
    None
}

/// Look up the value connected to a named Heightmap port on any Bundler node.
fn get_bundler_heightmap(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    port: &str,
) -> Option<Heightmap> {
    for (node_id, node) in graph.nodes() {
        if node.node_type == NodeType::Bundler {
            for conn in graph.connections() {
                if conn.to.node_id == *node_id && conn.to.port_name == port {
                    if let Some(upstream) = outputs.get(&conn.from.node_id) {
                        if let Some(PortValue::Heightmap(hm)) = upstream.get(&conn.from.port_name) {
                            return Some(hm.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Look up the value connected to a named Color port on any Bundler node.
fn get_bundler_color(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    port: &str,
) -> Option<bar_data::ColorBuffer> {
    for (node_id, node) in graph.nodes() {
        if node.node_type == NodeType::Bundler {
            for conn in graph.connections() {
                if conn.to.node_id == *node_id && conn.to.port_name == port {
                    if let Some(upstream) = outputs.get(&conn.from.node_id) {
                        if let Some(PortValue::Color(cb)) = upstream.get(&conn.from.port_name) {
                            return Some(cb.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::port::PortId;

    /// A simple test executor that produces a constant heightmap for generators.
    struct TestExecutor;

    impl NodeExecutor for TestExecutor {
        fn execute(
            &self,
            node_type: &NodeType,
            _params: &HashMap<String, ParamValue>,
            inputs: &HashMap<String, PortValue>,
            hm_width: u32,
            hm_height: u32,
            _tex_width: u32,
            _tex_height: u32,
        ) -> Result<HashMap<String, PortValue>, EvalError> {
            let (width, height) = (hm_width, hm_height);
            let mut outputs = HashMap::new();

            match node_type {
                NodeType::PerlinNoise => {
                    let hm = Heightmap::new(width, height)
                        .map_err(|e| EvalError::Compute(e.to_string()))?;
                    outputs.insert("output".to_string(), PortValue::Heightmap(hm));
                }
                NodeType::Bundler => {
                    // Terminal node — pass inputs through keyed by port name
                    for (k, v) in inputs {
                        outputs.insert(k.clone(), v.clone());
                    }
                }
                _ => {
                    // Pass first input through as output
                    if let Some((_, value)) = inputs.iter().next() {
                        outputs.insert("output".to_string(), value.clone());
                    }
                }
            }

            Ok(outputs)
        }
    }

    #[test]
    fn test_evaluate_simple_graph() {
        let mut graph = GraphEngine::new();

        let noise = Node::new(NodeId(0), NodeType::PerlinNoise, "Noise");
        let bundler = Node::new(NodeId(0), NodeType::Bundler, "Bundler");
        let noise_id = graph.add_node(noise);
        let bundler_id = graph.add_node(bundler);

        graph
            .connect(
                PortId {
                    node_id: noise_id,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: bundler_id,
                    port_name: "heightmap".to_string(),
                },
            )
            .unwrap();

        let executor = TestExecutor;
        let results = evaluate_graph(&graph, &executor, 64, 64, 64, 64).unwrap();

        // Both nodes should have outputs
        assert!(results.contains_key(&noise_id));
        assert!(results.contains_key(&bundler_id));

        // Should be able to get the final heightmap via the Bundler port
        let hm = get_heightmap_output(&graph, &results);
        assert!(hm.is_some());
        assert_eq!(hm.unwrap().width(), 64);
    }
}
