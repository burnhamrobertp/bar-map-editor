//! Editor (GPU) vs export (CPU) parity.
//!
//! A barproj must compile to the same terrain whether the editor evaluated it
//! on a GPU (`HybridExecutor`) or a GPU-less machine ran the CPU export
//! (`CpuExecutor`). Every node the HybridExecutor dispatches to the GPU is
//! checked here against the CPU path across the cases that have bitten us:
//! explicit params, omitted params (default drift), and `control`/`mask`
//! modulation (which the GPU paths must apply just like the CPU execs).
//! LightmapBake (Color output, not heightmap) is covered separately by
//! `bar_compute::gpu_lightmap` `gpu_matches_cpu_within_tolerance`.
//!
//! A failure here means a map that silently changes when moved between machines.
//! Skips when no GPU adapter is present (e.g. headless CI).

use bar_engine::recipe::Recipe;
use bar_engine::{CpuExecutor, HybridExecutor};
use bar_graph::{evaluate_graph, get_heightmap_output, NodeExecutor};

// 256x256 clears both GPU_NOISE_THRESHOLD (128) and GPU_FILTER_THRESHOLD (256),
// so noise, blur, and thermal erosion all take their GPU paths.
const DIM: u32 = 256;

fn recipe(filter_node: &str, extra_nodes: &str, extra_conns: &str) -> String {
    format!(
        r#"{{
          "name": "p", "author": "t", "description": "parity",
          "nodes": [
            {{ "key": "base", "type": "PerlinNoise", "label": "n",
              "params": {{ "frequency": {{"Float":4.0}}, "octaves": {{"UInt":5}}, "seed": {{"UInt":7}} }} }},
            {extra_nodes}
            {filter_node}
            {{ "key": "fc", "type": "FinalComposition", "label": "o", "params": {{}} }}
          ],
          "connections": [
            {{"from":"base.output","to":"f.input"}},
            {extra_conns}
            {{"from":"f.output","to":"fc.heightmap"}}
          ],
          "output": {{ "width": {DIM}, "height": {DIM}, "map_settings": {{ "min_height": 0.0, "max_height": 256.0 }} }}
        }}"#
    )
}

fn max_abs_diff(recipe_json: &str, cpu: &CpuExecutor, gpu: &HybridExecutor) -> f32 {
    let r = Recipe::from_json(recipe_json).expect("recipe parses");
    let graph = r.build_graph().expect("graph builds");
    let (w, h) = (r.output.width, r.output.height);

    let eval = |ex: &dyn NodeExecutor| {
        let res = evaluate_graph(&graph, ex, w, h, (w - 1) * 8, (h - 1) * 8).expect("eval");
        get_heightmap_output(&graph, &res).expect("heightmap output")
    };

    let a = eval(cpu);
    let b = eval(gpu);
    assert_eq!(a.data().len(), b.data().len());

    a.data()
        .iter()
        .zip(b.data().iter())
        .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

#[test]
fn gpu_matches_cpu_for_every_dispatched_node() {
    let Some(gpu) = pollster::block_on(bar_compute::GpuContext::new_standalone())
        .ok()
        .map(HybridExecutor::new)
    else {
        eprintln!("Skipping GPU parity test: no GPU adapter available");
        return;
    };
    let cpu = CpuExecutor;

    // A second noise source used as a modulation input (control / mask).
    let mod_node = r#"{ "key": "aux", "type": "PerlinNoise", "label": "a",
        "params": { "frequency": {"Float":2.0}, "octaves": {"UInt":3}, "seed": {"UInt":99} } },"#;

    let cases: Vec<(&str, String)> = vec![
        // Pure noise (replace the whole single-filter graph with just base -> fc).
        (
            "perlin_noise",
            r#"{"name":"p","author":"t","description":"d","nodes":[
                {"key":"base","type":"PerlinNoise","label":"n","params":{"frequency":{"Float":4.0},"octaves":{"UInt":5},"seed":{"UInt":7}}},
                {"key":"fc","type":"FinalComposition","label":"o","params":{}}],
              "connections":[{"from":"base.output","to":"fc.heightmap"}],
              "output":{"width":256,"height":256,"map_settings":{"min_height":0.0,"max_height":256.0}}}"#
                .to_string(),
        ),
        (
            "ridged_noise",
            r#"{"name":"p","author":"t","description":"d","nodes":[
                {"key":"base","type":"RidgedNoise","label":"n","params":{"frequency":{"Float":4.0},"octaves":{"UInt":5},"seed":{"UInt":7}}},
                {"key":"fc","type":"FinalComposition","label":"o","params":{}}],
              "connections":[{"from":"base.output","to":"fc.heightmap"}],
              "output":{"width":256,"height":256,"map_settings":{"min_height":0.0,"max_height":256.0}}}"#
                .to_string(),
        ),
        // Blur: explicit radius, omitted radius (default drift), control modulation.
        (
            "blur_explicit",
            recipe(
                r#"{ "key": "f", "type": "Blur", "label": "b", "params": { "radius": {"Float":6.0} } },"#,
                "",
                "",
            ),
        ),
        (
            "blur_default_params",
            recipe(
                r#"{ "key": "f", "type": "Blur", "label": "b", "params": {} },"#,
                "",
                "",
            ),
        ),
        (
            "blur_with_control",
            recipe(
                r#"{ "key": "f", "type": "Blur", "label": "b", "params": { "radius": {"Float":7.0} } },"#,
                mod_node,
                r#"{"from":"aux.output","to":"f.control"},"#,
            ),
        ),
        // Thermal erosion: explicit, omitted (default drift), mask modulation.
        (
            "thermal_explicit",
            recipe(
                r#"{ "key": "f", "type": "ThermalErosion", "label": "e", "params": { "iterations": {"UInt":60}, "talus_angle": {"Float":0.5} } },"#,
                "",
                "",
            ),
        ),
        (
            "thermal_default_params",
            recipe(
                r#"{ "key": "f", "type": "ThermalErosion", "label": "e", "params": {} },"#,
                "",
                "",
            ),
        ),
        (
            "thermal_with_mask",
            recipe(
                r#"{ "key": "f", "type": "ThermalErosion", "label": "e", "params": { "iterations": {"UInt":60}, "talus_angle": {"Float":0.5} } },"#,
                mod_node,
                r#"{"from":"aux.output","to":"f.mask"},"#,
            ),
        ),
    ];

    for (name, json) in &cases {
        let d = max_abs_diff(json, &cpu, &gpu);
        assert!(
            d < 1e-3,
            "{name}: editor(GPU) and export(CPU) diverge, max|diff|={d}"
        );
    }
}
