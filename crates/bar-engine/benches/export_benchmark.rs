//! End-to-end export pipeline benchmarks.
//!
//! Measures the full path: graph evaluation → bundler execution → disk write.
//! These benchmarks use a temp directory for output so they exercise real I/O.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use bar_engine::recipe::{MapSettings, OutputConfig, Recipe};
use bar_engine::{execute_bundlers, CpuExecutor};
use bar_graph::{evaluate_graph, GraphEngine, Node, NodeId, NodeType, ParamValue, PortId};

const RESOLUTIONS: &[u32] = &[256, 512, 1024];

/// Build a minimal graph: PerlinNoise → Bundler with spring-smf target.
fn build_graph() -> GraphEngine {
    let mut graph = GraphEngine::new();

    let mut noise_node = Node::new(NodeId(0), NodeType::PerlinNoise, "Noise");
    noise_node
        .params
        .insert("frequency".to_string(), ParamValue::Float(2.0));
    noise_node
        .params
        .insert("octaves".to_string(), ParamValue::UInt(4));
    noise_node
        .params
        .insert("persistence".to_string(), ParamValue::Float(0.5));
    noise_node
        .params
        .insert("lacunarity".to_string(), ParamValue::Float(2.0));
    noise_node
        .params
        .insert("seed".to_string(), ParamValue::UInt(0));
    let noise_id = graph.add_node(noise_node);

    let mut bundler_node = Node::new(NodeId(0), NodeType::Bundler, "BAR Export");
    bundler_node.params.insert(
        "target".to_string(),
        ParamValue::String("spring-smf".to_string()),
    );
    let bundler_id = graph.add_node(bundler_node);

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

    graph
}

fn bench_graph_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_evaluation");
    group.sample_size(10);

    let executor = CpuExecutor;
    let graph = build_graph();

    for &size in RESOLUTIONS {
        group.bench_with_input(
            BenchmarkId::new("noise_to_bundler", size),
            &size,
            |b, &size| {
                b.iter(|| evaluate_graph(&graph, &executor, size, size, size, size).unwrap());
            },
        );
    }
    group.finish();
}

fn bench_full_export(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_export");
    // Fewer samples — disk I/O makes these slower
    group.sample_size(10);

    let executor = CpuExecutor;
    let graph = build_graph();
    let tmp = std::env::temp_dir().join("om_bench_export");
    std::fs::create_dir_all(&tmp).ok();

    for &size in RESOLUTIONS {
        group.bench_with_input(BenchmarkId::new("noise_to_sd7", size), &size, |b, &size| {
            let recipe = Recipe {
                schema_version: bar_project::RECIPE_SCHEMA_VERSION,
                name: "bench".to_string(),
                shortname: None,
                description: String::new(),
                author: None,
                version: None,
                nodes: Vec::new(),
                connections: Vec::new(),
                output: OutputConfig {
                    width: size,
                    height: size,
                    map_settings: MapSettings::default(),
                },
                features: Vec::new(),
            };
            b.iter(|| {
                let outputs = evaluate_graph(&graph, &executor, size, size, size, size).unwrap();
                let _ = execute_bundlers(&graph, &outputs, &recipe, &tmp, None, None);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_graph_evaluation, bench_full_export);
criterion_main!(benches);
