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
}

impl Default for HydraulicErosionParams {
    fn default() -> Self {
        Self {
            num_droplets: 50_000,
            inertia: 0.05,
            capacity_factor: 4.0,
            min_capacity: 0.01,
            deposition_rate: 0.3,
            erosion_rate: 0.3,
            evaporation_rate: 0.01,
            gravity: 4.0,
            max_lifetime: 30,
            erosion_radius: 3,
            seed: 0,
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

/// Simulate hydraulic erosion on a heightmap (CPU implementation).
pub fn hydraulic_erosion(
    heightmap: &Heightmap,
    params: &HydraulicErosionParams,
) -> Result<Heightmap, ErosionError> {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut data = heightmap.data().to_vec();

    let get = |data: &[f32], x: i32, y: i32| -> f32 {
        let cx = x.clamp(0, w as i32 - 1) as usize;
        let cy = y.clamp(0, h as i32 - 1) as usize;
        data[cy * w as usize + cx]
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
    // Normalize weights
    for entry in brush_offsets.iter_mut() {
        entry.2 /= weight_sum;
    }

    let mut rng_state = params.seed;

    for _ in 0..params.num_droplets {
        // Random starting position
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
                // Random direction
                rng_state = pcg_hash(rng_state);
                let angle = (rng_state as f32 / u32::MAX as f32) * std::f32::consts::TAU;
                dir_x = angle.cos();
                dir_y = angle.sin();
            } else {
                dir_x /= dir_len;
                dir_y /= dir_len;
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

            // Sediment capacity
            let capacity =
                (-height_diff * speed * water * params.capacity_factor).max(params.min_capacity);

            if sediment > capacity || height_diff > 0.0 {
                // Deposit
                let deposit = if height_diff > 0.0 {
                    sediment.min(height_diff)
                } else {
                    (sediment - capacity) * params.deposition_rate
                };
                sediment -= deposit;

                // Deposit at cell
                let idx = iy as usize * w as usize + ix as usize;
                if idx < data.len() {
                    data[idx] += deposit;
                }
            } else {
                // Erode
                let erode_amount = ((capacity - sediment) * params.erosion_rate).min(-height_diff);

                // Apply erosion with brush
                for &(dx, dy, weight) in &brush_offsets {
                    let ex = ix + dx;
                    let ey = iy + dy;
                    if ex >= 0 && ex < w as i32 && ey >= 0 && ey < h as i32 {
                        let eidx = ey as usize * w as usize + ex as usize;
                        data[eidx] -= erode_amount * weight;
                    }
                }

                sediment += erode_amount;
            }

            // Update physics
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

    // Clamp output to [0, 1]
    for v in data.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }

    Heightmap::frbar_data(w, h, data).map_err(|e| ErosionError::Heightmap(e.to_string()))
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

        let result = hydraulic_erosion(&input, &params).unwrap();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 64);

        // Peak should be lower than original (erosion carves the top)
        let center = 32 * 64 + 32;
        assert!(
            result.data()[center] < input.data()[center],
            "Peak should be eroded: {} < {}",
            result.data()[center],
            input.data()[center]
        );

        // Terrain should not be completely destroyed — still has some height
        let result_max: f32 = result.data().iter().cloned().fold(0.0f32, f32::max);
        assert!(
            result_max > 0.3,
            "Erosion should not flatten terrain entirely: max={result_max}"
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

        let h_result = hydraulic_erosion(&input, &HydraulicErosionParams::default()).unwrap();
        for &v in h_result.data() {
            assert!((0.0..=1.0).contains(&v), "Value out of range: {v}");
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
}
