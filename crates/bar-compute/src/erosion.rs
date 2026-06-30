//! CPU-based erosion implementations.
//! These serve as fallbacks when GPU compute is unavailable, and as reference
//! implementations for validating the GPU shaders.

use bar_data::Heightmap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ErosionError {
    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("heightmap error: {0}")]
    Heightmap(String),
}

/// Parameters for hydraulic erosion simulation.
#[derive(Debug, Clone)]
pub struct HydraulicErosionParams {
    /// Number of water droplets to simulate.
    pub num_droplets: u32,
    /// Particle inertia (0 = no inertia, 1 = full inertia). Typically 0.05–0.1.
    pub inertia: f32,
    /// Sediment capacity multiplier.
    pub capacity_factor: f32,
    /// Minimum sediment capacity.
    pub min_capacity: f32,
    /// Rate of sediment deposition.
    pub deposition_rate: f32,
    /// Rate of terrain erosion.
    pub erosion_rate: f32,
    /// Water evaporation rate per step.
    pub evaporation_rate: f32,
    /// Gravity constant affecting speed.
    pub gravity: f32,
    /// Maximum lifetime (steps) of each droplet.
    pub max_lifetime: u32,
    /// Erosion brush radius (cells).
    pub erosion_radius: u32,
    /// Random seed.
    pub seed: u32,
    /// River-channel incision strength (0 = no-op). Higher values amplify
    /// erosion in fast-flowing droplets so channels carve deeper. 0.0 exactly
    /// reproduces the baseline droplet model.
    pub river_depth: f32,
}

impl Default for HydraulicErosionParams {
    fn default() -> Self {
        Self {
            num_droplets: 50_000,
            inertia: 0.05,
            capacity_factor: 4.0,
            min_capacity: 0.01,
            deposition_rate: 0.01,
            erosion_rate: 0.01,
            evaporation_rate: 0.01,
            gravity: 4.0,
            max_lifetime: 30,
            erosion_radius: 3,
            seed: 0,
            river_depth: 0.0,
        }
    }
}

/// Parameters for thermal erosion.
#[derive(Debug, Clone)]
pub struct ThermalErosionParams {
    /// Number of iterations to run.
    pub iterations: u32,
    /// Talus angle threshold — slopes steeper than this erode.
    /// Expressed as height difference per cell. Typically 0.001–0.01.
    pub talus_angle: f32,
    /// Rate of material transfer per iteration.
    pub erosion_rate: f32,
}

impl Default for ThermalErosionParams {
    fn default() -> Self {
        Self {
            iterations: 50,
            talus_angle: 0.004,
            erosion_rate: 0.5,
        }
    }
}

/// Parameters for the virtual-pipe hydraulic flow simulation.
///
/// Models rainfall, water flow, and sediment transport using the shallow-water
/// pipe formulation (Mei et al. 2007). Physically correct and deterministic —
/// no data races unlike the particle-based approach.
#[derive(Debug, Clone)]
pub struct FlowErosionParams {
    /// Number of simulation steps to run.
    pub iterations: u32,
    /// Rainfall added to every cell per second (normalised height units).
    pub rain_rate: f32,
    /// Fraction of water evaporated per second.
    pub evaporation_rate: f32,
    /// Sediment transport capacity multiplier.
    pub sediment_capacity: f32,
    /// Rate at which terrain is eroded into suspension.
    pub erosion_rate: f32,
    /// Rate at which suspended sediment settles onto terrain.
    pub deposition_rate: f32,
    /// Minimum effective tilt used in the capacity formula (avoids eroding flat areas).
    pub min_tilt: f32,
    /// Gravitational acceleration constant.
    pub gravity: f32,
    /// Simulation time step in seconds per iteration.
    pub dt: f32,
    /// Virtual pipe length — equal to the cell spacing (use 1.0 for normalised terrain).
    pub pipe_length: f32,
}

impl Default for FlowErosionParams {
    fn default() -> Self {
        Self {
            iterations: 50,
            rain_rate: 0.012,
            evaporation_rate: 0.015,
            sediment_capacity: 1.0,
            erosion_rate: 0.3,
            deposition_rate: 0.3,
            min_tilt: 0.01,
            gravity: 9.8,
            dt: 0.02,
            pipe_length: 1.0,
        }
    }
}

/// Simple PCG hash for deterministic pseudo-random numbers.
fn pcg_hash(input: u32) -> u32 {
    let state = input.wrapping_mul(747796405).wrapping_add(2891336453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277803737);
    (word >> 22) ^ word
}

/// Secondary maps produced by hydraulic erosion in addition to the eroded heightmap.
pub struct HydraulicErosionMaps {
    pub heightmap: Heightmap,
    /// Normalized flow accumulation: high where water channels ran most.
    pub flow: Heightmap,
    /// Normalized wear: high where rock was most aggressively eroded.
    pub wear: Heightmap,
    /// Normalized deposition: high where sediment settled most.
    pub deposit: Heightmap,
}

/// Simulate hydraulic erosion on a heightmap (CPU implementation).
/// Returns the eroded heightmap plus flow, wear, and deposit secondary maps.
///
/// `hardness` is an optional per-cell erosion-resistance map (0 = soft, erodes
/// fully; 1 = hard, does not erode). `None` behaves as hardness 0 everywhere,
/// reproducing the baseline droplet model exactly. The map is dimension-checked
/// against the heightmap; a mismatch is treated as no hardness map.
pub fn hydraulic_erosion(
    heightmap: &Heightmap,
    params: &HydraulicErosionParams,
    hardness: Option<&Heightmap>,
) -> Result<HydraulicErosionMaps, ErosionError> {
    let w = heightmap.width();
    let h = heightmap.height();
    let n = (w * h) as usize;
    let mut data = heightmap.data().to_vec();
    let mut flow_accum = vec![0.0f32; n];
    let mut wear_accum = vec![0.0f32; n];
    let mut deposit_accum = vec![0.0f32; n];

    let hardness_data = hardness
        .filter(|hm| hm.width() == w && hm.height() == h)
        .map(|hm| hm.data());

    let get = |data: &[f32], x: i32, y: i32| -> f32 {
        let cx = x.clamp(0, w as i32 - 1) as usize;
        let cy = y.clamp(0, h as i32 - 1) as usize;
        data[cy * w as usize + cx]
    };

    // Bilinear hardness sample at a fractional droplet position; mirrors the
    // height sampling so resistance lines up with the terrain it gates.
    let sample_hardness = |x: f32, y: f32| -> f32 {
        let Some(hd) = hardness_data else {
            return 0.0;
        };

        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - ix as f32;
        let fy = y - iy as f32;
        let s00 = get(hd, ix, iy);
        let s10 = get(hd, ix + 1, iy);
        let s01 = get(hd, ix, iy + 1);
        let s11 = get(hd, ix + 1, iy + 1);

        (s00 * (1.0 - fx) * (1.0 - fy)
            + s10 * fx * (1.0 - fy)
            + s01 * (1.0 - fx) * fy
            + s11 * fx * fy)
            .clamp(0.0, 1.0)
    };

    // Precompute erosion brush weights
    let r = params.erosion_radius as i32;
    let mut brush_offsets: Vec<(i32, i32, f32)> = Vec::new();
    let mut weight_sum = 0.0f32;
    for dy in -r..=r {
        for dx in -r..=r {
            let dist2 = (dx * dx + dy * dy) as f32;
            if dist2 <= (r * r) as f32 {
                let weight = (r as f32 - dist2.sqrt()).max(0.0);
                brush_offsets.push((dx, dy, weight));
                weight_sum += weight;
            }
        }
    }
    for entry in brush_offsets.iter_mut() {
        entry.2 /= weight_sum;
    }

    let mut rng_state = params.seed;

    for _ in 0..params.num_droplets {
        rng_state = pcg_hash(rng_state);
        let px_start = (rng_state as f32 / u32::MAX as f32) * (w - 1) as f32;
        rng_state = pcg_hash(rng_state);
        let py_start = (rng_state as f32 / u32::MAX as f32) * (h - 1) as f32;

        let mut pos_x = px_start;
        let mut pos_y = py_start;
        let mut dir_x = 0.0f32;
        let mut dir_y = 0.0f32;
        let mut speed = 1.0f32;
        let mut water = 1.0f32;
        let mut sediment = 0.0f32;

        for _ in 0..params.max_lifetime {
            let ix = pos_x.floor() as i32;
            let iy = pos_y.floor() as i32;

            if ix < 0 || ix >= w as i32 - 1 || iy < 0 || iy >= h as i32 - 1 {
                break;
            }

            let fx = pos_x - ix as f32;
            let fy = pos_y - iy as f32;

            // Bilinear height sample
            let h00 = get(&data, ix, iy);
            let h10 = get(&data, ix + 1, iy);
            let h01 = get(&data, ix, iy + 1);
            let h11 = get(&data, ix + 1, iy + 1);
            let old_height = h00 * (1.0 - fx) * (1.0 - fy)
                + h10 * fx * (1.0 - fy)
                + h01 * (1.0 - fx) * fy
                + h11 * fx * fy;

            // Gradient
            let gx = (h10 - h00) * (1.0 - fy) + (h11 - h01) * fy;
            let gy = (h01 - h00) * (1.0 - fx) + (h11 - h10) * fx;

            // Update direction with inertia
            dir_x = dir_x * params.inertia - gx * (1.0 - params.inertia);
            dir_y = dir_y * params.inertia - gy * (1.0 - params.inertia);

            let dir_len = (dir_x * dir_x + dir_y * dir_y).sqrt();
            if dir_len < 0.0001 {
                rng_state = pcg_hash(rng_state);
                let angle = (rng_state as f32 / u32::MAX as f32) * std::f32::consts::TAU;
                dir_x = angle.cos();
                dir_y = angle.sin();
            } else {
                dir_x /= dir_len;
                dir_y /= dir_len;
            }

            // Accumulate flow at current cell
            let flow_idx = iy as usize * w as usize + ix as usize;
            if flow_idx < n {
                flow_accum[flow_idx] += water;
            }

            // Move particle
            let new_x = pos_x + dir_x;
            let new_y = pos_y + dir_y;

            if new_x < 0.0 || new_x >= (w - 1) as f32 || new_y < 0.0 || new_y >= (h - 1) as f32 {
                break;
            }

            // Sample new height
            let nix = new_x.floor() as i32;
            let niy = new_y.floor() as i32;
            let nfx = new_x - nix as f32;
            let nfy = new_y - niy as f32;
            let nh00 = get(&data, nix, niy);
            let nh10 = get(&data, nix + 1, niy);
            let nh01 = get(&data, nix, niy + 1);
            let nh11 = get(&data, nix + 1, niy + 1);
            let new_height = nh00 * (1.0 - nfx) * (1.0 - nfy)
                + nh10 * nfx * (1.0 - nfy)
                + nh01 * (1.0 - nfx) * nfy
                + nh11 * nfx * nfy;

            let height_diff = new_height - old_height;

            let capacity =
                (-height_diff * speed * water * params.capacity_factor).max(params.min_capacity);

            if sediment > capacity || height_diff > 0.0 {
                // Deposit
                let deposit_amount = if height_diff > 0.0 {
                    sediment.min(height_diff)
                } else {
                    (sediment - capacity) * params.deposition_rate
                };
                sediment -= deposit_amount;

                let idx = iy as usize * w as usize + ix as usize;
                if idx < n {
                    data[idx] += deposit_amount;
                    deposit_accum[idx] += deposit_amount;
                }
            } else {
                // Erode. River incision amplifies erosion in fast, well-watered
                // droplets (channels carve deeper); hardness gates it per-cell.
                // river_depth == 0 and hardness == 0 leave erode_amount untouched.
                let mut erode_amount =
                    ((capacity - sediment) * params.erosion_rate).min(-height_diff);
                let river_gain = 1.0 + params.river_depth * speed * water;
                let resistance = 1.0 - sample_hardness(pos_x, pos_y);
                erode_amount = (erode_amount * river_gain * resistance).min(-height_diff);

                for &(dx, dy, weight) in &brush_offsets {
                    let ex = ix + dx;
                    let ey = iy + dy;
                    if ex >= 0 && ex < w as i32 && ey >= 0 && ey < h as i32 {
                        let eidx = ey as usize * w as usize + ex as usize;
                        let worn = erode_amount * weight;
                        data[eidx] -= worn;
                        wear_accum[eidx] += worn;
                    }
                }

                sediment += erode_amount;
            }

            speed = (speed * speed + height_diff * params.gravity)
                .max(0.0)
                .sqrt();
            water *= 1.0 - params.evaporation_rate;
            pos_x = new_x;
            pos_y = new_y;

            if water < 0.001 {
                break;
            }
        }
    }

    for v in data.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }

    let normalize = |buf: Vec<f32>| -> Vec<f32> {
        let max = buf.iter().cloned().fold(0.0f32, f32::max);
        if max > 1e-9 {
            buf.into_iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
        } else {
            buf
        }
    };

    let hm =
        Heightmap::frbar_data(w, h, data).map_err(|e| ErosionError::Heightmap(e.to_string()))?;
    let flow_hm = Heightmap::frbar_data(w, h, normalize(flow_accum))
        .map_err(|e| ErosionError::Heightmap(e.to_string()))?;
    let wear_hm = Heightmap::frbar_data(w, h, normalize(wear_accum))
        .map_err(|e| ErosionError::Heightmap(e.to_string()))?;
    let deposit_hm = Heightmap::frbar_data(w, h, normalize(deposit_accum))
        .map_err(|e| ErosionError::Heightmap(e.to_string()))?;

    Ok(HydraulicErosionMaps {
        heightmap: hm,
        flow: flow_hm,
        wear: wear_hm,
        deposit: deposit_hm,
    })
}

/// Differential erosion: soft rock is worn down while hard strata and steep
/// (slope-protected) faces stand proud, forming mesas, benches and stratified
/// walls -- the geological process that shapes buttes. Distinct from hydraulic
/// (droplet) erosion, which follows water flow and carves valleys.
///
/// Model: `layers` hard/soft strata band the height range (squared sine). Each
/// iteration, every cell's erodibility = `(1 - contrast * band)` (so `contrast`
/// 0 = uniform wear, 1 = only soft rock erodes) times a slope factor that
/// `slope_hardening` uses to spare steep faces (exposed bedrock). The cell is
/// lowered by that erodibility. Crucially the strata are anchored to the
/// ORIGINAL height range, so as a soft column wears down it eventually reaches a
/// harder band and stops -- that band becomes a flat shelf/cap. `strength`
/// (0..1) scales total wear; `iterations` sets how far the downcutting runs.
pub fn differential_erosion(
    heightmap: &Heightmap,
    strength: f32,
    layers: u32,
    contrast: f32,
    slope_hardening: f32,
    iterations: u32,
) -> Heightmap {
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.0 {
        return heightmap.clone();
    }
    let contrast = contrast.clamp(0.0, 1.0);
    let slope_hardening = slope_hardening.clamp(0.0, 1.0);
    let w = heightmap.width();
    let h = heightmap.height();
    let (ww, hh) = (w as i32, h as i32);
    // Strata anchored to the original range so downcutting halts at hard bands.
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in heightmap.data() {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(1e-4);
    let bands = layers.max(1) as f32;
    let iters = iterations.max(1);
    // Per-iteration wear ceiling (fraction of span) for a fully-soft, flat cell.
    let step = strength * 0.03;
    let dim = w.max(h) as f32;
    let mut cur = heightmap.data().to_vec();
    let mut next = cur.clone();
    let at = |buf: &[f32], x: i32, y: i32| -> f32 {
        buf[(y.clamp(0, hh - 1) as usize) * w as usize + x.clamp(0, ww - 1) as usize]
    };
    for _ in 0..iters {
        for y in 0..hh {
            for x in 0..ww {
                let v = at(&cur, x, y);
                let nh = ((v - lo) / span).clamp(0.0, 1.0);
                let s = 0.5 + 0.5 * (nh * bands * std::f32::consts::TAU).sin();
                let band = s * s;
                // slope in map-fraction units (resolution independent)
                let gx = (at(&cur, x + 1, y) - at(&cur, x - 1, y)) / (2.0 * span);
                let gy = (at(&cur, x, y + 1) - at(&cur, x, y - 1)) / (2.0 * span);
                let slope_norm = ((gx * gx + gy * gy).sqrt() * dim * 0.1).min(1.0);
                let erodibility = (1.0 - contrast * band) * (1.0 - slope_norm * slope_hardening);
                let lowered = v - step * erodibility.max(0.0) * span;
                next[(y * ww + x) as usize] = lowered.max(lo);
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    Heightmap::frbar_data(w, h, cur).unwrap_or_else(|_| heightmap.clone())
}

/// Differential strata terracing: the visible half of the geology model.
///
/// Hardness-gated hydraulic erosion alone produces only a faint, diffuse
/// effect (droplets follow flow, not strata). This carves the actual
/// differential landforms -- benches, mesas, stratified walls -- directly:
/// height is quantised into `layers` shelves, each with a flat tread and a
/// steep riser whose sharpness is set by `contrast` (0 = no terracing, off).
/// `strength` blends the terraced surface over the original. `contrast` sets how
/// pronounced the shelves are -- 0 = none (no terracing, the surface is left
/// alone), 1 = full flat-tread/steep-riser steps -- and is intentionally gentle
/// across the low/mid range so subtle benches are easy to dial in. The shelf
/// shape is C1-continuous (smootherstep), so low values produce soft undulation
/// rather than the hard contour-line creases a power curve leaves at every band
/// edge. `slope_hardening` suppresses terracing on already-steep faces so
/// existing cliffs/butte walls stay walls instead of being cut into stairs.
/// Resolution independent: band quantisation keys off normalized height, and the
/// slope term is normalized to map-fraction units (not raw per-cell deltas).
pub fn apply_strata_terracing(
    heightmap: &Heightmap,
    strength: f32,
    layers: u32,
    contrast: f32,
    slope_hardening: f32,
) -> Heightmap {
    let strength = strength.clamp(0.0, 1.0);
    let contrast = contrast.clamp(0.0, 1.0);
    let w = heightmap.width();
    let h = heightmap.height();
    let data = heightmap.data();
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in data {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(1e-4);
    let bands = layers.max(1) as f32;
    let (ww, hh) = (w as i32, h as i32);
    // Slope is measured in normalized-height per map-fraction so a given physical
    // steepness reads the same at any resolution: a raw per-cell delta shrinks as
    // the grid gets finer, which is why slope-hardening did nothing at full res.
    let dim = w.max(h) as f32;
    let at = |x: i32, y: i32| -> f32 {
        data[(y.clamp(0, hh - 1) as usize) * w as usize + x.clamp(0, ww - 1) as usize]
    };
    let mut out = vec![0.0f32; (w * h) as usize];
    for y in 0..hh {
        for x in 0..ww {
            let v = at(x, y);
            let nh = ((v - lo) / span).clamp(0.0, 1.0);
            let t = nh * bands;
            let k = t.floor();
            let f = (t - k).clamp(0.0, 1.0);
            // smootherstep shelf (zero slope at both band edges -> no crease),
            // blended toward linear by contrast so low contrast is genuinely soft
            let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
            let shaped = f + (s - f) * contrast;
            let terr_nh = (k + shaped) / bands;
            // preserve existing steep faces: per-cell gradient -> map-fraction units
            let gx = (at(x + 1, y) - at(x - 1, y)) / (2.0 * span);
            let gy = (at(x, y + 1) - at(x, y - 1)) / (2.0 * span);
            let slope_norm = ((gx * gx + gy * gy).sqrt() * dim * 0.1).min(1.0);
            let amt = strength * (1.0 - slope_norm * slope_hardening);
            let blended = nh + (terr_nh - nh) * amt;
            out[(y * ww + x) as usize] = blended * span + lo;
        }
    }
    Heightmap::frbar_data(w, h, out).unwrap_or_else(|_| heightmap.clone())
}

/// Simulate thermal erosion (weathering) on a heightmap (CPU implementation).
pub fn thermal_erosion(
    heightmap: &Heightmap,
    params: &ThermalErosionParams,
) -> Result<Heightmap, ErosionError> {
    let w = heightmap.width() as usize;
    let h = heightmap.height() as usize;
    let mut current = heightmap.data().to_vec();
    let mut next = current.clone();

    let offsets: [(i32, i32, f32); 8] = [
        (-1, -1, 1.414),
        (0, -1, 1.0),
        (1, -1, 1.414),
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (-1, 1, 1.414),
        (0, 1, 1.0),
        (1, 1, 1.414),
    ];

    for _ in 0..params.iterations {
        next.copy_from_slice(&current);

        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                let center = current[idx];

                // Find max height difference exceeding talus angle
                let mut max_diff = 0.0f32;
                let mut total_excess = 0.0f32;
                let mut lower_cells: Vec<(usize, f32)> = Vec::new();

                for &(dx, dy, dist) in &offsets {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                        let nidx = ny as usize * w + nx as usize;
                        let diff = (center - current[nidx]) / dist;
                        if diff > params.talus_angle {
                            let excess = diff - params.talus_angle;
                            max_diff = max_diff.max(diff);
                            total_excess += excess;
                            lower_cells.push((nidx, excess));
                        }
                    }
                }

                // Transfer material proportionally to lower neighbors
                if !lower_cells.is_empty() && max_diff > params.talus_angle {
                    let transfer = (max_diff - params.talus_angle) * params.erosion_rate * 0.5;
                    next[idx] -= transfer;

                    // Distribute transferred material to lower neighbors proportionally
                    for &(nidx, excess) in &lower_cells {
                        let proportion = excess / total_excess;
                        next[nidx] += transfer * proportion;
                    }
                }
            }
        }

        std::mem::swap(&mut current, &mut next);
    }

    // Clamp
    for v in current.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }

    Heightmap::frbar_data(heightmap.width(), heightmap.height(), current)
        .map_err(|e| ErosionError::Heightmap(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_heightmap() -> Heightmap {
        // Create a simple cone/peak for erosion testing
        let size = 64u32;
        let mut data = vec![0.0f32; (size * size) as usize];
        let center = size as f32 / 2.0;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                let h = (1.0 - dist / center).max(0.0);
                data[(y * size + x) as usize] = h;
            }
        }
        Heightmap::frbar_data(size, size, data).unwrap()
    }

    #[test]
    fn test_hydraulic_erosion_produces_lower_terrain() {
        let input = make_test_heightmap();

        let params = HydraulicErosionParams {
            num_droplets: 5000,
            max_lifetime: 20,
            erosion_radius: 2,
            ..Default::default()
        };

        let result = hydraulic_erosion(&input, &params, None).unwrap();
        let hm = &result.heightmap;
        assert_eq!(hm.width(), 64);
        assert_eq!(hm.height(), 64);

        let center = 32 * 64 + 32;
        assert!(
            hm.data()[center] < input.data()[center],
            "Peak should be eroded: {} < {}",
            hm.data()[center],
            input.data()[center]
        );

        let result_max: f32 = hm.data().iter().cloned().fold(0.0f32, f32::max);
        assert!(
            result_max > 0.3,
            "Erosion should not flatten terrain entirely: max={result_max}"
        );

        // Secondary maps should be non-trivial
        let flow_max: f32 = result.flow.data().iter().cloned().fold(0.0f32, f32::max);
        assert!(
            flow_max > 0.5,
            "Flow map should have significant values: max={flow_max}"
        );
        let wear_max: f32 = result.wear.data().iter().cloned().fold(0.0f32, f32::max);
        assert!(
            wear_max > 0.5,
            "Wear map should have significant values: max={wear_max}"
        );
    }

    #[test]
    fn test_thermal_erosion_reduces_slopes() {
        let input = make_test_heightmap();

        let params = ThermalErosionParams {
            iterations: 20,
            talus_angle: 0.02,
            erosion_rate: 0.5,
        };

        let result = thermal_erosion(&input, &params).unwrap();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 64);

        // After thermal erosion, maximum slope should be reduced
        let max_slope_before = compute_max_slope(input.data(), 64);
        let max_slope_after = compute_max_slope(result.data(), 64);
        assert!(
            max_slope_after < max_slope_before,
            "Max slope should decrease: before={max_slope_before}, after={max_slope_after}"
        );
    }

    #[test]
    fn test_erosion_output_in_range() {
        let input = make_test_heightmap();

        let h_result = hydraulic_erosion(&input, &HydraulicErosionParams::default(), None).unwrap();
        for &v in h_result.heightmap.data() {
            assert!((0.0..=1.0).contains(&v), "Value out of range: {v}");
        }
        for &v in h_result.flow.data() {
            assert!((0.0..=1.0).contains(&v), "Flow value out of range: {v}");
        }
        for &v in h_result.wear.data() {
            assert!((0.0..=1.0).contains(&v), "Wear value out of range: {v}");
        }
        for &v in h_result.deposit.data() {
            assert!((0.0..=1.0).contains(&v), "Deposit value out of range: {v}");
        }

        let t_result = thermal_erosion(&input, &ThermalErosionParams::default()).unwrap();
        for &v in t_result.data() {
            assert!((0.0..=1.0).contains(&v), "Value out of range: {v}");
        }
    }

    fn compute_max_slope(data: &[f32], width: u32) -> f32 {
        let w = width as usize;
        let h = data.len() / w;
        let mut max_slope = 0.0f32;
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let idx = y * w + x;
                let dx = (data[idx + 1] - data[idx]).abs();
                let dy = (data[idx + w] - data[idx]).abs();
                max_slope = max_slope.max(dx).max(dy);
            }
        }
        max_slope
    }

    fn uniform_hardness(input: &Heightmap, value: f32) -> Heightmap {
        let n = (input.width() * input.height()) as usize;
        Heightmap::frbar_data(input.width(), input.height(), vec![value; n]).unwrap()
    }

    /// Total material removed: sum of positive (input - eroded) deltas.
    fn total_eroded(input: &Heightmap, eroded: &Heightmap) -> f32 {
        input
            .data()
            .iter()
            .zip(eroded.data())
            .map(|(a, b)| (a - b).max(0.0))
            .sum()
    }

    fn erosion_params() -> HydraulicErosionParams {
        HydraulicErosionParams {
            num_droplets: 5000,
            max_lifetime: 20,
            erosion_radius: 2,
            seed: 42,
            ..Default::default()
        }
    }

    /// river_depth == 0 with no hardness map must reproduce the baseline droplet
    /// model bit-for-bit -- this is the regression guard for the new params.
    #[test]
    fn test_default_params_reproduce_baseline() {
        let input = make_test_heightmap();
        let params = erosion_params();

        let baseline = hydraulic_erosion(&input, &params, None).unwrap();

        // An all-zero hardness map must be identical to no map.
        let zero_hardness = uniform_hardness(&input, 0.0);
        let with_zero = hydraulic_erosion(&input, &params, Some(&zero_hardness)).unwrap();

        assert_eq!(
            baseline.heightmap.data(),
            with_zero.heightmap.data(),
            "zero hardness map must match no map exactly"
        );
        assert_eq!(baseline.wear.data(), with_zero.wear.data());
        assert_eq!(baseline.flow.data(), with_zero.flow.data());
        assert_eq!(baseline.deposit.data(), with_zero.deposit.data());
    }

    /// An all-hard map (1.0) blocks erosion; total material removed must be far
    /// below an all-soft map (0.0).
    #[test]
    fn test_hardness_reduces_erosion() {
        let input = make_test_heightmap();
        let params = erosion_params();

        let soft = uniform_hardness(&input, 0.0);
        let hard = uniform_hardness(&input, 1.0);

        let soft_res = hydraulic_erosion(&input, &params, Some(&soft)).unwrap();
        let hard_res = hydraulic_erosion(&input, &params, Some(&hard)).unwrap();

        let soft_eroded = total_eroded(&input, &soft_res.heightmap);
        let hard_eroded = total_eroded(&input, &hard_res.heightmap);

        assert!(
            hard_eroded < soft_eroded,
            "hard rock should erode less: hard={hard_eroded} soft={soft_eroded}"
        );
    }

    /// Positive river_depth amplifies channel incision -- total erosion must
    /// exceed the river_depth == 0 baseline on the same seed.
    #[test]
    fn test_river_depth_increases_erosion() {
        let input = make_test_heightmap();
        let base_params = erosion_params();
        let deep_params = HydraulicErosionParams {
            river_depth: 1.0,
            ..erosion_params()
        };

        let base = hydraulic_erosion(&input, &base_params, None).unwrap();
        let deep = hydraulic_erosion(&input, &deep_params, None).unwrap();

        let base_eroded = total_eroded(&input, &base.heightmap);
        let deep_eroded = total_eroded(&input, &deep.heightmap);

        assert!(
            deep_eroded > base_eroded,
            "river_depth should deepen channels: deep={deep_eroded} base={base_eroded}"
        );
    }
}
