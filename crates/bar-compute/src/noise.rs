//! Noise generation for terrain heightmaps.
//!
//! Provides both GPU (WGSL compute shader) and CPU (via `noise` crate) implementations.

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
    /// Contrast/sharpness about the midpoint (0..1, 0.5 = no-op).
    pub steepness: f32,
    /// Output bias (0..1, 0.5 = no-op).
    pub elevation: f32,
    /// Additive output offset (0.0 = no-op).
    pub offset: f32,
    /// Output contrast about 0.5 (0..1, 0.5 = no-op).
    pub gain: f32,
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
            steepness: 0.5,
            elevation: 0.5,
            offset: 0.0,
            gain: 0.5,
        }
    }
}

/// Apply WM-style output shaping to a normalized noise value.
///
/// All four params are an exact identity at their defaults
/// (steepness=0.5, elevation=0.5, offset=0.0, gain=0.5), so a
/// fully default `NoiseParams` reproduces the pre-shaping output bit-for-bit.
pub(crate) fn shape(v: f32, p: &NoiseParams) -> f32 {
    // steepness: blend toward a smoothstep (sharpen) or its inverse (soften).
    // t in [-1, 1]; t=0 (steepness=0.5) contributes zero blend -> identity.
    let t = (p.steepness - 0.5) * 2.0;
    let smooth = v * v * (3.0 - 2.0 * v);
    let inv = 0.5 + (v - smooth) + (v - 0.5);
    let target = if t >= 0.0 { smooth } else { inv };
    let mut out = v + (target - v) * t.abs();

    // gain: contrast about 0.5. g maps 0.5 -> 1.0 -> identity.
    let g = p.gain * 2.0;
    out = 0.5 + (out - 0.5) * g;

    // elevation: additive bias. 0.5 -> +0 -> identity.
    out += (p.elevation - 0.5) * 2.0;

    // offset: raw additive. 0.0 -> identity.
    out += p.offset;

    out.clamp(0.0, 1.0)
}

/// CPU-based noise generation (fallback when GPU unavailable).
pub fn generate_noise_cpu(params: &NoiseParams) -> Result<Heightmap, NoiseError> {
    let size = (params.width as usize) * (params.height as usize);
    let mut data = vec![0.0f32; size];

    match params.noise_type {
        // Perlin and Simplex share the unified gradient-noise FBM (matching the
        // single GPU FBM path); the historical distinction is dropped.
        NoiseType::Perlin | NoiseType::Simplex => fill_fbm(params, &mut data),
        NoiseType::Ridged => fill_ridged(params, &mut data),
        NoiseType::Billow => fill_billow(params, &mut data),
        NoiseType::Worley => fill_worley(params, &mut data),
    }

    for v in data.iter_mut() {
        *v = shape(*v, params);
    }

    Heightmap::frbar_data(params.width, params.height, data)
        .map_err(|e| NoiseError::Compute(e.to_string()))
}

/// 8-direction gradient table for the unified gradient noise.
const GRADS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (0.707_106_77, 0.707_106_77),
    (-0.707_106_77, 0.707_106_77),
    (0.707_106_77, -0.707_106_77),
    (-0.707_106_77, -0.707_106_77),
];

/// PCG-style integer lattice hash. Identical to `hash_cell` in noise_fbm.wgsl.
/// PCG is the standard portable WGSL integer hash, so the GPU reproduces this
/// CPU result bit-for-bit and both paths select the same gradients.
fn hash_cell(ix: i32, iy: i32, seed: u32) -> u32 {
    let mut n = (ix as u32)
        .wrapping_mul(1_597_334_677)
        .wrapping_add((iy as u32).wrapping_mul(3_812_015_801))
        .wrapping_add(seed.wrapping_mul(2_654_435_761));
    n = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((n >> ((n >> 28).wrapping_add(4))) ^ n).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Unified gradient (Perlin-style) noise in ~[-1, 1]. MUST stay identical to
/// `gnoise` in noise_fbm.wgsl -- that parity is what makes the GPU editor and
/// the CPU export agree.
pub(crate) fn gnoise(px: f32, py: f32, seed: u32) -> f32 {
    let x0 = px.floor();
    let y0 = py.floor();
    let (ix, iy) = (x0 as i32, y0 as i32);
    let (fx, fy) = (px - x0, py - y0);

    let g = |cx: i32, cy: i32| GRADS[(hash_cell(cx, cy, seed) & 7) as usize];
    let (g00, g10, g01, g11) = (g(ix, iy), g(ix + 1, iy), g(ix, iy + 1), g(ix + 1, iy + 1));

    let d00 = g00.0 * fx + g00.1 * fy;
    let d10 = g10.0 * (fx - 1.0) + g10.1 * fy;
    let d01 = g01.0 * fx + g01.1 * (fy - 1.0);
    let d11 = g11.0 * (fx - 1.0) + g11.1 * (fy - 1.0);

    let (u, v) = (quintic(fx), quintic(fy));
    let x0m = d00 + u * (d10 - d00);
    let x1m = d01 + u * (d11 - d01);
    (x0m + v * (x1m - x0m)) * std::f32::consts::SQRT_2
}

fn fill_fbm(params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;

    for y in 0..h {
        for x in 0..w {
            let bx = x as f32 / w as f32 + params.offset_x;
            let by = y as f32 / h as f32 + params.offset_y;
            let mut amplitude = 1.0f32;
            let mut frequency = params.frequency;
            let mut value = 0.0f32;
            let mut max_amplitude = 0.0f32;

            for _ in 0..params.octaves {
                value += gnoise(bx * frequency, by * frequency, params.seed) * amplitude;
                max_amplitude += amplitude;
                amplitude *= params.persistence;
                frequency *= params.lacunarity;
            }

            let normalized = (value / max_amplitude + 1.0) * 0.5;
            data[y * w + x] = normalized.clamp(0.0, 1.0);
        }
    }
}

/// Ridged multi-fractal noise — produces sharp ridge lines where the noise
/// signal crosses zero. Creates angular, mountain-like terrain features.
fn fill_ridged(params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;
    let offset = 1.0f32;
    let gain = 2.0f32;

    for y in 0..h {
        for x in 0..w {
            let bx = x as f32 / w as f32 + params.offset_x;
            let by = y as f32 / h as f32 + params.offset_y;
            let mut frequency = params.frequency;
            let mut amplitude = 1.0f32;
            let mut value = 0.0f32;
            let mut weight = 1.0f32;

            for _ in 0..params.octaves {
                // Invert absolute value to ridge, square to sharpen, weight by
                // the previous octave's signal strength.
                let signal = offset - gnoise(bx * frequency, by * frequency, params.seed).abs();
                let signal = signal * signal * weight;
                value += signal * amplitude;
                weight = (signal * gain).clamp(0.0, 1.0);
                frequency *= params.lacunarity;
                amplitude *= params.persistence;
            }

            data[y * w + x] = (value * 0.5).clamp(0.0, 1.0);
        }
    }
}

/// Billow noise — takes absolute value of FBM noise to produce puffy,
/// cloud-like or rolling-hill terrain.
fn fill_billow(params: &NoiseParams, data: &mut [f32]) {
    let w = params.width as usize;
    let h = params.height as usize;

    for y in 0..h {
        for x in 0..w {
            let bx = x as f32 / w as f32 + params.offset_x;
            let by = y as f32 / h as f32 + params.offset_y;
            let mut amplitude = 1.0f32;
            let mut frequency = params.frequency;
            let mut value = 0.0f32;
            let mut max_amplitude = 0.0f32;

            for _ in 0..params.octaves {
                value += gnoise(bx * frequency, by * frequency, params.seed).abs() * amplitude;
                max_amplitude += amplitude;
                amplitude *= params.persistence;
                frequency *= params.lacunarity;
            }

            data[y * w + x] = (value / max_amplitude).clamp(0.0, 1.0);
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
        rng_state = rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let px = ((rng_state >> 33) as f64) / (u32::MAX as f64);
        rng_state = rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
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

    fn mean(d: &[f32]) -> f32 {
        d.iter().sum::<f32>() / d.len() as f32
    }

    fn variance(d: &[f32]) -> f32 {
        let m = mean(d);
        d.iter().map(|&v| (v - m) * (v - m)).sum::<f32>() / d.len() as f32
    }

    #[test]
    fn shape_default_is_identity() {
        let p = NoiseParams::default();
        // Sweep the full normalized range; defaults must reproduce input exactly.
        for i in 0..=1000 {
            let v = i as f32 / 1000.0;
            let out = shape(v, &p);
            assert!(
                (out - v).abs() < 1e-5,
                "default shaping not identity at v={v}: got {out}"
            );
        }
    }

    #[test]
    fn elevation_above_half_raises_mean() {
        let base = NoiseParams {
            width: 48,
            height: 48,
            seed: 7,
            ..Default::default()
        };
        let raised = NoiseParams {
            elevation: 0.7,
            ..base.clone()
        };

        let m_base = mean(generate_noise_cpu(&base).unwrap().data());
        let m_raised = mean(generate_noise_cpu(&raised).unwrap().data());

        assert!(
            m_raised > m_base,
            "elevation>0.5 should raise mean: {m_base} -> {m_raised}"
        );
    }

    #[test]
    fn gain_above_half_increases_variance() {
        let base = NoiseParams {
            width: 48,
            height: 48,
            seed: 7,
            ..Default::default()
        };
        let punchy = NoiseParams {
            gain: 0.8,
            ..base.clone()
        };

        let v_base = variance(generate_noise_cpu(&base).unwrap().data());
        let v_punchy = variance(generate_noise_cpu(&punchy).unwrap().data());

        assert!(
            v_punchy > v_base,
            "gain>0.5 should increase variance: {v_base} -> {v_punchy}"
        );
    }
}
