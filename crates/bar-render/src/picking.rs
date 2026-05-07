//! Screen-space → world-space → heightmap-pixel picking.
//!
//! Used by the editor's 3D sculpt mode: convert a cursor position in the
//! viewport into a heightmap pixel coordinate so the brush footprint can
//! be applied at the right spot on the terrain.

use crate::Camera;
use glam::{Vec3, Vec4};
use bar_data::Heightmap;

/// Result of a successful pick.
#[derive(Debug, Clone, Copy)]
pub struct PickResult {
    /// World-space position where the ray hit the terrain.
    pub world: Vec3,
    /// Heightmap pixel coordinates. Floating-point so callers can apply
    /// fractional brush positions.
    pub hm_x: f32,
    pub hm_y: f32,
}

/// Cast a ray from the cursor through the terrain mesh and return the
/// first hit point. `cursor_uv` is the cursor position normalised to
/// `[0, 1]` over the viewport (origin top-left). Returns `None` if the
/// ray misses the terrain (e.g. cursor pointing at the sky).
///
/// `x_extent` / `z_extent` are the mesh's half-span in world units (the
/// mesh spans `[-x_extent, +x_extent]` in X and similarly for Z).
/// `height_scale` is the multiplier applied to the normalised
/// heightmap value to produce the world Y. These parameters must match
/// what was passed to `update_mesh_lod`.
pub fn pick_terrain(
    camera: &Camera,
    aspect_ratio: f32,
    cursor_uv: (f32, f32),
    heightmap: &Heightmap,
    x_extent: f32,
    z_extent: f32,
    height_scale: f32,
) -> Option<PickResult> {
    if heightmap.width() == 0 || heightmap.height() == 0 {
        return None;
    }

    let inv_vp = camera.view_projection(aspect_ratio).inverse();

    // Convert UV (0..1, top-left origin) to NDC (-1..1, bottom-up).
    let ndc_x = cursor_uv.0 * 2.0 - 1.0;
    let ndc_y = 1.0 - cursor_uv.1 * 2.0;

    // Unproject near (z = 0 in NDC for a wgpu/Vulkan-style range; this
    // matches glam's `Mat4::perspective_rh` output) and far (z = 1) to
    // world space, then derive the ray direction.
    let near_h = inv_vp * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
    let far_h = inv_vp * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
    let near = near_h.truncate() / near_h.w;
    let far = far_h.truncate() / far_h.w;
    let dir = (far - near).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }

    // March along the ray. The mesh is bounded; once we leave the
    // bounding box on the far side, we know we missed.
    let max_dist = (far - near).length();
    let steps = 256usize;
    let step_len = max_dist / steps as f32;

    let mut prev_t = 0.0_f32;
    let mut prev_dy = ray_y_above_terrain(near, dir, prev_t, heightmap, x_extent, z_extent, height_scale);
    for i in 1..=steps {
        let t = step_len * i as f32;
        let dy = ray_y_above_terrain(near, dir, t, heightmap, x_extent, z_extent, height_scale);
        // Sign-change in dy ⇒ we crossed the surface between prev_t and t.
        // (dy is "ray.y - terrain.y"; positive = above, negative = below.)
        if let (Some(prev), Some(now)) = (prev_dy, dy) {
            if prev > 0.0 && now <= 0.0 {
                // Linear interpolation gets us close enough for editor
                // picking. Iterate binary search for a couple steps to
                // tighten without going overboard.
                let mut lo = prev_t;
                let mut hi = t;
                for _ in 0..6 {
                    let mid = (lo + hi) * 0.5;
                    let dmid = ray_y_above_terrain(
                        near, dir, mid, heightmap, x_extent, z_extent, height_scale,
                    );
                    match dmid {
                        Some(v) if v > 0.0 => lo = mid,
                        Some(_) => hi = mid,
                        None => break,
                    }
                }
                let t_hit = (lo + hi) * 0.5;
                let world = near + dir * t_hit;
                return world_to_heightmap(world, x_extent, z_extent, heightmap)
                    .map(|(hx, hy)| PickResult { world, hm_x: hx, hm_y: hy });
            }
        }
        prev_t = t;
        prev_dy = dy;
    }
    None
}

/// Returns `Some(ray_y - terrain_y)` for the ray's position at parameter
/// `t`, or `None` if the ray's XZ falls outside the mesh bounds.
fn ray_y_above_terrain(
    origin: Vec3,
    dir: Vec3,
    t: f32,
    hm: &Heightmap,
    x_extent: f32,
    z_extent: f32,
    height_scale: f32,
) -> Option<f32> {
    let p = origin + dir * t;
    let (hx, hy) = world_to_heightmap(p, x_extent, z_extent, hm)?;
    let v = sample_bilinear(hm, hx, hy);
    let terrain_y = v * height_scale;
    Some(p.y - terrain_y)
}

/// Convert a world-space point's XZ to heightmap pixel coordinates.
/// Returns `None` if outside the mesh bounds.
fn world_to_heightmap(
    p: Vec3,
    x_extent: f32,
    z_extent: f32,
    hm: &Heightmap,
) -> Option<(f32, f32)> {
    if p.x < -x_extent || p.x > x_extent || p.z < -z_extent || p.z > z_extent {
        return None;
    }
    let u = (p.x + x_extent) / (2.0 * x_extent);
    let v = (p.z + z_extent) / (2.0 * z_extent);
    Some((u * (hm.width() - 1) as f32, v * (hm.height() - 1) as f32))
}

fn sample_bilinear(hm: &Heightmap, x: f32, y: f32) -> f32 {
    let w = hm.width() as i32;
    let h = hm.height() as i32;
    let x0 = (x.floor() as i32).clamp(0, w - 1);
    let x1 = (x0 + 1).min(w - 1);
    let y0 = (y.floor() as i32).clamp(0, h - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = (x - x0 as f32).clamp(0.0, 1.0);
    let fy = (y - y0 as f32).clamp(0.0, 1.0);
    let v00 = hm.get(x0 as u32, y0 as u32).unwrap_or(0.0);
    let v10 = hm.get(x1 as u32, y0 as u32).unwrap_or(0.0);
    let v01 = hm.get(x0 as u32, y1 as u32).unwrap_or(0.0);
    let v11 = hm.get(x1 as u32, y1 as u32).unwrap_or(0.0);
    let v0 = v00 * (1.0 - fx) + v10 * fx;
    let v1 = v01 * (1.0 - fx) + v11 * fx;
    v0 * (1.0 - fy) + v1 * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_hm(w: u32, h: u32, val: f32) -> Heightmap {
        let mut hm = Heightmap::new(w, h).unwrap();
        for y in 0..h {
            for x in 0..w {
                hm.set(x, y, val).unwrap();
            }
        }
        hm
    }

    #[test]
    fn picks_centre_when_camera_looks_straight_down() {
        // Camera directly above origin, looking down. Cursor at the
        // centre of the viewport should hit (0, 0) world XZ →
        // centre of the heightmap.
        let mut camera = Camera::default();
        camera.target = Vec3::ZERO;
        camera.distance = 2.0;
        camera.azimuth = 0.0;
        camera.elevation = std::f32::consts::FRAC_PI_2 - 0.01;

        let hm = flat_hm(65, 65, 0.0);
        let pick = pick_terrain(&camera, 1.0, (0.5, 0.5), &hm, 0.5, 0.5, 1.0);
        assert!(pick.is_some(), "expected a hit");
        let p = pick.unwrap();
        // Centre pixel is (32, 32) in a 65×65 hm.
        assert!((p.hm_x - 32.0).abs() < 1.0, "hm_x={}", p.hm_x);
        assert!((p.hm_y - 32.0).abs() < 1.0, "hm_y={}", p.hm_y);
    }

    #[test]
    fn returns_none_when_cursor_points_at_sky() {
        // Cursor at the top of the viewport pointing roughly upward
        // misses the terrain entirely.
        let mut camera = Camera::default();
        camera.elevation = 0.1; // shallow angle
        let hm = flat_hm(65, 65, 0.0);
        let pick = pick_terrain(&camera, 1.0, (0.5, 0.0), &hm, 0.5, 0.5, 1.0);
        assert!(pick.is_none(), "expected miss, got {:?}", pick);
    }
}
