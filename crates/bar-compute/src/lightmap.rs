//! Lightmap bake: horizon-based ambient occlusion + sun shadowing from a
//! heightfield, packed into an RGBA color buffer.
//!
//! Heights are normalized [0,1]; XY is treated in pixel units. A normalized
//! unit of height maps to `max(width, height)` pixels of world rise so horizon
//! and shadow ray angles are geometrically meaningful regardless of map size.
//!
//! Output channels: R = AO, G = sun visibility, B = AO*sun, A = 1.0.
//! Both this CPU path and the WGSL kernel in `shaders/lightmap.wgsl` must
//! implement the same math.

use bar_data::{ColorBuffer, Heightmap};
use std::f32::consts::TAU;

/// Lightmap bake parameters. `sun_dir` points from the surface toward the sun.
#[derive(Debug, Clone)]
pub struct LightmapParams {
    pub width: u32,
    pub height: u32,
    /// Scales the AO darkening (0 = no AO, 1 = full).
    pub ao_strength: f32,
    /// AO sampling radius as a fraction (0..1) of the map's larger dimension.
    pub ao_radius: f32,
    /// Number of azimuth directions sampled for horizon AO.
    pub num_directions: u32,
    /// Max steps marched per direction / per shadow ray.
    pub max_steps: u32,
    /// Unit vector from surface toward the sun. +Z is up.
    pub sun_dir: [f32; 3],
    /// Penumbra softness (0 = hard shadow, 1 = very soft).
    pub sun_softness: f32,
}

impl Default for LightmapParams {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            ao_strength: 1.0,
            ao_radius: 0.1,
            num_directions: 16,
            max_steps: 24,
            // azimuth 315 deg, elevation 45 deg
            sun_dir: [0.5, 0.5, std::f32::consts::FRAC_1_SQRT_2],
            sun_softness: 0.2,
        }
    }
}

/// World height (in pixel units) at integer texel (x, y), clamped to edges.
#[inline]
fn height_at(hm: &Heightmap, x: i32, y: i32, w: i32, h: i32, scale: f32) -> f32 {
    let cx = x.clamp(0, w - 1) as u32;
    let cy = y.clamp(0, h - 1) as u32;
    hm.get(cx, cy).unwrap_or(0.0) * scale
}

/// Bilinearly sampled world height (pixel units) at fractional (px, py).
#[inline]
fn sample_height(hm: &Heightmap, px: f32, py: f32, w: i32, h: i32, scale: f32) -> f32 {
    let x0 = px.floor() as i32;
    let y0 = py.floor() as i32;
    let fx = px - x0 as f32;
    let fy = py - y0 as f32;

    let h00 = height_at(hm, x0, y0, w, h, scale);
    let h10 = height_at(hm, x0 + 1, y0, w, h, scale);
    let h01 = height_at(hm, x0, y0 + 1, w, h, scale);
    let h11 = height_at(hm, x0 + 1, y0 + 1, w, h, scale);

    let top = h00 * (1.0 - fx) + h10 * fx;
    let bot = h01 * (1.0 - fx) + h11 * fx;

    top * (1.0 - fy) + bot * fy
}

/// Bake AO + sun visibility for a single texel. Returns (ao, sun) both in [0,1].
fn bake_texel(hm: &Heightmap, x: u32, y: u32, p: &LightmapParams, scale: f32) -> (f32, f32) {
    let w = p.width as i32;
    let h = p.height as i32;
    let max_dim = p.width.max(p.height) as f32;

    let px = x as f32;
    let py = y as f32;
    let origin_h = height_at(hm, x as i32, y as i32, w, h, scale);

    // ----- Horizon-based ambient occlusion -----
    let radius_px = (p.ao_radius * max_dim).max(1.0);
    let steps = p.max_steps.max(1);
    let step_len = (radius_px / steps as f32).max(1.0);
    let dirs = p.num_directions.max(1);

    let mut horizon_sum = 0.0f32;
    for d in 0..dirs {
        let az = (d as f32 / dirs as f32) * TAU;
        let dx = az.cos();
        let dy = az.sin();

        // Track the max elevation angle (as sin) to the heightfield along this ray.
        let mut max_sin = 0.0f32;
        for s in 1..=steps {
            let dist = s as f32 * step_len;
            let sx = px + dx * dist;
            let sy = py + dy * dist;
            let sh = sample_height(hm, sx, sy, w, h, scale);

            let dh = sh - origin_h;
            if dh > 0.0 {
                // sin(elevation) = dh / sqrt(dh^2 + dist^2)
                let sin_ang = dh / (dh * dh + dist * dist).sqrt();
                if sin_ang > max_sin {
                    max_sin = sin_ang;
                }
            }
        }
        horizon_sum += max_sin;
    }
    let avg_horizon = horizon_sum / dirs as f32;
    // AO = 1 at open sky, darkens with the average occluding horizon.
    let ao = (1.0 - avg_horizon * p.ao_strength).clamp(0.0, 1.0);

    // ----- Sun visibility (soft shadow) -----
    let sun = sun_visibility(hm, px, py, origin_h, p, scale, max_dim, steps, w, h);

    (ao, sun)
}

/// March toward the sun; 1.0 = lit, 0.0 = shadowed, soft penumbra between.
#[allow(clippy::too_many_arguments)]
fn sun_visibility(
    hm: &Heightmap,
    px: f32,
    py: f32,
    origin_h: f32,
    p: &LightmapParams,
    scale: f32,
    max_dim: f32,
    steps: u32,
    w: i32,
    h: i32,
) -> f32 {
    // Project the sun direction into heightfield space. The horizontal
    // component drives the march direction; the vertical component (per unit
    // horizontal distance) is the ray's rise rate.
    let sxy = (p.sun_dir[0] * p.sun_dir[0] + p.sun_dir[1] * p.sun_dir[1]).sqrt();

    // Sun at/near zenith: nothing can occlude it.
    if sxy < 1e-4 {
        return 1.0;
    }

    let dir_x = p.sun_dir[0] / sxy;
    let dir_y = p.sun_dir[1] / sxy;
    let rise_per_px = p.sun_dir[2] / sxy; // d(height)/d(horizontal pixel)

    // March across the whole AO-style extent toward the sun.
    let reach_px = (p.ao_radius * max_dim).max(1.0);
    let step_len = (reach_px / steps as f32).max(1.0);

    // Penumbra: smaller min clearance ratio => more shadowed. softness scales
    // how quickly partial blockers darken the result.
    let soft = p.sun_softness.clamp(0.0, 1.0);
    let mut min_clear = 1.0f32;

    for s in 1..=steps {
        let dist = s as f32 * step_len;
        let sx = px + dir_x * dist;
        let sy = py + dir_y * dist;
        let terrain = sample_height(hm, sx, sy, w, h, scale);
        let ray = origin_h + rise_per_px * dist;

        if terrain >= ray {
            // Fully blocked.
            return 0.0;
        }

        // Clearance of the ray above the terrain, normalized by how soft the
        // penumbra is. With soft~0 this band is tiny (hard edge); larger soft
        // widens the partial-shadow falloff.
        let gap = ray - terrain;
        let band = (soft * dist).max(1e-3);
        let clear = (gap / band).clamp(0.0, 1.0);
        if clear < min_clear {
            min_clear = clear;
        }
    }

    min_clear
}

/// Bake a lightmap (AO + sun shadow) from a heightfield to an RGBA buffer.
///
/// R = AO, G = sun visibility, B = AO*sun, A = 1.0.
pub fn bake_lightmap_cpu(heightmap: &Heightmap, params: &LightmapParams) -> ColorBuffer {
    let w = params.width;
    let h = params.height;
    let max_dim = w.max(h) as f32;
    // A full 0->1 height change spans the map's larger dimension in pixels.
    let scale = max_dim;

    let mut ao_ch = vec![0.0f32; (w as usize) * (h as usize)];
    let mut sun_ch = vec![0.0f32; (w as usize) * (h as usize)];
    let mut combined = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let (ao, sun) = bake_texel(heightmap, x, y, params, scale);
            let idx = (y as usize) * (w as usize) + (x as usize);
            ao_ch[idx] = ao;
            sun_ch[idx] = sun;
            combined[idx] = ao * sun;
        }
    }

    let r = Heightmap::frbar_data(w, h, ao_ch).unwrap();
    let g = Heightmap::frbar_data(w, h, sun_ch).unwrap();
    let b = Heightmap::frbar_data(w, h, combined).unwrap();

    ColorBuffer::from_channels(&r, &g, &b, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w * h) as usize]).unwrap()
    }

    /// A tall central spike with everything else flat.
    fn central_spike(w: u32, h: u32) -> Heightmap {
        let mut data = vec![0.0f32; (w * h) as usize];
        let cx = w / 2;
        let cy = h / 2;
        data[(cy * w + cx) as usize] = 1.0;
        Heightmap::frbar_data(w, h, data).unwrap()
    }

    #[test]
    fn flat_field_bakes_to_full_visibility() {
        let hm = flat(32, 32, 0.5);
        let params = LightmapParams {
            width: 32,
            height: 32,
            ..Default::default()
        };
        let cb = bake_lightmap_cpu(&hm, &params);
        assert_eq!(cb.width(), 32);
        assert_eq!(cb.height(), 32);

        // Interior pixels: AO ~ 1 (no occluders), sun ~ 1 (nothing casts shadow).
        for y in 4..28 {
            for x in 4..28 {
                let px = cb.get(x, y).unwrap();
                assert!(px[0] > 0.98, "flat AO should be ~1 at ({x},{y}): {}", px[0]);
                assert!(
                    px[1] > 0.98,
                    "flat sun should be ~1 at ({x},{y}): {}",
                    px[1]
                );
                assert!((px[3] - 1.0).abs() < 1e-6, "alpha must be 1");
            }
        }
    }

    #[test]
    fn all_channels_in_unit_range() {
        let hm = central_spike(48, 48);
        let params = LightmapParams {
            width: 48,
            height: 48,
            ..Default::default()
        };
        let cb = bake_lightmap_cpu(&hm, &params);
        for px in cb.data().chunks_exact(4) {
            for (c, &v) in px.iter().enumerate() {
                assert!((0.0..=1.0).contains(&v), "channel {c} out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn spike_casts_ao_dimple_at_its_base() {
        let w = 48;
        let h = 48;
        let hm = central_spike(w, h);
        let params = LightmapParams {
            width: w,
            height: h,
            ao_radius: 0.3,
            ..Default::default()
        };
        let cb = bake_lightmap_cpu(&hm, &params);

        // A texel adjacent to the spike sees the spike on its horizon -> AO < 1.
        let base_ao = cb.get(w / 2 + 1, h / 2).unwrap()[0];
        // A far corner sees nothing -> AO ~ 1.
        let far_ao = cb.get(2, 2).unwrap()[0];
        assert!(
            base_ao < far_ao - 0.01,
            "AO dimple at spike base: base {base_ao} should be < far {far_ao}"
        );
    }

    #[test]
    fn spike_shadows_side_away_from_sun() {
        let w = 64;
        let h = 64;
        let hm = central_spike(w, h);
        // Sun low in the +X/+Y direction (azimuth 45, low elevation) so the
        // shadow falls on the -X/-Y side of the spike.
        let params = LightmapParams {
            width: w,
            height: h,
            ao_radius: 0.4,
            sun_dir: [0.7, 0.7, 0.14], // low sun toward +X/+Y
            sun_softness: 0.1,
            max_steps: 48,
            ..Default::default()
        };
        let cb = bake_lightmap_cpu(&hm, &params);

        let cx = w / 2;
        let cy = h / 2;
        // Shadowed side: opposite the sun (toward -X/-Y, i.e. left/up of spike).
        let shadow = cb.get(cx - 3, cy - 3).unwrap()[1];
        // Lit side: toward the sun (+X/+Y, right/down of spike).
        let lit = cb.get(cx + 6, cy + 6).unwrap()[1];
        assert!(
            shadow < lit,
            "side away from sun should be darker: shadow {shadow} vs lit {lit}"
        );
        assert!(
            shadow < 0.9,
            "shadowed side should be notably dim: {shadow}"
        );
    }
}
