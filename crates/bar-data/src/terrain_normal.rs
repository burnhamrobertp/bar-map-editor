//! Full-resolution terrain normal bake for the SMF `detailNormalTex`.
//!
//! Unlike the tiling splat detail normals, `detailNormalTex` is sampled 1:1
//! across the whole map: it's the macro surface normal baked from the
//! heightmap at texture resolution, supplementing the engine's coarse vertex
//! normals. World dimensions set the horizontal spacing so slopes encode at
//! true steepness (a normal map baked without world scale flattens or
//! exaggerates depending on the heightmap's aspect).

use crate::{ColorBuffer, Heightmap};

/// Bake a tangent-space normal map (RGB = normal*0.5+0.5, B = up) from `hm`.
/// `world_width` / `world_length` are the map's size in elmos.
pub fn bake_terrain_normal(hm: &Heightmap, world_width: f32, world_length: f32) -> ColorBuffer {
    let w = hm.width();
    let h = hm.height();
    let mut color = ColorBuffer::new(w, h).expect("normal buffer alloc");
    let sx = if w > 1 {
        (world_width / (w - 1) as f32).max(1e-4)
    } else {
        1.0
    };
    let sy = if h > 1 {
        (world_length / (h - 1) as f32).max(1e-4)
    } else {
        1.0
    };
    let at = |x: i32, y: i32| -> f32 {
        let xx = x.clamp(0, w as i32 - 1) as u32;
        let yy = y.clamp(0, h as i32 - 1) as u32;
        hm.get(xx, yy).unwrap_or(0.0)
    };
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            // Central-difference slope in world units (height per elmo).
            let dhdx = (at(x + 1, y) - at(x - 1, y)) / (2.0 * sx);
            let dhdy = (at(x, y + 1) - at(x, y - 1)) / (2.0 * sy);
            let nx = -dhdx;
            let ny = -dhdy;
            let nz = 1.0f32;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            color.set(
                x as u32,
                y as u32,
                [
                    nx / len * 0.5 + 0.5,
                    ny / len * 0.5 + 0.5,
                    nz / len * 0.5 + 0.5,
                    1.0,
                ],
            );
        }
    }
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_terrain_is_all_up() {
        let hm = Heightmap::new(8, 8).unwrap();
        let nm = bake_terrain_normal(&hm, 512.0, 512.0);
        let c = nm.get(4, 4).unwrap();
        assert!((c[0] - 0.5).abs() < 1e-3 && (c[1] - 0.5).abs() < 1e-3);
        assert!(c[2] > 0.99, "flat normal should point straight up: {c:?}");
    }

    #[test]
    fn east_slope_tilts_normal_west() {
        // Height rising toward +x => normal tilts toward -x (R < 0.5).
        let mut hm = Heightmap::new(8, 8).unwrap();
        for y in 0..8 {
            for x in 0..8 {
                hm.set(x, y, x as f32 * 10.0).unwrap();
            }
        }
        let nm = bake_terrain_normal(&hm, 512.0, 512.0);
        let c = nm.get(4, 4).unwrap();
        assert!(
            c[0] < 0.5,
            "normal should lean -x on an east-facing slope: {c:?}"
        );
        assert!((c[1] - 0.5).abs() < 1e-3, "no north-south slope: {c:?}");
    }
}
