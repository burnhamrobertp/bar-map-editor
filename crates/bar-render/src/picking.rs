//! Screen-space → world-space → heightmap-pixel picking.
//!
//! Used by the editor's 3D sculpt mode: convert a cursor position in the
//! viewport into a heightmap pixel coordinate so the brush footprint can
//! be applied at the right spot on the terrain.

use crate::Camera;
use bar_data::Heightmap;
use glam::{Mat4, Vec3, Vec4};

/// A single feature instance the picker can test the cursor ray against.
/// `transform` maps the AABB out of model space into world space (the same
/// matrix used to draw the instance). Caller assembles one of these per
/// placed feature.
#[derive(Clone, Copy)]
pub struct PickableFeature {
    pub transform: Mat4,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

/// Cast a ray from the cursor and return the index of the closest feature
/// whose oriented bounding box the ray hits, or `None` if it misses every
/// feature. Order of `features` is preserved -- the returned index is into
/// that slice.
pub fn pick_feature(
    camera: &Camera,
    aspect_ratio: f32,
    cursor_uv: (f32, f32),
    features: &[PickableFeature],
) -> Option<usize> {
    let (origin, dir) = camera_ray(camera, aspect_ratio, cursor_uv)?;

    let mut best: Option<(usize, f32)> = None;
    for (idx, feat) in features.iter().enumerate() {
        // Transform the ray into the feature's model space. In model space
        // the AABB is axis-aligned, which makes the slab test trivial and
        // independent of the feature's rotation / scale.
        let inv = feat.transform.inverse();
        let local_origin = inv.transform_point3(origin);
        let local_dir = inv.transform_vector3(dir);
        let Some(t) = ray_aabb_t_min(local_origin, local_dir, feat.aabb_min, feat.aabb_max) else {
            continue;
        };
        // Re-project the model-space hit `t` to world space distance so we
        // can compare hits across features with different scales.
        let world_hit = feat
            .transform
            .transform_point3(local_origin + local_dir * t);
        let world_t = (world_hit - origin).dot(dir);
        if world_t < 0.0 {
            continue;
        }
        match best {
            Some((_, bt)) if world_t >= bt => {}
            _ => best = Some((idx, world_t)),
        }
    }
    best.map(|(i, _)| i)
}

/// Unproject the cursor at the near and far planes to produce a world-space
/// ray (origin, normalised direction). Returns `None` if the camera matrix
/// is degenerate.
pub fn camera_ray(
    camera: &Camera,
    aspect_ratio: f32,
    cursor_uv: (f32, f32),
) -> Option<(Vec3, Vec3)> {
    let inv_vp = camera.view_projection(aspect_ratio).inverse();
    let ndc_x = cursor_uv.0 * 2.0 - 1.0;
    let ndc_y = 1.0 - cursor_uv.1 * 2.0;
    let near_h = inv_vp * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
    let far_h = inv_vp * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
    if near_h.w.abs() < 1e-8 || far_h.w.abs() < 1e-8 {
        return None;
    }
    let near = near_h.truncate() / near_h.w;
    let far = far_h.truncate() / far_h.w;
    let dir = (far - near).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    Some((near, dir))
}

/// Slab-method ray-AABB intersection. Returns the entry-side `t` along the
/// ray when there is a hit, or `None` on miss. Handles rays starting inside
/// the box by returning `t = 0`.
fn ray_aabb_t_min(origin: Vec3, dir: Vec3, aabb_min: Vec3, aabb_max: Vec3) -> Option<f32> {
    let mut t_min = f32::NEG_INFINITY;
    let mut t_max = f32::INFINITY;
    for axis in 0..3 {
        let o = origin[axis];
        let d = dir[axis];
        let lo = aabb_min[axis];
        let hi = aabb_max[axis];
        if d.abs() < 1e-8 {
            if o < lo || o > hi {
                return None;
            }
        } else {
            let t1 = (lo - o) / d;
            let t2 = (hi - o) / d;
            let (ta, tb) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
            t_min = t_min.max(ta);
            t_max = t_max.min(tb);
            if t_min > t_max {
                return None;
            }
        }
    }
    if t_max < 0.0 {
        return None;
    }
    Some(t_min.max(0.0))
}

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

    // Clip the march to the terrain AABB so the step size is proportional
    // to the terrain, not the far plane distance. Without this, a far plane
    // of 1000 and only 256 steps gives ~4 world-unit steps that skip over
    // terrain that spans ±0.5 in XZ.
    let (t_min, t_max) = aabb_intersect(near, dir, x_extent, z_extent, height_scale);
    if t_min >= t_max {
        return None;
    }
    let march_start = t_min.max(0.0);
    let march_end = t_max;

    let steps = 256usize;
    let step_len = (march_end - march_start) / steps as f32;
    if step_len <= 0.0 {
        return None;
    }

    let mut prev_t = march_start;
    let mut prev_dy = ray_y_above_terrain(
        near,
        dir,
        prev_t,
        heightmap,
        x_extent,
        z_extent,
        height_scale,
    );
    for i in 1..=steps {
        let t = march_start + step_len * i as f32;
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
                        near,
                        dir,
                        mid,
                        heightmap,
                        x_extent,
                        z_extent,
                        height_scale,
                    );
                    match dmid {
                        Some(v) if v > 0.0 => lo = mid,
                        Some(_) => hi = mid,
                        None => break,
                    }
                }
                let t_hit = (lo + hi) * 0.5;
                let world = near + dir * t_hit;
                return world_to_heightmap(world, x_extent, z_extent, heightmap).map(|(hx, hy)| {
                    PickResult {
                        world,
                        hm_x: hx,
                        hm_y: hy,
                    }
                });
            }
        }
        prev_t = t;
        prev_dy = dy;
    }
    None
}

/// Slab-method ray-AABB intersection for the terrain bounding box.
/// Returns `(t_min, t_max)` along the ray; caller should check `t_min < t_max`.
fn aabb_intersect(
    origin: Vec3,
    dir: Vec3,
    x_extent: f32,
    z_extent: f32,
    height_scale: f32,
) -> (f32, f32) {
    let aabb_min = Vec3::new(-x_extent, -height_scale * 0.1, -z_extent);
    let aabb_max = Vec3::new(x_extent, height_scale * 1.1, z_extent);

    let mut t_min = f32::NEG_INFINITY;
    let mut t_max = f32::INFINITY;

    for axis in 0..3 {
        let o = origin[axis];
        let d = dir[axis];
        let lo = aabb_min[axis];
        let hi = aabb_max[axis];
        if d.abs() < 1e-8 {
            if o < lo || o > hi {
                return (1.0, 0.0); // miss
            }
        } else {
            let t1 = (lo - o) / d;
            let t2 = (hi - o) / d;
            let (ta, tb) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
            t_min = t_min.max(ta);
            t_max = t_max.min(tb);
        }
    }
    (t_min, t_max)
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

/// Test whether the heightmap occludes the ray `[origin, origin +
/// dir * max_t]`. Returns `true` if the ray's Y dips below the
/// terrain at any sample inside the playable mesh AABB before
/// reaching `max_t`. Used by editor gizmos that need to know whether
/// a world-space point is behind the terrain from the camera's POV.
///
/// Marches the ray inside the terrain AABB only (the slab-clipped
/// portion of `[0, max_t]`); each step is a single bilinear
/// heightmap lookup. Step count is `steps`; 128 is a reasonable
/// quality/cost tradeoff for editor UI use. Returns `false` if the
/// ray misses the AABB entirely, has zero length, or never crosses
/// below the terrain.
pub fn ray_terrain_occludes(
    origin: Vec3,
    dir: Vec3,
    max_t: f32,
    heightmap: &Heightmap,
    x_extent: f32,
    z_extent: f32,
    height_scale: f32,
    steps: u32,
) -> bool {
    if heightmap.width() == 0 || heightmap.height() == 0 || max_t <= 0.0 || steps == 0 {
        return false;
    }
    let (t_min, t_max) = aabb_intersect(origin, dir, x_extent, z_extent, height_scale);
    let t_start = t_min.max(0.0);
    let t_end = t_max.min(max_t);
    if t_start >= t_end {
        return false;
    }
    let step_len = (t_end - t_start) / steps as f32;
    if step_len <= 0.0 {
        return false;
    }
    for i in 0..=steps {
        let t = t_start + step_len * i as f32;
        if let Some(dy) =
            ray_y_above_terrain(origin, dir, t, heightmap, x_extent, z_extent, height_scale)
        {
            if dy < 0.0 {
                return true;
            }
        }
    }
    false
}

/// Sample the terrain's world-space Y at the given world-space XZ.
/// Returns `None` if the XZ falls outside the mesh bounds. Uses the
/// same bilinear-sampling math as `pick_terrain`'s ray-march so a
/// caller comparing camera position against terrain Y (e.g. to
/// prevent the camera from clipping through the ground) gets a
/// result consistent with what `pick_terrain` would report at the
/// same XZ.
pub fn terrain_y_at_world_xz(
    world_x: f32,
    world_z: f32,
    hm: &Heightmap,
    x_extent: f32,
    z_extent: f32,
    height_scale: f32,
) -> Option<f32> {
    let p = Vec3::new(world_x, 0.0, world_z);
    let (hx, hy) = world_to_heightmap(p, x_extent, z_extent, hm)?;
    Some(sample_bilinear(hm, hx, hy) * height_scale)
}

/// Convert a world-space point's XZ to heightmap pixel coordinates.
/// Returns `None` if outside the mesh bounds.
fn world_to_heightmap(p: Vec3, x_extent: f32, z_extent: f32, hm: &Heightmap) -> Option<(f32, f32)> {
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
        let camera = Camera {
            distance: 2.0,
            azimuth: 0.0,
            elevation: std::f32::consts::FRAC_PI_2 - 0.01,
            ..Camera::default()
        };

        let hm = flat_hm(65, 65, 0.0);
        let pick = pick_terrain(&camera, 1.0, (0.5, 0.5), &hm, 0.5, 0.5, 1.0);
        assert!(pick.is_some(), "expected a hit");
        let p = pick.unwrap();
        // Centre pixel is (32, 32) in a 65×65 hm.
        assert!((p.hm_x - 32.0).abs() < 1.0, "hm_x={}", p.hm_x);
        assert!((p.hm_y - 32.0).abs() < 1.0, "hm_y={}", p.hm_y);
    }

    #[test]
    fn terrain_y_at_world_xz_returns_height_scaled_value() {
        // Flat heightmap at value 0.5, height_scale 1.0 -> terrain Y
        // is 0.5 anywhere within bounds.
        let hm = flat_hm(33, 33, 0.5);
        let y = terrain_y_at_world_xz(0.0, 0.0, &hm, 0.5, 0.5, 1.0);
        assert!(y.is_some());
        assert!((y.unwrap() - 0.5).abs() < 1e-3);
        // And a non-trivial height_scale multiplies through.
        let y2 = terrain_y_at_world_xz(0.0, 0.0, &hm, 0.5, 0.5, 0.02);
        assert!((y2.unwrap() - 0.01).abs() < 1e-4);
    }

    #[test]
    fn terrain_y_at_world_xz_is_none_outside_mesh() {
        let hm = flat_hm(33, 33, 0.5);
        // x_extent / z_extent = 0.5; query at 1.0 is well outside.
        assert!(terrain_y_at_world_xz(1.0, 0.0, &hm, 0.5, 0.5, 1.0).is_none());
        assert!(terrain_y_at_world_xz(0.0, -2.0, &hm, 0.5, 0.5, 1.0).is_none());
    }

    #[test]
    fn returns_none_when_cursor_points_at_sky() {
        // Cursor at the top of the viewport pointing roughly upward
        // misses the terrain entirely.
        let camera = Camera {
            elevation: 0.1,
            ..Camera::default()
        }; // shallow angle
        let hm = flat_hm(65, 65, 0.0);
        let pick = pick_terrain(&camera, 1.0, (0.5, 0.0), &hm, 0.5, 0.5, 1.0);
        assert!(pick.is_none(), "expected miss, got {:?}", pick);
    }
}
