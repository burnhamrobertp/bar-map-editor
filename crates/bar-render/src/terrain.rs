/// Vertex for terrain mesh rendering.
///
/// UV encoding for special geometry types:
/// - Regular terrain: uv in [0,1]x[0,1] -- vertex shader samples heightmap GPU-side for Y.
/// - Skirt / bottom cap: uv.y = 2.0 -- world-space position passed through directly.
/// - Water / lava plane: uv.x = -1.0 -- world-space position passed through directly.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

impl TerrainVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

fn bilinear_sample(hm: &bar_data::Heightmap, u: f32, v: f32) -> f32 {
    let max_x = hm.width().saturating_sub(1) as f32;
    let max_y = hm.height().saturating_sub(1) as f32;
    let sx = (u * max_x).clamp(0.0, max_x);
    let sy = (v * max_y).clamp(0.0, max_y);
    let x0 = sx.floor() as u32;
    let y0 = sy.floor() as u32;
    let x1 = (x0 + 1).min(hm.width() - 1);
    let y1 = (y0 + 1).min(hm.height() - 1);
    let fx = sx - x0 as f32;
    let fy = sy - y0 as f32;
    let h00 = hm.get(x0, y0).unwrap_or(0.0);
    let h10 = hm.get(x1, y0).unwrap_or(0.0);
    let h01 = hm.get(x0, y1).unwrap_or(0.0);
    let h11 = hm.get(x1, y1).unwrap_or(0.0);
    h00 * (1.0 - fx) * (1.0 - fy)
        + h10 * fx * (1.0 - fy)
        + h01 * (1.0 - fx) * fy
        + h11 * fx * fy
}

fn add_edge_skirt(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    positions: &[(f32, f32, f32)],
    normal: [f32; 3],
) {
    let base = vertices.len() as u32;
    let n = positions.len();
    for (i, &(px, py, pz)) in positions.iter().enumerate() {
        let u = i as f32 / (n - 1) as f32;
        vertices.push(TerrainVertex { position: [px, py, pz], normal, uv: [u, 2.0] });
        vertices.push(TerrainVertex { position: [px, 0.0, pz], normal, uv: [u, 2.0] });
    }
    for i in 0..(n - 1) {
        let tl = base + (i * 2) as u32;
        let bl = base + (i * 2 + 1) as u32;
        let tr = base + ((i + 1) * 2) as u32;
        let br = base + ((i + 1) * 2 + 1) as u32;
        indices.push(tl); indices.push(bl); indices.push(tr);
        indices.push(tr); indices.push(bl); indices.push(br);
    }
}

/// Flat terrain grid for GPU-displacement rendering.
///
/// Positions are normalized: x in [-0.5, 0.5], y = 0, z in [-0.5, 0.5].
/// The vertex shader scales x/z by `2 * camera.x_extent` / `2 * camera.z_extent`
/// and samples the heightmap texture to displace y.
///
/// Only the terrain surface quad grid is produced here. Skirts, bottom cap,
/// and water plane are generated separately by `generate_terrain_skirts_and_cap`
/// and `generate_water_plane`.
pub fn generate_flat_grid(grid_n: u32) -> (Vec<TerrainVertex>, Vec<u32>) {
    let n = (grid_n as usize).max(2);
    let mut vertices = Vec::with_capacity(n * n);
    let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);

    for z in 0..n {
        for x in 0..n {
            let u = x as f32 / (n - 1) as f32;
            let v = z as f32 / (n - 1) as f32;
            vertices.push(TerrainVertex {
                position: [u - 0.5, 0.0, v - 0.5],
                normal: [0.0, 1.0, 0.0],
                uv: [u, v],
            });
        }
    }

    for z in 0..(n - 1) {
        for x in 0..(n - 1) {
            let tl = (z * n + x) as u32;
            let tr = (z * n + x + 1) as u32;
            let bl = ((z + 1) * n + x) as u32;
            let br = ((z + 1) * n + x + 1) as u32;
            indices.push(tl); indices.push(bl); indices.push(tr);
            indices.push(tr); indices.push(bl); indices.push(br);
        }
    }

    (vertices, indices)
}

/// Vertical skirt walls + bottom cap for the terrain mesh.
///
/// All geometry is in world space (uv.y = 2.0 sentinel -- vertex shader passes
/// position through without heightmap displacement). Skirt tops sample the
/// heightmap at `grid_n` evenly spaced edge positions so they match the GPU-
/// displaced surface at the boundary. Bottom cap sits at y = 0.
pub fn generate_terrain_skirts_and_cap(
    hm: &bar_data::Heightmap,
    height_scale: f32,
    x_extent: f32,
    z_extent: f32,
    grid_n: u32,
) -> (Vec<TerrainVertex>, Vec<u32>) {
    let n = (grid_n as usize).max(2);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // North (v = 0)
    let north: Vec<_> = (0..n).map(|i| {
        let u = i as f32 / (n - 1) as f32;
        ((u - 0.5) * 2.0 * x_extent, bilinear_sample(hm, u, 0.0) * height_scale, -z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &north, [0.0, 0.0, -1.0]);

    // South (v = 1)
    let south: Vec<_> = (0..n).map(|i| {
        let u = i as f32 / (n - 1) as f32;
        ((u - 0.5) * 2.0 * x_extent, bilinear_sample(hm, u, 1.0) * height_scale, z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &south, [0.0, 0.0, 1.0]);

    // West (u = 0)
    let west: Vec<_> = (0..n).map(|i| {
        let v = i as f32 / (n - 1) as f32;
        (-x_extent, bilinear_sample(hm, 0.0, v) * height_scale, (v - 0.5) * 2.0 * z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &west, [-1.0, 0.0, 0.0]);

    // East (u = 1)
    let east: Vec<_> = (0..n).map(|i| {
        let v = i as f32 / (n - 1) as f32;
        (x_extent, bilinear_sample(hm, 1.0, v) * height_scale, (v - 0.5) * 2.0 * z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &east, [1.0, 0.0, 0.0]);

    // Bottom cap (always at y = 0)
    let base = vertices.len() as u32;
    for &(px, pz) in &[
        (-x_extent, -z_extent),
        (x_extent,  -z_extent),
        (x_extent,   z_extent),
        (-x_extent,  z_extent),
    ] {
        vertices.push(TerrainVertex {
            position: [px, 0.0, pz],
            normal: [0.0, -1.0, 0.0],
            uv: [0.0, 2.0],
        });
    }
    // CCW from below (-Y)
    indices.push(base); indices.push(base + 2); indices.push(base + 1);
    indices.push(base); indices.push(base + 3); indices.push(base + 2);

    (vertices, indices)
}

/// Water / lava plane at world-Y = `water_y`.
/// Returns empty vecs when `water_y < 0` (no water).
pub fn generate_water_plane(
    x_extent: f32,
    z_extent: f32,
    water_y: f32,
) -> (Vec<TerrainVertex>, Vec<u32>) {
    if water_y < 0.0 {
        return (Vec::new(), Vec::new());
    }
    let mut vertices = Vec::new();
    for &(px, pz) in &[
        (-x_extent, -z_extent),
        (x_extent,  -z_extent),
        (x_extent,   z_extent),
        (-x_extent,  z_extent),
    ] {
        vertices.push(TerrainVertex {
            position: [px, water_y, pz],
            normal: [0.0, 1.0, 0.0],
            uv: [-1.0, 0.0],
        });
    }
    // CCW from above (+Y)
    let indices = vec![0u32, 1, 2, 0, 2, 3];
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_data::Heightmap;

    #[test]
    fn flat_grid_vertex_count() {
        let (verts, _) = generate_flat_grid(4);
        assert_eq!(verts.len(), 16);
    }

    #[test]
    fn flat_grid_index_count() {
        let (_, idxs) = generate_flat_grid(4);
        assert_eq!(idxs.len(), 3 * 3 * 6);
    }

    #[test]
    fn flat_grid_positions_are_normalized() {
        let (verts, _) = generate_flat_grid(4);
        for v in &verts {
            assert!(v.position[0] >= -0.5 && v.position[0] <= 0.5, "x out of range");
            assert_eq!(v.position[1], 0.0, "y must be 0 for flat grid");
            assert!(v.position[2] >= -0.5 && v.position[2] <= 0.5, "z out of range");
        }
    }

    #[test]
    fn flat_grid_uv_range() {
        let (verts, _) = generate_flat_grid(4);
        for v in &verts {
            assert!(v.uv[0] >= 0.0 && v.uv[0] <= 1.0);
            assert!(v.uv[1] >= 0.0 && v.uv[1] <= 1.0);
        }
    }

    #[test]
    fn skirts_use_world_space() {
        let hm = Heightmap::new(4, 4).unwrap();
        let (verts, _) = generate_terrain_skirts_and_cap(&hm, 1.0, 0.5, 0.5, 4);
        // All skirt/cap verts should have uv.y = 2.0 (the sentinel)
        for v in &verts {
            assert!((v.uv[1] - 2.0).abs() < 1e-4, "skirt uv.y must be 2.0");
        }
    }

    #[test]
    fn water_plane_absent_when_negative() {
        let (verts, idxs) = generate_water_plane(0.5, 0.5, -1.0);
        assert!(verts.is_empty());
        assert!(idxs.is_empty());
    }

    #[test]
    fn water_plane_present_when_nonnegative() {
        let (verts, idxs) = generate_water_plane(0.5, 0.5, 0.1);
        assert_eq!(verts.len(), 4);
        assert_eq!(idxs.len(), 6);
        for v in &verts {
            assert!((v.uv[0] - (-1.0)).abs() < 1e-4, "water uv.x must be -1");
        }
    }
}
