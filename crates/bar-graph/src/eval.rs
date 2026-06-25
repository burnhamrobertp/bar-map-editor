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

        // Scalar-parameter graph: any inbound `PortValue::Scalar` whose port
        // name matches an existing param key overrides that param (coerced to
        // the param's declared type). The topo sort guarantees the scalar
        // producer ran first; executors stay oblivious and read params as usual.
        let effective_params = apply_scalar_bindings(&node.params, &inputs);

        // Execute the node. Per-node failures are localised: the
        // failing node produces nothing; everything else proceeds.
        match executor.execute(
            &node.node_type,
            &effective_params,
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

/// Coerce a scalar wire value into the same `ParamValue` variant as the param
/// it overrides. Float/UInt/Int round-trip the number; any other variant keeps
/// its existing value (a scalar can't sensibly drive a String/Bool/Vec2/Spline).
pub fn coerce_scalar(existing: &ParamValue, s: f32) -> ParamValue {
    match existing {
        ParamValue::Float(_) => ParamValue::Float(s),
        ParamValue::UInt(_) => ParamValue::UInt(s.round().max(0.0) as u32),
        ParamValue::Int(_) => ParamValue::Int(s.round() as i32),
        other => other.clone(),
    }
}

/// Build the param map an executor actually sees: `params` with any inbound
/// `PortValue::Scalar` overriding the same-named existing param (coerced to
/// that param's type). Scalar inputs whose name isn't an existing param key are
/// ignored. Returns a clone of `params` unchanged when no scalar binds.
fn apply_scalar_bindings(
    params: &HashMap<String, ParamValue>,
    inputs: &HashMap<String, PortValue>,
) -> HashMap<String, ParamValue> {
    let mut effective = params.clone();
    for (port_name, value) in inputs {
        if let PortValue::Scalar(s) = value {
            if let Some(existing) = effective.get(port_name) {
                let coerced = coerce_scalar(existing, *s);
                effective.insert(port_name.clone(), coerced);
            }
        }
    }
    effective
}

/// Get the heightmap wired to the Bundler's `heightmap` port.
pub fn get_heightmap_output(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    get_bundler_heightmap(graph, outputs, "heightmap")
}

/// Get a heightmap suitable for the interactive preview.
///
/// When the graph contains a Bundler, ONLY the value wired to its
/// `heightmap` port is used -- disconnections show up in the viewport as
/// the heightmap going away, which is the user-visible behaviour you'd
/// expect from unhooking the wire.
///
/// When the graph has no Bundler at all (early bring-up, stub graphs,
/// CLI tests with synthetic recipes), we fall back to the last evaluated
/// Heightmap in topological order so the viewport stays live. Previously
/// this fallback fired any time the Bundler's `heightmap` port was unwired,
/// which silently masked disconnections -- the disconnected node was still
/// being evaluated and its output was being picked up by the topo walk.
pub fn get_preview_heightmap(graph: &GraphEngine, outputs: &NodeOutputs) -> Option<Heightmap> {
    let has_bundler = graph
        .nodes()
        .iter()
        .any(|(_, n)| n.node_type == NodeType::FinalComposition);
    if has_bundler {
        return get_bundler_heightmap(graph, outputs, "heightmap");
    }
    // No Bundler: fall back to the last Heightmap value in topo order.
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
        if node.node_type == NodeType::FinalComposition {
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
        if node.node_type == NodeType::FinalComposition {
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
                NodeType::FinalComposition => {
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
        let bundler = Node::new(NodeId(0), NodeType::FinalComposition, "Bundler");
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

    // ── Scalar-parameter-graph contract ─────────────────────────────────────

    #[test]
    fn coerce_scalar_preserves_param_type() {
        assert!(matches!(
            coerce_scalar(&ParamValue::Float(0.0), 7.5),
            ParamValue::Float(f) if f == 7.5
        ));
        // UInt rounds and clamps at 0.
        assert!(matches!(
            coerce_scalar(&ParamValue::UInt(1), 3.6),
            ParamValue::UInt(4)
        ));
        assert!(matches!(
            coerce_scalar(&ParamValue::UInt(1), -2.0),
            ParamValue::UInt(0)
        ));
        // Int rounds (to nearest), keeps sign.
        assert!(matches!(
            coerce_scalar(&ParamValue::Int(0), -2.4),
            ParamValue::Int(-2)
        ));
        // Non-numeric params are left as-is.
        assert!(matches!(
            coerce_scalar(&ParamValue::Bool(true), 1.0),
            ParamValue::Bool(true)
        ));
    }

    #[test]
    fn apply_scalar_bindings_overrides_matching_param() {
        let mut params = HashMap::new();
        params.insert("frequency".to_string(), ParamValue::Float(4.0));
        params.insert("octaves".to_string(), ParamValue::UInt(6));

        let mut inputs = HashMap::new();
        inputs.insert("frequency".to_string(), PortValue::Scalar(7.0));
        // Wire on a non-existent param key: ignored.
        inputs.insert("not_a_param".to_string(), PortValue::Scalar(99.0));
        // A non-scalar input on a param-named port: ignored (wrong value kind).
        inputs.insert(
            "octaves".to_string(),
            PortValue::Heightmap(Heightmap::new(2, 2).unwrap()),
        );

        let eff = apply_scalar_bindings(&params, &inputs);
        assert!(matches!(eff.get("frequency"), Some(ParamValue::Float(f)) if *f == 7.0));
        // octaves unchanged (heightmap input is not a scalar).
        assert!(matches!(eff.get("octaves"), Some(ParamValue::UInt(6))));
        assert!(!eff.contains_key("not_a_param"));
    }

    /// Executor that echoes the (effective) `frequency` param as a Scalar so a
    /// test can observe whether a scalar wire overrode the literal at eval time.
    struct FreqProbe;
    impl NodeExecutor for FreqProbe {
        fn execute(
            &self,
            node_type: &NodeType,
            params: &HashMap<String, ParamValue>,
            _inputs: &HashMap<String, PortValue>,
            _hw: u32,
            _hh: u32,
            _tw: u32,
            _th: u32,
        ) -> Result<HashMap<String, PortValue>, EvalError> {
            let mut out = HashMap::new();
            match node_type {
                NodeType::ScalarValue => {
                    let v = match params.get("value") {
                        Some(ParamValue::Float(f)) => *f,
                        _ => 0.0,
                    };
                    out.insert("output".to_string(), PortValue::Scalar(v));
                }
                _ => {
                    let f = match params.get("frequency") {
                        Some(ParamValue::Float(f)) => *f,
                        _ => -1.0,
                    };
                    out.insert("output".to_string(), PortValue::Scalar(f));
                }
            }
            Ok(out)
        }
    }

    #[test]
    fn scalar_wire_overrides_param_through_eval() {
        let mut graph = GraphEngine::new();
        let sv = graph.add_node(Node::new(NodeId(0), NodeType::ScalarValue, "S"));
        let noise = graph.add_node(Node::new(NodeId(0), NodeType::PerlinNoise, "N"));

        // Set the scalar source to 7.0.
        graph
            .get_node_mut(sv)
            .unwrap()
            .params
            .insert("value".into(), ParamValue::Float(7.0));

        // Wire S.output -> N.frequency (auto-appended scalar port).
        graph
            .connect(
                PortId {
                    node_id: sv,
                    port_name: "output".into(),
                },
                PortId {
                    node_id: noise,
                    port_name: "frequency".into(),
                },
            )
            .unwrap();

        let outputs = evaluate_graph(&graph, &FreqProbe, 8, 8, 8, 8).unwrap();
        let probed = outputs.get(&noise).and_then(|o| o.get("output"));
        // The literal frequency default is 4.0; the wire must have overridden it.
        assert!(matches!(probed, Some(PortValue::Scalar(f)) if *f == 7.0));
    }

    #[test]
    fn unconnected_scalar_port_keeps_literal() {
        let mut graph = GraphEngine::new();
        let noise = graph.add_node(Node::new(NodeId(0), NodeType::PerlinNoise, "N"));
        graph
            .get_node_mut(noise)
            .unwrap()
            .params
            .insert("frequency".into(), ParamValue::Float(4.0));

        let outputs = evaluate_graph(&graph, &FreqProbe, 8, 8, 8, 8).unwrap();
        let probed = outputs.get(&noise).and_then(|o| o.get("output"));
        assert!(matches!(probed, Some(PortValue::Scalar(f)) if *f == 4.0));
    }
}
