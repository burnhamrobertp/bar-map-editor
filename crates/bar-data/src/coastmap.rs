//! Coastmap baking: derive a distance-to-coast field from a heightmap
//! for use by the shore-foam shader stage in `BumpWaterFS.glsl`.
//!
//! BAR's engine builds the coastmap via a multi-pass
//! `BumpWaterCoastBlurFS.glsl` shader -- a chain of 8 iterations of a
//! growing maximum-distance kernel followed by smoothing passes
//! (`bar-recoil/cont/.../shaders/GLSL/BumpWaterCoastBlurFS.glsl`).
//! For BME's static preview we can do the same job CPU-side once per
//! heightmap update with a chamfer 3-4 distance transform -- standard
//! O(N) algorithm whose output is functionally equivalent at preview
//! quality.
//!
//! Output texture layout (matches what `water.wgsl`'s `GetShorewaves`
//! port reads):
//!
//! - R: refined coast intensity in `[0, 1]`. 1 = at the shoreline;
//!   0 = a foam-band's-worth offshore. Polarity matches engine's
//!   `coast.g`: `BumpWaterFS:204-206` uses `f * 1.4 - coastdist`
//!   then `1 - f * InvWavesLength`, which produces visible foam
//!   only when `coastdist` is high (i.e. near the shore). If you
//!   flip the polarity, the foam intensity saturates across the
//!   open ocean instead -- this was a Phase 6 regression we fixed
//!   by aligning with the engine's encoding.
//! - G: raw coast intensity, same polarity. Engine's `coast.r`. Used
//!   by the cliff-foam term via `pow(coast.r, 3)`.
//! - B: invwaterdepth in `[0, 1]`. 1 = at or above water plane;
//!   0 = at or below the deep-water threshold. Engine reads this in
//!   `GetWaterHeight` to gate foam off where the seabed is too deep.
//! - A: unused (1.0 baked in for clarity).
//!
//! Distance is normalised over `COAST_DISTANCE_TEXELS` so the foam
//! band scales with map resolution rather than absolute heightmap
//! texel count -- 32 texels is the engine-default-equivalent foam
//! reach.

/// Texel-radius of the foam band. Distances beyond this saturate to
/// 1 (full open water). Engine effectively does the same via its
/// `WaveLength` / `WaveOffsetFactor` falloff -- 32 is a reasonable
/// approximation that produces visible foam without a distance ramp
/// that runs across the entire map.
pub const COAST_DISTANCE_TEXELS: f32 = 32.0;

/// Texel-elmo depth at which `invwaterdepth` collapses to 0
/// (fully-deep). Engine packs depth into a normalised
/// [0, 1] range; 30 elmos lines up with the existing
/// `water_shallow_scale` 33-elmo gate the renderer already uses
/// elsewhere.
pub const FULL_DEPTH_ELMOS: f32 = 30.0;

/// Bake a coastmap from a heightmap.
///
/// Returns the RGBA8 buffer plus dimensions (which equal the
/// heightmap dimensions). `heightmap` is row-major `width * height`
/// floats in **elmos** (vertical render space); `water_y` is the
/// water-plane elevation in the same units (0 for typical
/// at-sea-level maps).
pub fn bake_coastmap(heightmap: &[f32], width: u32, height: u32, water_y: f32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(heightmap.len(), w * h, "heightmap length mismatch");

    // Two-pass chamfer 3-4 distance transform. Land texels start at
    // 0 (zero distance to coast), water texels at infinity. Forward
    // + backward passes propagate cardinal (cost 1) and diagonal
    // (cost ~1.414, approximated as `sqrt(2)`) min-distances.
    let inf = f32::MAX / 4.0;
    let mut dist: Vec<f32> = heightmap
        .iter()
        .map(|&z| if z > water_y { 0.0 } else { inf })
        .collect();

    let diag: f32 = std::f32::consts::SQRT_2;
    let cardinal: f32 = 1.0;

    // Forward pass: top-to-bottom, left-to-right.
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut d = dist[i];
            if x > 0 {
                d = d.min(dist[i - 1] + cardinal);
            }
            if y > 0 {
                d = d.min(dist[i - w] + cardinal);
                if x > 0 {
                    d = d.min(dist[i - w - 1] + diag);
                }
                if x + 1 < w {
                    d = d.min(dist[i - w + 1] + diag);
                }
            }
            dist[i] = d;
        }
    }
    // Backward pass: bottom-to-top, right-to-left.
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut d = dist[i];
            if x + 1 < w {
                d = d.min(dist[i + 1] + cardinal);
            }
            if y + 1 < h {
                d = d.min(dist[i + w] + cardinal);
                if x + 1 < w {
                    d = d.min(dist[i + w + 1] + diag);
                }
                if x > 0 {
                    d = d.min(dist[i + w - 1] + diag);
                }
            }
            dist[i] = d;
        }
    }

    // Pack to RGBA8.
    let mut out = vec![0u8; w * h * 4];
    for (i, &z) in heightmap.iter().enumerate() {
        let d = dist[i];
        // Normalised distance d_norm: 0 at the shoreline, 1 a
        // foam-band-radius offshore. We invert this below to match
        // engine `coast.g` polarity (high near shore, falls to 0 in
        // open ocean).
        let d_norm = (d / COAST_DISTANCE_TEXELS).clamp(0.0, 1.0);
        // Refined coast intensity (R, engine's coast.g): 1 at the
        // shoreline, 0 a band offshore. Smoothstep gives a soft
        // foam-band roll-off rather than a hard linear falloff.
        let shore = 1.0 - d_norm;
        let refined = shore * shore * (3.0 - 2.0 * shore);
        // Raw coast intensity (G, engine's coast.r): same polarity,
        // linear. Cubed by the cliff-foam term so the falloff
        // becomes very sharp.
        let raw = shore;
        // invwaterdepth (B): 1 at the shoreline / above water,
        // decreases as the depth grows. 0 means deep enough that
        // foam should not appear.
        let depth = (water_y - z).max(0.0);
        let invwaterdepth = (1.0 - (depth / FULL_DEPTH_ELMOS).clamp(0.0, 1.0)).clamp(0.0, 1.0);

        let o = i * 4;
        out[o] = (refined * 255.0) as u8;
        out[o + 1] = (raw * 255.0) as u8;
        out[o + 2] = (invwaterdepth * 255.0) as u8;
        out[o + 3] = 255;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Land / shoreline texels get peak coast intensity (R == 255);
    /// open-ocean texels past the foam band collapse to 0. Polarity
    /// matches engine `coast.g`.
    #[test]
    fn shore_intensity_peaks_on_land() {
        let size = (COAST_DISTANCE_TEXELS as usize) * 2 + 4;
        let hm: Vec<f32> = (0..(size * size))
            .map(|i| if i == 0 { 10.0 } else { -1.0 })
            .collect();
        let out = bake_coastmap(&hm, size as u32, size as u32, 0.0);
        // (0, 0) is on land -- R saturates to 255 (peak shore foam).
        assert_eq!(out[0], 255);
        // Far corner is past the foam band -- R == 0 (no foam).
        let far = (size - 1) * size + (size - 1);
        assert_eq!(out[far * 4], 0);
    }

    /// invwaterdepth is 1 on land and decreases with depth below the
    /// water plane.
    #[test]
    fn invwaterdepth_drops_with_depth() {
        let mut hm = vec![0.0f32; 4 * 4];
        hm[0] = 10.0; // land
        hm[1] = -1.0; // shallow
        hm[2] = -15.0; // medium
        hm[3] = -100.0; // deep
        let out = bake_coastmap(&hm, 4, 4, 0.0);
        // B channel of texels 0..3:
        let b0 = out[2];
        let b1 = out[4 + 2];
        let b2 = out[2 * 4 + 2];
        let b3 = out[3 * 4 + 2];
        assert_eq!(b0, 255);
        assert!(b1 > b2);
        assert!(b2 > b3);
        assert_eq!(b3, 0);
    }

    /// Diagonal-only land arrangement: shore intensity DROPS faster
    /// for the cardinal-step neighbour (closer to land) than for the
    /// diagonal-step neighbour (farther from land). Polarity-flipped
    /// from the raw chamfer distance.
    #[test]
    fn chamfer_metric_diagonal() {
        let mut hm = vec![-1.0f32; 4 * 4];
        hm[5] = 10.0; // land at (1, 1)
        let out = bake_coastmap(&hm, 4, 4, 0.0);
        // (0, 0): one diagonal step from land (~1.414 / 32 norm).
        let r_00 = out[0] as f32 / 255.0;
        // (2, 1): one cardinal step from land (1 / 32 norm) -- closer,
        // so shore intensity should be HIGHER.
        let r_21 = out[(4 + 2) * 4] as f32 / 255.0;
        assert!(
            r_21 > r_00,
            "cardinal neighbour {r_21} should have higher shore intensity than diagonal {r_00}",
        );
    }
}
