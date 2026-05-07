/// Vertex for terrain mesh rendering.
///
/// UV encoding for special geometry types:
/// - Regular terrain: uv in [0,1]×[0,1] — normal texture or height-based colour.
/// - Skirt / bottom cap: uv.y = 2.0 — always use height-based colour (no texture).
/// - Water / lava plane: uv.x = -1.0 — use water_r/g/b from the camera uniform.
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
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // normal
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // uv
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// Append a vertical skirt wall for one edge of the terrain.
///
/// `positions`: sequence of (px, py, pz) points along the edge, where py is the
/// terrain surface height at that point.  The skirt drops straight down to Y=0.
/// `normal`: outward-pointing face normal for this wall.
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
        // Top vertex at terrain height
        vertices.push(TerrainVertex { position: [px, py, pz], normal, uv: [u, 2.0] });
        // Bottom vertex at mesh base
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

/// Generate a terrain mesh from a heightmap.
///
/// The mesh includes:
/// - The terrain surface (height-mapped grid).
/// - Vertical skirt walls on all four edges, descending to Y=0.
/// - A flat bottom cap at Y=0.
/// - An optional water / lava plane at `water_y` (omitted when `water_y < 0`).
///
/// `x_extent` and `z_extent` control the half-span of the mesh in world space.
/// For a square map use `(0.5, 0.5)`.  For a rectangular map pass values derived
/// from the physical Spring map dimensions so that the XZ proportions are correct.
///
/// Returns `(vertices, indices)` for rendering.
pub fn generate_terrain_mesh(
    heightmap: &bar_data::Heightmap,
    height_scale: f32,
    x_extent: f32,
    z_extent: f32,
    water_y: f32,
) -> (Vec<TerrainVertex>, Vec<u32>) {
    let w = heightmap.width() as usize;
    let h = heightmap.height() as usize;

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // ── terrain surface ──────────────────────────────────────────────────────
    // step_x / step_z distribute vertices evenly across the physical extent.
    let step_x = (2.0 * x_extent) / (w - 1) as f32;
    let step_z = (2.0 * z_extent) / (h - 1) as f32;

    for y in 0..h {
        for x in 0..w {
            let height = heightmap.get(x as u32, y as u32).unwrap_or(0.0);
            let px = x as f32 * step_x - x_extent;
            let pz = y as f32 * step_z - z_extent;
            let py = height * height_scale;

            // Central-difference normal — separate step sizes for non-square maps.
            let hx0 = heightmap.get(x.saturating_sub(1) as u32, y as u32).unwrap_or(height);
            let hx1 = heightmap.get((x + 1).min(w - 1) as u32, y as u32).unwrap_or(height);
            let hy0 = heightmap.get(x as u32, y.saturating_sub(1) as u32).unwrap_or(height);
            let hy1 = heightmap.get(x as u32, (y + 1).min(h - 1) as u32).unwrap_or(height);

            let dx = (hx1 - hx0) * height_scale / (2.0 * step_x);
            let dz = (hy1 - hy0) * height_scale / (2.0 * step_z);
            let normal = glam::Vec3::new(-dx, 1.0, -dz).normalize();

            vertices.push(TerrainVertex {
                position: [px, py, pz],
                normal: [normal.x, normal.y, normal.z],
                uv: [x as f32 / (w - 1) as f32, y as f32 / (h - 1) as f32],
            });
        }
    }

    for y in 0..(h - 1) {
        for x in 0..(w - 1) {
            let tl = (y * w + x) as u32;
            let tr = (y * w + x + 1) as u32;
            let bl = ((y + 1) * w + x) as u32;
            let br = ((y + 1) * w + x + 1) as u32;
            indices.push(tl); indices.push(bl); indices.push(tr);
            indices.push(tr); indices.push(bl); indices.push(br);
        }
    }

    // ── skirt walls ──────────────────────────────────────────────────────────
    let north: Vec<_> = (0..w).map(|x| {
        let height = heightmap.get(x as u32, 0).unwrap_or(0.0);
        (x as f32 * step_x - x_extent, height * height_scale, -z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &north, [0.0, 0.0, -1.0]);

    let south: Vec<_> = (0..w).map(|x| {
        let height = heightmap.get(x as u32, (h - 1) as u32).unwrap_or(0.0);
        (x as f32 * step_x - x_extent, height * height_scale, z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &south, [0.0, 0.0, 1.0]);

    let west: Vec<_> = (0..h).map(|y| {
        let height = heightmap.get(0, y as u32).unwrap_or(0.0);
        (-x_extent, height * height_scale, y as f32 * step_z - z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &west, [-1.0, 0.0, 0.0]);

    let east: Vec<_> = (0..h).map(|y| {
        let height = heightmap.get((w - 1) as u32, y as u32).unwrap_or(0.0);
        (x_extent, height * height_scale, y as f32 * step_z - z_extent)
    }).collect();
    add_edge_skirt(&mut vertices, &mut indices, &east, [1.0, 0.0, 0.0]);

    // ── bottom cap ───────────────────────────────────────────────────────────
    let base = vertices.len() as u32;
    for &(px, pz) in &[(-x_extent, -z_extent), (x_extent, -z_extent), (x_extent, z_extent), (-x_extent, z_extent)] {
        vertices.push(TerrainVertex {
            position: [px, 0.0, pz],
            normal: [0.0, -1.0, 0.0],
            uv: [0.0, 2.0], // uv.y = 2.0 → height-colour path in shader
        });
    }
    // CCW winding for a face visible from below (-Y)
    indices.push(base); indices.push(base + 2); indices.push(base + 1);
    indices.push(base); indices.push(base + 3); indices.push(base + 2);

    // ── water / lava plane ───────────────────────────────────────────────────
    if water_y >= 0.0 {
        let base = vertices.len() as u32;
        for &(px, pz) in &[(-x_extent, -z_extent), (x_extent, -z_extent), (x_extent, z_extent), (-x_extent, z_extent)] {
            vertices.push(TerrainVertex {
                position: [px, water_y, pz],
                normal: [0.0, 1.0, 0.0],
                uv: [-1.0, 0.0], // uv.x < 0 → water-colour path in shader
            });
        }
        // CCW winding for a face visible from above (+Y)
        indices.push(base); indices.push(base + 1); indices.push(base + 2);
        indices.push(base); indices.push(base + 2); indices.push(base + 3);
    }

    (vertices, indices)
}

/// Generate a terrain mesh from a heightmap with LOD (level of detail).
/// The mesh is limited to `max_grid_size` vertices per side, downsampling
/// the heightmap with bilinear interpolation if needed.
/// `water_y`: render-space Y of the water/lava surface; negative = no water.
pub fn generate_terrain_mesh_lod(
    heightmap: &bar_data::Heightmap,
    height_scale: f32,
    max_grid_size: u32,
    x_extent: f32,
    z_extent: f32,
    water_y: f32,
) -> (Vec<TerrainVertex>, Vec<u32>) {
    let src_w = heightmap.width();
    let src_h = heightmap.height();

    // If already within limits, use full resolution
    if src_w <= max_grid_size && src_h <= max_grid_size {
        return generate_terrain_mesh(heightmap, height_scale, x_extent, z_extent, water_y);
    }

    // Compute decimated dimensions preserving aspect ratio
    let scale = (max_grid_size as f32 / src_w as f32).min(max_grid_size as f32 / src_h as f32);
    let dst_w = ((src_w as f32 * scale) as u32).max(2);
    let dst_h = ((src_h as f32 * scale) as u32).max(2);

    // Downsample with bilinear interpolation
    let mut decimated = bar_data::Heightmap::new(dst_w, dst_h).unwrap();
    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = dx as f32 / (dst_w - 1) as f32 * (src_w - 1) as f32;
            let sy = dy as f32 / (dst_h - 1) as f32 * (src_h - 1) as f32;

            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);

            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            let h00 = heightmap.get(x0, y0).unwrap_or(0.0);
            let h10 = heightmap.get(x1, y0).unwrap_or(0.0);
            let h01 = heightmap.get(x0, y1).unwrap_or(0.0);
            let h11 = heightmap.get(x1, y1).unwrap_or(0.0);

            let h = h00 * (1.0 - fx) * (1.0 - fy)
                + h10 * fx * (1.0 - fy)
                + h01 * (1.0 - fx) * fy
                + h11 * fx * fy;

            decimated.set(dx, dy, h).unwrap();
        }
    }

    generate_terrain_mesh(&decimated, height_scale, x_extent, z_extent, water_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_data::Heightmap;

    fn terrain_verts(w: usize, h: usize) -> usize {
        // surface + 4 skirt edges (2 verts each per edge point) + bottom cap (4 verts)
        w * h + 2 * (w + w + h + h) + 4
    }

    #[test]
    fn test_generate_mesh() {
        let hm = Heightmap::new(4, 4).unwrap();
        let (verts, _indices) = generate_terrain_mesh(&hm, 1.0, 0.5, 0.5, -1.0);
        assert_eq!(verts.len(), terrain_verts(4, 4)); // 16 + 32 + 4 = 52
    }

    #[test]
    fn test_mesh_with_height() {
        let mut hm = Heightmap::new(4, 4).unwrap();
        hm.set(2, 2, 0.5).unwrap();
        let (verts, _) = generate_terrain_mesh(&hm, 2.0, 0.5, 0.5, -1.0);
        // Vertex at (2,2) in the surface grid should have y = 0.5 * 2.0 = 1.0
        let idx = 2 * 4 + 2;
        assert!((verts[idx].position[1] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_mesh_water_plane_adds_verts() {
        let hm = Heightmap::new(4, 4).unwrap();
        let (verts_no_water, _) = generate_terrain_mesh(&hm, 1.0, 0.5, 0.5, -1.0);
        let (verts_with_water, _) = generate_terrain_mesh(&hm, 1.0, 0.5, 0.5, 0.0);
        assert_eq!(verts_with_water.len(), verts_no_water.len() + 4);
    }

    #[test]
    fn test_lod_mesh_decimation() {
        let hm = Heightmap::new(512, 512).unwrap();
        let (verts, _) = generate_terrain_mesh_lod(&hm, 1.0, 64, 0.5, 0.5, -1.0);
        assert_eq!(verts.len(), terrain_verts(64, 64));
    }

    #[test]
    fn test_lod_mesh_no_decimation_when_small() {
        let hm = Heightmap::new(32, 32).unwrap();
        let (verts, _) = generate_terrain_mesh_lod(&hm, 1.0, 64, 0.5, 0.5, -1.0);
        assert_eq!(verts.len(), terrain_verts(32, 32));
    }

    #[test]
    fn test_non_square_aspect_ratio() {
        // A 2:1 map with x_extent=0.5, z_extent=0.25 should place the east edge at x=0.5
        // and the south edge at z=0.25.
        let hm = Heightmap::new(4, 2).unwrap();
        let (verts, _) = generate_terrain_mesh(&hm, 1.0, 0.5, 0.25, -1.0);
        // Surface vertex at (3, 1) = index 4*1+3 = 7
        let v = &verts[7];
        assert!((v.position[0] - 0.5).abs() < 0.001, "east edge x should be 0.5");
        assert!((v.position[2] - 0.25).abs() < 0.001, "south edge z should be 0.25");
    }
}
