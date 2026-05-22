//! Metal-spot detection for BME's editor overlay.
//!
//! Ports the gameplay-side spot finder
//! (`bar-game/common/upgets/api_resource_spot_finder.lua`) -- the
//! algorithm engine and `gui_metalspots` reads at runtime to render
//! the rotating circles + worth labels over each cluster of metal
//! cells. Replicated in Rust so BME's editor viewport can show the
//! same overlay against the in-project metalmap, including before a
//! Test-in-BAR roundtrip.
//!
//! The engine uses a strip-scan that's optimal under Lua iteration
//! cost; we replace it with a 4-connected BFS flood fill. Both group
//! the same connected non-zero cells into clusters; BFS is direct
//! in Rust and produces clusters that match the engine's groupings
//! to within a few centroid pixels (the engine's strip-scan can fuse
//! two clusters that share a single-row bridge, BFS does the same).
//!
//! Output values match the engine's:
//! - `worth` is the SUM of `groundMetal` cell values in the cluster,
//!   in raw u8 metalmap units. `gui_metalspots` displays
//!   `worth * incomeMultiplier / 1000` as the floating text and gates
//!   on `0.001 < value < 15` (so a worth of 1 .. 14999 in
//!   metalmap-byte units survives the display cull).
//! - `(x_elmo, z_elmo)` is the value-weighted centroid in world
//!   elmos -- the same point the engine widget rotates the circle
//!   around.

/// One metal-spot cluster detected in a metalmap.
#[derive(Debug, Clone, Copy)]
pub struct MetalSpot {
    /// Cluster centroid X in world elmos.
    pub x_elmo: f32,
    /// Cluster centroid Z in world elmos.
    pub z_elmo: f32,
    /// Sum of metalmap-byte values across all cells in the cluster.
    /// The engine displays `worth * incomeMultiplier / 1000` as the
    /// floating "metal / sec" label.
    pub worth: u32,
}

/// Scan a metalmap (one u8 per cell) and return every connected
/// cluster of non-zero cells as a `MetalSpot`. `width` and `height`
/// are the metalmap dimensions; `map_w_elmos` and `map_h_elmos` are
/// the playable map size in elmos (Spring `Game.mapSizeX` /
/// `Game.mapSizeZ`). Centroid positions are in elmos relative to the
/// map origin (top-left = `(0, 0)`).
///
/// Empty / degenerate input returns an empty list. Cells are walked
/// with 4-connectivity (N, S, E, W) so diagonal touches don't fuse
/// otherwise-separate clusters, matching the engine widget's
/// strip-scan behaviour.
pub fn find_metal_spots(
    pixels: &[u8],
    width: u32,
    height: u32,
    map_w_elmos: f32,
    map_h_elmos: f32,
) -> Vec<MetalSpot> {
    if width == 0 || height == 0 || pixels.len() != (width as usize) * (height as usize) {
        return Vec::new();
    }
    let cell_w_elmos = map_w_elmos / width as f32;
    let cell_h_elmos = map_h_elmos / height as f32;

    let mut visited = vec![false; pixels.len()];
    let mut spots = Vec::new();
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    for start in 0..pixels.len() {
        if visited[start] || pixels[start] == 0 {
            continue;
        }
        queue.clear();
        queue.push_back(start);
        visited[start] = true;
        let mut total_worth: u64 = 0;
        let mut weighted_x: f64 = 0.0;
        let mut weighted_y: f64 = 0.0;
        let mut total_weight: f64 = 0.0;
        while let Some(idx) = queue.pop_front() {
            let v = pixels[idx] as u64;
            total_worth += v;
            let x = (idx as u32) % width;
            let y = (idx as u32) / width;
            let w = v as f64;
            weighted_x += (x as f64) * w;
            weighted_y += (y as f64) * w;
            total_weight += w;
            for (nx, ny) in neighbours_4(x, y, width, height) {
                let nidx = (ny * width + nx) as usize;
                if !visited[nidx] && pixels[nidx] != 0 {
                    visited[nidx] = true;
                    queue.push_back(nidx);
                }
            }
        }
        if total_weight > 0.0 {
            // +0.5 puts the position at the CENTRE of the cell rather
            // than its top-left corner, matching the engine widget's
            // `mx + halfSquare` strip-scan convention.
            let cx_cells = weighted_x / total_weight + 0.5;
            let cy_cells = weighted_y / total_weight + 0.5;
            spots.push(MetalSpot {
                x_elmo: (cx_cells as f32) * cell_w_elmos,
                z_elmo: (cy_cells as f32) * cell_h_elmos,
                worth: total_worth.min(u32::MAX as u64) as u32,
            });
        }
    }
    spots
}

fn neighbours_4(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ]
    .into_iter()
    .filter(move |&(nx, ny)| nx < width && ny < height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_metalmap_yields_no_spots() {
        let pixels = vec![0u8; 16];
        let spots = find_metal_spots(&pixels, 4, 4, 32.0, 32.0);
        assert!(spots.is_empty());
    }

    #[test]
    fn single_pixel_spot_centroid_is_pixel_centre() {
        // 4x4 grid, single metal cell at (2, 1) with value 50. The
        // centroid should land at the centre of that cell in elmo
        // space. Cell width = 32/4 = 8 elmos; centre of cell (2, 1)
        // is at elmo (2.5 * 8, 1.5 * 8) = (20, 12).
        let mut pixels = vec![0u8; 16];
        pixels[6] = 50; // (2, 1) in row-major 4x4
        let spots = find_metal_spots(&pixels, 4, 4, 32.0, 32.0);
        assert_eq!(spots.len(), 1);
        assert!((spots[0].x_elmo - 20.0).abs() < 1e-3);
        assert!((spots[0].z_elmo - 12.0).abs() < 1e-3);
        assert_eq!(spots[0].worth, 50);
    }

    #[test]
    fn adjacent_cells_fuse_into_one_spot() {
        // A 2x2 block of value-100 cells at (1..=2, 1..=2). One
        // cluster, worth = 4 * 100 = 400, centroid at the block
        // centre.
        let mut pixels = vec![0u8; 16];
        for x in 1..=2 {
            for y in 1..=2 {
                pixels[(y * 4 + x) as usize] = 100;
            }
        }
        let spots = find_metal_spots(&pixels, 4, 4, 32.0, 32.0);
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0].worth, 400);
        // Block spans cells (1, 1)..(2, 2); equal-weight centroid is
        // (1.5 cells, 1.5 cells) -> elmos (1.5 * 8 + 0.5*8/... wait
        // — function adds +0.5 to convert top-left to centre, then
        // multiplies by cell width. Mean cell index is (1+2)/2 = 1.5;
        // centroid lookup yields (1.5 + 0.5) * 8 = 16.
        assert!((spots[0].x_elmo - 16.0).abs() < 1e-3);
        assert!((spots[0].z_elmo - 16.0).abs() < 1e-3);
    }

    #[test]
    fn diagonal_cells_do_not_fuse() {
        // Two metal cells diagonally adjacent at (1, 1) and (2, 2).
        // Engine strip-scan and our BFS both treat them as separate
        // clusters because neither has a 4-connected neighbour.
        let mut pixels = vec![0u8; 16];
        pixels[5] = 50; // (1, 1)
        pixels[10] = 50; // (2, 2)
        let spots = find_metal_spots(&pixels, 4, 4, 32.0, 32.0);
        assert_eq!(spots.len(), 2);
    }

    #[test]
    fn weighted_centroid_pulls_toward_brighter_cell() {
        // Two adjacent cells: value 1 at (0, 0), value 99 at (1, 0).
        // Equal-weight centroid would be (0.5 + 0.5) * cell_w = 8;
        // weighted centroid pulls toward the 99 cell.
        let mut pixels = vec![0u8; 4];
        pixels[0] = 1;
        pixels[1] = 99;
        let spots = find_metal_spots(&pixels, 2, 2, 16.0, 16.0);
        assert_eq!(spots.len(), 1);
        // Cell width = 16 / 2 = 8 elmos. Weighted column is
        // (0*1 + 1*99)/100 = 0.99; +0.5 -> 1.49; *8 -> 11.92.
        assert!(
            (spots[0].x_elmo - 11.92).abs() < 0.01,
            "weighted centroid should pull toward bright cell, got x = {}",
            spots[0].x_elmo,
        );
        assert_eq!(spots[0].worth, 100);
    }

    #[test]
    fn mismatched_input_length_returns_empty() {
        // Defensive: caller passed a buffer that doesn't match
        // width*height. Don't panic; return nothing.
        let spots = find_metal_spots(&[0, 1, 2], 4, 4, 32.0, 32.0);
        assert!(spots.is_empty());
    }
}
