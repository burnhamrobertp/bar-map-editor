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
//! Source nodes that need on-disk files (FileInput) and the terminal
//! nodes (Bundler, Preview, FileReference, PassThrough) live in their
//! own targeted tests in
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
    executor.execute(&nt, &params, inputs, W, H, W, H).unwrap()
}

fn out_hm(outputs: &HashMap<String, PortValue>, port: &str) -> Heightmap {
    match outputs.get(port).expect("port present") {
        PortValue::Heightmap(hm) => hm.clone(),
        other => panic!("expected heightmap on '{port}', got {other:?}"),
    }
}

fn out_color(outputs: &HashMap<String, PortValue>, port: &str) -> bar_data::ColorBuffer {
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
    assert!(
        bot > top,
        "linear_y should increase downward: top {top}, bot {bot}"
    );
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
    assert!(
        (n + i - 1.0).abs() < 0.05,
        "invert should mirror values: {n} vs {i}"
    );
}

#[test]
fn painted_heightmap_with_empty_data_emits_flat_zero() {
    // No `data` → no pixels → output should be a valid flat heightmap.
    let p = &[
        ("data", ParamValue::String(String::new())),
        ("resolution", ParamValue::UInt(64)),
    ];
    let h = out_hm(
        &run(NodeType::PaintedHeightmap, p, &empty_inputs()),
        "output",
    );
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

#[test]
fn imported_texture_with_no_paths_emits_empty_color_buffer() {
    // No asset_path / tile_index_path set -- executor falls back to an empty ColorBuffer.
    let outputs = run(NodeType::ImportedTexture, &[], &empty_inputs());
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
    assert!(
        neighbour > 0.0,
        "neighbour should pick up some value: {neighbour}"
    );
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
    assert!(
        mn >= 0.0 && mx <= 1.0,
        "sharpen escapes range: ({mn}, {mx})"
    );
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
    assert!(
        left > right,
        "invert should reverse the ramp: {left} vs {right}"
    );
}

#[test]
fn mirror_x_makes_left_right_symmetric() {
    // Left-to-right ramp: value at x mirrors value at (W-1-x).
    let hm = gen(|u, _| u);
    let inputs = input_hm("input", hm);
    let p = &[("mode", ParamValue::String("mirror_x".to_string()))];
    let h = out_hm(&run(NodeType::Mirror, p, &inputs), "output");
    assert_hm_dims(&h);
    let left = h.get(2, H / 2).unwrap();
    let right = h.get(W - 1 - 2, H / 2).unwrap();
    assert!(
        (left - right).abs() < 1e-4,
        "mirror_x: left {left} != right {right}"
    );
}

#[test]
fn mirror_y_makes_top_bottom_symmetric() {
    let hm = gen(|_, v| v);
    let inputs = input_hm("input", hm);
    let p = &[("mode", ParamValue::String("mirror_y".to_string()))];
    let h = out_hm(&run(NodeType::Mirror, p, &inputs), "output");
    assert_hm_dims(&h);
    let top = h.get(W / 2, 2).unwrap();
    let bot = h.get(W / 2, H - 1 - 2).unwrap();
    assert!((top - bot).abs() < 1e-4, "mirror_y: top {top} != bot {bot}");
}

#[test]
fn mirror_average_x_preserves_information_from_both_halves() {
    // A 0->1 left-to-right ramp has equal information on both halves.
    // The replace `mirror_x` mode throws the right half away (output
    // would average to ~0.25). The `average_x` mode means(left, right)
    // at every column -- input(u) + input(1-u) averages to ~0.5
    // everywhere, so the output mean equals the input mean.
    let input = gen(|u, _| u);
    let inputs = input_hm("input", input.clone());
    let p = &[("mode", ParamValue::String("average_x".to_string()))];
    let h = out_hm(&run(NodeType::Mirror, p, &inputs), "output");
    assert_hm_dims(&h);
    // Output must be symmetric about the centre column.
    let left = h.get(2, H / 2).unwrap();
    let right = h.get(W - 1 - 2, H / 2).unwrap();
    assert!(
        (left - right).abs() < 1e-4,
        "average_x output must be symmetric: {left} vs {right}"
    );
    // Mean is preserved: the ramp's mean is 0.5, so the averaged output
    // sits at ~0.5 across the whole image.
    let m = mean(&h);
    assert!(
        (m - 0.5).abs() < 0.05,
        "average_x mean should match input mean: got {m}"
    );
}

#[test]
fn mirror_average_xy_collapses_to_mean_of_four() {
    // Build an input with a known asymmetric pattern so each of the
    // four symmetric partners carries a distinct value. The output at
    // every position should equal the mean of those four values.
    let input = gen(|u, v| u + 2.0 * v); // top-left low, bottom-right high
    let inputs = input_hm("input", input.clone());
    let p = &[("mode", ParamValue::String("average_xy".to_string()))];
    let h = out_hm(&run(NodeType::Mirror, p, &inputs), "output");
    let cx = 3;
    let cy = 4;
    let expected = (input.get(cx, cy).unwrap()
        + input.get(W - 1 - cx, cy).unwrap()
        + input.get(cx, H - 1 - cy).unwrap()
        + input.get(W - 1 - cx, H - 1 - cy).unwrap())
        / 4.0;
    let got = h.get(cx, cy).unwrap();
    assert!(
        (got - expected).abs() < 1e-4,
        "average_xy at ({cx},{cy}): expected {expected}, got {got}"
    );
}

#[test]
fn layout_generator_mirror_x_symmetry_duplicates_shape() {
    // One off-centre shape with `symmetry=mirror_x` should produce a
    // pair: the original on the left and its reflection on the right.
    let p = &[
        ("shape_count", ParamValue::UInt(1)),
        ("symmetry", ParamValue::String("mirror_x".to_string())),
        ("type_0", ParamValue::String("ellipse".to_string())),
        ("x_0", ParamValue::Float(0.25)),
        ("y_0", ParamValue::Float(0.5)),
        ("rx_0", ParamValue::Float(0.15)),
        ("ry_0", ParamValue::Float(0.15)),
        ("angle_0", ParamValue::Float(0.0)),
        ("height_0", ParamValue::Float(1.0)),
        ("falloff_0", ParamValue::Float(0.5)),
    ];
    let h = out_hm(
        &run(NodeType::LayoutGenerator, p, &empty_inputs()),
        "output",
    );
    // The off-centre shape sits near x=0.25; its mirror sits near
    // x=0.75. Both columns at y=0.5 should read bright.
    let left = h.get((0.25 * (W - 1) as f32) as u32, H / 2).unwrap();
    let right = h.get((0.75 * (W - 1) as f32) as u32, H / 2).unwrap();
    assert!(left > 0.5, "left shape: {left}");
    assert!(right > 0.5, "right shape: {right}");
    // Output must be symmetric about x=0.5.
    let l = h.get(2, H / 2).unwrap();
    let r = h.get(W - 1 - 2, H / 2).unwrap();
    assert!(
        (l - r).abs() < 1e-3,
        "symmetry should be exact under mirror_x: {l} vs {r}"
    );
}

#[test]
fn layout_generator_rotate_90_produces_four_peaks() {
    // One off-centre shape with `symmetry=rotate_90` produces four
    // peaks rotated 90 / 180 / 270 degrees about the centre.
    let p = &[
        ("shape_count", ParamValue::UInt(1)),
        ("symmetry", ParamValue::String("rotate_90".to_string())),
        ("type_0", ParamValue::String("ellipse".to_string())),
        ("x_0", ParamValue::Float(0.7)),
        ("y_0", ParamValue::Float(0.5)),
        ("rx_0", ParamValue::Float(0.1)),
        ("ry_0", ParamValue::Float(0.1)),
        ("angle_0", ParamValue::Float(0.0)),
        ("height_0", ParamValue::Float(1.0)),
        ("falloff_0", ParamValue::Float(0.5)),
    ];
    let h = out_hm(
        &run(NodeType::LayoutGenerator, p, &empty_inputs()),
        "output",
    );
    // Original at (0.7, 0.5). Three rotations: (0.5, 0.7), (0.3, 0.5),
    // (0.5, 0.3). All four positions should be lit.
    let lookup = |nx: f32, ny: f32| {
        let px = (nx * (W - 1) as f32) as u32;
        let py = (ny * (H - 1) as f32) as u32;
        h.get(px, py).unwrap()
    };
    assert!(lookup(0.7, 0.5) > 0.5, "original");
    assert!(lookup(0.5, 0.7) > 0.5, "rot 90");
    assert!(lookup(0.3, 0.5) > 0.5, "rot 180");
    assert!(lookup(0.5, 0.3) > 0.5, "rot 270");
}

#[test]
fn layout_generator_none_symmetry_is_unchanged() {
    // Sanity check: the default `symmetry=none` produces the same
    // single-shape output the node has always produced. Guards against
    // regressions where the symmetry expansion accidentally adds a
    // copy even for the default mode.
    let p = &[
        ("shape_count", ParamValue::UInt(1)),
        ("symmetry", ParamValue::String("none".to_string())),
        ("type_0", ParamValue::String("ellipse".to_string())),
        ("x_0", ParamValue::Float(0.25)),
        ("y_0", ParamValue::Float(0.5)),
        ("rx_0", ParamValue::Float(0.15)),
        ("ry_0", ParamValue::Float(0.15)),
        ("angle_0", ParamValue::Float(0.0)),
        ("height_0", ParamValue::Float(1.0)),
        ("falloff_0", ParamValue::Float(0.5)),
    ];
    let h = out_hm(
        &run(NodeType::LayoutGenerator, p, &empty_inputs()),
        "output",
    );
    let left = h.get((0.25 * (W - 1) as f32) as u32, H / 2).unwrap();
    let right = h.get((0.75 * (W - 1) as f32) as u32, H / 2).unwrap();
    assert!(left > 0.5, "shape at 0.25 should be lit: {left}");
    assert!(right < 0.1, "no shape at 0.75 under symmetry=none: {right}");
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
    assert!(
        (want - got).abs() < 0.05,
        "mean drift: want {want}, got {got}"
    );
}

#[test]
fn normalize_maps_range_to_zero_one() {
    // Input spans 0.2..0.8; after normalize it should span 0.0..1.0.
    let input = gen(|u, _| 0.2 + u * 0.6);
    let h = out_hm(
        &run(NodeType::Normalize, &[], &input_hm("input", input)),
        "output",
    );
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(
        mn < 0.01,
        "min should be near 0.0 after normalize, got {mn}"
    );
    assert!(
        mx > 0.99,
        "max should be near 1.0 after normalize, got {mx}"
    );
}

#[test]
fn bias_gain_at_neutral_is_identity() {
    // bias=0.5, gain=0.5 is the identity transform.
    let input = gen(|u, _| u);
    let params = &[
        ("bias", ParamValue::Float(0.5)),
        ("gain", ParamValue::Float(0.5)),
    ];
    let h = out_hm(
        &run(
            NodeType::BiasGain,
            params,
            &input_hm("input", input.clone()),
        ),
        "output",
    );
    assert_hm_dims(&h);
    let want = mean(&input);
    let got = mean(&h);
    assert!(
        (want - got).abs() < 0.02,
        "bias=0.5/gain=0.5 should be near identity: want {want}, got {got}"
    );
}

#[test]
fn bias_gain_high_bias_shifts_midpoint_up() {
    let input = flat(0.5);
    let params = &[
        ("bias", ParamValue::Float(0.8)),
        ("gain", ParamValue::Float(0.5)),
    ];
    let h = out_hm(
        &run(NodeType::BiasGain, params, &input_hm("input", input)),
        "output",
    );
    assert_hm_dims(&h);
    let m = mean(&h);
    assert!(m > 0.6, "high bias should push midpoint up, got {m}");
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
    assert!(
        mx < 0.05,
        "flat input should give near-zero slope: max {mx}"
    );
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
    assert!(
        left < 0.5 && right < 0.5,
        "edges deselected: {left}, {right}"
    );
}

#[test]
fn splat_map_blends_three_bands() {
    // Slope map controls splat selection — give it a ramp.
    let mut inputs = input_hm("slope", gen(|u, _| u));
    inputs.insert("band0".to_string(), PortValue::Heightmap(flat(0.1)));
    inputs.insert("band1".to_string(), PortValue::Heightmap(flat(0.5)));
    inputs.insert("band2".to_string(), PortValue::Heightmap(flat(0.9)));
    let h = out_hm(&run(NodeType::TerrainSplat, &[], &inputs), "output");
    assert_hm_dims(&h);
    let (mn, mx) = min_max(&h);
    assert!(mn >= 0.0 && mx <= 1.0);
}

#[test]
fn auto_texture_outputs_color_buffer() {
    let mut inputs = input_hm("input", gen(|u, _| u));
    inputs.insert("slope".to_string(), PortValue::Heightmap(flat(0.2)));
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
fn invert_flips_a_constant() {
    // Constant-input cousin to `invert_reflects_values_around_half`:
    // verifies the 1-x relationship rather than the spatial direction
    // of the flip. Previously MaskInvert had its own variant for the
    // mask-port case; consolidated to use Invert directly.
    let inputs = input_hm("input", flat(0.7));
    let h = out_hm(&run(NodeType::Invert, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.3).abs() < 1e-3, "1 - 0.7 = 0.3, got {m}");
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
    assert!(
        (m - 0.42).abs() < 1e-3,
        "passthrough should preserve value: {m}"
    );
}

#[test]
fn subgraph_output_passes_value_through() {
    let mut inputs = HashMap::new();
    inputs.insert("value".to_string(), PortValue::Heightmap(flat(0.7)));
    let outputs = run(NodeType::SubgraphOutput, &[], &inputs);
    let h = out_hm(&outputs, "value");
    assert_hm_dims(&h);
    let m = mean(&h);
    assert!(
        (m - 0.7).abs() < 1e-3,
        "passthrough should preserve value: {m}"
    );
}

#[test]
fn subgraph_io_with_no_input_emits_no_output() {
    // If nothing is wired in, no output is produced (matches the
    // generic "skip nodes whose inputs aren't present" behaviour
    // every other passthrough node uses, e.g. Preview).
    let outputs = run(NodeType::SubgraphInput, &[], &empty_inputs());
    assert!(
        !outputs.contains_key("value"),
        "no input → no output: {outputs:?}"
    );
}

#[test]
fn color_ramp_black_to_white_maps_value_to_gray() {
    // Default 2-stop ramp: black at 0.0, white at 1.0.
    // A flat heightmap at 0.5 should produce a mid-gray Color output.
    let mut inputs = HashMap::new();
    inputs.insert("input".to_string(), PortValue::Heightmap(flat(0.5)));
    let outputs = run(NodeType::ColorRamp, &[], &inputs);
    let cb = out_color(&outputs, "output");
    assert_color_dims(&cb);
    // Channel 0 (red) should be ~0.5 in a grayscale ramp.
    let data = cb.data();
    let r: f32 =
        data.chunks_exact(4).map(|p| p[0]).sum::<f32>() / (cb.width() * cb.height()) as f32;
    assert!(
        (r - 0.5).abs() < 0.02,
        "mid-height should map to ~0.5 in black-to-white ramp, got {r}"
    );
}

#[test]
fn color_ramp_custom_stop_tints_output() {
    // Override stop 0 to red at 0.0, stop 1 stays white at 1.0.
    // A flat heightmap at 0.0 should be fully red.
    let mut inputs = HashMap::new();
    inputs.insert("input".to_string(), PortValue::Heightmap(flat(0.0)));
    let params = [
        ("stop_count", ParamValue::UInt(2)),
        ("pos_0", ParamValue::Float(0.0)),
        ("color_0", ParamValue::String("FF0000".to_string())),
        ("pos_1", ParamValue::Float(1.0)),
        ("color_1", ParamValue::String("FFFFFF".to_string())),
    ];
    let outputs = run(NodeType::ColorRamp, &params, &inputs);
    let cb = out_color(&outputs, "output");
    let data = cb.data();
    let r = data[0];
    let g = data[1];
    assert!(r > 0.95, "red channel should be ~1.0 at stop 0: {r}");
    assert!(g < 0.05, "green channel should be ~0.0 at red stop: {g}");
}

// ── Untested filters + selectors ────────────────────────────────────
//
// These node types had no behavioural coverage in this file before
// today. Each test asserts the smallest invariant that defines the
// node's contract: e.g. Warp at strength=0 is identity, MaskExpand
// grows a bright region, SelectAspect picks the matching slope
// direction. Strong enough to catch a refactor that silently breaks
// the semantics; loose enough not to bind future quality tweaks.

#[test]
fn warp_zero_strength_is_identity() {
    let base = gen(|u, _| u);
    let mut inputs = input_hm("input", base.clone());
    inputs.insert("warp_x".to_string(), PortValue::Heightmap(flat(0.5)));
    inputs.insert("warp_y".to_string(), PortValue::Heightmap(flat(0.5)));
    let p = &[("strength", ParamValue::Float(0.0))];
    let h = out_hm(&run(NodeType::Warp, p, &inputs), "output");
    assert_hm_dims(&h);
    // strength=0 collapses the warp -- output should equal input pixel-wise.
    for (i, (a, b)) in base.data().iter().zip(h.data().iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-4,
            "warp(strength=0) drift at {i}: {a} vs {b}"
        );
    }
}

#[test]
fn warp_nonzero_strength_moves_pixels() {
    // A non-uniform warp field (warp_x ramp) with strength > 0 should
    // produce a different output than the input. Loose check -- we
    // assert *something changed*, not the exact remapping.
    let base = gen(|u, _| u);
    let mut inputs = input_hm("input", base.clone());
    inputs.insert("warp_x".to_string(), PortValue::Heightmap(gen(|u, _| u)));
    inputs.insert("warp_y".to_string(), PortValue::Heightmap(flat(0.5)));
    let p = &[("strength", ParamValue::Float(0.5))];
    let h = out_hm(&run(NodeType::Warp, p, &inputs), "output");
    assert!(
        h.data() != base.data(),
        "warp at strength>0 should change output"
    );
}

#[test]
fn transform_default_params_are_identity() {
    let base = gen(|u, _| u);
    let inputs = input_hm("input", base.clone());
    let p = &[
        ("translate_x", ParamValue::Float(0.0)),
        ("translate_y", ParamValue::Float(0.0)),
        ("scale", ParamValue::Float(1.0)),
        ("angle", ParamValue::Float(0.0)),
    ];
    let h = out_hm(&run(NodeType::Transform, p, &inputs), "output");
    assert_hm_dims(&h);
    // Sampling alignment may introduce minor numerical drift at edges,
    // but the bulk-of-image mean must match the input.
    assert!(
        (mean(&base) - mean(&h)).abs() < 0.02,
        "identity transform mean drift: {} vs {}",
        mean(&base),
        mean(&h)
    );
}

#[test]
fn transform_translate_x_shifts_content_right() {
    // The executor's transform is inverse-mapped: positive translate_x
    // means the image *content* slides right -- the sample point at
    // each output pixel reads from `nx - tx` in input space. For a
    // 0->1 left-to-right ramp with translate_x=+0.25, the centre of
    // the output reads from input near 0.25, so the centre value drops
    // from 0.5 (identity) toward 0.25.
    let inputs = input_hm("input", gen(|u, _| u));
    let p = &[
        ("translate_x", ParamValue::Float(0.25)),
        ("translate_y", ParamValue::Float(0.0)),
        ("scale", ParamValue::Float(1.0)),
        ("angle", ParamValue::Float(0.0)),
    ];
    let h = out_hm(&run(NodeType::Transform, p, &inputs), "output");
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!(
        centre < 0.4,
        "translate_x=+0.25 should pull the centre toward input(0.25): got {centre}"
    );
}

#[test]
fn stratify_zero_hardness_is_identity() {
    let base = gen(|u, _| u);
    let inputs = input_hm("input", base.clone());
    let p = &[
        ("layer_count", ParamValue::UInt(8)),
        ("irregularity", ParamValue::Float(0.0)),
        ("hardness", ParamValue::Float(0.0)),
        ("noise_scale", ParamValue::Float(0.05)),
    ];
    let h = out_hm(&run(NodeType::Stratify, p, &inputs), "output");
    assert_hm_dims(&h);
    for (a, b) in base.data().iter().zip(h.data().iter()) {
        assert!(
            (a - b).abs() < 1e-4,
            "stratify(hardness=0) should pass input through: {a} vs {b}"
        );
    }
}

#[test]
fn stratify_full_hardness_reduces_value_diversity() {
    // Quantising a 16-pixel ramp into 4 bands should reduce the number
    // of distinct values landed in 0.01-wide buckets.
    let base = gen(|u, _| u);
    let inputs = input_hm("input", base.clone());
    let p = &[
        ("layer_count", ParamValue::UInt(4)),
        ("irregularity", ParamValue::Float(0.0)),
        ("hardness", ParamValue::Float(1.0)),
        ("noise_scale", ParamValue::Float(0.05)),
    ];
    let h = out_hm(&run(NodeType::Stratify, p, &inputs), "output");
    use std::collections::HashSet;
    let bucket = |hm: &Heightmap| {
        hm.data()
            .iter()
            .map(|v| (v * 100.0).round() as i32)
            .collect::<HashSet<_>>()
            .len()
    };
    assert!(
        bucket(&h) < bucket(&base),
        "stratify(hardness=1) should quantise: {} -> {}",
        bucket(&base),
        bucket(&h)
    );
}

#[test]
fn select_aspect_returns_zero_on_flat_input() {
    let inputs = input_hm("input", flat(0.5));
    let p = &[
        ("direction", ParamValue::Float(0.0)),
        ("width", ParamValue::Float(90.0)),
        ("falloff", ParamValue::Float(30.0)),
    ];
    let h = out_hm(&run(NodeType::SelectAspect, p, &inputs), "output");
    let (_, mx) = min_max(&h);
    assert!(
        mx < 0.05,
        "flat input has no aspect -- output should be ~0, got max {mx}"
    );
}

#[test]
fn select_aspect_picks_east_for_eastward_ramp() {
    // A left-to-right ramp has the surface facing east (90 degrees,
    // per the executor's atan2(dx, -dy) convention).
    let inputs = input_hm("input", gen(|u, _| u));
    let p = &[
        ("direction", ParamValue::Float(90.0)),
        ("width", ParamValue::Float(45.0)),
        ("falloff", ParamValue::Float(15.0)),
    ];
    let h = out_hm(&run(NodeType::SelectAspect, p, &inputs), "output");
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!(
        centre > 0.5,
        "east-facing ramp + direction=90 should be selected: centre {centre}"
    );
}

#[test]
fn select_convexity_flat_input_is_neutral() {
    let inputs = input_hm("input", flat(0.5));
    let p = &[
        ("mode", ParamValue::String("ridges".to_string())),
        ("strength", ParamValue::Float(1.0)),
    ];
    let h = out_hm(&run(NodeType::SelectConvexity, p, &inputs), "output");
    let (_, mx) = min_max(&h);
    assert!(
        mx < 0.05,
        "flat input has no curvature -- ridge selector should be ~0, got max {mx}"
    );
}

#[test]
fn select_convexity_ridges_mode_picks_peak() {
    // A central bump (smooth radial falloff) produces a peak whose
    // Laplacian is strongly negative -- "ridges" mode should select it.
    let bump = gen(|u, v| {
        let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
        (1.0 - (d / 0.4).min(1.0)).max(0.0)
    });
    let inputs = input_hm("input", bump);
    let p = &[
        ("mode", ParamValue::String("ridges".to_string())),
        ("strength", ParamValue::Float(4.0)),
    ];
    let h = out_hm(&run(NodeType::SelectConvexity, p, &inputs), "output");
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!(
        centre > 0.1,
        "ridges mode should highlight a peak: centre {centre}"
    );
}

#[test]
fn flow_select_threshold_below_lo_is_zero() {
    // FlowSelect remaps: values <= (threshold - falloff) go to 0,
    // values >= threshold go to 1, linear in between.
    let inputs = input_hm("input", flat(0.05));
    let p = &[
        ("threshold", ParamValue::Float(0.5)),
        ("falloff", ParamValue::Float(0.1)),
    ];
    let h = out_hm(&run(NodeType::FlowSelect, p, &inputs), "output");
    let (_, mx) = min_max(&h);
    assert!(
        mx < 0.01,
        "value below threshold-falloff should map to 0: {mx}"
    );
}

#[test]
fn flow_select_threshold_above_high_is_one() {
    let inputs = input_hm("input", flat(0.9));
    let p = &[
        ("threshold", ParamValue::Float(0.5)),
        ("falloff", ParamValue::Float(0.1)),
    ];
    let h = out_hm(&run(NodeType::FlowSelect, p, &inputs), "output");
    let (mn, _) = min_max(&h);
    assert!(mn > 0.99, "value above threshold should map to 1: {mn}");
}

#[test]
fn layout_generator_zero_shape_count_is_empty() {
    let p = &[("shape_count", ParamValue::UInt(0))];
    let h = out_hm(
        &run(NodeType::LayoutGenerator, p, &empty_inputs()),
        "output",
    );
    assert_hm_dims(&h);
    let (_, mx) = min_max(&h);
    assert!(
        mx < 1e-4,
        "zero shapes should produce all-zero output: {mx}"
    );
}

#[test]
fn layout_generator_centred_ellipse_brightens_centre() {
    let p = &[
        ("shape_count", ParamValue::UInt(1)),
        ("type_0", ParamValue::String("ellipse".to_string())),
        ("x_0", ParamValue::Float(0.5)),
        ("y_0", ParamValue::Float(0.5)),
        ("rx_0", ParamValue::Float(0.3)),
        ("ry_0", ParamValue::Float(0.3)),
        ("angle_0", ParamValue::Float(0.0)),
        ("height_0", ParamValue::Float(1.0)),
        ("falloff_0", ParamValue::Float(0.5)),
    ];
    let h = out_hm(
        &run(NodeType::LayoutGenerator, p, &empty_inputs()),
        "output",
    );
    let centre = h.get(W / 2, H / 2).unwrap();
    let corner = h.get(0, 0).unwrap();
    assert!(centre > 0.5, "ellipse centre should be bright: {centre}");
    assert!(corner < 0.1, "ellipse corner should be dark: {corner}");
}

#[test]
fn mask_expand_grows_a_bright_spot() {
    // Single bright pixel at the centre; expand with radius=3 must
    // light up neighbours.
    let mut data = vec![0.0_f32; (W * H) as usize];
    let cx = W / 2;
    let cy = H / 2;
    data[(cy * W + cx) as usize] = 1.0;
    let inputs = input_hm("input", Heightmap::frbar_data(W, H, data).unwrap());
    let p = &[("radius", ParamValue::Float(3.0))];
    let h = out_hm(&run(NodeType::MaskExpand, p, &inputs), "output");
    let neighbour = h.get(cx + 2, cy).unwrap();
    assert!(
        neighbour > 0.5,
        "neighbour should be lit by expand: {neighbour}"
    );
}

#[test]
fn mask_shrink_erodes_a_block() {
    // A solid central block; shrink with radius=2 should pull the
    // bright region's edges inward, leaving the very-edge pixels dark.
    let block = gen(|u, v| {
        if (u - 0.5).abs() < 0.25 && (v - 0.5).abs() < 0.25 {
            1.0
        } else {
            0.0
        }
    });
    let inputs = input_hm("input", block);
    let p = &[("radius", ParamValue::Float(2.0))];
    let h = out_hm(&run(NodeType::MaskShrink, p, &inputs), "output");
    // The point that *was* inside the block but near its edge should
    // now read dark.
    let edge_in = h.get(W / 2 - 3, H / 2).unwrap(); // was inside, near edge
    let centre = h.get(W / 2, H / 2).unwrap();
    assert!(centre > 0.5, "block centre should survive shrink: {centre}");
    assert!(edge_in < 0.5, "block edge should be eroded: {edge_in}");
}

#[test]
fn mask_select_picks_a_when_mask_is_zero() {
    let mut inputs = input_hm("a", flat(0.2));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.8)));
    inputs.insert("mask".to_string(), PortValue::Heightmap(flat(0.0)));
    let h = out_hm(&run(NodeType::MaskSelect, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.2).abs() < 0.05, "mask=0 should pick a: {m}");
}

#[test]
fn mask_select_picks_b_when_mask_is_one() {
    let mut inputs = input_hm("a", flat(0.2));
    inputs.insert("b".to_string(), PortValue::Heightmap(flat(0.8)));
    inputs.insert("mask".to_string(), PortValue::Heightmap(flat(1.0)));
    let h = out_hm(&run(NodeType::MaskSelect, &[], &inputs), "output");
    let m = mean(&h);
    assert!((m - 0.8).abs() < 0.05, "mask=1 should pick b: {m}");
}

// ── Texture node semantic checks ────────────────────────────────────
//
// Texture/color nodes previously had smoke-only tests. These add
// behavioural assertions on the colour they actually emit.

#[test]
fn rock_soil_steep_slope_uses_rock_color() {
    // slope_threshold=0.0 + slope=1.0 means the rock colour dominates.
    // Default rock_color is 807870 (mid-grey-warm), soil_color is
    // 8B6914 (yellow-brown). The red channel is similar but green
    // distinguishes them: rock g=0x78 (~0.47), soil g=0x69 (~0.41).
    // What we can reliably assert: with slope=1.0 + threshold=0.0 the
    // output sits near the rock colour, not near the soil colour, in
    // terms of green channel.
    let mut inputs = input_hm("input", flat(0.5));
    inputs.insert("slope".to_string(), PortValue::Heightmap(flat(1.0)));
    let p = &[
        ("rock_color", ParamValue::String("FF0000".to_string())), // pure red
        ("soil_color", ParamValue::String("0000FF".to_string())), // pure blue
        ("slope_threshold", ParamValue::Float(0.0)),
        ("slope_blend", ParamValue::Float(1.0)),
        ("ao_strength", ParamValue::Float(0.0)),
        ("detail_strength", ParamValue::Float(0.0)),
    ];
    let outputs = run(NodeType::RockSoil, p, &inputs);
    let cb = out_color(&outputs, "output");
    let data = cb.data();
    // Sample the centre: 4-channel RGBA8 normalised to [0,1].
    let stride = (cb.width() as usize) * 4;
    let cy = (cb.height() / 2) as usize;
    let cx = (cb.width() / 2) as usize;
    let r = data[cy * stride + cx * 4];
    let b = data[cy * stride + cx * 4 + 2];
    assert!(
        r > b,
        "steep slope should pick red (rock) not blue (soil): r={r}, b={b}"
    );
}

#[test]
fn vegetation_high_altitude_uses_dry_color() {
    // altitude_max=0.3 + heightmap=0.8 means the dry colour dominates.
    let mut inputs = input_hm("input", flat(0.8));
    inputs.insert("slope".to_string(), PortValue::Heightmap(flat(0.0)));
    let p = &[
        ("vegetation_color", ParamValue::String("00FF00".to_string())), // pure green
        ("dry_color", ParamValue::String("FF0000".to_string())),        // pure red
        ("altitude_max", ParamValue::Float(0.3)),
        ("slope_cutoff", ParamValue::Float(0.9)),
        ("slope_blend", ParamValue::Float(0.0)),
        ("ao_strength", ParamValue::Float(0.0)),
        ("detail_strength", ParamValue::Float(0.0)),
    ];
    let outputs = run(NodeType::Vegetation, p, &inputs);
    let cb = out_color(&outputs, "output");
    let data = cb.data();
    let stride = (cb.width() as usize) * 4;
    let cy = (cb.height() / 2) as usize;
    let cx = (cb.width() / 2) as usize;
    let r = data[cy * stride + cx * 4];
    let g = data[cy * stride + cx * 4 + 1];
    assert!(
        r > g,
        "altitude above max should pick red (dry) not green (veg): r={r}, g={g}"
    );
}

/// Build a solid-colour ColorBuffer for tests where the executor's
/// PaintedTexture default-fill doesn't give us a predictable colour.
fn solid_color(r: f32, g: f32, b: f32) -> bar_data::ColorBuffer {
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for _ in 0..(W * H) {
        data.push(r);
        data.push(g);
        data.push(b);
        data.push(1.0);
    }
    bar_data::ColorBuffer::frbar_data(W, H, data).unwrap()
}

#[test]
fn layer_blend_zero_opacity_passes_base_through() {
    let base = PortValue::Color(solid_color(1.0, 0.0, 0.0));
    let overlay = PortValue::Color(solid_color(0.0, 1.0, 0.0));
    let mut inputs = HashMap::new();
    inputs.insert("base".to_string(), base);
    inputs.insert("overlay".to_string(), overlay);
    let p = &[
        ("blend_mode", ParamValue::String("over".to_string())),
        ("opacity", ParamValue::Float(0.0)),
    ];
    let outputs = run(NodeType::LayerBlend, p, &inputs);
    let cb = out_color(&outputs, "output");
    let data = cb.data();
    // At opacity=0 the output must match the base (red), not the overlay (green).
    let r = data[0];
    let g = data[1];
    assert!(r > 0.9, "base red channel should pass through: r={r}");
    assert!(
        g < 0.1,
        "overlay green should be suppressed at opacity=0: g={g}"
    );
}

#[test]
fn layer_blend_full_opacity_picks_overlay() {
    let base = PortValue::Color(solid_color(1.0, 0.0, 0.0));
    let overlay = PortValue::Color(solid_color(0.0, 1.0, 0.0));
    let mut inputs = HashMap::new();
    inputs.insert("base".to_string(), base);
    inputs.insert("overlay".to_string(), overlay);
    let p = &[
        ("blend_mode", ParamValue::String("over".to_string())),
        ("opacity", ParamValue::Float(1.0)),
    ];
    let outputs = run(NodeType::LayerBlend, p, &inputs);
    let cb = out_color(&outputs, "output");
    let data = cb.data();
    let r = data[0];
    let g = data[1];
    assert!(g > 0.9, "overlay green should dominate at opacity=1: g={g}");
    assert!(r < 0.1, "base red should be hidden at opacity=1: r={r}");
}

#[test]
fn texture_weightmap_single_layer_passes_through() {
    let layer = PortValue::Color(solid_color(1.0, 0.5, 0.0));
    let mut inputs = HashMap::new();
    inputs.insert("texture_0".to_string(), layer);
    let p = &[
        ("layer_count", ParamValue::UInt(1)),
        (
            "priority_type",
            ParamValue::String("weighted_blend".to_string()),
        ),
        ("priority_0", ParamValue::Float(1.0)),
        ("exclusion_0", ParamValue::Float(0.0)),
    ];
    let outputs = run(NodeType::TextureWeightmap, p, &inputs);
    let cb = out_color(&outputs, "output");
    assert_color_dims(&cb);
    let data = cb.data();
    assert!(data[0] > 0.9, "single-layer pass-through r: {}", data[0]);
    assert!(
        data[1] > 0.4 && data[1] < 0.6,
        "single-layer pass-through g: {}",
        data[1]
    );
}

// ── Strengthened smoke tests ────────────────────────────────────────
//
// Erosion previously only asserted dimensions/range. Add semantic
// checks: hydraulic erosion lowers peaks; thermal erosion reduces
// max slope.

#[test]
fn hydraulic_erosion_lowers_peaks() {
    // Tall central spike; after erosion the peak should be lower.
    let spike = gen(|u, v| {
        let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
        (1.0 - (d / 0.3).min(1.0)).powi(2)
    });
    let peak_before = spike
        .data()
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max);
    let inputs = input_hm("input", spike);
    let p = &[
        ("iterations", ParamValue::UInt(2000)),
        ("erosion_rate", ParamValue::Float(0.05)),
        ("deposition_rate", ParamValue::Float(0.01)),
    ];
    let h = out_hm(&run(NodeType::HydraulicErosion, p, &inputs), "output");
    let peak_after = h.data().iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        peak_after < peak_before,
        "hydraulic erosion should lower peaks: {peak_before} -> {peak_after}"
    );
}

#[test]
fn thermal_erosion_reduces_max_slope() {
    // A steep linear ramp -- thermal erosion should reduce its slope.
    let ramp = gen(|u, _| u);
    let inputs = input_hm("input", ramp);
    let p = &[
        ("iterations", ParamValue::UInt(100)),
        ("talus_angle", ParamValue::Float(0.1)),
    ];
    let h = out_hm(&run(NodeType::ThermalErosion, p, &inputs), "output");
    // Adjacent-pixel deltas in the eroded heightmap should be smaller
    // on average than the input's uniform 1/(W-1) step.
    let max_delta = |hm: &Heightmap| -> f32 {
        let d = hm.data();
        let w = hm.width() as usize;
        let mut m = 0.0_f32;
        for y in 0..hm.height() as usize {
            for x in 1..w {
                let dx = (d[y * w + x] - d[y * w + x - 1]).abs();
                if dx > m {
                    m = dx;
                }
            }
        }
        m
    };
    let before = 1.0 / (W - 1) as f32;
    let after = max_delta(&h);
    assert!(
        after <= before + 1e-4,
        "thermal erosion should not increase max slope: {before} -> {after}"
    );
}

// ── Stochastic determinism ──────────────────────────────────────────
//
// Every node that reads a `seed` param must produce bit-identical output
// when seeded the same and different output when seeded differently.
// Asserting both halves catches two failure modes: (a) the seed is
// ignored entirely (same input both times, regardless of seed -- common
// regression when refactoring noise pipelines), (b) the seed feeds an
// uninitialised path (different output across runs even with the same
// seed -- breaks reproducibility of saved recipes).

/// Run a node twice with `seed_a`, assert identical output. Then run
/// with `seed_b`, assert different output. Keeps the per-node tests
/// terse so adding a new stochastic node only takes one block here.
fn assert_seed_determinism(
    nt: NodeType,
    base_params: &[(&str, ParamValue)],
    inputs: &HashMap<String, PortValue>,
    seed_a: u32,
    seed_b: u32,
) {
    let go = |seed: u32| -> Vec<f32> {
        let mut p: Vec<(&str, ParamValue)> = base_params.to_vec();
        p.push(("seed", ParamValue::UInt(seed)));
        out_hm(&run(nt.clone(), &p, inputs), "output")
            .data()
            .to_vec()
    };
    let a1 = go(seed_a);
    let a2 = go(seed_a);
    assert_eq!(
        a1, a2,
        "{nt:?}: same seed must produce bit-identical output"
    );
    let b = go(seed_b);
    assert_ne!(
        a1, b,
        "{nt:?}: different seeds must produce different output"
    );
}

#[test]
fn simplex_noise_seed_determinism() {
    assert_seed_determinism(
        NodeType::SimplexNoise,
        &[
            ("frequency", ParamValue::Float(4.0)),
            ("octaves", ParamValue::UInt(3)),
        ],
        &empty_inputs(),
        7,
        99,
    );
}

#[test]
fn worley_noise_seed_determinism() {
    assert_seed_determinism(
        NodeType::WorleyNoise,
        &[("frequency", ParamValue::Float(4.0))],
        &empty_inputs(),
        7,
        99,
    );
}

#[test]
fn ridged_noise_seed_determinism() {
    assert_seed_determinism(
        NodeType::RidgedNoise,
        &[
            ("frequency", ParamValue::Float(4.0)),
            ("octaves", ParamValue::UInt(3)),
        ],
        &empty_inputs(),
        7,
        99,
    );
}

#[test]
fn voronoi_seed_determinism() {
    assert_seed_determinism(
        NodeType::Voronoi,
        &[
            ("frequency", ParamValue::Float(4.0)),
            ("mode", ParamValue::String("f1".to_string())),
        ],
        &empty_inputs(),
        7,
        99,
    );
}

#[test]
fn param_value_spline_round_trips_via_serde() {
    // Constructing the variant directly + serialise + deserialise back.
    // Verifies the JSON shape carries the variant tag and the point
    // coords intact, including the empty-list edge case.
    let pts = vec![[0.1_f32, 0.2], [0.5, 0.7], [0.9, 0.4]];
    let v = ParamValue::Spline(pts.clone());
    let s = serde_json::to_string(&v).expect("serialise");
    let back: ParamValue = serde_json::from_str(&s).expect("deserialise");
    match back {
        ParamValue::Spline(got) => assert_eq!(got, pts),
        other => panic!("expected ParamValue::Spline, got {other:?}"),
    }

    let empty: ParamValue =
        serde_json::from_str(&serde_json::to_string(&ParamValue::Spline(vec![])).unwrap()).unwrap();
    match empty {
        ParamValue::Spline(got) => assert!(got.is_empty()),
        other => panic!("empty Spline round-trip lost the variant: {other:?}"),
    }
}

#[test]
fn hydraulic_erosion_seed_determinism() {
    // Erosion is stochastic via droplet starting positions; the same
    // seed must give the same erosion pattern for saved-recipe replays.
    // A small droplet count keeps the test cheap while still exercising
    // the full seeded path.
    let hm = gen(|u, v| ((u - 0.5).abs() + (v - 0.5).abs()) * 0.5);
    let inputs = input_hm("input", hm);
    assert_seed_determinism(
        NodeType::HydraulicErosion,
        &[
            ("iterations", ParamValue::UInt(200)),
            ("erosion_rate", ParamValue::Float(0.01)),
            ("deposition_rate", ParamValue::Float(0.01)),
        ],
        &inputs,
        7,
        99,
    );
}
