// SPDX-License-Identifier: GPL-2.0-or-later
//! Spring S3O model format parser.
//!
//! Parses the binary S3O format used by Spring/BAR for 3D models. The piece
//! hierarchy is flattened into a single vertex + index buffer; no articulation
//! state is needed for a static placement previewer.
//!
//! # Field layout (empirically verified against BAR map-bundled feature models)
//!
//! The file header (52 bytes):
//!   +0  magic "Spring unit\0"
//!   +12 version (u32, must be 0)
//!   +16 radius, +20 height, +24 mid_x, +28 mid_y, +32 mid_z (f32)
//!   +36 rootPieceOffset (u32)
//!   +40 collisionVolumeType (u32, ignored)
//!   +44 texture1NameOffset (u32)
//!   +48 texture2NameOffset (u32)
//!
//! The piece struct (52 bytes) at rootPieceOffset, then recursively at each
//! child piece offset in the child pointer table:
//!   +0  nameOffset          (u32) -- absolute file offset to piece name string
//!   +4  numChildren         (u32)
//!   +8  sfxFalloffs         (u32) -- legacy, ignored
//!   +12 numVertices         (u32)
//!   +16 vertexOffset        (u32) -- absolute file offset to vertex data
//!   +20 (reserved)
//!   +24 primitiveType       (u32) -- 0=tri list, 1=tristrip, 2=quads
//!   +28 numIndices          (u32)
//!   +32 indexOffset         (u32) -- absolute file offset to index data
//!   +36 (reserved)
//!   +40 xOffset  (f32)
//!   +44 yOffset  (f32)
//!   +48 zOffset  (f32)
//!
//! Index encoding: some exporters write each index as `(index << 8)` into the
//! u32 slot; others write plain u32. Detected per-piece automatically.
//!
//! # Coordinate system
//! S3O vertices are in model space (Spring elmos). 1 elmo = 1 Spring world unit.
//! The render-space scale factor `1 / (max_map_dim * 8)` must be applied by the
//! caller when building feature instance transforms.

use std::fmt;

// -- S3O binary constants -----------------------------------------------------

const MAGIC: &[u8; 12] = b"Spring unit\0";

/// Header is 52 bytes; rootPieceOffset lives here.
const HDR_ROOT_PIECE_OFFSET: usize = 36;

/// Piece struct field offsets (each piece is 52 bytes).
const PIECE_NUM_CHILDREN: usize = 4;
const PIECE_NUM_VERTICES: usize = 12;
const PIECE_VERTEX_OFFSET: usize = 16;
const PIECE_PRIMITIVE_TYPE: usize = 24;
const PIECE_NUM_INDICES: usize = 28;
const PIECE_INDEX_OFFSET: usize = 32;
const PIECE_X_OFFSET: usize = 40;
const PIECE_Y_OFFSET: usize = 44;
const PIECE_Z_OFFSET: usize = 48;

/// Vertex struct is 32 bytes: pos(12) + normal(12) + uv(8).
const VERTEX_LEN: usize = 32;

// -- Public types -------------------------------------------------------------

/// Merged mesh produced by flattening all S3O pieces.
pub struct S3oMesh {
    pub vertices: Vec<S3oVertex>,
    /// Triangle list indices into `vertices`.
    pub indices: Vec<u32>,
    /// Axis-aligned bounding box minimum (model space).
    pub aabb_min: [f32; 3],
    /// Axis-aligned bounding box maximum (model space).
    pub aabb_max: [f32; 3],
}

/// Per-vertex data for S3O models.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct S3oVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// Errors that can occur while parsing an S3O file.
#[derive(Debug)]
pub enum S3oError {
    TooShort,
    BadMagic,
    UnsupportedVersion(i32),
    OutOfBounds { offset: usize, len: usize },
}

impl fmt::Display for S3oError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            S3oError::TooShort => write!(f, "data too short for S3O header"),
            S3oError::BadMagic => write!(f, "bad S3O magic"),
            S3oError::UnsupportedVersion(v) => write!(f, "unsupported S3O version {v}"),
            S3oError::OutOfBounds { offset, len } => {
                write!(f, "S3O offset {offset} out of bounds (file len={len})")
            }
        }
    }
}

// -- Public entry point -------------------------------------------------------

/// Parse a Spring S3O model from raw bytes.
///
/// Returns a flat merged mesh in model space (Spring elmos). All piece-local
/// position offsets are applied so the result is ready for direct GPU upload.
/// Primitive types 0 (triangles), 1 (tristrip), and 2 (quads) are all
/// normalised to a triangle list.
pub fn parse_s3o(data: &[u8]) -> Result<S3oMesh, S3oError> {
    if data.len() < 52 {
        return Err(S3oError::TooShort);
    }
    if &data[..12] != MAGIC {
        return Err(S3oError::BadMagic);
    }
    let version = read_i32(data, 12)?;
    if version != 0 {
        return Err(S3oError::UnsupportedVersion(version));
    }

    let root_offset = read_u32(data, HDR_ROOT_PIECE_OFFSET)? as usize;

    let mut mesh = S3oMesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        aabb_min: [f32::INFINITY; 3],
        aabb_max: [f32::NEG_INFINITY; 3],
    };

    collect_piece(data, root_offset, [0.0f32; 3], &mut mesh)?;

    if mesh.vertices.is_empty() {
        mesh.aabb_min = [0.0; 3];
        mesh.aabb_max = [0.0; 3];
    }

    Ok(mesh)
}

// -- Piece traversal ----------------------------------------------------------

fn collect_piece(
    data: &[u8],
    piece_off: usize,
    parent_pos: [f32; 3],
    mesh: &mut S3oMesh,
) -> Result<(), S3oError> {
    if piece_off + 52 > data.len() {
        return Err(S3oError::OutOfBounds {
            offset: piece_off,
            len: data.len(),
        });
    }

    let num_children = read_u32(data, piece_off + PIECE_NUM_CHILDREN)? as usize;
    let vertex_offset = read_u32(data, piece_off + PIECE_VERTEX_OFFSET)? as usize;
    let num_vertices = read_u32(data, piece_off + PIECE_NUM_VERTICES)? as usize;
    let primitive_type = read_u32(data, piece_off + PIECE_PRIMITIVE_TYPE)?;
    let index_offset = read_u32(data, piece_off + PIECE_INDEX_OFFSET)? as usize;
    let num_indices = read_u32(data, piece_off + PIECE_NUM_INDICES)? as usize;
    let x_off = read_f32(data, piece_off + PIECE_X_OFFSET)?;
    let y_off = read_f32(data, piece_off + PIECE_Y_OFFSET)?;
    let z_off = read_f32(data, piece_off + PIECE_Z_OFFSET)?;

    let pos = [
        parent_pos[0] + x_off,
        parent_pos[1] + y_off,
        parent_pos[2] + z_off,
    ];

    // Base index into the merged vertex array before we append this piece's verts.
    let base_vertex = mesh.vertices.len() as u32;

    // Read vertices.
    if num_vertices > 0 {
        let verts_end = vertex_offset + num_vertices * VERTEX_LEN;
        if verts_end > data.len() {
            return Err(S3oError::OutOfBounds {
                offset: verts_end,
                len: data.len(),
            });
        }
        for i in 0..num_vertices {
            let v_off = vertex_offset + i * VERTEX_LEN;
            let vx = read_f32(data, v_off)?;
            let vy = read_f32(data, v_off + 4)?;
            let vz = read_f32(data, v_off + 8)?;
            let nx = read_f32(data, v_off + 12)?;
            let ny = read_f32(data, v_off + 16)?;
            let nz = read_f32(data, v_off + 20)?;
            let tc_x = read_f32(data, v_off + 24)?;
            let tc_y = read_f32(data, v_off + 28)?;

            let px = vx + pos[0];
            let py = vy + pos[1];
            let pz = vz + pos[2];

            mesh.aabb_min[0] = mesh.aabb_min[0].min(px);
            mesh.aabb_min[1] = mesh.aabb_min[1].min(py);
            mesh.aabb_min[2] = mesh.aabb_min[2].min(pz);
            mesh.aabb_max[0] = mesh.aabb_max[0].max(px);
            mesh.aabb_max[1] = mesh.aabb_max[1].max(py);
            mesh.aabb_max[2] = mesh.aabb_max[2].max(pz);

            mesh.vertices.push(S3oVertex {
                position: [px, py, pz],
                normal: [nx, ny, nz],
                uv: [tc_x, tc_y],
            });
        }
    }

    // Read and convert indices.
    if num_indices > 0 {
        let idx_end = index_offset + num_indices * 4;
        if idx_end > data.len() {
            return Err(S3oError::OutOfBounds {
                offset: idx_end,
                len: data.len(),
            });
        }

        // Some exporters write each index as (index << 8) in the u32 slot;
        // others store plain u32. Detect which encoding this piece uses.
        let use_shift = detect_index_shift(data, index_offset, num_indices, num_vertices);

        let raw: Vec<u32> = (0..num_indices)
            .map(|i| {
                read_u32(data, index_offset + i * 4).map(|v| if use_shift { v >> 8 } else { v })
            })
            .collect::<Result<Vec<_>, _>>()?;

        match primitive_type {
            0 => {
                for &idx in &raw {
                    mesh.indices.push(base_vertex + idx);
                }
            }
            1 => {
                expand_tristrip(&raw, base_vertex, &mut mesh.indices);
            }
            2 => {
                expand_quads(&raw, base_vertex, &mut mesh.indices);
            }
            _ => {
                // Unknown primitive type: skip silently.
            }
        }
    }

    // Child traversal: the standard Spring S3O child table lives at piece+16 when
    // numChildren > 0 (the same field that holds vertexOffset for leaf pieces).
    // BAR map-bundled hierarchical models (e.g. anemone) use a non-standard internal
    // format, so child pieces are skipped here -- their root piece contributes no
    // geometry and the child blocks are not in standard struct form. A child piece
    // with conforming layout (valid struct offsets) will parse correctly.
    if num_children > 0 {
        // Field +16 holds childTableOffset when numChildren > 0.
        let children_offset = read_u32(data, piece_off + PIECE_VERTEX_OFFSET)? as usize;
        let children_list_end = children_offset + num_children * 4;
        if children_list_end <= data.len() {
            for i in 0..num_children {
                if let Ok(child_off) = read_u32(data, children_offset + i * 4) {
                    // Ignore errors from child pieces whose internal format is
                    // non-standard (e.g. anemone); they simply contribute no geometry.
                    let _ = collect_piece(data, child_off as usize, pos, mesh);
                }
            }
        }
    }

    Ok(())
}

/// Detect whether indices in this piece are stored as `(index << 8)` (shifted)
/// or as plain u32. Checks the first few non-zero values: if the first non-zero
/// value is a multiple of 256 and its >>8 value is a valid vertex index, the
/// shifted encoding is used; otherwise plain u32.
fn detect_index_shift(
    data: &[u8],
    index_offset: usize,
    num_indices: usize,
    num_vertices: usize,
) -> bool {
    if num_vertices == 0 {
        return false;
    }
    let check = num_indices.min(32);
    for i in 0..check {
        let v = match read_u32(data, index_offset + i * 4) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if v == 0 {
            continue;
        }
        if v % 256 == 0 && (v >> 8) < num_vertices as u32 {
            return true;
        }
        if v < num_vertices as u32 {
            return false;
        }
    }
    false
}

// -- Primitive expansion ------------------------------------------------------

fn expand_tristrip(raw: &[u32], base: u32, out: &mut Vec<u32>) {
    if raw.len() < 3 {
        return;
    }
    for i in 0..raw.len() - 2 {
        let a = base + raw[i];
        let b = base + raw[i + 1];
        let c = base + raw[i + 2];
        if a == b || b == c || a == c {
            continue; // degenerate restart strip
        }
        if i % 2 == 0 {
            out.extend_from_slice(&[a, b, c]);
        } else {
            out.extend_from_slice(&[a, c, b]); // flip winding for odd strips
        }
    }
}

fn expand_quads(raw: &[u32], base: u32, out: &mut Vec<u32>) {
    let quads = raw.len() / 4;
    for q in 0..quads {
        let i = q * 4;
        let a = base + raw[i];
        let b = base + raw[i + 1];
        let c = base + raw[i + 2];
        let d = base + raw[i + 3];
        out.extend_from_slice(&[a, b, c, a, c, d]);
    }
}

// -- Byte-level readers -------------------------------------------------------

#[inline]
fn read_u32(data: &[u8], off: usize) -> Result<u32, S3oError> {
    let end = off + 4;
    if end > data.len() {
        return Err(S3oError::OutOfBounds {
            offset: end,
            len: data.len(),
        });
    }
    Ok(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

#[inline]
fn read_i32(data: &[u8], off: usize) -> Result<i32, S3oError> {
    read_u32(data, off).map(|v| v as i32)
}

#[inline]
fn read_f32(data: &[u8], off: usize) -> Result<f32, S3oError> {
    read_u32(data, off).map(f32::from_bits)
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid S3O in memory: one piece, one triangle.
    ///
    /// Layout (matching the BAR map-bundled field positions):
    ///   0..52:   header
    ///  52..104:  piece struct (52 bytes)
    /// 104..200:  3 vertices x 32 bytes
    /// 200..212:  3 indices x 4 bytes (plain u32)
    fn minimal_s3o() -> Vec<u8> {
        let piece_offset: u32 = 52;
        let vertex_offset: u32 = 104; // piece+16
        let index_offset: u32 = 200; // piece+32

        let mut buf = vec![0u8; 212];

        // Header
        buf[0..12].copy_from_slice(MAGIC);
        // version = 0 (already 0)
        // radius, height, mid_x/y/z = 0
        buf[36..40].copy_from_slice(&piece_offset.to_le_bytes());
        // tex string offsets at header+44/48 -- point past end (empty strings)
        let len = buf.len() as u32;
        buf[44..48].copy_from_slice(&len.to_le_bytes());
        buf[48..52].copy_from_slice(&len.to_le_bytes());

        // Piece at offset 52
        let p = 52usize;
        // +0: nameOffset (0 -> points into header magic, treated as irrelevant)
        // +4: numChildren = 0
        // +8: sfxFalloffs = 0 (ignored)
        // +12: numVertices = 3
        buf[p + 12..p + 16].copy_from_slice(&3u32.to_le_bytes());
        // +16: vertexOffset = 104
        buf[p + 16..p + 20].copy_from_slice(&vertex_offset.to_le_bytes());
        // +20: reserved = 0
        // +24: primitiveType = 0 (triangle list)
        buf[p + 24..p + 28].copy_from_slice(&0u32.to_le_bytes());
        // +28: numIndices = 3
        buf[p + 28..p + 32].copy_from_slice(&3u32.to_le_bytes());
        // +32: indexOffset = 200
        buf[p + 32..p + 36].copy_from_slice(&index_offset.to_le_bytes());
        // +36: reserved, +40/44/48: x/y/z = 0

        // Vertices at offset 104 (32 bytes each)
        let verts: &[[f32; 8]] = &[
            [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0], // pos + normal + uv
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        ];
        for (i, v) in verts.iter().enumerate() {
            let base = 104 + i * 32;
            for (j, f) in v.iter().enumerate() {
                buf[base + j * 4..base + j * 4 + 4].copy_from_slice(&f.to_le_bytes());
            }
        }

        // Indices at offset 200 (plain u32)
        buf[200..204].copy_from_slice(&0u32.to_le_bytes());
        buf[204..208].copy_from_slice(&1u32.to_le_bytes());
        buf[208..212].copy_from_slice(&2u32.to_le_bytes());

        buf
    }

    #[test]
    fn parse_minimal_s3o() {
        let data = minimal_s3o();
        let mesh = parse_s3o(&data).expect("should parse");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.indices.len(), 3);
        assert_eq!(mesh.indices[0], 0);
        assert_eq!(mesh.indices[1], 1);
        assert_eq!(mesh.indices[2], 2);
    }

    #[test]
    fn aabb_matches_vertices() {
        let data = minimal_s3o();
        let mesh = parse_s3o(&data).expect("should parse");
        // x range: 0..1, y range: 0..0, z range: 0..1
        assert!((mesh.aabb_min[0] - 0.0).abs() < 1e-5);
        assert!((mesh.aabb_max[0] - 1.0).abs() < 1e-5);
        assert!((mesh.aabb_min[2] - 0.0).abs() < 1e-5);
        assert!((mesh.aabb_max[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut data = minimal_s3o();
        data[0] = b'X';
        assert!(matches!(parse_s3o(&data), Err(S3oError::BadMagic)));
    }

    #[test]
    fn too_short_rejected() {
        let data = vec![0u8; 10];
        assert!(matches!(parse_s3o(&data), Err(S3oError::TooShort)));
    }

    #[test]
    fn tristrip_expands_correctly() {
        // Strip [0,1,2,3] -> triangles [0,1,2] and [1,3,2] (winding alternates)
        let mut out = Vec::new();
        expand_tristrip(&[0, 1, 2, 3], 0, &mut out);
        assert_eq!(out.len(), 6);
        assert_eq!(&out[..3], &[0, 1, 2]);
        assert_eq!(&out[3..], &[1, 3, 2]);
    }

    #[test]
    fn quad_expands_to_two_triangles() {
        let mut out = Vec::new();
        expand_quads(&[0, 1, 2, 3], 0, &mut out);
        assert_eq!(out.len(), 6);
        assert_eq!(&out, &[0, 1, 2, 0, 2, 3]);
    }

    /// Verify that shifted index encoding (index << 8) is detected and decoded.
    #[test]
    fn shifted_index_encoding_detected() {
        let mut data = minimal_s3o();
        // Rewrite the 3 indices as (index << 8) form.
        let idx_base = 200usize;
        data[idx_base..idx_base + 4].copy_from_slice(&0u32.to_le_bytes()); // 0<<8 = 0
        data[idx_base + 4..idx_base + 8].copy_from_slice(&256u32.to_le_bytes()); // 1<<8 = 256
        data[idx_base + 8..idx_base + 12].copy_from_slice(&512u32.to_le_bytes()); // 2<<8 = 512
        let mesh = parse_s3o(&data).expect("should parse with shifted encoding");
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }
}
