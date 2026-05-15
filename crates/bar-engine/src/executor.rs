//! Bridges the graph engine to the compute layer.
//! Implements `NodeExecutor` to dispatch graph node operations to CPU compute.

use std::collections::HashMap;

use bar_compute::{
    generate_noise_cpu, hydraulic_erosion, thermal_erosion, HydraulicErosionParams, NoiseParams,
    NoiseType, ThermalErosionParams,
};
use bar_data::{smt::TILE_SIZE, ColorBuffer, Heightmap};
use bar_graph::{EvalError, NodeExecutor, NodeType, ParamValue, PortValue};
use std::f32::consts::PI;

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
        // Heightmap nodes use hm_width/hm_height throughout; re-bind so
        // all existing references below compile without change.
        let (width, height) = (hm_width, hm_height);
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
            NodeType::Mirror => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let mode = get_string(params, "mode", "mirror_x");
                let hm = apply_mirror(&input, mode);
                let hm = apply_modulation(&input, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Terrace => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let step_count = get_uint(params, "step_count", 4).clamp(1, 64);
                let smoothing = get_float(params, "smoothing", 0.0).clamp(0.0, 1.0);
                let hm = apply_terrace(&input, step_count, smoothing);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Sharpen => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let radius = get_float(params, "radius", 1.0).max(0.1);
                let strength = get_float(params, "strength", 1.0).clamp(0.0, 4.0);
                let hm = apply_sharpen(&input, radius, strength);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::Curve => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let hm = apply_curve(&input, params);
                let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
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
                let result = hydraulic_erosion(&input, &params_e)
                    .map_err(|e| EvalError::Compute(e.to_string()))?;
                let hm = apply_modulation(&input, result.heightmap, ctrl.as_ref(), mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
                outputs.insert("flow".to_string(), PortValue::Heightmap(result.flow));
                outputs.insert("wear".to_string(), PortValue::Heightmap(result.wear));
                outputs.insert("deposit".to_string(), PortValue::Heightmap(result.deposit));
            }
            NodeType::ThermalErosion => {
                let input = get_input_heightmap(inputs, "input")?;
                let ctrl = get_optional_heightmap(inputs, "control");
                let mask = get_optional_heightmap(inputs, "mask");
                let params_e = ThermalErosionParams {
                    iterations: get_uint(params, "iterations", 100),
                    talus_angle: get_float(params, "talus_angle", 0.6),
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
            NodeType::TerrainSplat => {
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
                let color = resize_color_to_tex(color, tex_width, tex_height);
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::RockSoil => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let mask = get_optional_heightmap(inputs, "mask");
                let color = generate_rock_soil(&input, slope.as_ref(), params);
                let color =
                    apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
                let color = resize_color_to_tex(color, tex_width, tex_height);
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::Vegetation => {
                let input = get_input_heightmap(inputs, "input")?;
                let slope = get_optional_heightmap(inputs, "slope");
                let mask = get_optional_heightmap(inputs, "mask");
                let color = generate_vegetation(&input, slope.as_ref(), params);
                let color =
                    apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
                let color = resize_color_to_tex(color, tex_width, tex_height);
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::LayerBlend => {
                let base = get_input_color(inputs, "base")?;
                let overlay = get_input_color(inputs, "overlay")?;
                let distribution = get_optional_heightmap(inputs, "distribution");
                let color =
                    generate_texture_overlay(&base, &overlay, distribution.as_ref(), params);
                outputs.insert("output".to_string(), PortValue::Color(color));
            }

            NodeType::TextureWeightmap => {
                let priority_type = get_string(params, "priority_type", "weighted_blend");
                let layer_count = get_uint(params, "layer_count", 2).clamp(2, 8) as usize;

                struct Layer {
                    tex: ColorBuffer,
                    priority: f32,
                    exclusion: f32,
                }
                let mut layers: Vec<Layer> = Vec::new();
                for i in 0..layer_count {
                    let Some(PortValue::Color(tex)) = inputs.get(&format!("texture_{i}")) else {
                        continue;
                    };
                    let priority = get_float(params, &format!("priority_{i}"), (7 - i) as f32);
                    let exclusion =
                        get_float(params, &format!("exclusion_{i}"), 0.0).clamp(0.0, 1.0);
                    layers.push(Layer {
                        tex: tex.clone(),
                        priority,
                        exclusion,
                    });
                }

                if layers.is_empty() {
                    let out = ColorBuffer::new(tex_width, tex_height).unwrap();
                    outputs.insert("output".to_string(), PortValue::Color(out));
                } else {
                    let w = layers[0].tex.width();
                    let h = layers[0].tex.height();
                    let mut out = ColorBuffer::new(w, h).unwrap();

                    match priority_type {
                        "priority" => {
                            // Sort highest priority first.
                            layers.sort_by(|a, b| {
                                b.priority
                                    .partial_cmp(&a.priority)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });
                            for y in 0..h {
                                for x in 0..w {
                                    let mut remaining = 1.0f32;
                                    let mut r = 0.0f32;
                                    let mut g = 0.0f32;
                                    let mut b_out = 0.0f32;
                                    for layer in &layers {
                                        if remaining <= 0.001 {
                                            break;
                                        }
                                        let raw_w = sample_color_nn(&layer.tex, x, y, w, h)[3]
                                            .clamp(0.0, 1.0);
                                        let contribution =
                                            (raw_w * remaining).clamp(0.0, remaining);
                                        let col = sample_color_nn(&layer.tex, x, y, w, h);
                                        r += col[0] * contribution;
                                        g += col[1] * contribution;
                                        b_out += col[2] * contribution;
                                        remaining -= contribution * layer.exclusion;
                                        remaining = remaining.max(0.0);
                                    }
                                    out.set(x, y, [r, g, b_out, 1.0]);
                                }
                            }
                        }
                        _ => {
                            // weighted_blend: normalize all weights at each pixel.
                            for y in 0..h {
                                for x in 0..w {
                                    let weights: Vec<f32> = layers
                                        .iter()
                                        .map(|l| {
                                            sample_color_nn(&l.tex, x, y, w, h)[3].clamp(0.0, 1.0)
                                        })
                                        .collect();
                                    let total: f32 = weights.iter().sum();
                                    if total < 0.0001 {
                                        out.set(x, y, [0.0, 0.0, 0.0, 0.0]);
                                        continue;
                                    }
                                    let (mut r, mut g, mut b_out) = (0.0f32, 0.0f32, 0.0f32);
                                    for (layer, &wt) in layers.iter().zip(weights.iter()) {
                                        let col = sample_color_nn(&layer.tex, x, y, w, h);
                                        let norm = wt / total;
                                        r += col[0] * norm;
                                        g += col[1] * norm;
                                        b_out += col[2] * norm;
                                    }
                                    out.set(x, y, [r, g, b_out, 1.0]);
                                }
                            }
                        }
                    }
                    outputs.insert("output".to_string(), PortValue::Color(out));
                }
            }

            NodeType::ColorRamp => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let color = apply_color_ramp(&input, params);
                let color =
                    apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
                let color = resize_color_to_tex(color, tex_width, tex_height);
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
                let color = resize_color_to_tex(color, tex_width, tex_height);
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
                let src_res = get_uint(params, "resolution", 256).max(1);
                let asset_path = get_string(params, "asset_path", "");
                let hm = read_painted_heightmap_asset(asset_path, src_res, width, height);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::Sculpt => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let asset_path = get_string(params, "asset_path", "");
                let mut sculpted = input.clone();
                if !asset_path.is_empty() {
                    let src_res = get_uint(params, "resolution", 256).max(1);
                    let scale = get_float(params, "scale", 0.5);
                    let pixels = read_asset_bytes(asset_path);
                    apply_sculpt_delta(&mut sculpted, &pixels, src_res, scale);
                }
                // Mask confines sculpt delta to specific areas (mask=0: original, mask=1: sculpted)
                let hm = apply_modulation(&input, sculpted, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }
            NodeType::PaintedTexture => {
                let path = get_string(params, "asset_path", "");
                // Read the header to get actual stored dimensions -- imported
                // textures can be any resolution, not just PAINTED_TEXTURE_RES.
                let (src_res, pixels) = if path.is_empty() {
                    (PAINTED_TEXTURE_RES, Vec::new())
                } else {
                    match bar_project::read_asset_file(std::path::Path::new(path)) {
                        Ok((header, data)) => (header.width.max(1), data),
                        Err(e) => {
                            tracing::warn!(path, error = %e, "Failed to read texture asset");
                            (PAINTED_TEXTURE_RES, Vec::new())
                        }
                    }
                };
                let tex = painted_rgb_to_color_buffer(pixels, src_res, tex_width, tex_height);
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

            NodeType::FlowSelect => {
                let input = get_input_heightmap(inputs, "input")?;
                let threshold = get_float(params, "threshold", 0.2);
                let falloff = get_float(params, "falloff", 0.15).max(1e-6);
                let hm = apply_flow_select(&input, threshold, falloff);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::SelectConvexity => {
                let input = get_input_heightmap(inputs, "input")?;
                let mode = get_string(params, "mode", "ridges");
                let strength = get_float(params, "strength", 1.0);
                let hm = apply_select_convexity(&input, mode, strength);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::LayoutGenerator => {
                let mask = get_optional_heightmap(inputs, "mask");
                let shape_count = get_uint(params, "shape_count", 1).min(8) as usize;
                let hm = apply_layout_generator(params, shape_count, width, height, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::Transform => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let tx = get_float(params, "translate_x", 0.0);
                let ty = get_float(params, "translate_y", 0.0);
                let scale = get_float(params, "scale", 1.0).max(1e-4);
                let angle = get_float(params, "angle", 0.0);
                let hm = apply_transform(&input, tx, ty, scale, angle);
                let hm = apply_modulation(&input, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::Warp => {
                let input = get_input_heightmap(inputs, "input")?;
                let warp_x = get_optional_heightmap(inputs, "warp_x");
                let warp_y = get_optional_heightmap(inputs, "warp_y");
                let strength = get_float(params, "strength", 0.1);
                let hm = apply_warp(&input, warp_x.as_ref(), warp_y.as_ref(), strength);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::Stratify => {
                let input = get_input_heightmap(inputs, "input")?;
                let mask = get_optional_heightmap(inputs, "mask");
                let layer_count = get_uint(params, "layer_count", 8).clamp(2, 32);
                let irregularity = get_float(params, "irregularity", 0.3);
                let hardness = get_float(params, "hardness", 0.8);
                let noise_scale = get_float(params, "noise_scale", 0.05);
                let hm = apply_stratify(&input, layer_count, irregularity, hardness, noise_scale);
                let hm = apply_modulation(&input, hm, None, mask.as_ref());
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::MaskExpand | NodeType::MaskShrink => {
                let input = get_input_heightmap(inputs, "input")?;
                let radius = get_float(params, "radius", 4.0).max(0.5);
                let expand = matches!(node_type, NodeType::MaskExpand);
                let hm = apply_morphology(&input, radius, expand);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            NodeType::SelectAspect => {
                let input = get_input_heightmap(inputs, "input")?;
                let direction = get_float(params, "direction", 0.0);
                let width = get_float(params, "width", 90.0);
                let falloff = get_float(params, "falloff", 30.0).max(1e-4);
                let hm = apply_select_aspect(&input, direction, width, falloff);
                outputs.insert("output".to_string(), PortValue::Heightmap(hm));
            }

            // --- Additional Combiners ---
            NodeType::MaskSelect => {
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

            NodeType::ImportedTexture => {
                let asset_path = get_string(params, "asset_path", "");
                let idx_path = get_string(params, "tile_index_path", "");
                let tiles_x = get_uint(params, "tiles_x", 0);
                let tiles_y = get_uint(params, "tiles_y", 0);
                let color =
                    if asset_path.is_empty() || idx_path.is_empty() || tiles_x == 0 || tiles_y == 0
                    {
                        ColorBuffer::new(tex_width, tex_height).unwrap()
                    } else {
                        let tiles_result = (|| {
                            let file = std::fs::File::open(asset_path).ok()?;
                            bar_data::smt::read_smt(&mut std::io::BufReader::new(file)).ok()
                        })();
                        let idx_result = std::fs::read(idx_path).ok();
                        match (tiles_result, idx_result) {
                            (Some(tiles), Some(idx_bytes)) => {
                                let tile_indices: Vec<i32> = idx_bytes
                                    .chunks(4)
                                    .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                                    .collect();
                                let rgba = assemble_texture_preview(
                                    &tiles,
                                    &tile_indices,
                                    tiles_x,
                                    tiles_y,
                                    tex_width,
                                    tex_height,
                                );
                                let mut buf = ColorBuffer::new(tex_width, tex_height).unwrap();
                                for (i, px) in rgba.chunks(4).enumerate() {
                                    let x = (i as u32) % tex_width;
                                    let y = (i as u32) / tex_width;
                                    buf.set(
                                        x,
                                        y,
                                        [
                                            px[0] as f32 / 255.0,
                                            px[1] as f32 / 255.0,
                                            px[2] as f32 / 255.0,
                                            1.0,
                                        ],
                                    );
                                }
                                buf
                            }
                            _ => {
                                tracing::warn!(
                                    asset_path,
                                    "ImportedTexture: failed to read SMT or tile index"
                                );
                                ColorBuffer::new(tex_width, tex_height).unwrap()
                            }
                        }
                    };
                outputs.insert("output".to_string(), PortValue::Color(color));
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
pub(crate) fn assemble_texture_preview(
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

/// Sample `tex` at output pixel `(ox, oy)` using nearest-neighbour scaling
/// to the output dimensions `(ow, oh)`.
fn sample_color_nn(tex: &ColorBuffer, ox: u32, oy: u32, ow: u32, oh: u32) -> [f32; 4] {
    let sx = ((ox as f32 / ow as f32) * tex.width() as f32) as u32;
    let sy = ((oy as f32 / oh as f32) * tex.height() as f32) as u32;
    tex.get(sx.min(tex.width() - 1), sy.min(tex.height() - 1))
        .unwrap_or([0.0; 4])
}

/// Resize a ColorBuffer to (tw, th) only when dimensions differ.
/// Bridge nodes (heightmap-in, color-out) generate at hm dims then call this
/// to match the working/compile texture resolution.
fn resize_color_to_tex(cb: ColorBuffer, tw: u32, th: u32) -> ColorBuffer {
    if cb.width() == tw && cb.height() == th {
        cb
    } else {
        cb.resize(tw, th)
    }
}

fn get_input_heightmap(
    inputs: &HashMap<String, PortValue>,
    name: &str,
) -> Result<Heightmap, EvalError> {
    match inputs.get(name) {
        Some(PortValue::Heightmap(hm)) => Ok(hm.clone()),
        Some(PortValue::Mask(hm)) => Ok(hm.clone()),
        _ => Err(EvalError::MissingInput {
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

/// Maps every pixel through the user-defined color-stop gradient.
/// Stops are read from indexed params (pos_i, color_i) up to stop_count.
/// Stops are sorted by position before interpolation so order in params
/// doesn't matter.
fn apply_color_ramp(input: &Heightmap, params: &HashMap<String, ParamValue>) -> ColorBuffer {
    let stop_count = match params.get("stop_count") {
        Some(ParamValue::UInt(n)) => (*n).clamp(2, 8) as usize,
        _ => 2,
    };

    let mut stops: Vec<(f32, [f32; 3])> = (0..stop_count)
        .map(|i| {
            let pos = match params.get(&format!("pos_{i}")) {
                Some(ParamValue::Float(v)) => v.clamp(0.0, 1.0),
                _ => i as f32 / (stop_count - 1).max(1) as f32,
            };
            let hex = match params.get(&format!("color_{i}")) {
                Some(ParamValue::String(s)) => s.as_str(),
                _ => "808080",
            };
            let rgb = parse_hex_color_srgb(hex).unwrap_or([0.5, 0.5, 0.5]);
            (pos, rgb)
        })
        .collect();

    stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let w = input.width();
    let h = input.height();
    let mut out = ColorBuffer::new(w, h).unwrap();

    for (i, &hv) in input.data().iter().enumerate() {
        let hv = hv.clamp(0.0, 1.0);
        let color = if stops.len() < 2 {
            stops.first().map_or([0.0f32; 3], |s| s.1)
        } else if hv <= stops[0].0 {
            stops[0].1
        } else if hv >= stops[stops.len() - 1].0 {
            stops[stops.len() - 1].1
        } else {
            let hi = stops
                .iter()
                .position(|s| s.0 >= hv)
                .unwrap_or(stops.len() - 1);
            let lo = hi.saturating_sub(1);
            let span = stops[hi].0 - stops[lo].0;
            let t = if span > 1e-6 {
                (hv - stops[lo].0) / span
            } else {
                0.0
            };
            let a = stops[lo].1;
            let b = stops[hi].1;
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        let base = i * 4;
        let data = out.data_mut();
        data[base] = color[0];
        data[base + 1] = color[1];
        data[base + 2] = color[2];
        // alpha stays 1.0 from ColorBuffer::new
    }
    out
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

fn apply_terrace(input: &Heightmap, step_count: u32, smoothing: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let steps = step_count.max(1) as f32;
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| {
            let t = v * steps;
            let lo = t.floor();
            let frac = t - lo;
            // Smoothstep within each step band, lerped by `smoothing`.
            let smooth = frac * frac * (3.0 - 2.0 * frac);
            let hard = lo / steps;
            let soft = (lo + smooth) / steps;
            hard + smoothing * (soft - hard)
        })
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

fn apply_sharpen(input: &Heightmap, radius: f32, strength: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let blurred = apply_blur(input, radius);
    let data: Vec<f32> = input
        .data()
        .iter()
        .zip(blurred.data().iter())
        .map(|(&v, &b)| (v + strength * (v - b)).clamp(0.0, 1.0))
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
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

fn apply_mirror(input: &Heightmap, mode: &str) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let src = input.data();
    let mut data = vec![0.0f32; w * h];
    for py in 0..h {
        for px in 0..w {
            let (sx, sy) = match mode {
                "mirror_x" => {
                    let sx = if px < w / 2 { px } else { w - 1 - px };
                    (sx, py)
                }
                "mirror_y" => {
                    let sy = if py < h / 2 { py } else { h - 1 - py };
                    (px, sy)
                }
                "mirror_xy" => {
                    let sx = if px < w / 2 { px } else { w - 1 - px };
                    let sy = if py < h / 2 { py } else { h - 1 - py };
                    (sx, sy)
                }
                "rotate_180" => {
                    // Left half is canonical; right half gets value rotated 180.
                    if px < w / 2 {
                        (px, py)
                    } else {
                        (w - 1 - px, h - 1 - py)
                    }
                }
                "rotate_90_4way" => {
                    // Top-left quadrant is canonical. Other quadrants are mapped
                    // back by 90-degree rotations (assumes a square map).
                    if px < w / 2 && py < h / 2 {
                        (px, py)
                    } else if px >= w / 2 && py < h / 2 {
                        (py, w - 1 - px)
                    } else if px < w / 2 {
                        (h - 1 - py, px)
                    } else {
                        (w - 1 - px, h - 1 - py)
                    }
                }
                _ => (px, py),
            };
            data[py * w + px] = src[sy * w + sx];
        }
    }
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
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
    let detail_strength = get_float(params, "detail_strength", 0.15).clamp(0.0, 1.0);
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

            // FBM micro-detail grain: same pattern as RockSoil/Vegetation.
            let ux = x as f32 / w as f32;
            let uy = y as f32 / h as f32;
            let detail = 1.0 + detail_strength * (micro_fbm(ux, uy, 8.0) * 2.0 - 1.0);

            color.set(
                x,
                y,
                [
                    (r * ao * detail).clamp(0.0, 1.0),
                    (g * ao * detail).clamp(0.0, 1.0),
                    (b * ao * detail).clamp(0.0, 1.0),
                    1.0,
                ],
            );
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

/// MaskSelect: select between A and B based on a mask (0=A, 1=B, interpolated in between).
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

/// Read raw pixel bytes from a binary asset file at `path`.
/// Returns empty Vec on empty path or any I/O error; the executor treats
/// empty data as "no paint" and produces a flat/zero output.
fn read_asset_bytes(path: &str) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }
    match bar_project::read_asset_file(std::path::Path::new(path)) {
        Ok((_header, data)) => data,
        Err(e) => {
            tracing::warn!(path, error = %e, "Failed to read asset file");
            Vec::new()
        }
    }
}

/// Source resolution for the `PaintedTexture` node's brush canvas.
/// Fixed for now; could be made a param like PaintedHeightmap.
pub(crate) const PAINTED_TEXTURE_RES: u32 = 256;

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

/// Read a `PaintedHeightmap` asset, dispatching on its pixel format.
/// Brush-painted assets are still 8-bit (the brush dab range is captured
/// at u8 granularity); SD7-imported asset are f32 to preserve the full
/// SMF height precision (16-bit native; u8 storage was visibly terraced).
fn read_painted_heightmap_asset(
    asset_path: &str,
    fallback_res: u32,
    out_w: u32,
    out_h: u32,
) -> Heightmap {
    if asset_path.is_empty() {
        return painted_grayscale_to_heightmap(Vec::new(), fallback_res, out_w, out_h);
    }
    match bar_project::read_asset_file(std::path::Path::new(asset_path)) {
        Ok((header, data)) => match header.kind {
            bar_project::AssetKind::GrayscaleU8 => {
                painted_grayscale_to_heightmap(data, header.width.max(1), out_w, out_h)
            }
            bar_project::AssetKind::GrayscaleF32 => {
                painted_f32_to_heightmap(&data, header.width.max(1), out_w, out_h)
            }
            other => {
                tracing::warn!(
                    asset_path,
                    ?other,
                    "PaintedHeightmap asset has non-grayscale kind; falling back to zero heightmap",
                );
                painted_grayscale_to_heightmap(Vec::new(), fallback_res, out_w, out_h)
            }
        },
        Err(e) => {
            tracing::warn!(asset_path, error = %e, "Failed to read PaintedHeightmap asset");
            painted_grayscale_to_heightmap(Vec::new(), fallback_res, out_w, out_h)
        }
    }
}

/// Bilinearly resample a `src_res × src_res` f32 heightmap (stored as
/// little-endian f32 bytes) into a `out_w × out_h` `Heightmap`. Sample
/// values are clamped to `[0, 1]` to match the contract of the rest of
/// the heightmap pipeline.
fn painted_f32_to_heightmap(bytes: &[u8], src_res: u32, out_w: u32, out_h: u32) -> Heightmap {
    let expected = (src_res as usize)
        .saturating_mul(src_res as usize)
        .saturating_mul(4);
    if bytes.len() != expected || src_res == 0 {
        // Wrong size -- produce a flat zero heightmap so downstream nodes
        // still have something to operate on.
        return Heightmap::frbar_data(
            out_w,
            out_h,
            vec![0.0f32; (out_w as usize) * (out_h as usize)],
        )
        .unwrap();
    }
    // Reinterpret the bytes as &[f32] without an alloc copy. `read_asset_file`
    // returns a `Vec<u8>` whose backing buffer isn't guaranteed f32-aligned,
    // so we go via from_le_bytes per sample. Still O(N) and roughly as fast
    // as a transmute thanks to compiler vectorisation of the loop.
    let src: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let src_w = src_res;
    let src_h = src_res;
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
            let v00 = src[(y0 as usize) * (src_w as usize) + x0 as usize];
            let v10 = src[(y0 as usize) * (src_w as usize) + x1 as usize];
            let v01 = src[(y1 as usize) * (src_w as usize) + x0 as usize];
            let v11 = src[(y1 as usize) * (src_w as usize) + x1 as usize];
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;
            data[(oy as usize) * (out_w as usize) + ox as usize] = v.clamp(0.0, 1.0);
        }
    }
    Heightmap::frbar_data(out_w, out_h, data).unwrap()
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
/// Threshold selector for flow/wear/deposit maps.
/// Ramps from 0 at (threshold - falloff) to 1 at threshold.
fn apply_flow_select(input: &Heightmap, threshold: f32, falloff: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let lo = threshold - falloff;
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| ((v - lo) / falloff).clamp(0.0, 1.0))
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Surface curvature (Laplacian) selector.
fn apply_select_convexity(input: &Heightmap, mode: &str, strength: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let data = input.data();

    // Compute raw Laplacian, collecting its range for normalization.
    let mut raw = vec![0.0f32; w * h];
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for y in 0..h {
        for x in 0..w {
            let c = data[y * w + x];
            let l = data[y * w + x.saturating_sub(1)];
            let r = data[y * w + (x + 1).min(w - 1)];
            let u = data[y.saturating_sub(1) * w + x];
            let d = data[(y + 1).min(h - 1) * w + x];
            // Negative = ridge/peak; positive = valley/bowl.
            let lap = l + r + u + d - 4.0 * c;
            raw[y * w + x] = lap;
            if lap < lo {
                lo = lap;
            }
            if lap > hi {
                hi = lap;
            }
        }
    }

    let range = (hi - lo).max(1e-9);
    let out: Vec<f32> = raw
        .iter()
        .map(|&lap| {
            // Normalize lap to roughly [-1, 1] then scale by strength.
            let norm = lap / range * 2.0 * strength;
            match mode {
                // High on ridges/peaks (negative Laplacian).
                "ridges" => (-norm).clamp(0.0, 1.0),
                // High in valleys/bowls (positive Laplacian).
                "valleys" => norm.clamp(0.0, 1.0),
                // Full map: 0.5 = flat, >0.5 = ridges, <0.5 = valleys.
                _ => (-norm * 0.5 + 0.5).clamp(0.0, 1.0),
            }
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, out).unwrap()
}

/// Composites up to 8 primitive shapes into a heightmap.
/// Each shape contributes via a smooth radial falloff; shapes are max-blended.
fn apply_layout_generator(
    params: &HashMap<String, ParamValue>,
    shape_count: usize,
    width: u32,
    height: u32,
    mask: Option<&Heightmap>,
) -> Heightmap {
    let mut data = vec![0.0f32; (width * height) as usize];

    for i in 0..shape_count {
        let shape_type = match params.get(&format!("type_{i}")) {
            Some(ParamValue::String(s)) => s.as_str(),
            _ => "ellipse",
        };
        let cx = get_float(params, &format!("x_{i}"), 0.5);
        let cy = get_float(params, &format!("y_{i}"), 0.5);
        let rx = get_float(params, &format!("rx_{i}"), 0.2).max(1e-4);
        let ry = get_float(params, &format!("ry_{i}"), 0.2).max(1e-4);
        let angle_deg = get_float(params, &format!("angle_{i}"), 0.0);
        let peak = get_float(params, &format!("height_{i}"), 0.5);
        let falloff = get_float(params, &format!("falloff_{i}"), 0.5).clamp(0.001, 1.0);
        let angle_rad = angle_deg * PI / 180.0;
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();

        for py in 0..height {
            for px in 0..width {
                let ux = px as f32 / (width - 1).max(1) as f32 - cx;
                let uy = py as f32 / (height - 1).max(1) as f32 - cy;
                // Rotate into shape-local space.
                let lx = (ux * cos_a + uy * sin_a) / rx;
                let ly = (-ux * sin_a + uy * cos_a) / ry;
                // Normalized distance from centre.
                let d = match shape_type {
                    "rectangle" => lx.abs().max(ly.abs()),
                    "ridge" => ly.abs(), // infinite ridge along the rotated X axis
                    _ => (lx * lx + ly * ly).sqrt(), // ellipse
                };
                if d >= 1.0 {
                    continue;
                }
                // Smooth falloff: cos curve from d=1-falloff (peak) to d=1 (zero).
                let t = 1.0 - d; // 1 at centre, 0 at edge
                let smoothed = if t >= falloff {
                    1.0
                } else {
                    let s = t / falloff;
                    s * s * (3.0 - 2.0 * s) // smoothstep
                };
                let v = smoothed * peak;
                let idx = py as usize * width as usize + px as usize;
                if v > data[idx] {
                    data[idx] = v;
                }
            }
        }
    }

    // Apply optional mask: mask=0 → output=0.
    if let Some(m) = mask {
        for (i, v) in data.iter_mut().enumerate() {
            let mx = (i % width as usize) as u32;
            let my = (i / width as usize) as u32;
            let mw = m.width();
            let mh = m.height();
            let smx = (mx as f32 * mw as f32 / width as f32) as u32;
            let smy = (my as f32 * mh as f32 / height as f32) as u32;
            let mv = m.get(smx.min(mw - 1), smy.min(mh - 1)).unwrap_or(1.0);
            *v *= mv;
        }
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}

/// Bilinear sample with clamp-to-edge.
fn bilinear_sample(data: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x0 = (x.floor() as i32).clamp(0, w as i32 - 1) as usize;
    let y0 = (y.floor() as i32).clamp(0, h as i32 - 1) as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = (x - x.floor()).clamp(0.0, 1.0);
    let fy = (y - y.floor()).clamp(0.0, 1.0);
    let v00 = data[y0 * w + x0];
    let v10 = data[y0 * w + x1];
    let v01 = data[y1 * w + x0];
    let v11 = data[y1 * w + x1];
    let v0 = v00 + (v10 - v00) * fx;
    let v1 = v01 + (v11 - v01) * fx;
    v0 + (v1 - v0) * fy
}

/// Translate, scale, rotate a heightmap via inverse-mapped bilinear sampling.
fn apply_transform(input: &Heightmap, tx: f32, ty: f32, scale: f32, angle_deg: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let angle_rad = angle_deg * PI / 180.0;
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();
    let inv_scale = 1.0 / scale;
    let data_in = input.data();

    let data: Vec<f32> = (0..h)
        .flat_map(|py| {
            (0..w).map(move |px| {
                // Normalize output pixel to [-0.5, 0.5].
                let nx = px as f32 / w as f32 - 0.5;
                let ny = py as f32 / h as f32 - 0.5;
                // Inverse transform: undo translate, undo rotate, undo scale.
                let ux = nx - tx;
                let uy = ny - ty;
                let rx = (ux * cos_a + uy * sin_a) * inv_scale;
                let ry = (-ux * sin_a + uy * cos_a) * inv_scale;
                // Map back to pixel space.
                let sx = (rx + 0.5) * w as f32;
                let sy = (ry + 0.5) * h as f32;
                if sx < 0.0 || sy < 0.0 || sx > w as f32 || sy > h as f32 {
                    return 0.0;
                }
                bilinear_sample(data_in, w, h, sx, sy)
            })
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
}

/// Domain warp using separate X and Y displacement maps.
/// Each warp map is treated as a signed offset: 0.5 = no displacement.
fn apply_warp(
    input: &Heightmap,
    warp_x: Option<&Heightmap>,
    warp_y: Option<&Heightmap>,
    strength: f32,
) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let data_in = input.data();

    let data: Vec<f32> = (0..h)
        .flat_map(|py| {
            (0..w).map(move |px| {
                let dx = warp_x
                    .and_then(|m| m.get(px as u32, py as u32))
                    .unwrap_or(0.5)
                    - 0.5;
                let dy = warp_y
                    .and_then(|m| m.get(px as u32, py as u32))
                    .unwrap_or(0.5)
                    - 0.5;
                let sx = px as f32 + dx * strength * w as f32;
                let sy = py as f32 + dy * strength * h as f32;
                bilinear_sample(data_in, w, h, sx, sy)
            })
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
}

/// Simple 2D value noise in [0, 1].
fn strat_hash(x: i32, y: i32) -> f32 {
    let n = x
        .wrapping_mul(1619)
        .wrapping_add(y.wrapping_mul(31337))
        .wrapping_mul(6364136)
        ^ 0x5851f42d_u32 as i32;
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(n.wrapping_add(15731)).wrapping_add(789221) ^ 1376312589;
    ((n as u32) as f32) / u32::MAX as f32
}

fn value_noise_2d(x: f32, y: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let ux = xf * xf * (3.0 - 2.0 * xf);
    let uy = yf * yf * (3.0 - 2.0 * yf);
    let v00 = strat_hash(xi, yi);
    let v10 = strat_hash(xi + 1, yi);
    let v01 = strat_hash(xi, yi + 1);
    let v11 = strat_hash(xi + 1, yi + 1);
    let v0 = v00 + (v10 - v00) * ux;
    let v1 = v01 + (v11 - v01) * ux;
    v0 + (v1 - v0) * uy
}

/// Procedural horizontal rock strata.
fn apply_stratify(
    input: &Heightmap,
    layer_count: u32,
    irregularity: f32,
    hardness: f32,
    noise_scale: f32,
) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let n = layer_count as f32;

    let data: Vec<f32> = input
        .data()
        .iter()
        .enumerate()
        .map(|(idx, &v)| {
            let px = (idx % w) as f32;
            let py = (idx / w) as f32;
            let perturb = if irregularity > 0.0 {
                let scale = noise_scale * w as f32;
                (value_noise_2d(px / scale, py / scale) - 0.5) * irregularity * (1.0 / n)
            } else {
                0.0
            };
            let vp = (v + perturb).clamp(0.0, 1.0);
            let band = (vp * n).floor().min(n - 1.0);
            let band_h = (band + 0.5) / n;
            v * (1.0 - hardness) + band_h * hardness
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
}

/// Morphological dilation (expand=true) or erosion (expand=false) via
/// a separable max/min filter. O(w*h*r) rather than O(w*h*r^2).
fn apply_morphology(input: &Heightmap, radius: f32, expand: bool) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let r = radius.round() as usize;
    let identity = if expand { 0.0f32 } else { 1.0f32 };

    // Horizontal pass.
    let mut temp = vec![identity; w * h];
    let data_in = input.data();
    for py in 0..h {
        for px in 0..w {
            let lo = px.saturating_sub(r);
            let hi = (px + r).min(w - 1);
            let mut acc = data_in[py * w + lo];
            for kx in lo..=hi {
                let v = data_in[py * w + kx];
                acc = if expand { acc.max(v) } else { acc.min(v) };
            }
            temp[py * w + px] = acc;
        }
    }

    // Vertical pass.
    let mut out = vec![identity; w * h];
    for py in 0..h {
        for px in 0..w {
            let lo = py.saturating_sub(r);
            let hi = (py + r).min(h - 1);
            let mut acc = temp[lo * w + px];
            for ky in lo..=hi {
                let v = temp[ky * w + px];
                acc = if expand { acc.max(v) } else { acc.min(v) };
            }
            out[py * w + px] = acc;
        }
    }

    Heightmap::frbar_data(w as u32, h as u32, out).unwrap()
}

/// Aspect-direction mask. High where terrain faces `direction` degrees
/// (0=North/up, 90=East, 180=South, 270=West).
fn apply_select_aspect(input: &Heightmap, direction: f32, width: f32, falloff: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let data = input.data();

    let out: Vec<f32> = (0..h)
        .flat_map(|py| {
            (0..w).map(move |px| {
                let xm = px.saturating_sub(1);
                let xp = (px + 1).min(w - 1);
                let ym = py.saturating_sub(1);
                let yp = (py + 1).min(h - 1);
                let dx = (data[py * w + xp] - data[py * w + xm]) / (xp - xm).max(1) as f32;
                let dy = (data[yp * w + px] - data[ym * w + px]) / (yp - ym).max(1) as f32;
                if dx * dx + dy * dy < 1e-12 {
                    return 0.0;
                }
                // atan2(dx, -dy): 0=North, 90=East, 180=South, 270=West.
                let aspect = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                let mut diff = (aspect - direction).abs().rem_euclid(360.0);
                if diff > 180.0 {
                    diff = 360.0 - diff;
                }
                let half = width * 0.5;
                if diff <= half {
                    1.0
                } else if diff <= half + falloff {
                    let t = (diff - half) / falloff;
                    1.0 - t * t * (3.0 - 2.0 * t)
                } else {
                    0.0
                }
            })
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, out).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{GraphEngine, Node, NodeId, PortId};

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
            .execute(&NodeType::PassThrough, &params, &inputs, 1, 1, 1, 1)
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
            .execute(&NodeType::PerlinNoise, &params, &inputs, 64, 64, 64, 64)
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

        let results = bar_graph::evaluate_graph(&graph, &executor, 64, 64, 64, 64).unwrap();
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
            .execute(&NodeType::Blend, &params, &inputs, 4, 4, 4, 4)
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
            .execute(&NodeType::Voronoi, &params, &HashMap::new(), 64, 64, 64, 64)
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
            .execute(&NodeType::Gradient, &params, &HashMap::new(), 8, 8, 8, 8)
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
            .execute(&NodeType::Normalize, &HashMap::new(), &inputs, 8, 8, 8, 8)
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
            .execute(&NodeType::BiasGain, &params, &inputs, 4, 4, 4, 4)
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
            .execute(&NodeType::MaskSelect, &HashMap::new(), &inputs, 4, 4, 4, 4)
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
            .execute(&NodeType::Add, &HashMap::new(), &inputs, 2, 2, 2, 2)
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

    // ── Tier-1 node executor tests ────────────────────────────────────────────

    #[test]
    fn hydraulic_erosion_emits_four_output_ports() {
        let executor = CpuExecutor;
        let data: Vec<f32> = (0..64 * 64)
            .map(|i| {
                let x = (i % 64) as f32 / 63.0;
                let y = (i / 64) as f32 / 63.0;
                ((x - 0.5) * (x - 0.5) + (y - 0.5) * (y - 0.5)).sqrt()
            })
            .collect();
        let hm = Heightmap::frbar_data(64, 64, data).unwrap();
        let params = HashMap::from([("iterations".to_string(), ParamValue::UInt(2000))]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(
                &NodeType::HydraulicErosion,
                &params,
                &inputs,
                64,
                64,
                64,
                64,
            )
            .unwrap();
        for port in ["output", "flow", "wear", "deposit"] {
            let val = result.get(port).expect(port);
            let PortValue::Heightmap(out) = val else {
                panic!("{port} should be Heightmap");
            };
            for &v in out.data() {
                assert!((0.0..=1.0).contains(&v), "{port} value {v} out of range");
            }
        }
    }

    #[test]
    fn flow_select_thresholds_correctly() {
        let executor = CpuExecutor;
        // Uniform gradient 0..1 across 8 pixels.
        let data: Vec<f32> = (0..8).map(|i| i as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 1, data).unwrap();
        let params = HashMap::from([
            ("threshold".to_string(), ParamValue::Float(0.5)),
            ("falloff".to_string(), ParamValue::Float(0.25)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::FlowSelect, &params, &inputs, 8, 1, 8, 1)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // v=0 (well below threshold-falloff=0.25) should produce 0.
        assert!(
            out.get(0, 0).unwrap() < 0.01,
            "pixel 0 should be ~0, got {}",
            out.get(0, 0).unwrap()
        );
        // v=1 (above threshold) should produce 1.
        assert!(
            out.get(7, 0).unwrap() > 0.99,
            "pixel 7 should be ~1, got {}",
            out.get(7, 0).unwrap()
        );
    }

    #[test]
    fn select_convexity_ridges_mode_peaks_high() {
        let executor = CpuExecutor;
        // Single spike in centre on a flat background.
        // The spike pixel itself has a strongly negative Laplacian
        // (neighbors - 4*center < 0), so "ridges" mode should score it highest.
        let mut data = vec![0.0f32; 16 * 16];
        data[8 * 16 + 8] = 1.0;
        let hm = Heightmap::frbar_data(16, 16, data).unwrap();
        let params = HashMap::from([
            ("mode".to_string(), ParamValue::String("ridges".to_string())),
            ("strength".to_string(), ParamValue::Float(1.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::SelectConvexity, &params, &inputs, 16, 16, 16, 16)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // The spike pixel has the most negative Laplacian; ridges mode maps
        // strongly negative -> high output.
        let spike = out.get(8, 8).unwrap();
        assert!(
            spike > 0.8,
            "spike should score high in ridges mode, got {spike}"
        );
        // Flat background pixel far from spike should be low.
        let flat = out.get(0, 0).unwrap();
        assert!(flat < 0.2, "flat area should score low, got {flat}");
    }

    #[test]
    fn layout_generator_ellipse_peak_at_centre() {
        let executor = CpuExecutor;
        let mut params = HashMap::new();
        params.insert("shape_count".to_string(), ParamValue::UInt(1));
        params.insert(
            "type_0".to_string(),
            ParamValue::String("ellipse".to_string()),
        );
        params.insert("x_0".to_string(), ParamValue::Float(0.5));
        params.insert("y_0".to_string(), ParamValue::Float(0.5));
        params.insert("rx_0".to_string(), ParamValue::Float(0.3));
        params.insert("ry_0".to_string(), ParamValue::Float(0.3));
        params.insert("angle_0".to_string(), ParamValue::Float(0.0));
        params.insert("height_0".to_string(), ParamValue::Float(0.8));
        params.insert("falloff_0".to_string(), ParamValue::Float(0.5));
        let result = executor
            .execute(
                &NodeType::LayoutGenerator,
                &params,
                &HashMap::new(),
                32,
                32,
                32,
                32,
            )
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        let centre = out.get(16, 16).unwrap();
        let edge = out.get(0, 0).unwrap();
        assert!(centre > 0.5, "centre should be high, got {centre}");
        assert!(edge < 0.01, "corner should be near zero, got {edge}");
    }

    // ── Tier-2 node executor tests ────────────────────────────────────────────

    #[test]
    fn transform_identity_roundtrips() {
        let executor = CpuExecutor;
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        let params = HashMap::from([
            ("translate_x".to_string(), ParamValue::Float(0.0)),
            ("translate_y".to_string(), ParamValue::Float(0.0)),
            ("scale".to_string(), ParamValue::Float(1.0)),
            ("angle".to_string(), ParamValue::Float(0.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm.clone()))]);
        let result = executor
            .execute(&NodeType::Transform, &params, &inputs, 4, 4, 4, 4)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Identity transform: output should closely match input.
        let diff: f32 = hm
            .data()
            .iter()
            .zip(out.data().iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 16.0;
        assert!(diff < 0.05, "identity transform mean error = {diff}");
    }

    #[test]
    fn transform_180_rotation_flips_values() {
        let executor = CpuExecutor;
        // Ramp increasing left-to-right.
        let data: Vec<f32> = (0..8).map(|i| i as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 1, data).unwrap();
        let params = HashMap::from([
            ("translate_x".to_string(), ParamValue::Float(0.0)),
            ("translate_y".to_string(), ParamValue::Float(0.0)),
            ("scale".to_string(), ParamValue::Float(1.0)),
            ("angle".to_string(), ParamValue::Float(180.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm.clone()))]);
        let result = executor
            .execute(&NodeType::Transform, &params, &inputs, 8, 1, 8, 1)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // After 180 rotation left pixel should be high, right pixel low.
        assert!(
            out.get(0, 0).unwrap() > out.get(7, 0).unwrap(),
            "left={} should exceed right={}",
            out.get(0, 0).unwrap(),
            out.get(7, 0).unwrap()
        );
    }

    #[test]
    fn warp_no_displacement_is_identity() {
        let executor = CpuExecutor;
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        // Neutral warp maps: all 0.5 means zero displacement.
        let neutral = Heightmap::frbar_data(4, 4, vec![0.5; 16]).unwrap();
        let params = HashMap::from([("strength".to_string(), ParamValue::Float(0.5))]);
        let inputs = HashMap::from([
            ("input".to_string(), PortValue::Heightmap(hm.clone())),
            ("warp_x".to_string(), PortValue::Heightmap(neutral.clone())),
            ("warp_y".to_string(), PortValue::Heightmap(neutral)),
        ]);
        let result = executor
            .execute(&NodeType::Warp, &params, &inputs, 4, 4, 4, 4)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        let diff: f32 = hm
            .data()
            .iter()
            .zip(out.data().iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 16.0;
        assert!(diff < 0.01, "neutral warp should be identity, diff={diff}");
    }

    #[test]
    fn warp_shifts_output() {
        let executor = CpuExecutor;
        // Flat 0 on the left half, flat 1 on the right half.
        let mut data = vec![0.0f32; 8 * 8];
        for y in 0..8usize {
            for x in 4..8usize {
                data[y * 8 + x] = 1.0;
            }
        }
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        // warp_x = 1.0 -> dx = 0.5, so each output pixel samples from
        // input_x + 0.5 * strength * width = px + 4.  The right-half content
        // (bright) therefore appears in the left half of the output.
        let wx = Heightmap::frbar_data(8, 8, vec![1.0; 64]).unwrap();
        let wy = Heightmap::frbar_data(8, 8, vec![0.5; 64]).unwrap();
        let params = HashMap::from([("strength".to_string(), ParamValue::Float(1.0))]);
        let inputs = HashMap::from([
            ("input".to_string(), PortValue::Heightmap(hm)),
            ("warp_x".to_string(), PortValue::Heightmap(wx)),
            ("warp_y".to_string(), PortValue::Heightmap(wy)),
        ]);
        let result = executor
            .execute(&NodeType::Warp, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Left column should now sample from the right half (bright).
        let left_mean: f32 = (0..8).map(|y| out.get(1, y).unwrap()).sum::<f32>() / 8.0;
        assert!(
            left_mean > 0.5,
            "positive warp should bring bright area left, mean={left_mean}"
        );
    }

    #[test]
    fn stratify_quantises_to_bands() {
        let executor = CpuExecutor;
        // Linear ramp 0..1.
        let data: Vec<f32> = (0..8).map(|i| i as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 1, data).unwrap();
        let params = HashMap::from([
            ("layer_count".to_string(), ParamValue::UInt(4)),
            ("irregularity".to_string(), ParamValue::Float(0.0)),
            ("hardness".to_string(), ParamValue::Float(1.0)),
            ("noise_scale".to_string(), ParamValue::Float(0.05)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::Stratify, &params, &inputs, 8, 1, 8, 1)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // With hardness=1 and 4 bands, all values should land on band centres.
        let valid_centres = [0.125f32, 0.375, 0.625, 0.875];
        for x in 0..8u32 {
            let v = out.get(x, 0).unwrap();
            let ok = valid_centres.iter().any(|&c| (v - c).abs() < 0.01);
            assert!(ok, "pixel {x} value {v} is not a band centre");
        }
    }

    #[test]
    fn mask_expand_dilates() {
        let executor = CpuExecutor;
        // Single bright pixel in the centre of a dark field.
        let mut data = vec![0.0f32; 8 * 8];
        data[4 * 8 + 4] = 1.0;
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        let params = HashMap::from([("radius".to_string(), ParamValue::Float(1.5))]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::MaskExpand, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Centre + direct neighbours should be 1.
        assert!(out.get(4, 4).unwrap() > 0.99);
        assert!(
            out.get(3, 4).unwrap() > 0.99,
            "left neighbour should be expanded"
        );
        assert!(
            out.get(5, 4).unwrap() > 0.99,
            "right neighbour should be expanded"
        );
        // Far corner should still be 0.
        assert!(
            out.get(0, 0).unwrap() < 0.01,
            "corner should not be expanded"
        );
    }

    #[test]
    fn mask_shrink_erodes() {
        let executor = CpuExecutor;
        // Mostly bright except a single dark pixel in the centre.
        let mut data = vec![1.0f32; 8 * 8];
        data[4 * 8 + 4] = 0.0;
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        let params = HashMap::from([("radius".to_string(), ParamValue::Float(1.5))]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::MaskShrink, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Centre + direct neighbours should be 0.
        assert!(out.get(4, 4).unwrap() < 0.01);
        assert!(
            out.get(3, 4).unwrap() < 0.01,
            "left neighbour should be eroded"
        );
        // Far corner should still be 1.
        assert!(out.get(0, 0).unwrap() > 0.99, "corner should not be eroded");
    }

    #[test]
    fn select_aspect_east_facing_slopes() {
        let executor = CpuExecutor;
        // Ramp increasing left-to-right: slopes face east (90 deg).
        let data: Vec<f32> = (0..8 * 8).map(|i| (i % 8) as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        // Select east-facing (direction=90), full-strength band=60 deg.
        let params = HashMap::from([
            ("direction".to_string(), ParamValue::Float(90.0)),
            ("width".to_string(), ParamValue::Float(60.0)),
            ("falloff".to_string(), ParamValue::Float(30.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::SelectAspect, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Interior pixels (not at edge) should have a high mask value.
        let centre = out.get(4, 4).unwrap();
        assert!(
            centre > 0.8,
            "east-facing slope should score high, got {centre}"
        );
    }

    #[test]
    fn select_aspect_opposite_direction_is_zero() {
        let executor = CpuExecutor;
        // Ramp increasing left-to-right: slopes face east (90 deg).
        let data: Vec<f32> = (0..8 * 8).map(|i| (i % 8) as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        // Select west-facing (direction=270), tight band.
        let params = HashMap::from([
            ("direction".to_string(), ParamValue::Float(270.0)),
            ("width".to_string(), ParamValue::Float(30.0)),
            ("falloff".to_string(), ParamValue::Float(10.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::SelectAspect, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        let centre = out.get(4, 4).unwrap();
        assert!(
            centre < 0.01,
            "west selector on east-facing slope should be 0, got {centre}"
        );
    }
}
