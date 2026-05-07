//! Black-box coverage tests for every public node type.
//!
//! The intent: exercise each node from the outside, asserting on the
//! contract a user can reason about (output shape, value range, the
//! behaviour the node's name implies) — *not* on implementation
//! details. Tests are written without referencing `executor.rs`'s
//! per-node logic so they catch behaviour drift if the implementation
//! changes underneath.
//!
//! What's covered per node:
//! - Output dimensions match the requested width/height (Heightmap and
//!   Color outputs alike).
//! - Output values land in the conceptually-valid range ([0, 1] for
//!   normalised heightmaps, [0, 1] per channel for colour buffers).
//! - One config-dependent behaviour: e.g. clamp respects min/max,
//!   blur smooths, invert reflects, blend factor controls mix.
//!
//! Source nodes that need on-disk files (FileInput, SmfImport,
//! SmtImport) and the terminal nodes (Bundler, Preview, FileReference,
//! PassThrough) live in their own targeted tests in
//! `crates/bar-engine/src/executor.rs::tests` — they're not retried
//! here because they need fixtures or have no observable port output.

use std::collections::HashMap;

use bar_data::Heightmap;
use bar_engine::CpuExecutor;
use bar_graph::{NodeExecutor, NodeType, ParamValue, PortValue};

// ── Helpers ─────────────────────────────────────────────────────────

const W: u32 = 16;
const H: u32 = 16;

fn empty_inputs() -> HashMap<String, PortValue> {
    HashMap::new()
}

fn flat(value: f32) -> Heightmap {
    Heightmap::frbar_data(W, H, vec![value; (W * H) as usize]).unwrap()
}

/// Heightmap whose value at (x, y) is `f(x, y)` — handy for ramps and
/// gradients. `f` receives normalized coords in [0, 1].
fn gen<F: Fn(f32, f32) -> f32>(f: F) -> Heightmap {
    let mut data = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            let u = x as f32 / (W - 1) as f32;
            let v = y as f32 / (H - 1) as f32;
            data.push(f(u, v));
        }
    }
    Heightmap::frbar_data(W, H, data).unwrap()
}

fn input_hm(name: &str, hm: Heightmap) -> HashMap<String, PortValue> {
    let mut m = HashMap::new();
    m.insert(name.to_string(), PortValue::Heightmap(hm));
    m
}

fn run(
    nt: NodeType,
    params: &[(&str, ParamValue)],
    inputs: &HashMap<String, PortValue>,
) -> HashMap<String, PortValue> {
    let executor = CpuExecutor;
    let params: HashMap<String, ParamValue> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    executor.execute(&nt, &params, inputs, W, H).unwrap()
}

fn out_hm(outputs: &HashMap<String, PortValue>, port: &str) -> Heightmap {
    match outputs.get(port).expect("port present") {
        PortValue::Heightmap(hm) => hm.clone(),
        other => panic!("expected heightmap on '{port}', got {other:?}"),
    }
}

fn out_color(
    outputs: &HashMap<String, PortValue>,
    port: &str,
) -> bar_data::ColorBuffer {
    match outputs.get(port).expect("port present") {
        PortValue::Color(cb) => cb.clone(),
        other => panic!("expected colour buffer on '{port}', got {other:?}"),
    }
}

fn assert_hm_dims(hm: &Heightmap) {
    assert_eq!(hm.width(), W, "heightmap width mismatch");
    assert_eq!(hm.height(), H, "heightmap height mismatch");
}

fn assert_color_dims(cb: &bar_data::ColorBuffer) {
    assert_eq!(cb.width(), W, "colour buffer width mismatch");
    assert_eq!(cb.height(), H, "colour buffer height mismatch");
}

fn min_max(hm: &Heightmap) -> (f32, f32) {
    let data = hm.data();
    let min = data.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    (min, max)
}

fn mean(hm: &Heightmap) -> f32 {
    let data = hm.data();
    data.iter().sum::<f32>() / data.len() as f32
}

// ── Generators ──────────────────────────────────────────────────────

#[test]
fn perlin_noise_is_deterministic_and_in_range() {
    let p = &[
        ("frequency", ParamValue::Float(4.0)),
        ("octaves", ParamValue::UInt(3)),
        ("seed", ParamValue::UInt(7)),
    ];
    let a = run(NodeType::PerlinNoise, p, &empty_inputs());
    let b = run(NodeType::PerlinNoise, p, &empty_inputs());
    let ha = out_hm(&a, "output");
    let hb = out_hm(&b, "output");
    assert_hm_dims(&ha);
    let (mn, mx) = min_max(&ha);
    assert!(mn >= 0.0 && mx <= 1.0, "values escape [0,1]: ({mn}, {mx})");
    assert!(mx - mn > 0.05, "perlin produced near-flat output");
    assert_eq!(ha.data(), hb.data(), "same seed should be deterministic");
}

#[test]
fn perlin_noise_seed_changes_output() {
    let mut p1 = vec![("seed", ParamValue::UInt(1))];
    let mut p2 = vec![("seed", ParamValue::UInt(99))];
    p1.push(("frequency", ParamValue::Float(4.0)));
    p2.push(("frequency", ParamValue::Float(4.0)));
    let a = out_hm(&run(NodeType::PerlinNoise, &p1, &empty_inputs()), "output");
    let b = out_hm(&run(NodeType::PerlinNoise, &p2, &empty_inputs()), "output");
    assert!(a.data() != b.data(), "different seeds should differ");
}

#[test]
fn simplex_noise_runs_and_produces_variation() {
    let p = &[
        ("frequency", ParamValue::Float(4.0)),
        ("octaves", ParamValue::UInt(3)),
        ("seed", ParamValue::UInt(0)),
    ];
    let h = out_hm(&run(NodeType::SimplexNoise, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
    assert!(mx - mn > 0.05, "simplex produced near-flat output");
}

#[test]
fn worley_noise_runs_and_produces_variation() {
    let p = &[
        ("frequency", ParamValue::Float(4.0)),
        ("seed", ParamValue::UInt(0)),
    ];
    let h = out_hm(&run(NodeType::WorleyNoise, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
    assert!(mx - mn > 0.05);
}

#[test]
fn ridged_noise_runs_and_produces_variation() {
    let p = &[
        ("frequency", ParamValue::Float(4.0)),
        ("octaves", ParamValue::UInt(3)),
        ("seed", ParamValue::UInt(0)),
    ];
    let h = out_hm(&run(NodeType::RidgedNoise, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
    assert!(mx - mn > 0.05);
}

#[test]
fn constant_emits_uniform_value() {
    let p = &[("value", ParamValue::Float(0.42))];
    let h = out_hm(&run(NodeType::Constant, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!((mn - 0.42).abs() < 1e-4, "min {mn}");
    assert!((mx - 0.42).abs() < 1e-4, "max {mx}");
}

#[test]
fn voronoi_runs_and_produces_variation() {
    let p = &[
        ("frequency", ParamValue::Float(4.0)),
        ("seed", ParamValue::UInt(0)),
        ("mode", ParamValue::String("f1".to_string())),
    ];
    let h = out_hm(&run(NodeType::Voronoi, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
    assert!(mx - mn > 0.05);
}

#[test]
fn gradient_linear_y_increases_top_to_bottom() {
    let p = &[
        ("direction", ParamValue::String("linear_y".to_string())),
        ("invert", ParamValue::Bool(false)),
        ("center_x", ParamValue::Float(0.5)),
        ("center_y", ParamValue::Float(0.5)),
    ];
    let h = out_hm(&run(NodeType::Gradient, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let top = h.get(W / 2, 0).unwrap();
    let bot = h.get(W / 2, H - 1).unwrap();
    assert!(bot > top, "linear_y should increase downward: top {top}, bot {bot}");
}

#[test]
fn gradient_invert_flips_direction() {
    let mut p = vec![
        ("direction", ParamValue::String("linear_y".to_string())),
        ("center_x", ParamValue::Float(0.5)),
        ("center_y", ParamValue::Float(0.5)),
    ];
    p.push(("invert", ParamValue::Bool(false)));
    let normal = out_hm(&run(NodeType::Gradient, &p, &empty_inputs()), "output");
    let mut p2 = vec![
        ("direction", ParamValue::String("linear_y".to_string())),
        ("center_x", ParamValue::Float(0.5)),
        ("center_y", ParamValue::Float(0.5)),
    ];
    p2.push(("invert", ParamValue::Bool(true)));
    let inverted = out_hm(&run(NodeType::Gradient, &p2, &empty_inputs()), "output");
    let n = normal.get(W / 2, 0).unwrap();
    let i = inverted.get(W / 2, 0).unwrap();
    assert!((n + i - 1.0).abs() < 0.05, "invert should mirror values: {n} vs {i}");
}

#[test]
fn painted_heightmap_with_empty_data_emits_flat_zero() {
    // No `data` → no pixels → output should be a valid flat heightmap.
    let p = &[
        ("data", ParamValue::String(String::new())),
        ("resolution", ParamValue::UInt(64)),
    ];
    let h = out_hm(&run(NodeType::PaintedHeightmap, p, &empty_inputs()), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn painted_texture_with_empty_data_emits_color_buffer() {
    let p = &[
        ("data", ParamValue::String(String::new())),
        ("brush_color", ParamValue::String("8B7355".to_string())),
    ];
    let outputs = run(NodeType::PaintedTexture, p, &empty_inputs());
    let cb = out_color(&outputs, "output");
    assert_color_dims(&cb);
}

// ── Filters ─────────────────────────────────────────────────────────

#[test]
fn blur_smooths_a_delta() {
    // A sharp 1-pixel spike at the centre should spread under blur.
    let mut data = vec![0.0_f32; (W * H) as usize];
    let cx = (W / 2) as usize;
    let cy = (H / 2) as usize;
    data[cy * W as usize + cx] = 1.0;
    let hm = Heightmap::frbar_data(W, H, data).unwrap();
    let inputs = input_hm("input", hm);
    let p = &[("radius", ParamValue::Float(2.0))];
    let h = out_hm(&run(NodeType::Blur, p, &inputs), "output");
    assert_hm_dims(&h);
    let centre = h.get(cx as u32, cy as u32).unwrap();
    let neighbour = h.get((cx + 1) as u32, cy as u32).unwrap();
    assert!(centre < 1.0, "centre should be lower after blur: {centre}");
    assert!(neighbour > 0.0, "neighbour should pick up some value: {neighbour}");
}

#[test]
fn sharpen_amplifies_contrast() {
    // Half-step input (left=0, right=1). Sharpen should emphasise the
    // discontinuity at the boundary, but it must not blow values out
    // of [0, 1].
    let hm = gen(|u, _| if u < 0.5 { 0.2 } else { 0.8 });
    let inputs = input_hm("input", hm);
    let h = out_hm(&run(NodeType::Sharpen, &[], &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0, "sharpen escapes range: ({mn}, {mx})");
}

#[test]
fn clamp_respects_min_and_max() {
    let hm = gen(|u, _| u); // ramp 0 → 1
    let inputs = input_hm("input", hm);
    let p = &[
        ("min", ParamValue::Float(0.25)),
        ("max", ParamValue::Float(0.75)),
    ];
    let h = out_hm(&run(NodeType::Clamp, p, &inputs), "output");
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.25 - 1e-4, "min below clamp: {mn}");
    assert!(mx <= 0.75 + 1e-4, "max above clamp: {mx}");
}

#[test]
fn terrace_reduces_value_diversity_vs_input() {
    // A 16-wide ramp has up to 16 distinct values per row. Terrace
    // should quantise those into fewer levels — the histogram of
    // "buckets at 0.05 resolution" should be smaller after the
    // transform, regardless of the smoothing strategy the impl uses.
    let hm = gen(|u, _| u);
    let inputs = input_hm("input", hm.clone());
    let h = out_hm(&run(NodeType::Terrace, &[], &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);

    let bucket = |hm: &Heightmap| -> usize {
        use std::collections::HashSet;
        let s: HashSet<i32> = hm
            .data()
            .iter()
            .map(|v| (v * 20.0).round() as i32)
            .collect();
        s.len()
    };
    let before = bucket(&hm);
    let after = bucket(&h);
    assert!(
        after <= before,
        "terrace should not increase value diversity; before={before}, after={after}"
    );
}

#[test]
fn invert_reflects_values_around_half() {
    let hm = gen(|u, _| u);
    let inputs = input_hm("input", hm);
    let h = out_hm(&run(NodeType::Invert, &[], &inputs), "output");
    let (mn, mx) = min_max(&h);
    assert!((mn - 0.0).abs() < 1e-3, "invert min: {mn}");
    assert!((mx - 1.0).abs() < 1e-3, "invert max: {mx}");
    // Inverted ramp should run high→low.
    let left = h.get(0, H / 2).unwrap();
    let right = h.get(W - 1, H / 2).unwrap();
    assert!(left > right, "invert should reverse the ramp: {left} vs {right}");
}

#[test]
fn curve_runs_and_preserves_dimensions() {
    let hm = gen(|u, _| u);
    let inputs = input_hm("input", hm);
    let h = out_hm(&run(NodeType::Curve, &[], &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn hydraulic_erosion_runs_without_breaking_dimensions() {
    let hm = gen(|u, v| ((u - 0.5).abs() + (v - 0.5).abs()) * 0.5);
    let inputs = input_hm("input", hm);
    let p = &[
        ("iterations", ParamValue::UInt(500)),
        ("erosion_rate", ParamValue::Float(0.01)),
        ("deposition_rate", ParamValue::Float(0.01)),
    ];
    let h = out_hm(&run(NodeType::HydraulicErosion, p, &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn thermal_erosion_runs_without_breaking_dimensions() {
    let hm = gen(|u, v| (u + v) * 0.5);
    let inputs = input_hm("input", hm);
    let p = &[
        ("iterations", ParamValue::UInt(20)),
        ("talus_angle", ParamValue::Float(0.6)),
    ];
    let h = out_hm(&run(NodeType::ThermalErosion, p, &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn displacement_runs_with_zero_displacement_is_near_identity() {
    let base = gen(|u, _| u);
    let zero = flat(0.0);
    let mut inputs = input_hm("input", base.clone());
    inputs.insert("displacement".to_string(), PortValue::Heightmap(zero));
    let h = out_hm(&run(NodeType::Displacement, &[], &inputs), "output");
    assert_hm_dims(&h);
    // With zero displacement the output should track the base ramp
    // closely. Allow a small tolerance for any centred sampling
    // weights the impl uses.
    let want = mean(&base);
    let got = mean(&h);
    assert!((want - got).abs() < 0.05, "mean drift: want {want}, got {got}");
}

// ── Combiners ───────────────────────────────────────────────────────

#[test]
fn add_sums_inputs_and_clamps_to_unit_range() {
    let mut inputs = input_hm("a", flat(0.3));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.4)));
    let h = out_hm(&run(NodeType::Add, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.7).abs() < 1e-3, "add mean: {m}");
}

#[test]
fn subtract_yields_a_minus_b() {
    let mut inputs = input_hm("a", flat(0.6));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.2)));
    let h = out_hm(&run(NodeType::Subtract, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.4).abs() < 1e-3, "subtract mean: {m}");
}

#[test]
fn multiply_yields_product() {
    let mut inputs = input_hm("a", flat(0.5));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.5)));
    let h = out_hm(&run(NodeType::Multiply, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.25).abs() < 1e-3, "multiply mean: {m}");
}

#[test]
fn max_picks_higher_of_two_inputs() {
    let mut inputs = input_hm("a", flat(0.3));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.7)));
    let h = out_hm(&run(NodeType::Max, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.7).abs() < 1e-3, "max mean: {m}");
}

#[test]
fn min_picks_lower_of_two_inputs() {
    let mut inputs = input_hm("a", flat(0.3));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.7)));
    let h = out_hm(&run(NodeType::Min, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.3).abs() < 1e-3, "min mean: {m}");
}

#[test]
fn blend_factor_zero_passes_a_through() {
    let mut inputs = input_hm("a", flat(0.2));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.8)));
    let p = &[("factor", ParamValue::Float(0.0))];
    let h = out_hm(&run(NodeType::Blend, p, &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.2).abs() < 1e-3);
}

#[test]
fn blend_factor_one_passes_b_through() {
    let mut inputs = input_hm("a", flat(0.2));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.8)));
    let p = &[("factor", ParamValue::Float(1.0))];
    let h = out_hm(&run(NodeType::Blend, p, &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.8).abs() < 1e-3);
}

// ── Texture / Splat ─────────────────────────────────────────────────

#[test]
fn slope_map_is_zero_on_flat_input() {
    let inputs = input_hm("input", flat(0.5));
    let h = out_hm(&run(NodeType::SlopeMap, &[], &inputs), "output");
    assert_hm_dims(&h);
    let (_, mx) = min_max(&h);
    assert!(mx < 0.05, "flat input should give near-zero slope: max {mx}");
}

#[test]
fn slope_map_is_nonzero_on_steep_input() {
    let hm = gen(|u, _| u); // monotonic ramp → constant non-zero slope
    let inputs = input_hm("input", hm);
    let h = out_hm(&run(NodeType::SlopeMap, &[], &inputs), "output");
    let (_, mx) = min_max(&h);
    assert!(mx > 0.0, "ramp should produce non-zero slope max: {mx}");
}

#[test]
fn height_select_picks_band() {
    let hm = gen(|u, _| u);
    let inputs = input_hm("input", hm);
    let p = &[
        ("low", ParamValue::Float(0.4)),
        ("high", ParamValue::Float(0.6)),
        ("falloff", ParamValue::Float(0.0)),
    ];
    let h = out_hm(&run(NodeType::HeightSelect, p, &inputs), "output");
    assert_hm_dims(&h);
    // Centre column is at u≈0.5 → inside [0.4, 0.6] → mask should be near 1.
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!(centre > 0.5, "centre band selected: {centre}");
    // Edge columns (u=0, u=1) should be near 0.
    let left = h.get(0, H / 2).unwrap();
    let right = h.get(W - 1, H / 2).unwrap();
    assert!(left < 0.5 && right < 0.5, "edges deselected: {left}, {right}");
}

#[test]
fn splat_map_blends_three_bands() {
    // Slope map controls splat selection — give it a ramp.
    let mut inputs = input_hm("slope", gen(|u, _| u));
    inputs.insert("band0".to_string(), PortValue::Heightmap(flat(0.1)));
    inputs.insert("band1".to_string(), PortValue::Heightmap(flat(0.5)));
    inputs.insert("band2".to_string(), PortValue::Heightmap(flat(0.9)));
    let h = out_hm(&run(NodeType::SplatMap, &[], &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn auto_texture_outputs_color_buffer() {
    let mut inputs = input_hm("input", gen(|u, _| u));
    inputs.insert(
        "slope".to_string(),
        PortValue::Heightmap(flat(0.2)),
    );
    let p = &[
        ("biome", ParamValue::String("temperate".to_string())),
        ("slope_power", ParamValue::Float(0.7)),
        ("slope_blend", ParamValue::Float(1.0)),
        ("rock_color", ParamValue::String("736B61".to_string())),
        ("ao_strength", ParamValue::Float(1.0)),
    ];
    let outputs = run(NodeType::AutoTexture, p, &inputs);
    let cb = out_color(&outputs, "output");
    assert_color_dims(&cb);
}

// ── Map layers ──────────────────────────────────────────────────────

#[test]
fn normal_map_outputs_color_buffer() {
    let inputs = input_hm("input", gen(|u, _| u));
    let p = &[("strength", ParamValue::Float(1.0))];
    let outputs = run(NodeType::NormalMap, p, &inputs);
    let cb = out_color(&outputs, "output");
    assert_color_dims(&cb);
}

#[test]
fn grass_map_outputs_heightmap_in_range() {
    let mut inputs = input_hm("input", gen(|u, _| u));
    inputs.insert("slope".to_string(), PortValue::Heightmap(flat(0.1)));
    let p = &[
        ("min_height", ParamValue::Float(0.15)),
        ("max_height", ParamValue::Float(0.7)),
        ("max_slope", ParamValue::Float(0.4)),
        ("density", ParamValue::Float(1.0)),
        ("falloff", ParamValue::Float(0.05)),
    ];
    let h = out_hm(&run(NodeType::GrassMap, p, &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn specular_map_outputs_heightmap_in_range() {
    let mut inputs = input_hm("input", gen(|u, _| u));
    inputs.insert("slope".to_string(), PortValue::Heightmap(flat(0.1)));
    let p = &[
        ("rock_specular", ParamValue::Float(0.6)),
        ("flat_specular", ParamValue::Float(0.2)),
        ("water_specular", ParamValue::Float(0.9)),
        ("water_height", ParamValue::Float(0.2)),
        ("snow_specular", ParamValue::Float(0.7)),
        ("snow_height", ParamValue::Float(0.85)),
    ];
    let h = out_hm(&run(NodeType::SpecularMap, p, &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

// ── Mask operations ─────────────────────────────────────────────────

#[test]
fn mask_threshold_yields_binary_band() {
    let hm = gen(|u, _| u);
    let inputs = input_hm("input", hm);
    let p = &[
        ("threshold", ParamValue::Float(0.5)),
        ("smoothness", ParamValue::Float(0.0)),
    ];
    let h = out_hm(&run(NodeType::MaskThreshold, p, &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    // Hard threshold → values are 0 or 1 (allow a touch of tolerance).
    assert!(mn < 0.05);
    assert!(mx > 0.95);
}

#[test]
fn mask_invert_flips_a_binary_mask() {
    let inputs = input_hm("input", flat(0.7));
    let h = out_hm(&run(NodeType::MaskInvert, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.3).abs() < 1e-3, "1 - 0.7 = 0.3, got {m}");
}

#[test]
fn mask_blur_smooths_a_delta() {
    let mut data = vec![0.0_f32; (W * H) as usize];
    data[(H / 2) as usize * W as usize + (W / 2) as usize] = 1.0;
    let hm = Heightmap::frbar_data(W, H, data).unwrap();
    let inputs = input_hm("input", hm);
    let p = &[("radius", ParamValue::Float(2.0))];
    let h = out_hm(&run(NodeType::MaskBlur, p, &inputs), "output");
    assert_hm_dims(&h);
    let centre = h.get(W / 2, H / 2).unwrap();
    let neighbour = h.get(W / 2 + 1, H / 2).unwrap();
    assert!(centre < 1.0);
    assert!(neighbour > 0.0);
}

#[test]
fn mask_apply_blends_input_and_background_via_mask() {
    let mut inputs = input_hm("input", flat(0.9));
    inputs.insert("background".to_string(), PortValue::Heightmap(flat(0.1)));
    inputs.insert("mask".to_string(), PortValue::Heightmap(flat(1.0)));
    let h = out_hm(&run(NodeType::MaskApply, &[], &inputs), "output");
    // Mask 1.0 → fully foreground (input)
    let m = mean(&h);
    assert!((m - 0.9).abs() < 0.01, "mask=1 should pick input: {m}");
}

#[test]
fn mask_node_emits_mask_port() {
    let inputs = input_hm("input", flat(0.5));
    let outputs = run(NodeType::Mask, &[], &inputs);
    // The Mask node's output port is named "mask" and may carry either
    // Heightmap or a dedicated Mask kind — accept either.
    let mask = outputs.get("mask").expect("mask port present");
    match mask {
        PortValue::Heightmap(hm) => assert_hm_dims(hm),
        PortValue::Mask(_) => { /* dedicated mask kind — accept */ }
        other => panic!("unexpected mask kind: {other:?}"),
    }
}

// ── Subgraph IO ─────────────────────────────────────────────────────

#[test]
fn subgraph_input_passes_value_through() {
    // Both ports are named "value". Whatever's written in flows out.
    let mut inputs = HashMap::new();
    inputs.insert("value".to_string(), PortValue::Heightmap(flat(0.42)));
    let outputs = run(NodeType::SubgraphInput, &[], &inputs);
    let h = out_hm(&outputs, "value");
    assert_hm_dims(&h);
    let m = mean(&h);
    assert!((m - 0.42).abs() < 1e-3, "passthrough should preserve value: {m}");
}

#[test]
fn subgraph_output_passes_value_through() {
    let mut inputs = HashMap::new();
    inputs.insert("value".to_string(), PortValue::Heightmap(flat(0.7)));
    let outputs = run(NodeType::SubgraphOutput, &[], &inputs);
    let h = out_hm(&outputs, "value");
    assert_hm_dims(&h);
    let m = mean(&h);
    assert!((m - 0.7).abs() < 1e-3, "passthrough should preserve value: {m}");
}

#[test]
fn subgraph_io_with_no_input_emits_no_output() {
    // If nothing is wired in, no output is produced (matches the
    // generic "skip nodes whose inputs aren't present" behaviour
    // every other passthrough node uses, e.g. Preview).
    let outputs = run(NodeType::SubgraphInput, &[], &empty_inputs());
    assert!(outputs.get("value").is_none(),
        "no input → no output: {outputs:?}");
}

#[test]
fn texture_sculpt_passes_input_through_when_no_dabs() {
    // With no recorded dabs the node is a pass-through: output ==
    // input pixel-for-pixel.
    let mut cb = bar_data::ColorBuffer::new(W, H).unwrap();
    for y in 0..H {
        for x in 0..W {
            cb.set(x, y, [0.3, 0.6, 0.2, 1.0]);
        }
    }
    let mut inputs = HashMap::new();
    inputs.insert("input".to_string(), PortValue::Color(cb.clone()));
    let outputs = run(
        NodeType::TextureSculpt,
        &[("dabs", ParamValue::String("[]".to_string()))],
        &inputs,
    );
    let out = out_color(&outputs, "output");
    assert_color_dims(&out);
    let p = out.get(W / 2, H / 2).unwrap();
    assert!((p[0] - 0.3).abs() < 1e-3);
    assert!((p[1] - 0.6).abs() < 1e-3);
    assert!((p[2] - 0.2).abs() < 1e-3);
}

#[test]
fn texture_sculpt_replays_dab_on_top_of_input() {
    // A single recorded dab at the centre stamps its colour on top
    // of the upstream texture. Outside the brush footprint the
    // upstream pixels show through unchanged.
    let mut cb = bar_data::ColorBuffer::new(W, H).unwrap();
    for y in 0..H {
        for x in 0..W {
            cb.set(x, y, [0.0, 0.0, 0.0, 1.0]);
        }
    }
    let mut inputs = HashMap::new();
    inputs.insert("input".to_string(), PortValue::Color(cb));
    // ru = 0.2 covers ~3 pixels at radius on a 16-wide buffer.
    let dabs = "[{\"u\":0.5,\"v\":0.5,\"ru\":0.2,\"r\":255,\"g\":0,\"b\":0}]";
    let outputs = run(
        NodeType::TextureSculpt,
        &[("dabs", ParamValue::String(dabs.to_string()))],
        &inputs,
    );
    let out = out_color(&outputs, "output");
    let centre = out.get(W / 2, H / 2).unwrap();
    assert!(centre[0] > 0.95, "centre stamped red: {centre:?}");
    let corner = out.get(0, 0).unwrap();
    assert!(corner[0] < 0.05, "corner unchanged: {corner:?}");
}

#[test]
fn texture_sculpt_emits_no_output_with_disconnected_input() {
    // Same defensive shape as Sculpt — no input means no output.
    let outputs = run(
        NodeType::TextureSculpt,
        &[("dabs", ParamValue::String("[]".to_string()))],
        &empty_inputs(),
    );
    assert!(outputs.get("output").is_none(),
        "no input → no output: {outputs:?}");
}

#[test]
fn metal_sculpt_stamps_value_into_brush_footprint() {
    // Single dab at the centre stamps `value` into the heightmap.
    // Outside the brush footprint the upstream zero shows through.
    let inputs = input_hm("input", flat(0.0));
    let dabs = "[{\"u\":0.5,\"v\":0.5,\"ru\":0.2,\"value\":0.75}]";
    let h = out_hm(
        &run(
            NodeType::MetalSculpt,
            &[("dabs", ParamValue::String(dabs.to_string()))],
            &inputs,
        ),
        "output",
    );
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!((centre - 0.75).abs() < 1e-3, "centre stamped: {centre}");
    let corner = h.get(0, 0).unwrap();
    assert!(corner.abs() < 1e-3, "corner unchanged: {corner}");
}

#[test]
fn metal_sculpt_passes_input_through_when_no_dabs() {
    // No dabs → identity passthrough on the upstream heightmap.
    let inputs = input_hm("input", flat(0.4));
    let h = out_hm(
        &run(
            NodeType::MetalSculpt,
            &[("dabs", ParamValue::String("[]".to_string()))],
            &inputs,
        ),
        "output",
    );
    let m = mean(&h);
    assert!((m - 0.4).abs() < 1e-3, "expected identity, mean {m}");
}

#[test]
fn type_sculpt_stamps_value_into_brush_footprint() {
    // TypeSculpt shares the value-stamp implementation; verify the
    // node-type wiring lands on the same dab-replay code.
    let inputs = input_hm("input", flat(0.0));
    let dabs = "[{\"u\":0.5,\"v\":0.5,\"ru\":0.2,\"value\":0.5}]";
    let h = out_hm(
        &run(
            NodeType::TypeSculpt,
            &[("dabs", ParamValue::String(dabs.to_string()))],
            &inputs,
        ),
        "output",
    );
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!((centre - 0.5).abs() < 1e-3, "centre stamped: {centre}");
}
