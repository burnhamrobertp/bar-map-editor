//! Downsampled texture preview assembly from decoded SMT tiles.

use bar_data::smt::TILE_SIZE;

/// Assemble a downsampled RGBA8 texture from decoded SMT tiles.
///
/// Uses nearest-neighbor sampling directly against the tile grid — no full-resolution
/// intermediate buffer is ever allocated. Each output pixel maps to one source texel.
pub(crate) fn assemble_texture_preview(
    tiles: &[Vec<u8>],
    tile_indices: &[i32],
    tiles_x: u32,
    tiles_y: u32,
    out_w: u32,
    out_h: u32,
) -> Vec<u8> {
    let src_w = tiles_x * TILE_SIZE;
    let src_h = tiles_y * TILE_SIZE;
    let tile_sz = TILE_SIZE as usize;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];

    for dy in 0..out_h {
        for dx in 0..out_w {
            // Map output pixel to nearest source texel
            let sx = (dx as u64 * src_w as u64 / out_w as u64) as u32;
            let sy = (dy as u64 * src_h as u64 / out_h as u64) as u32;

            let tile_x = (sx / TILE_SIZE).min(tiles_x.saturating_sub(1));
            let tile_y = (sy / TILE_SIZE).min(tiles_y.saturating_sub(1));
            let px = (sx % TILE_SIZE) as usize;
            let py = (sy % TILE_SIZE) as usize;

            let flat = (tile_y * tiles_x + tile_x) as usize;
            if let Some(&idx_raw) = tile_indices.get(flat) {
                if idx_raw >= 0 {
                    if let Some(tile) = tiles.get(idx_raw as usize) {
                        let src = (py * tile_sz + px) * 4;
                        let dst = (dy * out_w + dx) as usize * 4;
                        out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                    }
                }
            }
        }
    }
    out
}
