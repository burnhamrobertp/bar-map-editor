//! Noise generation for terrain heightmaps.
//!
//! Provides both GPU (WGSL compute shader) and CPU (via `noise` crate) implementations.

use noise::NoiseFn;
use bar_data::Heightmap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NoiseError {
    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("compute error: {0}")]
    Compute(String),
}

/// Parameters for noise generation.
#[derive(Debug, Clone)]
pub struct NoiseParams {
    /// Output width in pixels
    pub width: u32,
    /// Output height in pixels
    pub height: u32,
    /// Noise type
    pub noise_type: NoiseType,
    /// Number of octaves for fractal noise
    pub octaves: u32,
    /// Frequency multiplier per octave
    pub lacunarity: f32,
    /// Amplitude multiplier per octave
    pub persistence: f32,
    /// Base frequency
    pub frequency: f32,
    /// Random seed
    pub seed: u32,
    /// X offset for tiling/panning
    pub offset_x: f32,
    /// Y offset for tiling/panning
    pub offset_y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseType {
    Perlin,
    Simplex,
    Worley,
    Ridged,
    Billow,
}

impl Default for NoiseParams {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            noise_type: NoiseType::Perlin,
            octaves: 6,
            lacunarity: 2.0,
            persistence: 0.5,
            frequency: 1.0,
            seed: 0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

/// CPU-based noise generation (fallback when GPU unavailable).
pub fn generate_noise_cpu(params: &NoiseParams) -> Result<Heightmap, NoiseError> {
    use noise::{Perlin, Simplex};

    let size = (params.width as usize) * (params.height as usize);
    let mut data = vec![0.0f32; size];

    match params.noise_type {
        NoiseType::Perlin => {
            let noise_fn = Perlin::new(params.seed);
            fill_fbm(&noise_fn, params, &mut data);
        }
        NoiseType::Simplex => {
            let noise_fn = Simplex::new(params.seed);
            fill_fbm(&noise_fn, params, &mut data);
        }
        NoiseType::Ridged => {
            let noise_fn = Perlin::new(params.seed);
            fill_ridged(&noise_fn, params, &mut data);
        }
        NoiseType::Billow => {
            let noise_fn = Perlin::new(params.seed);
            fill_billow(&noise_fn, params, &mut data);
        }
        NoiseType::Worley => {
            fill_worley(params, &mut data);
        }
    }

    Heightmap::frbar_data(params.width, params.height, data)
        .map_err(|e| NoiseError::Compute(e.to_string()))
}

fn fill_fbm(noise_fn: &dyn NoiseFn<f64, 2>, params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;

    for y in 0..h {
        for x in 0..w {
            let mut amplitude = 1.0f64;
            let mut frequency = params.frequency as f64;
            let mut value = 0.0f64;
            let mut max_amplitude = 0.0f64;

            for _ in 0..params.octaves {
                let nx = (x as f64 / w as f64 + params.offset_x as f64) * frequency;
                let ny = (y as f64 / h as f64 + params.offset_y as f64) * frequency;

                value += noise_fn.get([nx, ny]) * amplitude;
                max_amplitude += amplitude;
                amplitude *= params.persistence as f64;
                frequency *= params.lacunarity as f64;
            }

            // Normalize to [0, 1]
            let normalized = (value / max_amplitude + 1.0) * 0.5;
            data[y * w + x] = normalized.clamp(0.0, 1.0) as f32;
        }
    }
}

/// Ridged multi-fractal noise — produces sharp ridge lines where the noise
/// signal crosses zero. Creates angular, mountain-like terrain features.
fn fill_ridged(noise_fn: &dyn NoiseFn<f64, 2>, params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;
    let offset = 1.0f64;
    let gain = 2.0f64;

    for y in 0..h {
        for x in 0..w {
            let mut frequency = params.frequency as f64;
            let mut amplitude = 1.0f64;
            let mut value = 0.0f64;
            let mut weight = 1.0f64;

            for _ in 0..params.octaves {
                let nx = (x as f64 / w as f64 + params.offset_x as f64) * frequency;
                let ny = (y as f64 / h as f64 + params.offset_y as f64) * frequency;

                // Get noise signal, create ridge by inverting absolute value
                let signal = noise_fn.get([nx, ny]);
                let signal = offset - signal.abs();
                // Square to sharpen ridges
                let signal = signal * signal;

                // Weight by previous octave's contribution (signal-dependent)
                let signal = signal * weight;
                value += signal * amplitude;

                // Next octave weight is controlled by current signal strength
                weight = (signal * gain).clamp(0.0, 1.0);
                frequency *= params.lacunarity as f64;
                amplitude *= params.persistence as f64;
            }

            // Ridged noise naturally falls in ~[0, 1] range but can exceed
            data[y * w + x] = (value * 0.5).clamp(0.0, 1.0) as f32;
        }
    }
}

/// Billow noise — takes absolute value of FBM noise to produce puffy,
/// cloud-like or rolling-hill terrain.
fn fill_billow(noise_fn: &dyn NoiseFn<f64, 2>, params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;

    for y in 0..h {
        for x in 0..w {
            let mut amplitude = 1.0f64;
            let mut frequency = params.frequency as f64;
            let mut value = 0.0f64;
            let mut max_amplitude = 0.0f64;

            for _ in 0..params.octaves {
                let nx = (x as f64 / w as f64 + params.offset_x as f64) * frequency;
                let ny = (y as f64 / h as f64 + params.offset_y as f64) * frequency;

                // Billow: take absolute value to create puffy shapes
                let signal = noise_fn.get([nx, ny]).abs();
                value += signal * amplitude;
                max_amplitude += amplitude;
                amplitude *= params.persistence as f64;
                frequency *= params.lacunarity as f64;
            }

            let normalized = value / max_amplitude;
            data[y * w + x] = normalized.clamp(0.0, 1.0) as f32;
        }
    }
}

/// Simple cellular/Worley noise — distance to nearest point in a grid.
/// Produces cell-like patterns useful for rock textures, cracked terrain, etc.
fn fill_worley(params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;

    // Use a simple PCG-based point scatter for deterministic Worley
    let cell_count = (params.frequency as usize).max(2);
    let num_points = cell_count * cell_count;
    let mut points = Vec::with_capacity(num_points);

    // Generate random feature points using PCG hash
    let mut rng_state = params.seed as u64;
    for _ in 0..num_points {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let px = ((rng_state >> 33) as f64) / (u32::MAX as f64);
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let py = ((rng_state >> 33) as f64) / (u32::MAX as f64);
        points.push((px, py));
    }

    for y in 0..h {
        for x in 0..w {
            let px = x as f64 / w as f64;
            let py = y as f64 / h as f64;

            let mut min_dist = f64::MAX;
            for &(fx, fy) in &points {
                let dx = px - fx;
                let dy = py - fy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < min_dist {
                    min_dist = dist;
                }
            }

            // Normalize: max possible distance is ~0.7 (diagonal of unit square)
            let normalized = (min_dist * cell_count as f64 * 0.7).clamp(0.0, 1.0);
            data[y * w + x] = normalized as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_noise_params() {
        let params = NoiseParams::default();
        assert_eq!(params.width, 512);
        assert_eq!(params.octaves, 6);
    }

    #[test]
    fn test_cpu_noise_generation() {
        let params = NoiseParams {
            width: 64,
            height: 64,
            ..Default::default()
        };
        let hm = generate_noise_cpu(&params).unwrap();
        assert_eq!(hm.width(), 64);
        assert_eq!(hm.height(), 64);

        // All values should be in [0, 1]
        assert!(hm.data().iter().all(|&v| (0.0..=1.0).contains(&v)));

        // Should not be all zeros (noise has variance)
        assert!(hm.data().iter().any(|&v| v > 0.01));
    }

    #[test]
    fn test_deterministic_seed() {
        let params = NoiseParams {
            width: 32,
            height: 32,
            seed: 42,
            ..Default::default()
        };
        let hm1 = generate_noise_cpu(&params).unwrap();
        let hm2 = generate_noise_cpu(&params).unwrap();
        assert_eq!(hm1.data(), hm2.data());
    }
}
