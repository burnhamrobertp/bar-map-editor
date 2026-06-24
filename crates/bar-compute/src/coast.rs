//! CPU coastal/beach erosion.
//!
//! Reshapes terrain near a configured sea level into a gentler shoreline:
//! cells inside a band around `sea_level` are pulled toward a smooth beach
//! profile, near-shore land just above the band is dragged downward, and the
//! submerged seabed is box-blurred to settle silt.

use bar_data::Heightmap;

#[derive(Debug, Clone)]
pub struct CoastErosionParams {
    /// Water line in normalised height [0,1].
    pub sea_level: f32,
    /// Half-width of the beach band around `sea_level`, in height units.
    pub beach_size: f32,
    /// How strongly land just above the band is dragged down toward the
    /// shore (0 = untouched, 1 = pulled fully to the band's upper edge).
    pub inland_height_influence: f32,
    /// Box-blur iterations applied to submerged (below `sea_level`) cells.
    pub underwater_smoothing: u32,
}

impl Default for CoastErosionParams {
    fn default() -> Self {
        Self {
            sea_level: 0.3,
            beach_size: 0.05,
            inland_height_influence: 0.3,
            underwater_smoothing: 3,
        }
    }
}

/// Apply coastal erosion. Output is the same dimensions as `input`, clamped to
/// [0,1].
pub fn coast_erosion(input: &Heightmap, params: &CoastErosionParams) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let sea = params.sea_level;
    let beach = params.beach_size.max(0.0);
    let band_lo = sea - beach;
    let band_hi = sea + beach;

    let src = input.data();
    let mut out: Vec<f32> = Vec::with_capacity(src.len());

    for &v in src {
        let nv = if beach > 0.0 && v >= band_lo && v <= band_hi {
            // Inside the beach band: pull toward a smooth beach profile.
            // smoothstep maps the band onto a gentle S-curve in the same
            // [band_lo, band_hi] range, flattening mid-band so the slope
            // eases as it meets the water.
            let t = (v - band_lo) / (band_hi - band_lo);
            let eased = t * t * (3.0 - 2.0 * t);
            band_lo + eased * (band_hi - band_lo)
        } else if params.inland_height_influence > 0.0 && v > band_hi {
            // Just above the band: drag inland terrain down toward the shore,
            // with the pull fading out the higher (further inland) we go.
            let above = v - band_hi;
            let falloff = (1.0 - above / (beach.max(1e-4) * 4.0)).clamp(0.0, 1.0);
            v - above * params.inland_height_influence * falloff
        } else {
            v
        };

        out.push(nv.clamp(0.0, 1.0));
    }

    // Box-blur the submerged seabed only. Each iteration averages a cell with
    // its below-sea neighbours; land cells are left untouched.
    let wi = w as usize;
    let hi = h as usize;
    for _ in 0..params.underwater_smoothing {
        let prev = out.clone();
        for y in 0..hi {
            for x in 0..wi {
                let idx = y * wi + x;
                if prev[idx] >= sea {
                    continue;
                }
                let mut sum = 0.0;
                let mut n = 0.0;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        sum += prev[ny as usize * wi + nx as usize];
                        n += 1.0;
                    }
                }
                out[idx] = (sum / n).clamp(0.0, 1.0);
            }
        }
    }

    Heightmap::frbar_data(w, h, out).expect("coast_erosion preserves dimensions")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(w: u32, h: u32, data: Vec<f32>) -> Heightmap {
        Heightmap::frbar_data(w, h, data).unwrap()
    }

    fn variance(slice: &[f32]) -> f32 {
        let mean = slice.iter().sum::<f32>() / slice.len() as f32;
        slice.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / slice.len() as f32
    }

    #[test]
    fn far_cells_unchanged_at_low_influence() {
        // A cell far below sea level (and not submerged-smoothed because it's
        // isolated/flat) and one far above the band stay ~unchanged when the
        // inland influence is low and smoothing is off.
        let input = hm(2, 1, vec![0.9, 0.9]);
        let params = CoastErosionParams {
            sea_level: 0.3,
            beach_size: 0.05,
            inland_height_influence: 0.0,
            underwater_smoothing: 0,
        };
        let out = coast_erosion(&input, &params);
        assert!((out.data()[0] - 0.9).abs() < 1e-5);

        // Far below sea level, no smoothing: a uniform seabed averages to
        // itself, so it stays put.
        let below = hm(2, 1, vec![0.05, 0.05]);
        let out2 = coast_erosion(&below, &params);
        assert!((out2.data()[0] - 0.05).abs() < 1e-5);
    }

    #[test]
    fn underwater_variance_decreases_with_smoothing() {
        // A noisy seabed (all below sea level) should get flatter as smoothing
        // iterations increase.
        let data = vec![0.05, 0.20, 0.02, 0.18, 0.04, 0.22, 0.01, 0.19, 0.06];
        let input = hm(3, 3, data);
        let base = CoastErosionParams {
            sea_level: 0.3,
            beach_size: 0.05,
            inland_height_influence: 0.0,
            underwater_smoothing: 0,
        };
        let smoothed = CoastErosionParams { underwater_smoothing: 5, ..base.clone() };

        let v0 = variance(coast_erosion(&input, &base).data());
        let v5 = variance(coast_erosion(&input, &smoothed).data());
        assert!(v5 < v0, "smoothing should reduce seabed variance: {v5} !< {v0}");
    }

    #[test]
    fn output_stays_in_unit_range() {
        let data = vec![0.0, 0.31, 0.5, 1.0, 0.28, 0.33, 0.7, 0.1, 0.95];
        let input = hm(3, 3, data);
        let params = CoastErosionParams {
            sea_level: 0.3,
            beach_size: 0.1,
            inland_height_influence: 0.8,
            underwater_smoothing: 4,
        };
        let out = coast_erosion(&input, &params);
        for &v in out.data() {
            assert!((0.0..=1.0).contains(&v), "value {v} out of [0,1]");
        }
    }

    #[test]
    fn inland_terrain_is_dragged_down() {
        // A land cell just above the band should drop when influence is high.
        let input = hm(1, 1, vec![0.4]); // band_hi = 0.35, so this is above
        let params = CoastErosionParams {
            sea_level: 0.3,
            beach_size: 0.05,
            inland_height_influence: 0.8,
            underwater_smoothing: 0,
        };
        let out = coast_erosion(&input, &params);
        assert!(out.data()[0] < 0.4, "inland cell should be pulled down");
        assert!(out.data()[0] >= 0.35, "but not below the band's upper edge");
    }
}
