//! Per-node executor dispatch.
//!
//! Each node's compute lives in `exec/<family>/<node>.rs` as a `pub fn
//! exec(&ExecCtx)`, mirroring its `bar-graph` descriptor file. `EXEC` is a
//! `NodeType`-keyed table built from each family's `register()`. `CpuExecutor`
//! dispatches every node through that table.

use std::collections::HashMap;
use std::sync::LazyLock;

use bar_graph::{EvalError, NodeExecutor, NodeType, ParamValue, PortValue};

pub mod channels;
pub mod combiners;
pub mod expr;
pub mod filters;
pub mod io;
pub mod layout;
pub mod misc;
pub mod noise;
pub mod paint;
pub mod scalar;
pub mod selectors;
pub mod shared;
pub mod terminal;
pub mod texture;

/// The inputs every node executor receives -- bundles the positional args the
/// `NodeExecutor::execute` trait method passes, so a per-node fn takes one
/// struct instead of seven parameters.
pub struct ExecCtx<'a> {
    pub params: &'a HashMap<String, ParamValue>,
    pub inputs: &'a HashMap<String, PortValue>,
    pub hm_w: u32,
    pub hm_h: u32,
    pub tex_w: u32,
    pub tex_h: u32,
}

pub type ExecFn = fn(&ExecCtx) -> Result<HashMap<String, PortValue>, EvalError>;

pub static EXEC: LazyLock<HashMap<NodeType, ExecFn>> = LazyLock::new(|| {
    let mut m: HashMap<NodeType, ExecFn> = HashMap::new();
    noise::register(&mut m);
    combiners::register(&mut m);
    channels::register(&mut m);
    selectors::register(&mut m);
    filters::register(&mut m);
    texture::register(&mut m);
    layout::register(&mut m);
    paint::register(&mut m);
    io::register(&mut m);
    terminal::register(&mut m);
    misc::register(&mut m);
    expr::register(&mut m);
    scalar::register(&mut m);
    m
});

/// Run a node's executor, or `None` if no executor is registered for it.
pub fn dispatch(
    node_type: &NodeType,
    ctx: &ExecCtx,
) -> Option<Result<HashMap<String, PortValue>, EvalError>> {
    EXEC.get(node_type).map(|f| f(ctx))
}

/// Executor that runs node operations using CPU compute.
/// GPU execution can be added later without changing the graph layer.
pub struct CpuExecutor;

impl NodeExecutor for CpuExecutor {
    fn execute(
        &self,
        node_type: &NodeType,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
        hm_width: u32,
        hm_height: u32,
        tex_width: u32,
        tex_height: u32,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        dispatch(
            node_type,
            &ExecCtx {
                params,
                inputs,
                hm_w: hm_width,
                hm_h: hm_height,
                tex_w: tex_width,
                tex_h: tex_height,
            },
        )
        .unwrap_or_else(|| Ok(HashMap::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuExecutor, EXEC};
    use bar_data::Heightmap;
    use bar_graph::{
        evaluate_graph, get_heightmap_output, GraphEngine, Node, NodeExecutor, NodeId, NodeType,
        ParamValue, PortId, PortValue,
    };
    use std::collections::HashMap;

    /// Every migrated descriptor must have an executor and vice-versa. This
    /// cross-crate check replaces the old per-match compiler exhaustiveness:
    /// a node with a `NodeDef` but no `exec` (or the reverse) fails here.
    #[test]
    fn descriptors_and_executors_are_paired() {
        for d in bar_graph::nodes::all_defs() {
            assert!(
                EXEC.contains_key(&d.node_type),
                "descriptor {:?} has no registered executor",
                d.node_type
            );
        }
        for nt in EXEC.keys() {
            assert!(
                bar_graph::nodes::def(nt).is_some(),
                "executor {:?} has no descriptor",
                nt
            );
        }
    }

    fn const_hm(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w as usize) * (h as usize)]).unwrap()
    }

    #[test]
    fn test_end_to_end_graph_evaluation() {
        let executor = CpuExecutor;
        let mut graph = GraphEngine::new();

        let noise = Node::new(NodeId(0), NodeType::PerlinNoise, "Noise");
        let bundler = Node::new(NodeId(0), NodeType::FinalComposition, "Final Composition");
        let noise_id = graph.add_node(noise);
        let bundler_id = graph.add_node(bundler);

        if let Some(node) = graph.get_node_mut(noise_id) {
            node.params
                .insert("frequency".to_string(), ParamValue::Float(4.0));
            node.params
                .insert("octaves".to_string(), ParamValue::UInt(4));
            node.params.insert("seed".to_string(), ParamValue::UInt(1));
        }

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

        let results = evaluate_graph(&graph, &executor, 64, 64, 64, 64).unwrap();
        let hm = get_heightmap_output(&graph, &results).unwrap();

        assert_eq!(hm.width(), 64);
        assert_eq!(hm.height(), 64);
        let mean: f32 = hm.data().iter().sum::<f32>() / hm.data().len() as f32;
        assert!(
            mean > 0.1 && mean < 0.9,
            "Expected varied noise, got mean={mean}"
        );
    }

    #[test]
    fn blend_node_uses_apply_modulation_helper() {
        // factor=1, mask=0 -> output equals `a` (mask gates the blend back to a).
        let executor = CpuExecutor;
        let a = const_hm(2, 2, 0.1);
        let b = const_hm(2, 2, 0.9);
        let mask = const_hm(2, 2, 0.0);
        let params = HashMap::from([("factor".to_string(), ParamValue::Float(1.0))]);
        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
            ("mask".to_string(), PortValue::Mask(mask)),
        ]);
        let result = executor
            .execute(&NodeType::Blend, &params, &inputs, 2, 2, 2, 2)
            .unwrap();
        let PortValue::Heightmap(hm) = result.get("output").unwrap() else {
            panic!("expected heightmap")
        };
        for &v in hm.data() {
            assert!(
                (v - 0.1).abs() < 1e-6,
                "blend with mask=0 should keep `a`, got {v}"
            );
        }
    }
}

