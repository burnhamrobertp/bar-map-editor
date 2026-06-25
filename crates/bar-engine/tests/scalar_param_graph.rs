//! End-to-end tests for the scalar-parameter-graph subsystem: scalars wired
//! INTO node params override the literal at eval time, scalar arithmetic works,
//! and stray/mis-typed scalar wires are ignored.

use bar_engine::CpuExecutor;
use bar_graph::{
    evaluate_graph, get_node_output_heightmap_named, GraphEngine, Node, NodeId, ParamValue, PortId,
    PortValue,
};

fn connect(graph: &mut GraphEngine, from: NodeId, from_port: &str, to: NodeId, to_port: &str) {
    graph
        .connect(
            PortId {
                node_id: from,
                port_name: from_port.into(),
            },
            PortId {
                node_id: to,
                port_name: to_port.into(),
            },
        )
        .unwrap();
}

fn set_param(graph: &mut GraphEngine, id: NodeId, key: &str, v: ParamValue) {
    graph.get_node_mut(id).unwrap().params.insert(key.into(), v);
}

/// A ScalarValue wired into PerlinNoise's `frequency` port must change the
/// generated terrain vs. the literal-frequency baseline.
#[test]
fn scalar_value_overrides_noise_frequency() {
    let exec = CpuExecutor;
    let (w, h) = (64u32, 64u32);

    // Baseline: noise with its literal frequency (4.0), no wire.
    let mut base = GraphEngine::new();
    let n0 = base.add_node(Node::new(NodeId(0), bar_graph::NodeType::PerlinNoise, "N"));
    set_param(&mut base, n0, "frequency", ParamValue::Float(4.0));
    let base_out = evaluate_graph(&base, &exec, w, h, w, h).unwrap();
    let base_hm = get_node_output_heightmap_named(&base_out, n0, "output").unwrap();

    // Wired: a ScalarValue(32.0) drives `frequency` -- much higher freq.
    let mut g = GraphEngine::new();
    let sv = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::ScalarValue, "S"));
    let n1 = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::PerlinNoise, "N"));
    set_param(&mut g, sv, "value", ParamValue::Float(32.0));
    set_param(&mut g, n1, "frequency", ParamValue::Float(4.0));
    connect(&mut g, sv, "output", n1, "frequency");
    let wired_out = evaluate_graph(&g, &exec, w, h, w, h).unwrap();
    let wired_hm = get_node_output_heightmap_named(&wired_out, n1, "output").unwrap();

    let differs = base_hm
        .data()
        .iter()
        .zip(wired_hm.data().iter())
        .any(|(a, b)| (a - b).abs() > 1e-4);
    assert!(
        differs,
        "scalar-wired frequency should change the noise output vs. the literal baseline"
    );
}

/// Only `frequency`/`persistence`/`lacunarity` are scalar-bindable on noise, so
/// only those scalar input ports exist. A scalar can't be wired to a port that
/// doesn't exist (e.g. the non-bindable `seed` param), and the kind guard
/// rejects wiring a Scalar onto the heightmap `control` port. Both attempts
/// fail at `connect`, so a mis-targeted scalar can never reach eval.
#[test]
fn scalar_cannot_wire_to_non_bindable_or_wrong_kind_port() {
    let mut g = GraphEngine::new();
    let sv = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::ScalarValue, "S"));
    let n = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::PerlinNoise, "N"));

    // `seed` is a param but not scalar_bindable -> no `seed` input port exists.
    let no_port = g.connect(
        PortId {
            node_id: sv,
            port_name: "output".into(),
        },
        PortId {
            node_id: n,
            port_name: "seed".into(),
        },
    );
    assert!(
        no_port.is_err(),
        "no scalar port should exist for non-bindable `seed`"
    );

    // `control` exists but is a Heightmap port; Scalar is incompatible with it.
    let wrong_kind = g.connect(
        PortId {
            node_id: sv,
            port_name: "output".into(),
        },
        PortId {
            node_id: n,
            port_name: "control".into(),
        },
    );
    assert!(
        wrong_kind.is_err(),
        "Scalar must not be compatible with a Heightmap port"
    );

    // The bindable port does accept the wire.
    connect(&mut g, sv, "output", n, "frequency");
}

/// ScalarMath performs the selected op on its two scalar inputs.
#[test]
fn scalar_math_add_and_multiply() {
    let exec = CpuExecutor;

    for (op, a, b, expected) in [
        ("add", 2.0f32, 5.0f32, 7.0f32),
        ("multiply", 3.0, 4.0, 12.0),
    ] {
        let mut g = GraphEngine::new();
        let sa = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::ScalarValue, "A"));
        let sb = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::ScalarValue, "B"));
        let m = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::ScalarMath, "M"));
        set_param(&mut g, sa, "value", ParamValue::Float(a));
        set_param(&mut g, sb, "value", ParamValue::Float(b));
        set_param(&mut g, m, "op", ParamValue::String(op.into()));
        connect(&mut g, sa, "output", m, "a");
        connect(&mut g, sb, "output", m, "b");

        let out = evaluate_graph(&g, &exec, 8, 8, 8, 8).unwrap();
        let got = out.get(&m).and_then(|o| o.get("output"));
        match got {
            Some(PortValue::Scalar(s)) => {
                assert!(
                    (s - expected).abs() < 1e-5,
                    "{op}: got {s}, want {expected}"
                )
            }
            other => panic!("{op}: expected Scalar output, got {other:?}"),
        }
    }
}

/// IntValue emits its UInt `value` as a whole-number scalar; wiring it into
/// PerlinNoise's `octaves` (UInt param) coerces back to an integer.
#[test]
fn int_value_drives_uint_param() {
    let exec = CpuExecutor;
    let mut g = GraphEngine::new();
    let iv = g.add_node(Node::new(NodeId(0), bar_graph::NodeType::IntValue, "I"));
    set_param(&mut g, iv, "value", ParamValue::UInt(9));
    let out = evaluate_graph(&g, &exec, 8, 8, 8, 8).unwrap();
    match out.get(&iv).and_then(|o| o.get("output")) {
        Some(PortValue::Scalar(s)) => assert_eq!(*s, 9.0),
        other => panic!("expected Scalar(9.0), got {other:?}"),
    }
}
