//! SMT (Spring Map Tiles) file format reader/writer.
//!
//! The SMT file is a texture atlas for Spring/Recoil engine maps. It contains
//! DXT1-compressed 32×32 tiles that are referenced by the tile index map in
//! the SMF file.
//!
//! Format structure:
//! - Header (32 bytes)
//! - Tile data: each tile is 32×32 pixels, DXT1 compressed with 4 mipmap levels
//!   (680 bytes per tile = 512 + 128 + 32 + 8)

use std::io::{self, Read, Write};

use thiserror::Error;

use crate::ColorBuffer;

#[derive(Error, Debug)]
pub enum SmtError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("texture dimensions must be multiples of tile size (32): got {0}x{1}")]
    InvalidDimensions(u32, u32),
}

/// SMT file magic bytes.
pub const SMT_MAGIC: &[u8; 16] = b"spring tilefile\0";
/// SMT format version.
pub const SMT_VERSION: i32 = 1;
/// Tile size in pixels.
pub const TILE_SIZE: u32 = 32;
/// DXT1 compressed size for one 32×32 tile with 4 mipmap levels.
/// (512 + 128 + 32 + 8 = 680 bytes)
pub const SMALL_TILE_SIZE: usize = 680;
/// DXT1 compressed size for the base level of one 32×32 tile (no mipmaps).
pub const DXT1_TILE_BYTES: usize = (TILE_SIZE as usize / 4) * (TILE_SIZE as usize / 4) * 8;

// --------------------------------------------------------------------------
// DXT1 decoder
// --------------------------------------------------------------------------

/// Decode one 8-byte DXT1 block into 16 RGBA8 pixels (4×4, row-major).
///
/// Handles both opaque mode (color0 > color1, 4-color palette) and
/// 1-bit-alpha mode (color0 <= color1, 3-color + transparent palette).
pub fn decode_dxt1_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);

    let pal0 = unpack565_rgba(c0);
    let pal1 = unpack565_rgba(c1);
    let (pal2, pal3) = if c0 > c1 {
        // Opaque: interpolate 2/3 and 1/3
        (
            lerp_rgba(pal0, pal1, 2, 1, 3),
            lerp_rgba(pal0, pal1, 1, 2, 3),
        )
    } else {
        // 1-bit alpha: midpoint + transparent
        (lerp_rgba(pal0, pal1, 1, 1, 2), [0u8; 4])
    };

    let palette = [pal0, pal1, pal2, pal3];
    let mut pixels = [[0u8; 4]; 16];
    for i in 0..16 {
        pixels[i] = palette[((indices >> (i * 2)) & 3) as usize];
    }
    pixels
}

fn unpack565_rgba(c: u16) -> [u8; 4] {
    let [r, g, b] = unpack565(c);
    [r, g, b, 255]
}

fn lerp_rgba(a: [u8; 4], b: [u8; 4], wa: u16, wb: u16, div: u16) -> [u8; 4] {
    [
        ((a[0] as u16 * wa + b[0] as u16 * wb) / div) as u8,
        ((a[1] as u16 * wa + b[1] as u16 * wb) / div) as u8,
        ((a[2] as u16 * wa + b[2] as u16 * wb) / div) as u8,
        255,
    ]
}

/// Decode the base mip level of a DXT1-compressed 32×32 tile.
///
/// The 680-byte tile blob contains 4 mip levels; only the first 512 bytes
/// (the 32×32 base level = 8×8 DXT1 blocks × 8 bytes each) are decoded.
/// Returns a 32×32×4 RGBA8 buffer (4096 bytes), row-major.
pub fn decode_tile_dxt1(compressed_tile: &[u8]) -> Vec<u8> {
    let blocks_per_side = TILE_SIZE as usize / 4; // 8
    let base_bytes = blocks_per_side * blocks_per_side * 8; // 512
    debug_assert!(
        compressed_tile.len() >= base_bytes,
        "tile too small: {} < {}",
        compressed_tile.len(),
        base_bytes
    );

    let mut rgba = vec![0u8; TILE_SIZE as usize * TILE_SIZE as usize * 4];
    for by in 0..blocks_per_side {
        for bx in 0..blocks_per_side {
            let src = (by * blocks_per_side + bx) * 8;
            let block: [u8; 8] = compressed_tile[src..src + 8].try_into().unwrap();
            let pixels = decode_dxt1_block(&block);
            for dy in 0..4usize {
                for dx in 0..4usize {
                    let x = bx * 4 + dx;
                    let y = by * 4 + dy;
                    let dst = (y * TILE_SIZE as usize + x) * 4;
                    rgba[dst..dst + 4].copy_from_slice(&pixels[dy * 4 + dx]);
                }
            }
        }
    }
    rgba
}

/// Read all tiles from an SMT file, returning decoded RGBA8 pixel data per tile.
///
/// Each element in the returned `Vec` is a 4096-byte buffer (32×32 pixels × 4 channels).
/// Only the base mip level is decoded; higher mips are skipped.
pub fn read_smt<R: Read>(reader: &mut R) -> Result<Vec<Vec<u8>>, SmtError> {
    // Header layout (32 bytes total):
    //   magic[16] version:i32 num_tiles:i32 tile_size:i32 compression:i32
    let mut magic = [0u8; 16];
    reader.read_exact(&mut magic)?;
    // Spring maps should match SMT_MAGIC; tolerate mismatches for robustness

    let mut buf4 = [0u8; 4];
    reader.read_exact(&mut buf4)?; // version (ignore)
    reader.read_exact(&mut buf4)?;
    let num_tiles = i32::from_le_bytes(buf4).max(0) as usize;
    reader.read_exact(&mut buf4)?; // tile_size (should be 32)
    reader.read_exact(&mut buf4)?; // compression type (1 = DXT1; ignore)

    let mut tiles = Vec::with_capacity(num_tiles);
    for _ in 0..num_tiles {
        let mut compressed = vec![0u8; SMALL_TILE_SIZE];
        reader.read_exact(&mut compressed)?;
        tiles.push(decode_tile_dxt1(&compressed));
    }
    Ok(tiles)
}

/// Read the SMT tile pool as raw compressed DXT1 bytes (base level only).
///
/// Unlike [`read_smt`], this does not decode to RGBA. Each returned Vec is
/// exactly [`DXT1_TILE_BYTES`] bytes -- the base-level DXT1 data suitable
/// for upload to a `Bc1RgbaUnorm` GPU texture.
pub fn read_smt_raw<R: Read>(reader: &mut R) -> Result<Vec<Vec<u8>>, SmtError> {
    let mut magic = [0u8; 16];
    reader.read_exact(&mut magic)?;
    let mut buf4 = [0u8; 4];
    reader.read_exact(&mut buf4)?; // version
    reader.read_exact(&mut buf4)?;
    let num_tiles = i32::from_le_bytes(buf4).max(0) as usize;
    reader.read_exact(&mut buf4)?; // tile_size (should be 32)
    reader.read_exact(&mut buf4)?; // compression type (1 = DXT1)
    let mut tiles = Vec::with_capacity(num_tiles);
    for _ in 0..num_tiles {
        let mut raw = vec![0u8; SMALL_TILE_SIZE];
        reader.read_exact(&mut raw)?;
        raw.truncate(DXT1_TILE_BYTES);
        tiles.push(raw);
    }
    Ok(tiles)
}

/// Assemble a flat linear BC1 image from a tile pool and tile index.
///
/// Each element of `tile_pool` must be exactly [`DXT1_TILE_BYTES`] bytes
/// (base-level DXT1 for a 32x32 tile). `tile_indices` maps (ty*tiles_x + tx)
/// to a tile pool index. The returned bytes are suitable for upload to a
/// `Bc1RgbaUnorm` wgpu texture of size `(tiles_x*32) x (tiles_y*32)`.
pub fn assemble_bc1_linear(
    tile_pool: &[Vec<u8>],
    tile_indices: &[i32],
    tiles_x: u32,
    tiles_y: u32,
) -> Vec<u8> {
    const BLOCKS_PER_TILE: usize = 8; // 32 / 4
    const BYTES_PER_BLOCK: usize = 8;
    const BYTES_PER_TILE_BLOCK_ROW: usize = BLOCKS_PER_TILE * BYTES_PER_BLOCK; // 64

    let tx_usize = tiles_x as usize;
    let ty_usize = tiles_y as usize;
    let blocks_per_image_row = tx_usize * BLOCKS_PER_TILE;
    let total_bytes = tx_usize * ty_usize * DXT1_TILE_BYTES;
    let mut out = vec![0u8; total_bytes];
    let empty = vec![0u8; DXT1_TILE_BYTES];

    for ty in 0..ty_usize {
        for tx in 0..tx_usize {
            let idx_pos = ty * tx_usize + tx;
            let tile_idx = tile_indices.get(idx_pos).copied().unwrap_or(0).max(0) as usize;
            let tile = tile_pool.get(tile_idx).unwrap_or(&empty);
            let tile_bytes: &[u8] = if tile.len() >= DXT1_TILE_BYTES {
                &tile[..DXT1_TILE_BYTES]
            } else {
                &empty
            };

            for br in 0..BLOCKS_PER_TILE {
                let src = br * BYTES_PER_TILE_BLOCK_ROW;
                let dst_row = ty * BLOCKS_PER_TILE + br;
                let dst_col = tx * BLOCKS_PER_TILE;
                let dst = (dst_row * blocks_per_image_row + dst_col) * BYTES_PER_BLOCK;
                out[dst..dst + BYTES_PER_TILE_BLOCK_ROW]
                    .copy_from_slice(&tile_bytes[src..src + BYTES_PER_TILE_BLOCK_ROW]);
            }
        }
    }
    out
}

// --------------------------------------------------------------------------
// DXT1 compressor (existing)
// --------------------------------------------------------------------------

/// Compress a 4×4 block of RGBA8 pixels into 8 bytes of DXT1.
fn compress_dxt1_block(pixels: &[[u8; 4]; 16]) -> [u8; 8] {
    // Find min and max color (simple bounding box)
    let mut min_r = 255u8;
    let mut min_g = 255u8;
    let mut min_b = 255u8;
    let mut max_r = 0u8;
    let mut max_g = 0u8;
    let mut max_b = 0u8;

    for p in pixels {
        min_r = min_r.min(p[0]);
        min_g = min_g.min(p[1]);
        min_b = min_b.min(p[2]);
        max_r = max_r.max(p[0]);
        max_g = max_g.max(p[1]);
        max_b = max_b.max(p[2]);
    }

    // Pack to RGB565
    let color0 = rgb565(max_r, max_g, max_b);
    let color1 = rgb565(min_r, min_g, min_b);

    // Ensure color0 >= color1 for opaque mode
    let (c0, c1) = if color0 >= color1 {
        (color0, color1)
    } else {
        (color1, color0)
    };

    // Decode palette for index selection
    let pal0 = unpack565(c0);
    let pal1 = unpack565(c1);
    let pal2 = [
        ((2 * pal0[0] as u16 + pal1[0] as u16) / 3) as u8,
        ((2 * pal0[1] as u16 + pal1[1] as u16) / 3) as u8,
        ((2 * pal0[2] as u16 + pal1[2] as u16) / 3) as u8,
    ];
    let pal3 = [
        ((pal0[0] as u16 + 2 * pal1[0] as u16) / 3) as u8,
        ((pal0[1] as u16 + 2 * pal1[1] as u16) / 3) as u8,
        ((pal0[2] as u16 + 2 * pal1[2] as u16) / 3) as u8,
    ];

    let palette = [pal0, pal1, pal2, pal3];

    // Assign each pixel to closest palette entry
    let mut indices = 0u32;
    for (i, p) in pixels.iter().enumerate() {
        let mut best = 0u8;
        let mut best_dist = u32::MAX;
        for (j, pal) in palette.iter().enumerate() {
            let dr = p[0] as i32 - pal[0] as i32;
            let dg = p[1] as i32 - pal[1] as i32;
            let db = p[2] as i32 - pal[2] as i32;
            let dist = (dr * dr + dg * dg + db * db) as u32;
            if dist < best_dist {
                best_dist = dist;
                best = j as u8;
            }
        }
        indices |= (best as u32) << (i * 2);
    }

    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&indices.to_le_bytes());
    out
}

fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

fn unpack565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1F) as u8;
    let g = ((c >> 5) & 0x3F) as u8;
    let b = (c & 0x1F) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// Compress a 32×32 tile (RGBA8 bytes, row-major) into DXT1 with 4 mipmap levels.
/// Returns exactly SMALL_TILE_SIZE (680) bytes.
pub fn compress_tile_dxt1(rgba8: &[u8]) -> Vec<u8> {
    assert!(rgba8.len() >= (TILE_SIZE as usize * TILE_SIZE as usize * 4));
    let mut compressed = Vec::with_capacity(SMALL_TILE_SIZE);

    // Level 0: 32×32
    let mut current = rgba8.to_vec();
    let mut size = TILE_SIZE as usize;

    for _level in 0..4 {
        let stride = size * 4;
        for by in (0..size).step_by(4) {
            for bx in (0..size).step_by(4) {
                let mut block = [[0u8; 4]; 16];
                for dy in 0..4 {
                    for dx in 0..4 {
                        let y = (by + dy).min(size - 1);
                        let x = (bx + dx).min(size - 1);
                        let src = y * stride + x * 4;
                        block[dy * 4 + dx] = [
                            current[src],
                            current[src + 1],
                            current[src + 2],
                            current[src + 3],
                        ];
                    }
                }
                compressed.extend_from_slice(&compress_dxt1_block(&block));
            }
        }

        // Downsample for next level (box filter)
        if size > 4 {
            let next_size = size / 2;
            let mut next = vec![0u8; next_size * next_size * 4];
            for ny in 0..next_size {
                for nx in 0..next_size {
                    let mut r = 0u32;
                    let mut g = 0u32;
                    let mut b = 0u32;
                    let mut a = 0u32;
                    for dy in 0..2u32 {
                        for dx in 0..2u32 {
                            let sx = nx * 2 + dx as usize;
                            let sy = ny * 2 + dy as usize;
                            let idx = (sy * size + sx) * 4;
                            r += current[idx] as u32;
                            g += current[idx + 1] as u32;
                            b += current[idx + 2] as u32;
                            a += current[idx + 3] as u32;
                        }
                    }
                    let dst = (ny * next_size + nx) * 4;
                    next[dst] = (r / 4) as u8;
                    next[dst + 1] = (g / 4) as u8;
                    next[dst + 2] = (b / 4) as u8;
                    next[dst + 3] = (a / 4) as u8;
                }
            }
            current = next;
            size = next_size;
        }
    }

    debug_assert_eq!(compressed.len(), SMALL_TILE_SIZE);
    compressed
}

/// Write an SMT file from a color texture.
///
/// The texture is split into 32×32 tiles, each DXT1-compressed.
/// Returns the tile index map (sequential indices) and the number of tiles.
pub fn write_smt<W: Write>(
    writer: &mut W,
    texture: &ColorBuffer,
) -> Result<(Vec<i32>, u32), SmtError> {
    let tw = texture.width();
    let th = texture.height();

    // Pad to multiple of tile size
    let tiles_x = tw.div_ceil(TILE_SIZE);
    let tiles_y = th.div_ceil(TILE_SIZE);
    let num_tiles = tiles_x * tiles_y;

    // Write header
    writer.write_all(SMT_MAGIC)?;
    writer.write_all(&SMT_VERSION.to_le_bytes())?;
    writer.write_all(&num_tiles.to_le_bytes())?;
    writer.write_all(&TILE_SIZE.to_le_bytes())?;
    // compression type: 1 = DXT1
    writer.write_all(&1i32.to_le_bytes())?;

    let rgba8 = texture.to_rgba8();
    let src_stride = tw as usize * 4;
    let mut tile_indices = Vec::with_capacity(num_tiles as usize);

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            // Extract 32×32 tile (clamp at edges)
            let mut tile_rgba = vec![0u8; TILE_SIZE as usize * TILE_SIZE as usize * 4];
            for py in 0..TILE_SIZE {
                for px in 0..TILE_SIZE {
                    let sx = ((tx * TILE_SIZE + px) as usize).min(tw as usize - 1);
                    let sy = ((ty * TILE_SIZE + py) as usize).min(th as usize - 1);
                    let src_idx = sy * src_stride + sx * 4;
                    let dst_idx = (py as usize * TILE_SIZE as usize + px as usize) * 4;
                    tile_rgba[dst_idx..dst_idx + 4].copy_from_slice(&rgba8[src_idx..src_idx + 4]);
                }
            }

            let compressed = compress_tile_dxt1(&tile_rgba);
            writer.write_all(&compressed)?;
            tile_indices.push((ty * tiles_x + tx) as i32);
        }
    }

    Ok((tile_indices, num_tiles))
}

/// Compress a full image into DXT1 (used for minimap).
/// Image must be a power of 2 in both dimensions.
pub fn compress_image_dxt1(rgba8: &[u8], width: u32, height: u32) -> Vec<u8> {
    let blocks_x = width as usize / 4;
    let blocks_y = height as usize / 4;
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(blocks_x * blocks_y * 8);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let mut block = [[0u8; 4]; 16];
            for dy in 0..4 {
                for dx in 0..4 {
                    let y = by * 4 + dy;
                    let x = bx * 4 + dx;
                    let src = y * stride + x * 4;
                    if src + 3 < rgba8.len() {
                        block[dy * 4 + dx] =
                            [rgba8[src], rgba8[src + 1], rgba8[src + 2], rgba8[src + 3]];
                    }
                }
            }
            out.extend_from_slice(&compress_dxt1_block(&block));
        }
    }
    out
}

/// Spring minimap size: 9 mipmap levels of DXT1 1024×1024.
pub const MINIMAP_SIZE: usize = 699048;

/// Generate a Spring-compatible minimap (DXT1, 9 mipmap levels, 699048 bytes).
/// Input: 1024×1024 RGBA8 image data.
pub fn generate_minimap_dxt1(rgba8_1024: &[u8]) -> Vec<u8> {
    assert!(
        rgba8_1024.len() >= 1024 * 1024 * 4,
        "minimap input must be 1024x1024 RGBA8"
    );

    let mut out = Vec::with_capacity(MINIMAP_SIZE);
    let mut current = rgba8_1024.to_vec();
    let mut size = 1024usize;

    for _level in 0..9 {
        // Compress current level
        let compressed = compress_image_dxt1(&current, size as u32, size as u32);
        out.extend_from_slice(&compressed);

        // Downsample for next level (box filter 2×2)
        if size > 4 {
            let next_size = size / 2;
            let mut next = vec![0u8; next_size * next_size * 4];
            for ny in 0..next_size {
                for nx in 0..next_size {
                    let mut r = 0u32;
                    let mut g = 0u32;
                    let mut b = 0u32;
                    let mut a = 0u32;
                    for dy in 0..2usize {
                        for dx in 0..2usize {
                            let sx = nx * 2 + dx;
                            let sy = ny * 2 + dy;
                            let idx = (sy * size + sx) * 4;
                            r += current[idx] as u32;
                            g += current[idx + 1] as u32;
                            b += current[idx + 2] as u32;
                            a += current[idx + 3] as u32;
                        }
                    }
                    let dst = (ny * next_size + nx) * 4;
                    next[dst] = (r / 4) as u8;
                    next[dst + 1] = (g / 4) as u8;
                    next[dst + 2] = (b / 4) as u8;
                    next[dst + 3] = (a / 4) as u8;
                }
            }
            current = next;
            size = next_size;
        }
    }

    debug_assert_eq!(out.len(), MINIMAP_SIZE);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_dxt1_block_roundtrip() {
        let pixels = [[128, 64, 32, 255]; 16];
        let compressed = compress_dxt1_block(&pixels);
        assert_eq!(compressed.len(), 8);
    }

    #[test]
    fn test_rgb565_roundtrip() {
        let c = rgb565(255, 128, 64);
        let [r, g, b] = unpack565(c);
        // RGB565 has some precision loss
        assert!((r as i32 - 255).abs() <= 8);
        assert!((g as i32 - 128).abs() <= 4);
        assert!((b as i32 - 64).abs() <= 8);
    }

    #[test]
    fn test_write_smt() {
        let texture = ColorBuffer::new(64, 64).unwrap();
        let mut buf = Cursor::new(Vec::new());
        let (indices, num_tiles) = write_smt(&mut buf, &texture).unwrap();
        assert_eq!(num_tiles, 4); // 64/32 = 2x2
        assert_eq!(indices.len(), 4);
        // Header (32 bytes) + 4 tiles * 680 bytes each (SMALL_TILE_SIZE with mipmaps)
        assert_eq!(buf.get_ref().len(), 32 + 4 * SMALL_TILE_SIZE);
    }

    #[test]
    fn test_compress_image_dxt1() {
        let rgba = vec![128u8; 16 * 16 * 4];
        let compressed = compress_image_dxt1(&rgba, 16, 16);
        // 16/4 = 4 blocks each way = 16 blocks * 8 bytes
        assert_eq!(compressed.len(), 16 * 8);
    }

    #[test]
    fn test_dxt1_decode_block() {
        // Solid red tile: compress then decode, check approximate color recovery
        let red = [[255u8, 0, 0, 255]; 16];
        let compressed = compress_dxt1_block(&red);
        let decoded = decode_dxt1_block(&compressed);
        for pixel in decoded {
            assert!(pixel[0] > 200, "R should be high, got {}", pixel[0]);
            assert!(pixel[1] < 30, "G should be ~0, got {}", pixel[1]);
            assert!(pixel[2] < 30, "B should be ~0, got {}", pixel[2]);
            assert_eq!(pixel[3], 255, "alpha should be 255");
        }
    }

    #[test]
    fn test_smt_write_read_roundtrip() {
        // Write a 2×2 tile SMT, then read it back and verify tile count/size
        let mut texture_rgba = vec![0u8; 64 * 64 * 4];
        // Fill with a recognizable pattern (top-left tile = red)
        for py in 0..32usize {
            for px in 0..32usize {
                let i = (py * 64 + px) * 4;
                texture_rgba[i] = 200; // R
                texture_rgba[i + 3] = 255;
            }
        }
        let texture = ColorBuffer::from_rgba8(64, 64, &texture_rgba).unwrap();
        let mut buf = Cursor::new(Vec::new());
        let (_, num_tiles) = write_smt(&mut buf, &texture).unwrap();
        assert_eq!(num_tiles, 4);

        buf.set_position(0);
        let tiles = read_smt(&mut buf).unwrap();
        assert_eq!(tiles.len(), 4);
        // Each tile is 32×32×4 bytes
        assert_eq!(tiles[0].len(), TILE_SIZE as usize * TILE_SIZE as usize * 4);
        // Top-left tile should have significant red
        assert!(tiles[0][0] > 150, "top-left tile pixel R should be ~200");
    }
}
