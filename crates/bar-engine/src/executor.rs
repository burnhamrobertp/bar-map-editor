//! Bridges the graph engine to the compute layer.
//! Implements `NodeExecutor` to dispatch graph node operations to CPU compute.

use std::collections::HashMap;

use bar_compute::{
    generate_noise_cpu, hydraulic_erosion, thermal_erosion, HydraulicErosionParams, NoiseParams,
    NoiseType, ThermalErosionParams,
};
use bar_data::{smt::TILE_SIZE, ColorBuffer, Heightmap, SmfMap};
use bar_graph::{EvalError, NodeExecutor, NodeType, ParamValue, PortValue};

/// Executor that runs node operations using CPU compute.
/// GPU execution can be added later without changing the graph layer.
pub struct CpuExecutor;

impl NodeExecutor for CpuExecutor {
    fn execute(
        &self,
        node_type: &NodeType,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
        width: u32,
        height: u32,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let mut outputs = HashMap::new();

        match node_type {
            // --- Generators ---
            NodeType::PerlinNoise => {
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = generate_noise(NoiseType::Perlin, params, width, height)?;
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::SimplexNoise => {
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = generate_noise(NoiseType::Simplex, params, width, height)?;
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::WorleyNoise => {
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = generate_noise(NoiseType::Worley, params, width, height)?;
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::RidgedNoise => {
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = generate_noise(NoiseType::Ridged, params, width, height)?;
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Constant => {
                let value = get_float(params, "value", 0.5);
                let data = vec![value; (width as usize) * (height as usize)];
                let hm = Heightmap::frbar_data(width, height, data)
                    .map_err(|e| EvalError::Compute(e.to_string()))?;
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Filters ---
            NodeType::Blur => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let radius = get_float(params, "radius", 1.0);
                let hm = apply_blur(&input, radius);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Clamp => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let min_val = get_float(params, "min", 0.0);
                let max_val = get_float(params, "max", 1.0);
                let hm = apply_clamp(&input, min_val, max_val);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Invert => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = apply_invert(&input);
                let hm = apply_modulation(&input, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Terrace | NodeType::Sharpen => {
                // Placeholder until the underlying transform is implemented.
                // Ports are kept minimal (input + output) so users don't wire
                // modulators to a no-op.
                let input = get_input_heightmap(inputs, "input")?;
                outputs.insert("output".to_string(), PortValue::Heightmap(input));
            }
            NodeType::Curve => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = apply_curve(&input, params);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Preview => {
                // Preview is a real node: its executor receives
                // upstream values via its declared input ports
                // exactly like any other node. We pass them
                // through into the runtime output map under the
                // same names so the 3D viewport can read them via
                // the standard `outputs[node_id][port_name]`
                // accessor — no special "find Preview nodes
                // globally and inspect their incoming wires"
                // pathway. The node has no declared output ports
                // (nothing downstream consumes it), but its
                // runtime output map is what the viewport reads
                // from, the same way the viewport for any node
                // would.
                if let Some(v) = inputs.get("heightmap") {
                    outputs.insert("heightmap".to_string(), v.clone());
                }
                if let Some(v) = inputs.get("texture") {
                    outputs.insert("texture".to_string(), v.clone());
                }
                if let Some(v) = inputs.get("normal_map") {
                    outputs.insert("normal_map".to_string(), v.clone());
                }
                if let Some(v) = inputs.get("specular_map") {
                    outputs.insert("specular_map".to_string(), v.clone());
                }
            }

            NodeType::SubgraphInput | NodeType::SubgraphOutput => {
                // Both subgraph IO nodes are pure passthrough — the
                // value crossing the boundary on the input side
                // becomes the value the inner / outer graph reads
                // on the output side. Identical math; the only
                // difference is which "side" of the subgraph
                // boundary each one is rendered on.
                if let Some(v) = inputs.get("value") {
                    outputs.insert("value".to_string(), v.clone());
                }
            }

            // --- Erosion ---
            NodeType::HydraulicErosion => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let params_e = HydraulicErosionParams {
                    num_droplets: get_uint(params, "iterations", 50_000),
                    inertia: get_float(params, "inertia", 0.05),
                    capacity_factor: get_float(params, "capacity_factor", 4.0),
                    min_capacity: get_float(params, "min_capacity", 0.01),
                    deposition_rate: get_float(params, "deposition_rate", 0.3),
                    erosion_rate: get_float(params, "erosion_rate", 0.3),
                    evaporation_rate: get_float(params, "evaporation_rate", 0.01),
                    gravity: get_float(params, "gravity", 4.0),
                    max_lifetime: get_uint(params, "max_lifetime", 30),
                    erosion_radius: get_uint(params, "erosion_radius", 3),
                    seed: get_uint(params, "seed", 0),
                };
                let hm = hydraulic_erosion(&input, &params_e)
                    .map_err(|e| EvalError::Compute(e.to_string()))?;
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::ThermalErosion => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let params_e = ThermalErosionParams {
                    iterations: get_uint(params, "iterations", 50),
                    talus_angle: get_float(params, "talus_angle", 0.004),
                    erosion_rate: get_float(params, "erosion_rate", 0.5),
                };
                let hm = thermal_erosion(&input, &params_e)
                    .map_err(|e| EvalError::Compute(e.to_string()))?;
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Combiners ---
            // Mask gates the combine result: lerp(a, combined, mask).
            // Equivalent to "where mask=0 the operation has no effect."
            NodeType::Blend => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let factor = get_float(params, "factor", 0.5).clamp(0.0, 1.0);
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let blended = blend_heightmaps(&a, &b, factor);
                // Control scales blend strength toward `b`, mask gates the
                // result back toward `a`. Both lerp from `a` toward `blended`.
                let hm = apply_modulation(&a, blended, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Add => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = combine_heightmaps(&a, &b, |va, vb| (va + vb).min(1.0));
                let hm = apply_modulation(&a, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Subtract => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = combine_heightmaps(&a, &b, |va, vb| (va - vb).max(0.0));
                let hm = apply_modulation(&a, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Multiply => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = combine_heightmaps(&a, &b, |va, vb| va * vb);
                let hm = apply_modulation(&a, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Max => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = combine_heightmaps(&a, &b, |va, vb| va.max(vb));
                let hm = apply_modulation(&a, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Min => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = combine_heightmaps(&a, &b, |va, vb| va.min(vb));
                let hm = apply_modulation(&a, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Texture/Splat ---
            NodeType::SlopeMap => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = compute_slope_map(&input);
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::HeightSelect => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let low = get_float(params, "low", 0.3);
                let high = get_float(params, "high", 0.7);
                let falloff = get_float(params, "falloff", 0.1);
                let hm = compute_height_select(&input, low, high, falloff);
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::SplatMap => {
                let slope = get_optional_heightmap(inputs, "slope");
                let band0 = get_optional_heightmap(inputs, "band0");
                let band1 = get_optional_heightmap(inputs, "band1");
                let band2 = get_optional_heightmap(inputs, "band2");
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = compose_splat_map(
                    slope.as_ref(),
                    band0.as_ref(),
                    band1.as_ref(),
                    band2.as_ref(),
                    width,
                    height,
                );
                let hm = scale_by_field(hm, ctrl.as_ref());
                let hm = scale_by_field(hm, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::AutoTexture => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let color = generate_auto_texture(&input, slope.as_ref(), params);
                // Neutral = transparent black so masked regions don't paint
                // opaque gray over downstream composite layers.
                let color = apply_color_modulation(
                    [0.0, 0.0, 0.0, 0.0],
                    color,
                    ctrl.as_ref(),
                    mask.as_ref(),
                );
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::RockSoil => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let mask = get_optional_heightmap(inputs, "mask");
                let color = generate_rock_soil(&input, slope.as_ref(), params);
                let color =
                    apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::Vegetation => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let mask = get_optional_heightmap(inputs, "mask");
                let color = generate_vegetation(&input, slope.as_ref(), params);
                let color =
                    apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::TextureOverlay => {
                let base = get_input_color(inputs, "base")?;
                let overlay = get_input_color(inputs, "overlay")?;
                let distribution = get_optional_heightmap(inputs, "distribution");
                let color =
                    generate_texture_overlay(&base, &overlay, distribution.as_ref(), params);
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            // --- Map Layer Generators ---
            NodeType::NormalMap => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let strength = get_float(params, "strength", 1.0);
                let color = generate_normal_map(&input, strength);
                // Neutral normal = flat surface [0.5, 0.5, 1.0, 1.0] in tangent space
                let color =
                    apply_color_modulation([0.5, 0.5, 1.0, 1.0], color, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Color(color));
            }
            NodeType::GrassMap => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let density = get_optional_heightmap(inputs, "density");
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = generate_grass_map(&input, slope.as_ref(), params);
                let hm = scale_by_field(hm, density.as_ref());
                let hm = scale_by_field(hm, ctrl.as_ref());
                let hm = scale_by_field(hm, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::SpecularMap => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = generate_specular_map(&input, slope.as_ref(), params);
                let hm = scale_by_field(hm, ctrl.as_ref());
                let hm = scale_by_field(hm, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Mask ---
            NodeType::Mask => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = scale_by_field(input, ctrl.as_ref());
                outputs.insert("mask".to_string(), PortValue::Mask(hm));
            }

            NodeType::PaintedHeightmap => {
                let data_str = get_string(params, "data", "");
                let src_res = get_uint(params, "resolution", 256).max(1);
                let pixels = hex_decode_mask(data_str);
                let hm = painted_grayscale_to_heightmap(pixels, src_res, width, height);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::Sculpt => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let data_str = get_string(params, "data", "");
                let mut sculpted = input.clone();
                if !data_str.is_empty() {
                    let src_res = get_uint(params, "resolution", 256).max(1);
                    let scale = get_float(params, "scale", 0.5);
                    let pixels = hex_decode_mask(data_str);
                    apply_sculpt_delta(&mut sculpted, &pixels, src_res, scale);
                }
                // Mask confines sculpt delta to specific areas (mask=0: original, mask=1: sculpted)
                let hm = apply_modulation(&input, sculpted, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::PaintedTexture => {
                let data_str = get_string(params, "data", "");
                let pixels = hex_decode_mask(data_str);
                let tex = painted_rgb_to_color_buffer(pixels, PAINTED_TEXTURE_RES, width, height);
                outputs.insert("output".to_string(), PortValue::Color(tex));
            }

            // --- Mask Operations ---
            NodeType::MaskThreshold => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let threshold = get_float(params, "threshold", 0.5);
                let smoothness = get_float(params, "smoothness", 0.0);
                let hm = if let Some(c) = &ctrl {
                    // Control shifts the threshold spatially (WM: higher control = threshold moves up)
                    let data: Vec<f32> = input
                        .data()
                        .iter()
                        .zip(c.data())
                        .map(|(&v, &cv)| {
                            let t = (threshold + cv - 0.5).clamp(0.0, 1.0);
                            if smoothness <= 0.001 {
                                if v >= t {
                                    1.0
                                } else {
                                    0.0
                                }
                            } else {
                                let s = ((v - t) / smoothness + 0.5).clamp(0.0, 1.0);
                                s * s * (3.0 - 2.0 * s)
                            }
                        })
                        .collect();
                    Heightmap::frbar_data(input.width(), input.height(), data).unwrap()
                } else {
                    apply_mask_threshold(&input, threshold, smoothness)
                };
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::MaskInvert => {
                let input = get_input_heightmap(inputs, "input")?;
                let hm = apply_invert(&input);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::MaskBlur => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let radius = get_float(params, "radius", 2.0);
                let hm = apply_blur(&input, radius);
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::MaskApply => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_input_heightmap(inputs, "mask")?;
                let bg = inputs.get("background").and_then(|v| match v {
                    PortValue::Heightmap(h) => Some(h.clone()),
                    _ => None,
                });
                let hm = apply_mask(&input, &mask, bg.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Additional Generators ---
            NodeType::FileInput => {
                let path = get_string(params, "path", "");
                let hm = load_file_input(path, width, height)?;
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Voronoi => {
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = generate_voronoi(params, width, height);
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Gradient => {
                let ctrl = get_optional_heightmap(inputs, "control");
                let hm = generate_gradient(params, width, height);
                let hm = scale_by_field(hm, ctrl.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Additional Filters ---
            NodeType::SimpleTransform => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let scale = get_float(params, "scale", 1.0);
                let offset = get_float(params, "offset", 0.0);
                let invert = get_bool(params, "invert", false);
                let hm = apply_simple_transform(&input, scale, offset, invert);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Normalize => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = apply_normalize(&input);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::BiasGain => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let bias = get_float(params, "bias", 0.5);
                let gain = get_float(params, "gain", 0.5);
                let hm = apply_bias_gain(&input, bias, gain);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Displacement => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let displacement = get_input_heightmap(inputs, "displacement")?;
                let strength = get_float(params, "strength", 0.1);
                let hm = apply_displacement(&input, &displacement, strength);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Additional Combiners ---
            NodeType::Chooser => {
                let a = get_input_heightmap(inputs, "a")?;
                let b = get_input_heightmap(inputs, "b")?;
                let mask = get_input_heightmap(inputs, "mask")?;
                let hm = apply_chooser(&a, &b, &mask);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Bundler/Packaging ---
            NodeType::FileReference => {
                // Produce a File port value from node params
                let path = match params.get("path") {
                    Some(ParamValue::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let bundle_path = match params.get("bundle_path") {
                    Some(ParamValue::String(s)) => s.clone(),
                    _ => path.clone(),
                };
                outputs.insert(
                    "file".to_string(),
                    PortValue::File(bar_graph::FileRef { path, bundle_path }),
                );
            }

            NodeType::Bundler => {
                // Terminal node — inputs are collected after graph evaluation by execute_bundlers().
            }

            NodeType::SmfImport => {
                let path = get_string(params, "path", "");
                if path.is_empty() {
                    return Ok(outputs);
                }

                let file = std::fs::File::open(path).map_err(|e| {
                    EvalError::Compute(format!("SmfImport: cannot open '{}': {}", path, e))
                })?;
                let smf = SmfMap::read(&mut std::io::BufReader::new(file))
                    .map_err(|e| EvalError::Compute(format!("SmfImport: parse error: {}", e)))?;

                outputs.insert("heightmap".to_string(), PortValue::Heightmap(smf.heightmap));

                if get_bool(params, "load_metalmap", true) {
                    let (mm_w, mm_h) = smf.header.metalmap_size();
                    let mm_data: Vec<f32> =
                        smf.metalmap.iter().map(|&v| v as f32 / 255.0).collect();
                    if let Ok(mm_hm) = Heightmap::frbar_data(mm_w, mm_h, mm_data) {
                        outputs.insert("metalmap".to_string(), PortValue::Heightmap(mm_hm));
                    }
                }

                if get_bool(params, "load_typemap", true) {
                    let (tm_w, tm_h) = smf.header.metalmap_size();
                    let tm_data: Vec<f32> = smf.typemap.iter().map(|&v| v as f32 / 255.0).collect();
                    if let Ok(tm_hm) = Heightmap::frbar_data(tm_w, tm_h, tm_data) {
                        outputs.insert("typemap".to_string(), PortValue::Heightmap(tm_hm));
                    }
                }
            }

            NodeType::SmtImport => {
                let path = get_string(params, "path", "");
                if path.is_empty() {
                    return Ok(outputs);
                }
                let max_preview = get_uint(params, "max_preview_size", 4096);

                // Determine tile grid: prefer reading from the paired .smf file.
                let smf_path = get_string(params, "smf_path", "");
                let (tiles_x, tiles_y, tile_indices) = if !smf_path.is_empty() {
                    if let Ok(f) = std::fs::File::open(smf_path) {
                        if let Ok(smf) = SmfMap::read(&mut std::io::BufReader::new(f)) {
                            let (tx, ty) = smf.header.tile_grid_size();
                            (tx, ty, smf.tile_indices)
                        } else {
                            let tx = get_uint(params, "tiles_x", 0);
                            let ty = get_uint(params, "tiles_y", 0);
                            let seq: Vec<i32> = (0..(tx * ty) as i32).collect();
                            (tx, ty, seq)
                        }
                    } else {
                        let tx = get_uint(params, "tiles_x", 0);
                        let ty = get_uint(params, "tiles_y", 0);
                        let seq: Vec<i32> = (0..(tx * ty) as i32).collect();
                        (tx, ty, seq)
                    }
                } else {
                    let tx = get_uint(params, "tiles_x", 0);
                    let ty = get_uint(params, "tiles_y", 0);
                    let seq: Vec<i32> = (0..(tx * ty) as i32).collect();
                    (tx, ty, seq)
                };

                if tiles_x == 0 || tiles_y == 0 {
                    return Ok(outputs);
                }

                let file = std::fs::File::open(path).map_err(|e| {
                    EvalError::Compute(format!("SmtImport: cannot open '{}': {}", path, e))
                })?;
                let tiles = bar_data::smt::read_smt(&mut std::io::BufReader::new(file))
                    .map_err(|e| EvalError::Compute(format!("SmtImport: parse error: {}", e)))?;

                let src_w = tiles_x * TILE_SIZE;
                let src_h = tiles_y * TILE_SIZE;
                let out_w = max_preview.min(src_w).max(1);
                let out_h = max_preview.min(src_h).max(1);

                let rgba =
                    assemble_texture_preview(&tiles, &tile_indices, tiles_x, tiles_y, out_w, out_h);
                let color_buf = ColorBuffer::from_rgba8(out_w, out_h, &rgba)
                    .map_err(|e| EvalError::Compute(e.to_string()))?;
                outputs.insert("texture".to_string(), PortValue::Color(color_buf));
            }

            NodeType::PassThrough => {
                let files_str = get_string(params, "files", "");
                let file_list: Vec<bar_graph::FileRef> = files_str
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.splitn(2, '|');
                        let path = parts.next()?.trim().to_string();
                        // Bundle paths must use forward slashes; self-heal any
                        // legacy backslashed entries from older saved projects
                        // so the bundler validator doesn't reject them.
                        let bundle_path = parts.next()?.trim().replace('\\', "/");
                        if path.is_empty() {
                            None
                        } else {
                            Some(bar_graph::FileRef { path, bundle_path })
                        }
                    })
                    .collect();
                outputs.insert("files".to_string(), PortValue::FileList(file_list));
            }
        }

        Ok(outputs)
    }
}

fn generate_noise(
    noise_type: NoiseType,
    params: &HashMap<String, ParamValue>,
    width: u32,
    height: u32,
) -> Result<Heightmap, EvalError> {
    let noise_params = NoiseParams {
        width,
        height,
        noise_type,
        octaves: get_uint(params, "octaves", 6),
        lacunarity: get_float(params, "lacunarity", 2.0),
        persistence: get_float(params, "persistence", 0.5),
        frequency: get_float(params, "frequency", 4.0),
        seed: get_uint(params, "seed", 0),
        offset_x: get_float(params, "offset_x", 0.0),
        offset_y: get_float(params, "offset_y", 0.0),
    };

    generate_noise_cpu(&noise_params).map_err(|e| EvalError::Compute(e.to_string()))
}

/// Assemble a downsampled RGBA8 texture from decoded SMT tiles.
///
/// Uses nearest-neighbor sampling directly against the tile grid — no full-resolution
/// intermediate buffer is ever allocated. Each output pixel maps to one source texel.
fn assemble_texture_preview(
    tiles: &[Vec<u8>],
    tile_indices: &[i32],
    tiles_x: u32,
    tiles_y: u32,
    out_w: u32,
    out_h: u32,
) -> Vec<u8> {
    let src_w = tiles_x * TILE_SIZE;
    let src_h = tiles_y * TILE_SIZE;
    let tile_sz = TILE_SIZE as usize;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];

    for dy in 0..out_h {
        for dx in 0..out_w {
            // Map output pixel to nearest source texel
            let sx = (dx as u64 * src_w as u64 / out_w as u64) as u32;
            let sy = (dy as u64 * src_h as u64 / out_h as u64) as u32;

            let tile_x = (sx / TILE_SIZE).min(tiles_x.saturating_sub(1));
            let tile_y = (sy / TILE_SIZE).min(tiles_y.saturating_sub(1));
            let px = (sx % TILE_SIZE) as usize;
            let py = (sy % TILE_SIZE) as usize;

            let flat = (tile_y * tiles_x + tile_x) as usize;
            if let Some(&idx_raw) = tile_indices.get(flat) {
                if idx_raw >= 0 {
                    if let Some(tile) = tiles.get(idx_raw as usize) {
                        let src = (py * tile_sz + px) * 4;
                        let dst = (dy * out_w + dx) as usize * 4;
                        out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                    }
                }
            }
        }
    }
    out
}

fn get_input_heightmap(
    inputs: &HashMap<String, PortValue>,
    name: &str,
) -> Result<Heightmap, EvalError> {
    match inputs.get(name) {
        Some(PortValue::Heightmap(hm)) => Ok(hm.clone()),
        Some(PortValue::Mask(hm)) => Ok(hm.clone()),
        _ => Err(EvalError::MissingInput {
            node: bar_graph::NodeId(0),
            port: name.to_string(),
        }),
    }
}

fn get_input_color(
    inputs: &HashMap<String, PortValue>,
    name: &str,
) -> Result<ColorBuffer, EvalError> {
    match inputs.get(name) {
        Some(PortValue::Color(cb)) => Ok(cb.clone()),
        _ => Err(EvalError::MissingInput {
            node: bar_graph::NodeId(0),
            port: name.to_string(),
        }),
    }
}

fn get_float(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}

fn get_uint(params: &HashMap<String, ParamValue>, key: &str, default: u32) -> u32 {
    match params.get(key) {
        Some(ParamValue::UInt(v)) => *v,
        _ => default,
    }
}

fn blend_heightmaps(a: &Heightmap, b: &Heightmap, factor: f32) -> Heightmap {
    let w = a.width().min(b.width());
    let h = a.height().min(b.height());
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let va = a.get(x, y).unwrap_or(0.0);
            let vb = b.get(x, y).unwrap_or(0.0);
            data[(y as usize) * (w as usize) + (x as usize)] = va * (1.0 - factor) + vb * factor;
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

fn combine_heightmaps(a: &Heightmap, b: &Heightmap, op: impl Fn(f32, f32) -> f32) -> Heightmap {
    let w = a.width().min(b.width());
    let h = a.height().min(b.height());
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let va = a.get(x, y).unwrap_or(0.0);
            let vb = b.get(x, y).unwrap_or(0.0);
            data[(y as usize) * (w as usize) + (x as usize)] = op(va, vb);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

// ── Modulation helpers ────────────────────────────────────────────────────────
// All three port types (Control, Density, Mask) arrive as PortValue::Heightmap
// or PortValue::Mask -- the graph routes by port name, not kind. These helpers
// are the single implementation point for WM-compatible modulation semantics.
//
// Every helper assumes its inputs are at the eval-graph resolution. The graph
// pipeline normalizes generators / FileInput to (width, height), so callers
// should never feed mismatched buffers; the debug_assert_eq! guards catch any
// regression that would otherwise panic inside Heightmap::frbar_data.

/// Optional heightmap input -- None if the port is unconnected or mistyped.
fn get_optional_heightmap(inputs: &HashMap<String, PortValue>, name: &str) -> Option<Heightmap> {
    match inputs.get(name) {
        Some(PortValue::Heightmap(hm)) | Some(PortValue::Mask(hm)) => Some(hm.clone()),
        _ => None,
    }
}

/// Multiply every pixel by an optional scale field, in place.
/// Returns `effect` unchanged when `field` is None.
fn scale_by_field(mut effect: Heightmap, field: Option<&Heightmap>) -> Heightmap {
    let Some(f) = field else {
        return effect;
    };
    debug_assert_eq!(effect.width(), f.width(), "scale_by_field: width mismatch");
    debug_assert_eq!(
        effect.height(),
        f.height(),
        "scale_by_field: height mismatch"
    );
    for (e, &s) in effect.data_mut().iter_mut().zip(f.data()) {
        *e *= s.clamp(0.0, 1.0);
    }
    effect
}

/// Apply optional control and mask to a filter node (one with a passthrough `input`).
///
/// WM semantics:
///   - Control modulates effect strength: `lerp(input, effect, control)`
///   - Mask gates where the effect applies:  `lerp(input, effect, mask)`
///   - Both together multiply the weights in a single pass
///
/// Mutates `effect` in place; returns it untouched when both ports are unconnected.
fn apply_modulation(
    input: &Heightmap,
    mut effect: Heightmap,
    control: Option<&Heightmap>,
    mask: Option<&Heightmap>,
) -> Heightmap {
    if control.is_none() && mask.is_none() {
        return effect;
    }
    debug_assert_eq!(
        input.width(),
        effect.width(),
        "apply_modulation: input/effect width"
    );
    debug_assert_eq!(
        input.height(),
        effect.height(),
        "apply_modulation: input/effect height"
    );
    if let Some(c) = control {
        debug_assert_eq!(input.width(), c.width(), "apply_modulation: control width");
        debug_assert_eq!(
            input.height(),
            c.height(),
            "apply_modulation: control height"
        );
    }
    if let Some(m) = mask {
        debug_assert_eq!(input.width(), m.width(), "apply_modulation: mask width");
        debug_assert_eq!(input.height(), m.height(), "apply_modulation: mask height");
    }
    let in_d = input.data();
    let ef_d = effect.data_mut();
    match (control, mask) {
        (Some(c), Some(m)) => {
            let cd = c.data();
            let md = m.data();
            for i in 0..in_d.len() {
                let t = (cd[i].clamp(0.0, 1.0) * md[i].clamp(0.0, 1.0)).clamp(0.0, 1.0);
                ef_d[i] = in_d[i] + (ef_d[i] - in_d[i]) * t;
            }
        }
        (Some(w), None) | (None, Some(w)) => {
            let wd = w.data();
            for i in 0..in_d.len() {
                let t = wd[i].clamp(0.0, 1.0);
                ef_d[i] = in_d[i] + (ef_d[i] - in_d[i]) * t;
            }
        }
        (None, None) => unreachable!(),
    }
    effect
}

/// Apply optional control and mask to a Color output.
/// Blends each pixel from `neutral` toward `effect` by `control * mask`.
/// Returns `effect` unchanged when both are None; otherwise mutates in place.
fn apply_color_modulation(
    neutral: [f32; 4],
    mut effect: ColorBuffer,
    control: Option<&Heightmap>,
    mask: Option<&Heightmap>,
) -> ColorBuffer {
    if control.is_none() && mask.is_none() {
        return effect;
    }
    if let Some(c) = control {
        debug_assert_eq!(
            effect.width(),
            c.width(),
            "apply_color_modulation: control width"
        );
        debug_assert_eq!(
            effect.height(),
            c.height(),
            "apply_color_modulation: control height"
        );
    }
    if let Some(m) = mask {
        debug_assert_eq!(
            effect.width(),
            m.width(),
            "apply_color_modulation: mask width"
        );
        debug_assert_eq!(
            effect.height(),
            m.height(),
            "apply_color_modulation: mask height"
        );
    }
    let ctrl_d = control.map(Heightmap::data);
    let mask_d = mask.map(Heightmap::data);
    for (i, pixel) in effect.data_mut().chunks_exact_mut(4).enumerate() {
        let cv = ctrl_d.map_or(1.0, |d| d[i].clamp(0.0, 1.0));
        let mv = mask_d.map_or(1.0, |d| d[i].clamp(0.0, 1.0));
        let t = cv * mv;
        for ch in 0..4 {
            pixel[ch] = neutral[ch] + (pixel[ch] - neutral[ch]) * t;
        }
    }
    effect
}

/// Gaussian blur approximation using separable box blur (3 passes).
fn apply_blur(input: &Heightmap, radius: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let r = (radius.round() as usize).clamp(1, 64);

    let mut src: Vec<f32> = input.data().to_vec();
    let mut dst: Vec<f32> = vec![0.0; w * h];

    // 3-pass box blur approximates Gaussian
    for _ in 0..3 {
        // Horizontal pass
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                let mut count = 0.0;
                let x_start = x.saturating_sub(r);
                let x_end = (x + r + 1).min(w);
                for xx in x_start..x_end {
                    sum += src[y * w + xx];
                    count += 1.0;
                }
                dst[y * w + x] = sum / count;
            }
        }
        std::mem::swap(&mut src, &mut dst);

        // Vertical pass
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                let mut count = 0.0;
                let y_start = y.saturating_sub(r);
                let y_end = (y + r + 1).min(h);
                for yy in y_start..y_end {
                    sum += src[yy * w + x];
                    count += 1.0;
                }
                dst[y * w + x] = sum / count;
            }
        }
        std::mem::swap(&mut src, &mut dst);
    }

    Heightmap::frbar_data(w as u32, h as u32, src).unwrap()
}

fn apply_clamp(input: &Heightmap, min_val: f32, max_val: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| v.clamp(min_val, max_val))
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

fn apply_invert(input: &Heightmap) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data: Vec<f32> = input.data().iter().map(|&v| 1.0 - v).collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Threshold a heightmap into a binary (or smooth) mask.
/// With smoothness=0: hard binary. With smoothness>0: smooth sigmoid-like transition.
fn apply_mask_threshold(input: &Heightmap, threshold: f32, smoothness: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| {
            if smoothness <= 0.001 {
                if v >= threshold {
                    1.0
                } else {
                    0.0
                }
            } else {
                // Smooth transition using hermite interpolation
                let t = ((v - threshold) / smoothness + 0.5).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
        })
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Apply a mask to blend between input and background.
/// output = input * mask + background * (1 - mask)
fn apply_mask(input: &Heightmap, mask: &Heightmap, background: Option<&Heightmap>) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let val = input.get(x, y).unwrap_or(0.0);
            let m = mask.get(x, y).unwrap_or(1.0);
            let bg = background.and_then(|b| b.get(x, y)).unwrap_or(0.0);
            data[(y as usize) * (w as usize) + (x as usize)] = val * m + bg * (1.0 - m);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Compute slope map: each pixel is the maximum gradient magnitude at that point.
/// Output range [0, 1] where 0 = flat, 1 = very steep.
fn compute_slope_map(input: &Heightmap) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let c = input.get(x, y).unwrap_or(0.0);
            let r = input.get((x + 1).min(w - 1), y).unwrap_or(c);
            let l = input.get(x.saturating_sub(1), y).unwrap_or(c);
            let d = input.get(x, (y + 1).min(h - 1)).unwrap_or(c);
            let u = input.get(x, y.saturating_sub(1)).unwrap_or(c);

            let dx = (r - l) * 0.5;
            let dy = (d - u) * 0.5;
            // Scale to reasonable range (slopes are typically small values)
            let slope = (dx * dx + dy * dy).sqrt() * 4.0;
            data[(y as usize) * (w as usize) + (x as usize)] = slope.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Height band selection with smooth falloff.
/// Returns 1.0 for heights in [low, high], fading to 0.0 within `falloff` distance.
fn compute_height_select(input: &Heightmap, low: f32, high: f32, falloff: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| {
            if v >= low && v <= high {
                1.0
            } else if v < low {
                let dist = low - v;
                if falloff > 0.0 {
                    (1.0 - dist / falloff).max(0.0)
                } else {
                    0.0
                }
            } else {
                let dist = v - high;
                if falloff > 0.0 {
                    (1.0 - dist / falloff).max(0.0)
                } else {
                    0.0
                }
            }
        })
        .collect();

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Compose up to 4 weight channels into a single normalized splat map.
/// The output encodes the dominant channel index (0–3) as a value in [0, 1].
/// For use with Spring/Recoil typemap (8-bit indices) this would be quantized.
fn compose_splat_map(
    slope: Option<&Heightmap>,
    band0: Option<&Heightmap>,
    band1: Option<&Heightmap>,
    band2: Option<&Heightmap>,
    width: u32,
    height: u32,
) -> Heightmap {
    let size = (width as usize) * (height as usize);
    let mut data = vec![0.0f32; size];

    let zero = vec![0.0f32; size];
    let slope_data = slope.map(|h| h.data()).unwrap_or(&zero);
    let b0_data = band0.map(|h| h.data()).unwrap_or(&zero);
    let b1_data = band1.map(|h| h.data()).unwrap_or(&zero);
    let b2_data = band2.map(|h| h.data()).unwrap_or(&zero);

    for (i, pixel) in data.iter_mut().enumerate() {
        let channels = [
            *b0_data.get(i).unwrap_or(&0.0),
            *b1_data.get(i).unwrap_or(&0.0),
            *b2_data.get(i).unwrap_or(&0.0),
            *slope_data.get(i).unwrap_or(&0.0),
        ];

        // Find dominant channel
        let max_idx = channels
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Spring typemap: terrain type index (0-255)
        // Encode as type index normalized to [0,1]
        *pixel = max_idx as f32 / 255.0;
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}

/// Biome gradient: list of `(rgb, height)` stops sorted by height.
/// Heights are normalised to `[0, 1]`. Each biome owns both its
/// palette AND its thresholds — a "snow" stop in mountainous sits
/// at 0.85, in temperate at 0.95, and is absent entirely in tropical.
type BiomeGradient = &'static [([f32; 3], f32)];

const BIOME_TEMPERATE: BiomeGradient = &[
    ([0.05, 0.10, 0.30], 0.00), // deep water
    ([0.10, 0.25, 0.50], 0.15), // shallow water
    ([0.76, 0.70, 0.50], 0.20), // sand/beach
    ([0.30, 0.55, 0.15], 0.30), // lowland grass
    ([0.20, 0.45, 0.10], 0.50), // forest
    ([0.40, 0.35, 0.25], 0.65), // dirt
    ([0.45, 0.42, 0.38], 0.75), // rock
    ([0.55, 0.52, 0.48], 0.85), // light rock
    ([0.90, 0.92, 0.95], 0.95), // snow
    ([1.00, 1.00, 1.00], 1.00), // peak snow
];

const BIOME_GRASSLAND: BiomeGradient = &[
    ([0.20, 0.30, 0.20], 0.00), // muddy water
    ([0.30, 0.45, 0.25], 0.10), // marsh
    ([0.78, 0.72, 0.50], 0.15), // sand
    ([0.45, 0.65, 0.20], 0.25), // bright grass
    ([0.55, 0.60, 0.25], 0.55), // dry grass
    ([0.55, 0.45, 0.30], 0.80), // dirt
    ([0.70, 0.60, 0.45], 1.00), // tan rock — no snow
];

const BIOME_MOUNTAINOUS: BiomeGradient = &[
    ([0.04, 0.08, 0.20], 0.00), // dark water
    ([0.10, 0.30, 0.45], 0.10), // alpine lake
    ([0.45, 0.42, 0.40], 0.15), // scree
    ([0.35, 0.45, 0.20], 0.25), // sparse grass
    ([0.20, 0.35, 0.10], 0.40), // forest
    ([0.40, 0.32, 0.25], 0.50), // dirt
    ([0.45, 0.42, 0.38], 0.60), // rock
    ([0.60, 0.58, 0.55], 0.75), // light rock
    ([0.92, 0.94, 0.96], 0.85), // snow line — early
    ([1.00, 1.00, 1.00], 1.00), // peak snow
];

const BIOME_TROPICAL: BiomeGradient = &[
    ([0.05, 0.40, 0.55], 0.00), // deep tropical water
    ([0.30, 0.70, 0.75], 0.15), // shallow turquoise
    ([0.95, 0.92, 0.80], 0.20), // white sand
    ([0.20, 0.55, 0.15], 0.30), // jungle
    ([0.10, 0.40, 0.10], 0.55), // dense jungle
    ([0.55, 0.30, 0.20], 0.75), // red dirt
    ([0.70, 0.45, 0.30], 1.00), // red rock — no snow
];

const BIOME_DESERT: BiomeGradient = &[
    ([0.78, 0.72, 0.55], 0.00), // dry lakebed (no water)
    ([0.85, 0.75, 0.55], 0.20), // tan sand
    ([0.90, 0.78, 0.50], 0.40), // golden sand
    ([0.70, 0.45, 0.30], 0.60), // red dirt
    ([0.60, 0.35, 0.25], 0.75), // red rock
    ([0.45, 0.25, 0.20], 0.90), // dark red rock
    ([0.85, 0.75, 0.60], 1.00), // pale rock crown
];

const BIOME_TUNDRA: BiomeGradient = &[
    ([0.10, 0.15, 0.25], 0.00), // dark cold water
    ([0.30, 0.30, 0.30], 0.15), // frozen mud
    ([0.40, 0.45, 0.30], 0.25), // sparse moss
    ([0.45, 0.50, 0.40], 0.40), // grey-green tundra
    ([0.60, 0.62, 0.65], 0.55), // frost rock
    ([0.85, 0.88, 0.92], 0.70), // snow takes over early
    ([0.95, 0.97, 1.00], 1.00), // ice
];

const BIOME_LUNAR: BiomeGradient = &[
    ([0.10, 0.10, 0.10], 0.00), // crater shadow
    ([0.30, 0.30, 0.30], 0.30), // regolith
    ([0.55, 0.55, 0.55], 0.60), // light regolith
    ([0.75, 0.75, 0.75], 0.85), // highland
    ([0.90, 0.90, 0.90], 1.00), // peak
];

/// Resolve a biome name (from the AutoTexture `biome` param) to its
/// gradient table. Falls back to temperate for unknown values.
fn biome_gradient(name: &str) -> BiomeGradient {
    match name {
        "grassland" => BIOME_GRASSLAND,
        "mountainous" => BIOME_MOUNTAINOUS,
        "tropical" => BIOME_TROPICAL,
        "desert" => BIOME_DESERT,
        "tundra" => BIOME_TUNDRA,
        "lunar" => BIOME_LUNAR,
        _ => BIOME_TEMPERATE,
    }
}

fn detail_hash(ix: i32, iy: i32) -> f32 {
    let h = ix
        .wrapping_mul(374761393i32)
        .wrapping_add(iy.wrapping_mul(668265263i32));
    let h = (h ^ (h >> 13)).wrapping_mul(1274126177i32);
    let h = h ^ (h >> 16);
    (h as u32) as f32 / u32::MAX as f32
}

fn detail_value_noise(fx: f32, fy: f32) -> f32 {
    let ix = fx.floor() as i32;
    let iy = fy.floor() as i32;
    let tx = fx - ix as f32;
    let ty = fy - iy as f32;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let a = detail_hash(ix, iy) + sx * (detail_hash(ix + 1, iy) - detail_hash(ix, iy));
    let b = detail_hash(ix, iy + 1) + sx * (detail_hash(ix + 1, iy + 1) - detail_hash(ix, iy + 1));
    a + sy * (b - a)
}

/// 4-octave value-noise FBM over UV space. Returns [0, 1].
fn micro_fbm(ux: f32, uy: f32, base_freq: f32) -> f32 {
    let mut val = 0.0f32;
    let mut amp = 0.5f32;
    let mut freq = base_freq;
    let mut norm = 0.0f32;
    for _ in 0..4 {
        val += detail_value_noise(ux * freq, uy * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    val / norm
}

/// Slope-driven rock overlay. Alpha encodes rock coverage so this composites
/// only over steep terrain when layered on top of a base texture (e.g. AutoTexture).
/// detail_strength breaks up the flat color with FBM micro-variation.
fn generate_rock_soil(
    heightmap: &Heightmap,
    slope_input: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    let rock_hex = get_string(params, "rock_color", "807870");
    let soil_hex = get_string(params, "soil_color", "8B6914");
    let rock_rgb = parse_hex_color_srgb(rock_hex).unwrap_or([0.50, 0.47, 0.44]);
    let soil_rgb = parse_hex_color_srgb(soil_hex).unwrap_or([0.55, 0.41, 0.08]);
    let threshold = get_float(params, "slope_threshold", 0.4).clamp(0.0, 1.0);
    let blend = get_float(params, "slope_blend", 0.3).max(0.001);
    let ao_strength = get_float(params, "ao_strength", 0.8).clamp(0.0, 1.0);
    let detail_strength = get_float(params, "detail_strength", 0.25).clamp(0.0, 1.0);

    let computed_slope = slope_input.is_none().then(|| compute_slope_map(heightmap));
    let slope_map = slope_input.unwrap_or_else(|| computed_slope.as_ref().unwrap());

    for y in 0..h {
        for x in 0..w {
            let s = slope_map.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            let t = ((s - threshold) / blend).clamp(0.0, 1.0);
            let rock_w = t * t * (3.0 - 2.0 * t);
            let ao = {
                let raw = compute_local_ao(heightmap, x, y);
                1.0 - ao_strength * (1.0 - raw)
            };
            let ux = x as f32 / w as f32;
            let uy = y as f32 / h as f32;
            let noise = micro_fbm(ux, uy, 8.0);
            let detail = 1.0 + detail_strength * (noise * 2.0 - 1.0);
            let base_r = soil_rgb[0] + rock_w * (rock_rgb[0] - soil_rgb[0]);
            let base_g = soil_rgb[1] + rock_w * (rock_rgb[1] - soil_rgb[1]);
            let base_b = soil_rgb[2] + rock_w * (rock_rgb[2] - soil_rgb[2]);
            color.set(
                x,
                y,
                [
                    (base_r * ao * detail).clamp(0.0, 1.0),
                    (base_g * ao * detail).clamp(0.0, 1.0),
                    (base_b * ao * detail).clamp(0.0, 1.0),
                    rock_w, // alpha = rock coverage; transparent on flat terrain
                ],
            );
        }
    }
    color
}

/// Altitude+slope vegetation overlay. Alpha encodes coverage so this composites
/// only over gentle low terrain when layered on top of a base texture.
/// detail_strength breaks up the flat green with FBM micro-variation.
fn generate_vegetation(
    heightmap: &Heightmap,
    slope_input: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    let veg_hex = get_string(params, "vegetation_color", "4A7020");
    let dry_hex = get_string(params, "dry_color", "8B7355");
    let veg_rgb = parse_hex_color_srgb(veg_hex).unwrap_or([0.29, 0.44, 0.13]);
    let dry_rgb = parse_hex_color_srgb(dry_hex).unwrap_or([0.55, 0.45, 0.33]);
    let altitude_max = get_float(params, "altitude_max", 0.6).clamp(0.0, 1.0);
    let slope_cutoff = get_float(params, "slope_cutoff", 0.5).clamp(0.0, 1.0);
    let slope_blend = get_float(params, "slope_blend", 0.2).max(0.001);
    let ao_strength = get_float(params, "ao_strength", 0.6).clamp(0.0, 1.0);
    let detail_strength = get_float(params, "detail_strength", 0.2).clamp(0.0, 1.0);

    let computed_slope = slope_input.is_none().then(|| compute_slope_map(heightmap));
    let slope_map = slope_input.unwrap_or_else(|| computed_slope.as_ref().unwrap());

    const ALT_BLEND: f32 = 0.1;
    for y in 0..h {
        for x in 0..w {
            let elev = heightmap.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            let s = slope_map.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);

            let alt_t = ((elev - altitude_max) / ALT_BLEND).clamp(0.0, 1.0);
            let alt_factor = 1.0 - alt_t * alt_t * (3.0 - 2.0 * alt_t);

            let slp_t = ((s - slope_cutoff) / slope_blend).clamp(0.0, 1.0);
            let slope_factor = 1.0 - slp_t * slp_t * (3.0 - 2.0 * slp_t);

            let veg_weight = alt_factor * slope_factor;
            let ao = {
                let raw = compute_local_ao(heightmap, x, y);
                1.0 - ao_strength * (1.0 - raw)
            };
            let ux = x as f32 / w as f32;
            let uy = y as f32 / h as f32;
            // Slightly higher frequency than rock detail for finer vegetation texture.
            let noise = micro_fbm(ux, uy, 12.0);
            let detail = 1.0 + detail_strength * (noise * 2.0 - 1.0);
            let base_r = dry_rgb[0] + veg_weight * (veg_rgb[0] - dry_rgb[0]);
            let base_g = dry_rgb[1] + veg_weight * (veg_rgb[1] - dry_rgb[1]);
            let base_b = dry_rgb[2] + veg_weight * (veg_rgb[2] - dry_rgb[2]);
            color.set(
                x,
                y,
                [
                    (base_r * ao * detail).clamp(0.0, 1.0),
                    (base_g * ao * detail).clamp(0.0, 1.0),
                    (base_b * ao * detail).clamp(0.0, 1.0),
                    veg_weight,
                ],
            );
        }
    }
    color
}

/// Porter-Duff compositor for Color layers. Blends overlay over base using
/// `distribution` heightmap as per-pixel weight (falls back to overlay alpha).
fn generate_texture_overlay(
    base: &ColorBuffer,
    overlay: &ColorBuffer,
    distribution: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = base.width();
    let h = base.height();
    let mut out = ColorBuffer::new(w, h).unwrap();

    let blend_mode = get_string(params, "blend_mode", "over");
    let opacity = get_float(params, "opacity", 1.0).clamp(0.0, 1.0);

    for y in 0..h {
        for x in 0..w {
            let b = base.get(x, y).unwrap_or([0.0; 4]);
            // Sample overlay at the same UV; if sizes differ, nearest neighbour
            let ov_x = ((x as f32 / w as f32) * overlay.width() as f32) as u32;
            let ov_y = ((y as f32 / h as f32) * overlay.height() as f32) as u32;
            let ov = overlay
                .get(
                    ov_x.min(overlay.width() - 1),
                    ov_y.min(overlay.height() - 1),
                )
                .unwrap_or([0.0; 4]);

            let dist = if let Some(dm) = distribution {
                let dm_x = ((x as f32 / w as f32) * dm.width() as f32) as u32;
                let dm_y = ((y as f32 / h as f32) * dm.height() as f32) as u32;
                dm.get(dm_x.min(dm.width() - 1), dm_y.min(dm.height() - 1))
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0)
            } else {
                ov[3].clamp(0.0, 1.0)
            };
            let alpha = (dist * opacity).clamp(0.0, 1.0);

            let (or, og, ob) = (ov[0], ov[1], ov[2]);
            let (br, bg, bb) = (b[0], b[1], b[2]);
            let (r, g, ob_) = match blend_mode {
                "multiply" => (
                    (br * or * alpha + br * (1.0 - alpha)).clamp(0.0, 1.0),
                    (bg * og * alpha + bg * (1.0 - alpha)).clamp(0.0, 1.0),
                    (bb * ob * alpha + bb * (1.0 - alpha)).clamp(0.0, 1.0),
                ),
                "screen" => {
                    let sr = 1.0 - (1.0 - br) * (1.0 - or);
                    let sg = 1.0 - (1.0 - bg) * (1.0 - og);
                    let sb = 1.0 - (1.0 - bb) * (1.0 - ob);
                    (
                        (sr * alpha + br * (1.0 - alpha)).clamp(0.0, 1.0),
                        (sg * alpha + bg * (1.0 - alpha)).clamp(0.0, 1.0),
                        (sb * alpha + bb * (1.0 - alpha)).clamp(0.0, 1.0),
                    )
                }
                "add" => (
                    (br + or * alpha).clamp(0.0, 1.0),
                    (bg + og * alpha).clamp(0.0, 1.0),
                    (bb + ob * alpha).clamp(0.0, 1.0),
                ),
                // "over" and default
                _ => (
                    (or * alpha + br * (1.0 - alpha)).clamp(0.0, 1.0),
                    (og * alpha + bg * (1.0 - alpha)).clamp(0.0, 1.0),
                    (ob * alpha + bb * (1.0 - alpha)).clamp(0.0, 1.0),
                ),
            };
            let out_a = b[3].max(alpha);
            out.set(x, y, [r, g, ob_, out_a]);
        }
    }
    out
}

/// Generate a diffuse texture from a heightmap using elevation-banded
/// gradient mapping + slope-driven rock blending. Drives `AutoTexture`.
fn generate_auto_texture(
    heightmap: &Heightmap,
    slope: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    let slope_power = get_float(params, "slope_power", 0.7).max(0.01);
    let slope_blend_scale = get_float(params, "slope_blend", 1.0).clamp(0.0, 1.0);
    let ao_strength = get_float(params, "ao_strength", 1.0).clamp(0.0, 1.0);
    let rock_hex = get_string(params, "rock_color", "736B61");
    let rock_rgb = parse_hex_color_srgb(rock_hex).unwrap_or([0.45, 0.42, 0.38]);
    let biome = get_string(params, "biome", "temperate");
    let gradient = biome_gradient(biome);

    for y in 0..h {
        for x in 0..w {
            let height_val = heightmap.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            let slope_val = slope
                .and_then(|s| s.get(x, y))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);

            let base_color = sample_gradient(gradient, height_val);

            let slope_blend = slope_val.powf(slope_power) * slope_blend_scale;
            let r = base_color[0] * (1.0 - slope_blend) + rock_rgb[0] * slope_blend;
            let g = base_color[1] * (1.0 - slope_blend) + rock_rgb[1] * slope_blend;
            let b = base_color[2] * (1.0 - slope_blend) + rock_rgb[2] * slope_blend;

            // Lerp AO toward 1.0 by (1 - ao_strength) so the param
            // smoothly fades the darkening rather than gating it.
            let ao_raw = compute_local_ao(heightmap, x, y);
            let ao = 1.0 - (1.0 - ao_raw) * ao_strength;

            color.set(x, y, [r * ao, g * ao, b * ao, 1.0]);
        }
    }

    color
}

/// Parse a 6-digit `RRGGBB` hex string into an `[r, g, b]` array of
/// `f32` values in [0.0, 1.0]. Returns `None` if the string isn't six
/// valid hex digits.
fn parse_hex_color_srgb(s: &str) -> Option<[f32; 3]> {
    let bytes = s.as_bytes();
    if bytes.len() != 6 {
        return None;
    }
    let mut out = [0f32; 3];
    for i in 0..3 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = ((hi << 4 | lo) as f32) / 255.0;
    }
    Some(out)
}

fn sample_gradient(stops: &[([f32; 3], f32)], t: f32) -> [f32; 3] {
    if t <= stops[0].1 {
        return stops[0].0;
    }
    for i in 1..stops.len() {
        if t <= stops[i].1 {
            let frac = (t - stops[i - 1].1) / (stops[i].1 - stops[i - 1].1);
            let a = stops[i - 1].0;
            let b = stops[i].0;
            return [
                a[0] + (b[0] - a[0]) * frac,
                a[1] + (b[1] - a[1]) * frac,
                a[2] + (b[2] - a[2]) * frac,
            ];
        }
    }
    stops.last().unwrap().0
}

/// Simple ambient occlusion: compare center height to neighbors.
fn compute_local_ao(heightmap: &Heightmap, x: u32, y: u32) -> f32 {
    let c = heightmap.get(x, y).unwrap_or(0.5);
    let w = heightmap.width();
    let h = heightmap.height();
    let mut sum = 0.0f32;
    let mut count = 0.0f32;

    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
            let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
            let nh = heightmap.get(nx, ny).unwrap_or(c);
            // If neighbor is higher, this point is "occluded"
            sum += (c - nh).max(0.0);
            count += 1.0;
        }
    }

    // Map occlusion to brightness [0.7, 1.0]
    let occlusion = (sum / count * 5.0).clamp(0.0, 1.0);
    1.0 - occlusion * 0.3
}

/// Generate a tangent-space normal map from a heightmap.
/// `strength` controls the intensity of the normals (higher = more pronounced bumps).
fn generate_normal_map(heightmap: &Heightmap, strength: f32) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    // Scale factor accounts for the pixel spacing vs height range
    let scale = strength * 2.0;

    for y in 0..h {
        for x in 0..w {
            // Sample neighboring heights using Sobel-like kernel
            let x0 = if x > 0 { x - 1 } else { 0 };
            let x1 = if x < w - 1 { x + 1 } else { w - 1 };
            let y0 = if y > 0 { y - 1 } else { 0 };
            let y1 = if y < h - 1 { y + 1 } else { h - 1 };

            let tl = heightmap.get(x0, y0).unwrap_or(0.0);
            let t = heightmap.get(x, y0).unwrap_or(0.0);
            let tr = heightmap.get(x1, y0).unwrap_or(0.0);
            let l = heightmap.get(x0, y).unwrap_or(0.0);
            let r = heightmap.get(x1, y).unwrap_or(0.0);
            let bl = heightmap.get(x0, y1).unwrap_or(0.0);
            let b = heightmap.get(x, y1).unwrap_or(0.0);
            let br = heightmap.get(x1, y1).unwrap_or(0.0);

            // Sobel filter for dx and dy
            let dx = (tr + 2.0 * r + br) - (tl + 2.0 * l + bl);
            let dy = (bl + 2.0 * b + br) - (tl + 2.0 * t + tr);

            // Construct normal vector (tangent space: Z is up)
            let nx = -dx * scale;
            let ny = -dy * scale;
            let nz = 1.0f32;

            // Normalize
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            let nx = nx / len;
            let ny = ny / len;
            let nz = nz / len;

            // Encode to [0,1] range for storage as RGB
            let r = nx * 0.5 + 0.5;
            let g = ny * 0.5 + 0.5;
            let b = nz * 0.5 + 0.5;

            color.set(x, y, [r, g, b, 1.0]);
        }
    }

    color
}

/// Generate a grass density map based on height and slope constraints.
/// Parameters:
/// - `min_height`: minimum height for grass (default 0.15)
/// - `max_height`: maximum height for grass (default 0.7)
/// - `max_slope`: maximum slope for grass growth (default 0.4)
/// - `density`: overall density multiplier (default 1.0)
fn generate_grass_map(
    heightmap: &Heightmap,
    slope: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> Heightmap {
    let w = heightmap.width();
    let h = heightmap.height();
    let size = (w as usize) * (h as usize);
    let mut data = vec![0.0f32; size];

    let min_height = get_float(params, "min_height", 0.15);
    let max_height = get_float(params, "max_height", 0.7);
    let max_slope = get_float(params, "max_slope", 0.4);
    let density = get_float(params, "density", 1.0);
    let falloff = get_float(params, "falloff", 0.05);

    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            let height_val = heightmap.get(x, y).unwrap_or(0.0);
            let slope_val = slope.and_then(|s| s.get(x, y)).unwrap_or(0.0);

            // Height band with smooth falloff
            let height_factor = smooth_band(height_val, min_height, max_height, falloff);

            // Slope attenuation: grass doesn't grow on steep slopes
            let slope_factor = if slope_val < max_slope {
                1.0
            } else {
                let over = (slope_val - max_slope) / falloff.max(0.01);
                (1.0 - over).max(0.0)
            };

            data[idx] = (height_factor * slope_factor * density).clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Generate a specular intensity map from height and slope.
/// Parameters:
/// - `rock_specular`: specularity for steep rocky areas (default 0.6)
/// - `flat_specular`: specularity for flat ground (default 0.2)
/// - `water_specular`: specularity for low areas (water/wet, default 0.9)
/// - `water_height`: height threshold below which ground is considered wet (default 0.2)
fn generate_specular_map(
    heightmap: &Heightmap,
    slope: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> Heightmap {
    let w = heightmap.width();
    let h = heightmap.height();
    let size = (w as usize) * (h as usize);
    let mut data = vec![0.0f32; size];

    let rock_specular = get_float(params, "rock_specular", 0.6);
    let flat_specular = get_float(params, "flat_specular", 0.2);
    let water_specular = get_float(params, "water_specular", 0.9);
    let water_height = get_float(params, "water_height", 0.2);
    let snow_specular = get_float(params, "snow_specular", 0.7);
    let snow_height = get_float(params, "snow_height", 0.85);

    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            let height_val = heightmap.get(x, y).unwrap_or(0.0);
            let slope_val = slope.and_then(|s| s.get(x, y)).unwrap_or(0.0);

            // Base specular from slope: steep = shiny rock, flat = dull ground
            let base = flat_specular + (rock_specular - flat_specular) * slope_val;

            // Override for water/wet areas
            let spec = if height_val < water_height {
                let wet_factor = 1.0 - (height_val / water_height);
                base + (water_specular - base) * wet_factor
            } else if height_val > snow_height {
                let snow_factor = (height_val - snow_height) / (1.0 - snow_height);
                base + (snow_specular - base) * snow_factor
            } else {
                base
            };

            data[idx] = spec.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Smooth band function: returns 1.0 inside [low,high], smoothly falls to 0 outside.
fn smooth_band(value: f32, low: f32, high: f32, falloff: f32) -> f32 {
    if value < low - falloff || value > high + falloff {
        return 0.0;
    }
    if value >= low && value <= high {
        return 1.0;
    }
    if value < low {
        (value - (low - falloff)) / falloff
    } else {
        ((high + falloff) - value) / falloff
    }
}

// ============================================================
// New node implementations
// ============================================================

fn get_string<'a>(params: &'a HashMap<String, ParamValue>, key: &str, default: &'a str) -> &'a str {
    match params.get(key) {
        Some(ParamValue::String(s)) => s.as_str(),
        _ => default,
    }
}

fn get_bool(params: &HashMap<String, ParamValue>, key: &str, default: bool) -> bool {
    match params.get(key) {
        Some(ParamValue::Bool(v)) => *v,
        _ => default,
    }
}

/// Load an image file as a heightmap. Supports PNG/TIFF 8/16-bit grayscale + RGB.
fn load_file_input(path: &str, width: u32, height: u32) -> Result<Heightmap, EvalError> {
    if path.is_empty() {
        // No file specified — return flat heightmap at 0.5
        let data = vec![0.5f32; (width as usize) * (height as usize)];
        return Heightmap::frbar_data(width, height, data)
            .map_err(|e| EvalError::Compute(e.to_string()));
    }

    let img = image::open(path)
        .map_err(|e| EvalError::Compute(format!("Failed to load image '{}': {}", path, e)))?;

    let gray = img.to_luma16();
    let (iw, ih) = gray.dimensions();

    // If dimensions match, use directly; otherwise resample
    let data: Vec<f32> = if iw == width && ih == height {
        gray.pixels().map(|p| p.0[0] as f32 / 65535.0).collect()
    } else {
        // Bilinear resample to target dimensions
        let mut resampled = Vec::with_capacity((width as usize) * (height as usize));
        for y in 0..height {
            for x in 0..width {
                let sx = x as f32 * (iw as f32 - 1.0) / (width as f32 - 1.0).max(1.0);
                let sy = y as f32 * (ih as f32 - 1.0) / (height as f32 - 1.0).max(1.0);
                let x0 = (sx as u32).min(iw - 1);
                let y0 = (sy as u32).min(ih - 1);
                let x1 = (x0 + 1).min(iw - 1);
                let y1 = (y0 + 1).min(ih - 1);
                let fx = sx - sx.floor();
                let fy = sy - sy.floor();
                let v00 = gray.get_pixel(x0, y0).0[0] as f32;
                let v10 = gray.get_pixel(x1, y0).0[0] as f32;
                let v01 = gray.get_pixel(x0, y1).0[0] as f32;
                let v11 = gray.get_pixel(x1, y1).0[0] as f32;
                let v = v00 * (1.0 - fx) * (1.0 - fy)
                    + v10 * fx * (1.0 - fy)
                    + v01 * (1.0 - fx) * fy
                    + v11 * fx * fy;
                resampled.push(v / 65535.0);
            }
        }
        resampled
    };

    Heightmap::frbar_data(width, height, data).map_err(|e| EvalError::Compute(e.to_string()))
}

/// Generate Voronoi (plateau/cell) terrain.
/// Params: frequency, seed, mode ("f1", "f2", "f2_f1", "cell")
fn generate_voronoi(params: &HashMap<String, ParamValue>, width: u32, height: u32) -> Heightmap {
    let frequency = get_float(params, "frequency", 8.0);
    let seed = get_uint(params, "seed", 0);
    let mode = get_string(params, "mode", "f1");

    // Simple Voronoi via random cell points
    let num_cells = (frequency * frequency) as usize;
    let mut rng_state: u64 = seed as u64 ^ 0xDEAD_BEEF;

    let mut cell_points: Vec<(f32, f32, f32)> = Vec::with_capacity(num_cells);
    for _ in 0..num_cells {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cx = ((rng_state >> 32) as f32) / (u32::MAX as f32);
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cy = ((rng_state >> 32) as f32) / (u32::MAX as f32);
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cval = ((rng_state >> 32) as f32) / (u32::MAX as f32);
        cell_points.push((cx, cy, cval));
    }

    let mut data = vec![0.0f32; (width as usize) * (height as usize)];
    for y in 0..height {
        for x in 0..width {
            let px = x as f32 / width as f32;
            let py = y as f32 / height as f32;

            let mut d1 = f32::MAX;
            let mut d2 = f32::MAX;
            let mut closest_val = 0.0f32;

            for &(cx, cy, cval) in &cell_points {
                let dx = px - cx;
                let dy = py - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < d1 {
                    d2 = d1;
                    d1 = dist;
                    closest_val = cval;
                } else if dist < d2 {
                    d2 = dist;
                }
            }

            let v = match mode {
                "f2" => (d2 * frequency).min(1.0),
                "f2_f1" => ((d2 - d1) * frequency).min(1.0),
                "cell" => closest_val,
                _ => (d1 * frequency).min(1.0), // "f1"
            };

            data[(y as usize) * (width as usize) + (x as usize)] = v.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}

/// Generate a gradient (ramp).
/// Params: direction ("linear_x", "linear_y", "radial", "angular"), invert
fn generate_gradient(params: &HashMap<String, ParamValue>, width: u32, height: u32) -> Heightmap {
    let direction = get_string(params, "direction", "linear_y");
    let invert = get_bool(params, "invert", false);
    let center_x = get_float(params, "center_x", 0.5);
    let center_y = get_float(params, "center_y", 0.5);

    let mut data = vec![0.0f32; (width as usize) * (height as usize)];

    for y in 0..height {
        for x in 0..width {
            let nx = x as f32 / (width as f32 - 1.0).max(1.0);
            let ny = y as f32 / (height as f32 - 1.0).max(1.0);

            // Piecewise-linear remap that honours the center param
            // for linear modes: `center` is where the ramp's midpoint
            // (v=0.5) sits. center=0.5 reproduces the simple v=axis
            // gradient; smaller values push the ramp toward the start
            // of the axis, larger values toward the end.
            let remap = |t: f32, center: f32| -> f32 {
                let c = center.clamp(0.001, 0.999);
                if t <= c {
                    0.5 * t / c
                } else {
                    0.5 + 0.5 * (t - c) / (1.0 - c)
                }
            };
            let v = match direction {
                "linear_x" => remap(nx, center_x),
                "radial" => {
                    let dx = nx - center_x;
                    let dy = ny - center_y;
                    let dist = (dx * dx + dy * dy).sqrt() * std::f32::consts::SQRT_2;
                    1.0 - dist.min(1.0)
                }
                "angular" => {
                    let dx = nx - center_x;
                    let dy = ny - center_y;
                    (dy.atan2(dx) / std::f32::consts::TAU + 0.5).fract()
                }
                _ => remap(ny, center_y), // "linear_y" (default)
            };

            let v = if invert { 1.0 - v } else { v };
            data[(y as usize) * (width as usize) + (x as usize)] = v.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}

/// Apply curve/remap: piecewise-linear transfer function defined by control points.
/// Params: points (encoded as pairs in "p0_x", "p0_y", "p1_x", "p1_y", ... or "num_points")
/// Default: S-curve (smoothstep)
fn apply_curve(input: &Heightmap, params: &HashMap<String, ParamValue>) -> Heightmap {
    let w = input.width();
    let h = input.height();

    // Build control points from params (default: smoothstep-like S-curve)
    let num_points = get_uint(params, "num_points", 0) as usize;
    let points: Vec<(f32, f32)> = if num_points >= 2 {
        (0..num_points)
            .map(|i| {
                let px = get_float(
                    params,
                    &format!("p{}_x", i),
                    i as f32 / (num_points - 1) as f32,
                );
                let py = get_float(params, &format!("p{}_y", i), px);
                (px, py)
            })
            .collect()
    } else {
        // Default: smoothstep S-curve
        vec![(0.0, 0.0), (0.25, 0.1), (0.5, 0.5), (0.75, 0.9), (1.0, 1.0)]
    };

    let mut data = vec![0.0f32; (w as usize) * (h as usize)];
    for y in 0..h {
        for x in 0..w {
            let v = input.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            data[(y as usize) * (w as usize) + (x as usize)] = eval_piecewise_linear(&points, v);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Evaluate a piecewise-linear curve at a given x value.
fn eval_piecewise_linear(points: &[(f32, f32)], x: f32) -> f32 {
    if points.is_empty() {
        return x;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[points.len() - 1].0 {
        return points[points.len() - 1].1;
    }
    for i in 1..points.len() {
        if x <= points[i].0 {
            let (x0, y0) = points[i - 1];
            let (x1, y1) = points[i];
            let t = if (x1 - x0).abs() < 1e-8 {
                0.0
            } else {
                (x - x0) / (x1 - x0)
            };
            return y0 + t * (y1 - y0);
        }
    }
    points[points.len() - 1].1
}

/// Simple linear transform: output = input * scale + offset, optionally inverted.
fn apply_simple_transform(input: &Heightmap, scale: f32, offset: f32, invert: bool) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let mut v = input.get(x, y).unwrap_or(0.0);
            if invert {
                v = 1.0 - v;
            }
            v = v * scale + offset;
            data[(y as usize) * (w as usize) + (x as usize)] = v.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Normalize: remap all values to fill the 0..1 range.
fn apply_normalize(input: &Heightmap) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data_in = input.data();

    let mut min_val = f32::MAX;
    let mut max_val = f32::MIN;
    for &v in data_in {
        if v < min_val {
            min_val = v;
        }
        if v > max_val {
            max_val = v;
        }
    }

    let range = max_val - min_val;
    let data: Vec<f32> = if range.abs() < 1e-8 {
        vec![0.5; data_in.len()]
    } else {
        data_in.iter().map(|&v| (v - min_val) / range).collect()
    };

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Christophe Schlick's bias and gain functions.
/// bias(t, b) = t^(log(b) / log(0.5))
/// gain(t, g) = bias(2t, 1-g)/2 for t < 0.5, else 1 - bias(2-2t, 1-g)/2
fn apply_bias_gain(input: &Heightmap, bias: f32, gain: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    let bias_exp = if bias.abs() < 1e-6 {
        0.0
    } else {
        (bias.clamp(0.001, 0.999)).ln() / (0.5f32).ln()
    };

    for y in 0..h {
        for x in 0..w {
            let t = input.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);

            // Apply bias
            let biased = t.powf(bias_exp);

            // Apply gain
            let gained = if biased < 0.5 {
                let bt = (2.0 * biased).powf((1.0 - gain).clamp(0.001, 0.999).ln() / (0.5f32).ln());
                bt / 2.0
            } else {
                let bt = (2.0 - 2.0 * biased)
                    .powf((1.0 - gain).clamp(0.001, 0.999).ln() / (0.5f32).ln());
                1.0 - bt / 2.0
            };

            data[(y as usize) * (w as usize) + (x as usize)] = gained.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Displace/warp terrain using another heightmap as the displacement field.
/// Displaces in X direction proportional to displacement map gradient.
fn apply_displacement(input: &Heightmap, displacement: &Heightmap, strength: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let dw = displacement.width();
    let dh = displacement.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    // Strength is in pixels
    let pixel_strength = strength * w as f32;

    for y in 0..h {
        for x in 0..w {
            // Sample displacement map (rescale if dimensions differ)
            let dx_coord = (x as f32 * dw as f32 / w as f32) as u32;
            let dy_coord = (y as f32 * dh as f32 / h as f32) as u32;
            let dx_coord = dx_coord.min(dw - 1);
            let dy_coord = dy_coord.min(dh - 1);

            // Compute displacement gradient (central differences)
            let dx_left = if dx_coord > 0 { dx_coord - 1 } else { 0 };
            let dx_right = (dx_coord + 1).min(dw - 1);
            let dy_top = if dy_coord > 0 { dy_coord - 1 } else { 0 };
            let dy_bot = (dy_coord + 1).min(dh - 1);

            let grad_x = displacement.get(dx_right, dy_coord).unwrap_or(0.0)
                - displacement.get(dx_left, dy_coord).unwrap_or(0.0);
            let grad_y = displacement.get(dx_coord, dy_bot).unwrap_or(0.0)
                - displacement.get(dx_coord, dy_top).unwrap_or(0.0);

            // Displaced source coordinates
            let sx = (x as f32 + grad_x * pixel_strength).clamp(0.0, (w - 1) as f32);
            let sy = (y as f32 + grad_y * pixel_strength).clamp(0.0, (h - 1) as f32);

            // Bilinear interpolation from source
            let x0 = sx as u32;
            let y0 = sy as u32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();

            let v00 = input.get(x0, y0).unwrap_or(0.0);
            let v10 = input.get(x1, y0).unwrap_or(0.0);
            let v01 = input.get(x0, y1).unwrap_or(0.0);
            let v11 = input.get(x1, y1).unwrap_or(0.0);
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;

            data[(y as usize) * (w as usize) + (x as usize)] = v;
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Chooser: select between A and B based on a mask (0=A, 1=B, interpolated in between).
fn apply_chooser(a: &Heightmap, b: &Heightmap, mask: &Heightmap) -> Heightmap {
    let w = a.width().min(b.width()).min(mask.width());
    let h = a.height().min(b.height()).min(mask.height());
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let va = a.get(x, y).unwrap_or(0.0);
            let vb = b.get(x, y).unwrap_or(0.0);
            let m = mask.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            data[(y as usize) * (w as usize) + (x as usize)] = va * (1.0 - m) + vb * m;
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Decode a hex-encoded painted mask string into pixel bytes.
/// Non-hex characters are skipped. Returns empty Vec on empty/invalid input.
fn hex_decode_mask(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        if let (Some(h), Some(l)) = (hi, lo) {
            out.push((h << 4 | l) as u8);
        }
        i += 2;
    }
    out
}

/// Source resolution for the `PaintedTexture` node's brush canvas.
/// Fixed for now; could be made a param like PaintedHeightmap.
pub const PAINTED_TEXTURE_RES: u32 = 256;

/// Apply a sculpt delta buffer onto a heightmap in place.
/// `pixels` is a flat u8 array at `src_res × src_res`: 128 = no change,
/// 0 = max subtract, 255 = max add. `scale` controls the maximum magnitude
/// of the applied delta (e.g. 0.5 = max ±50% shift). If `pixels` is empty
/// or wrong length the heightmap is left unchanged.
fn apply_sculpt_delta(hm: &mut Heightmap, pixels: &[u8], src_res: u32, scale: f32) {
    let out_w = hm.width();
    let out_h = hm.height();
    let src_w = src_res;
    let src_h = src_res;
    if pixels.len() != (src_w as usize) * (src_h as usize) {
        return;
    }
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let x0 = sx as u32;
            let y0 = sy as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();
            let v00 = pixels[(y0 as usize) * (src_w as usize) + x0 as usize] as f32;
            let v10 = pixels[(y0 as usize) * (src_w as usize) + x1 as usize] as f32;
            let v01 = pixels[(y1 as usize) * (src_w as usize) + x0 as usize] as f32;
            let v11 = pixels[(y1 as usize) * (src_w as usize) + x1 as usize] as f32;
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;
            // Map [0,255] → [-1,1], multiply by scale, add to input
            let delta = (v - 128.0) / 128.0 * scale;
            let cur = hm.get(ox, oy).unwrap_or(0.0);
            let _ = hm.set(ox, oy, (cur + delta).clamp(0.0, 1.0));
        }
    }
}

/// Bilinearly scale a painted greyscale image at `src_res × src_res`
/// up/down to the output dims and normalise `[0,255] → [0.0, 1.0]`.
/// `src_res` comes from the node's `resolution` param.
fn painted_grayscale_to_heightmap(
    pixels: Vec<u8>,
    src_res: u32,
    out_w: u32,
    out_h: u32,
) -> Heightmap {
    let src_w = src_res;
    let src_h = src_res;

    // Fill with zeros if no painted data
    let pixels = if pixels.len() == (src_w as usize) * (src_h as usize) {
        pixels
    } else {
        vec![0u8; (src_w as usize) * (src_h as usize)]
    };

    let mut data = vec![0.0f32; (out_w as usize) * (out_h as usize)];
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let x0 = sx as u32;
            let y0 = sy as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();

            let v00 = pixels[(y0 as usize) * (src_w as usize) + x0 as usize] as f32 / 255.0;
            let v10 = pixels[(y0 as usize) * (src_w as usize) + x1 as usize] as f32 / 255.0;
            let v01 = pixels[(y1 as usize) * (src_w as usize) + x0 as usize] as f32 / 255.0;
            let v11 = pixels[(y1 as usize) * (src_w as usize) + x1 as usize] as f32 / 255.0;
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;
            data[(oy as usize) * (out_w as usize) + ox as usize] = v;
        }
    }
    Heightmap::frbar_data(out_w, out_h, data).unwrap()
}

/// Bilinearly scale a painted RGB image (3 bytes per pixel at
/// `src_res × src_res`) up/down to the output dims, returning a
/// `ColorBuffer` (RGBA with alpha = 1.0).
fn painted_rgb_to_color_buffer(
    pixels: Vec<u8>,
    src_res: u32,
    out_w: u32,
    out_h: u32,
) -> ColorBuffer {
    let src_w = src_res as usize;
    let src_h = src_res as usize;
    let expected = src_w * src_h * 3;

    // Fall back to opaque mid-grey if no painted data — same shape
    // as PaintedHeightmap's "no data → zeros" fallback.
    let pixels = if pixels.len() == expected {
        pixels
    } else {
        vec![128u8; expected]
    };

    let mut buf = ColorBuffer::new(out_w, out_h).unwrap();
    let sample =
        |x: usize, y: usize, c: usize| -> f32 { pixels[(y * src_w + x) * 3 + c] as f32 / 255.0 };
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let x0 = sx as usize;
            let y0 = sy as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();

            let mut rgb = [0.0_f32; 3];
            for (c, slot) in rgb.iter_mut().enumerate() {
                let v00 = sample(x0, y0, c);
                let v10 = sample(x1, y0, c);
                let v01 = sample(x0, y1, c);
                let v11 = sample(x1, y1, c);
                *slot = v00 * (1.0 - fx) * (1.0 - fy)
                    + v10 * fx * (1.0 - fy)
                    + v01 * (1.0 - fx) * fy
                    + v11 * fx * fy;
            }
            buf.set(ox, oy, [rgb[0], rgb[1], rgb[2], 1.0]);
        }
    }
    buf
}
#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{GraphEngine, Node, NodeId, PortId};

    #[test]
    fn preview_node_passes_inputs_through_as_outputs() {
        // Preview is a real node: its executor takes upstream
        // values via its declared input ports and re-emits them as
        // runtime outputs under the same names. That's what lets
        // the 3D viewport read its data via the standard
        // `outputs[node_id][port_name]` accessor — same shape any
        // consumer uses for any other node, no special-case
        // "global ingest" path.
        let executor = CpuExecutor;
        let mut hm = Heightmap::new(8, 8).unwrap();
        for y in 0..8 {
            for x in 0..8 {
                hm.set(x, y, ((x * 7 + y) as f32) / 100.0).unwrap();
            }
        }
        let inputs = HashMap::from([("heightmap".to_string(), PortValue::Heightmap(hm.clone()))]);
        let result = executor
            .execute(&NodeType::Preview, &HashMap::new(), &inputs, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result
            .get("heightmap")
            .expect("Preview should re-emit `heightmap` from its input")
        else {
            panic!("expected Heightmap port value");
        };
        for y in 0..8 {
            for x in 0..8 {
                assert!((out.get(x, y).unwrap() - hm.get(x, y).unwrap()).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn preview_node_emits_no_unrequested_ports() {
        // With no inputs supplied, the runtime output map is empty.
        // (Preview only re-emits what it received.)
        let executor = CpuExecutor;
        let result = executor
            .execute(&NodeType::Preview, &HashMap::new(), &HashMap::new(), 8, 8)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_passthrough_normalises_backslash_bundle_paths() {
        // Regression: the GUI's SD7 import path used to write file lists with
        // native (Windows) separators, which the bundler validator rejects.
        // The executor now normalises bundle paths to forward slashes.
        let executor = CpuExecutor;
        let params = HashMap::from([(
            "files".to_string(),
            ParamValue::String(
                "C:\\src\\unittextures\\rock.dds|unittextures\\rock.dds\n\
                 C:\\src\\maps\\info.lua|maps\\info.lua"
                    .to_string(),
            ),
        )]);
        let inputs = HashMap::new();
        let result = executor
            .execute(&NodeType::PassThrough, &params, &inputs, 1, 1)
            .unwrap();
        let PortValue::FileList(list) = result.get("files").unwrap() else {
            panic!("Expected FileList output");
        };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].bundle_path, "unittextures/rock.dds");
        assert_eq!(list[1].bundle_path, "maps/info.lua");
    }

    #[test]
    fn test_cpu_executor_noise() {
        let executor = CpuExecutor;
        let params = HashMap::from([
            ("frequency".to_string(), ParamValue::Float(4.0)),
            ("octaves".to_string(), ParamValue::UInt(4)),
            ("seed".to_string(), ParamValue::UInt(42)),
        ]);
        let inputs = HashMap::new();
        let result = executor
            .execute(&NodeType::PerlinNoise, &params, &inputs, 64, 64)
            .unwrap();

        let output = result.get("output").unwrap();
        match output {
            PortValue::Heightmap(hm) => {
                assert_eq!(hm.width(), 64);
                assert_eq!(hm.height(), 64);
                assert!(hm.data().iter().any(|&v| v > 0.1));
            }
            _ => panic!("Expected heightmap output"),
        }
    }

    #[test]
    fn test_end_to_end_graph_evaluation() {
        let executor = CpuExecutor;
        let mut graph = GraphEngine::new();

        let noise = Node::new(NodeId(0), NodeType::PerlinNoise, "Noise");
        let bundler = Node::new(NodeId(0), NodeType::Bundler, "Bundler");
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

        let results = bar_graph::evaluate_graph(&graph, &executor, 64, 64).unwrap();
        let hm = bar_graph::get_heightmap_output(&graph, &results).unwrap();

        assert_eq!(hm.width(), 64);
        assert_eq!(hm.height(), 64);
        let mean: f32 = hm.data().iter().sum::<f32>() / hm.data().len() as f32;
        assert!(
            mean > 0.1 && mean < 0.9,
            "Expected varied noise, got mean={mean}"
        );
    }

    #[test]
    fn test_blend_combiner() {
        let executor = CpuExecutor;
        let a = Heightmap::frbar_data(4, 4, vec![0.0; 16]).unwrap();
        let b = Heightmap::frbar_data(4, 4, vec![1.0; 16]).unwrap();

        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
        ]);
        let params = HashMap::from([("factor".to_string(), ParamValue::Float(0.5))]);

        let result = executor
            .execute(&NodeType::Blend, &params, &inputs, 4, 4)
            .unwrap();
        let output = result.get("output").unwrap();
        match output {
            PortValue::Heightmap(hm) => {
                assert!((hm.get(0, 0).unwrap() - 0.5).abs() < 0.01);
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn test_voronoi_generator() {
        let executor = CpuExecutor;
        let params = HashMap::from([
            ("frequency".to_string(), ParamValue::Float(4.0)),
            ("seed".to_string(), ParamValue::UInt(42)),
            ("mode".to_string(), ParamValue::String("f1".to_string())),
        ]);
        let result = executor
            .execute(&NodeType::Voronoi, &params, &HashMap::new(), 64, 64)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                assert_eq!(hm.width(), 64);
                assert_eq!(hm.height(), 64);
                // Should have variation
                let min = hm.data().iter().cloned().fold(f32::INFINITY, f32::min);
                let max = hm.data().iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                assert!(max - min > 0.1, "Voronoi should have variation");
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn test_gradient_generator() {
        let executor = CpuExecutor;
        let params = HashMap::from([(
            "direction".to_string(),
            ParamValue::String("vertical".to_string()),
        )]);
        let result = executor
            .execute(&NodeType::Gradient, &params, &HashMap::new(), 8, 8)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                // Vertical gradient: top row ~0, bottom row ~1
                assert!(hm.get(0, 0).unwrap() < 0.01);
                assert!(hm.get(0, 7).unwrap() > 0.99);
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn test_normalize_filter() {
        let executor = CpuExecutor;
        // Input with values in [0.3, 0.7] — normalize should stretch to [0, 1]
        let data: Vec<f32> = (0..64).map(|i| 0.3 + 0.4 * (i as f32 / 63.0)).collect();
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);

        let result = executor
            .execute(&NodeType::Normalize, &HashMap::new(), &inputs, 8, 8)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                let min = hm.data().iter().cloned().fold(f32::INFINITY, f32::min);
                let max = hm.data().iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                assert!(min.abs() < 0.001, "Min should be ~0, got {min}");
                assert!((max - 1.0).abs() < 0.001, "Max should be ~1, got {max}");
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn test_simple_transform() {
        let executor = CpuExecutor;
        let data = vec![0.5_f32; 16];
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        // scale=2.0, offset=0.1 → 0.5*2.0 + 0.1 = 1.1 → clamped to 1.0
        let params = HashMap::from([
            ("scale".to_string(), ParamValue::Float(2.0)),
            ("offset".to_string(), ParamValue::Float(0.1)),
        ]);

        let result = executor
            .execute(&NodeType::SimpleTransform, &params, &inputs, 4, 4)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                assert!((hm.get(0, 0).unwrap() - 1.0).abs() < 0.001);
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn test_bias_gain() {
        let executor = CpuExecutor;
        // Uniform ramp
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        // Default bias=0.5, gain=0.5 should be roughly identity
        let params = HashMap::from([
            ("bias".to_string(), ParamValue::Float(0.5)),
            ("gain".to_string(), ParamValue::Float(0.5)),
        ]);

        let result = executor
            .execute(&NodeType::BiasGain, &params, &inputs, 4, 4)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                // With bias=gain=0.5, output ≈ input
                assert!((hm.get(0, 0).unwrap() - 0.0).abs() < 0.01);
                assert!((hm.get(3, 3).unwrap() - 1.0).abs() < 0.01);
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn test_chooser() {
        let executor = CpuExecutor;
        let a = Heightmap::frbar_data(4, 4, vec![0.2; 16]).unwrap();
        let b = Heightmap::frbar_data(4, 4, vec![0.8; 16]).unwrap();
        // Mask: top half 0.0 (choose a), bottom half 1.0 (choose b)
        let mut mask_data = vec![0.0_f32; 16];
        for v in mask_data[8..16].iter_mut() {
            *v = 1.0;
        }
        let mask = Heightmap::frbar_data(4, 4, mask_data).unwrap();

        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
            ("mask".to_string(), PortValue::Heightmap(mask)),
        ]);

        let result = executor
            .execute(&NodeType::Chooser, &HashMap::new(), &inputs, 4, 4)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                // Top half = a (0.2), bottom half = b (0.8)
                assert!((hm.get(0, 0).unwrap() - 0.2).abs() < 0.01);
                assert!((hm.get(0, 3).unwrap() - 0.8).abs() < 0.01);
            }
            _ => panic!("Expected heightmap"),
        }
    }

    // ── Modulation helpers ────────────────────────────────────────────────────

    fn const_hm(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w as usize) * (h as usize)]).unwrap()
    }

    #[test]
    fn scale_by_field_none_is_identity() {
        let effect = const_hm(4, 4, 0.7);
        let out = scale_by_field(effect.clone(), None);
        assert_eq!(out.data(), effect.data());
    }

    #[test]
    fn scale_by_field_clamps_and_multiplies_per_pixel() {
        // Field values outside [0, 1] are clamped before multiply, so the
        // result never exceeds the effect.
        let effect = const_hm(2, 2, 0.6);
        let mut field = const_hm(2, 2, 0.0);
        field.data_mut().copy_from_slice(&[0.0, 0.5, 1.0, 2.0]);
        let out = scale_by_field(effect, Some(&field));
        assert!((out.data()[0] - 0.0).abs() < 1e-6);
        assert!((out.data()[1] - 0.30).abs() < 1e-6);
        assert!((out.data()[2] - 0.60).abs() < 1e-6);
        assert!((out.data()[3] - 0.60).abs() < 1e-6); // 2.0 -> clamp(1.0)
    }

    #[test]
    fn apply_modulation_no_inputs_returns_effect_untouched() {
        let input = const_hm(2, 2, 0.0);
        let effect = const_hm(2, 2, 0.9);
        let out = apply_modulation(&input, effect.clone(), None, None);
        assert_eq!(out.data(), effect.data());
    }

    #[test]
    fn apply_modulation_mask_zero_falls_back_to_input() {
        // Where mask is 0, the output must equal `input`. Where mask is 1,
        // the output must equal `effect`. Halfway lerps to the midpoint.
        let input = const_hm(2, 2, 0.0);
        let effect = const_hm(2, 2, 1.0);
        let mut mask = const_hm(2, 2, 0.0);
        mask.data_mut().copy_from_slice(&[0.0, 0.5, 1.0, 1.0]);
        let out = apply_modulation(&input, effect, None, Some(&mask));
        assert!((out.data()[0] - 0.0).abs() < 1e-6);
        assert!((out.data()[1] - 0.5).abs() < 1e-6);
        assert!((out.data()[2] - 1.0).abs() < 1e-6);
        assert!((out.data()[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn apply_modulation_control_and_mask_multiply() {
        // Both fields collapse to a single weight = clamp(c)*clamp(m).
        let input = const_hm(1, 1, 0.0);
        let effect = const_hm(1, 1, 1.0);
        let ctrl = const_hm(1, 1, 0.5);
        let mask = const_hm(1, 1, 0.4);
        let out = apply_modulation(&input, effect, Some(&ctrl), Some(&mask));
        // 0 + (1 - 0) * (0.5 * 0.4) = 0.2
        assert!((out.data()[0] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn add_node_honours_mask() {
        // Add with mask=0 should leave `a` untouched everywhere.
        let executor = CpuExecutor;
        let a = const_hm(2, 2, 0.3);
        let b = const_hm(2, 2, 0.4);
        let mask = const_hm(2, 2, 0.0);
        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
            ("mask".to_string(), PortValue::Mask(mask)),
        ]);
        let result = executor
            .execute(&NodeType::Add, &HashMap::new(), &inputs, 2, 2)
            .unwrap();
        let PortValue::Heightmap(hm) = result.get("output").unwrap() else {
            panic!("expected heightmap")
        };
        for &v in hm.data() {
            assert!((v - 0.3).abs() < 1e-6, "mask=0 should keep `a`, got {v}");
        }
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
            .execute(&NodeType::Blend, &params, &inputs, 2, 2)
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
