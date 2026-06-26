//! Procedural tileable detail-normal maps.
//!
//! Generates a seamlessly-tiling tangent-space normal map from periodic
//! fractal noise -- the map's `detailNormalTex`: high-frequency close-up
//! surface bump (rock grain, gravel) the heightmap is too coarse to encode.
//! Tiling is exact because the noise lattice wraps at an integer period and
//! the Sobel pass samples neighbours with wraparound.

use crate::ColorBuffer;

const GRADS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (0.707_106_77, 0.707_106_77),
    (-0.707_106_77, 0.707_106_77),
    (0.707_106_77, -0.707_106_77),
    (-0.707_106_77, -0.707_106_77),
];

/// Built-in detail-normal styles surfaced in the editor's surface-detail picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailNormalPreset {
    Rock,
    Gravel,
    Sand,
}

impl DetailNormalPreset {
    /// (base lattice period in cells, octaves, persistence, base bump strength).
    fn params(self) -> (u32, u32, f32, f32) {
        match self {
            DetailNormalPreset::Rock => (4, 4, 0.5, 3.0),
            DetailNormalPreset::Gravel => (8, 4, 0.55, 2.2),
            DetailNormalPreset::Sand => (16, 2, 0.4, 1.1),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DetailNormalPreset::Rock => "Rock",
            DetailNormalPreset::Gravel => "Gravel",
            DetailNormalPreset::Sand => "Sand",
        }
    }
}

fn hash(ix: i32, iy: i32, seed: u32) -> u32 {
    let mut n = (ix as u32)
        .wrapping_mul(1_597_334_677)
        .wrapping_add((iy as u32).wrapping_mul(3_812_015_801))
        .wrapping_add(seed.wrapping_mul(2_654_435_761));
    n = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((n >> ((n >> 28).wrapping_add(4))) ^ n).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Periodic gradient noise: lattice cells wrap at `period`, so sampling
/// px/py over [0, period] tiles seamlessly.
fn pnoise(px: f32, py: f32, period: i32, seed: u32) -> f32 {
    let x0 = px.floor();
    let y0 = py.floor();
    let (ix, iy) = (x0 as i32, y0 as i32);
    let (fx, fy) = (px - x0, py - y0);
    let g = |cx: i32, cy: i32| {
        GRADS[(hash(cx.rem_euclid(period), cy.rem_euclid(period), seed) & 7) as usize]
    };
    let (g00, g10, g01, g11) = (g(ix, iy), g(ix + 1, iy), g(ix, iy + 1), g(ix + 1, iy + 1));
    let d00 = g00.0 * fx + g00.1 * fy;
    let d10 = g10.0 * (fx - 1.0) + g10.1 * fy;
    let d01 = g01.0 * fx + g01.1 * (fy - 1.0);
    let d11 = g11.0 * (fx - 1.0) + g11.1 * (fy - 1.0);
    let (u, v) = (quintic(fx), quintic(fy));
    let a = d00 + u * (d10 - d00);
    let b = d01 + u * (d11 - d01);
    a + v * (b - a)
}

/// Tileable fractal bump height in [0, 1] at normalized coords (u, v).
fn bump(u: f32, v: f32, base_period: u32, octaves: u32, persistence: f32, seed: u32) -> f32 {
    let mut value = 0.0;
    let mut amp = 1.0;
    let mut max = 0.0;
    let mut period = base_period as i32;
    for _ in 0..octaves.max(1) {
        value += pnoise(u * period as f32, v * period as f32, period, seed) * amp;
        max += amp;
        amp *= persistence;
        period *= 2; // lacunarity 2 keeps every octave's period an integer -> still tiles
    }
    (value / max.max(1e-6)) * 0.5 + 0.5
}

/// Generate a `size`x`size` seamlessly-tiling tangent-space normal map for the
/// given preset. `strength` (>= 0, 1.0 = preset default) scales bump intensity.
pub fn generate_detail_normal(preset: DetailNormalPreset, size: u32, strength: f32) -> ColorBuffer {
    let (period, octaves, persistence, base_strength) = preset.params();
    let s = base_strength * strength.max(0.0);
    let n = size.max(2);

    let mut hgt = vec![0.0f32; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let u = x as f32 / n as f32;
            let v = y as f32 / n as f32;
            hgt[(y * n + x) as usize] = bump(u, v, period, octaves, persistence, 1337);
        }
    }
    let at = |x: i32, y: i32| {
        let xx = x.rem_euclid(n as i32) as u32;
        let yy = y.rem_euclid(n as i32) as u32;
        hgt[(yy * n + xx) as usize]
    };

    let mut color = ColorBuffer::new(n, n).unwrap();
    for y in 0..n as i32 {
        for x in 0..n as i32 {
            // Sobel with wraparound so the normals tile with the height field.
            let dx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1));
            let dy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1));
            let nx = -dx * s;
            let ny = -dy * s;
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
    fn valid_normals_and_seamless_tiling() {
        let n = 64u32;
        let nm = generate_detail_normal(DetailNormalPreset::Rock, n, 1.0);

        // Every texel is a roughly-unit vector pointing up (z dominant).
        for y in [0, 31, 63] {
            for x in [0, 31, 63] {
                let c = nm.get(x, y).unwrap();
                assert!(c[2] > 0.5, "nz should point up at ({x},{y}): {c:?}");
                let (vx, vy, vz) = (c[0] * 2.0 - 1.0, c[1] * 2.0 - 1.0, c[2] * 2.0 - 1.0);
                let m = (vx * vx + vy * vy + vz * vz).sqrt();
                assert!((m - 1.0).abs() < 0.05, "normal not unit-length: {m}");
            }
        }

        // No hard seam: the wrap edge (col n-1 -> col 0) is as continuous as a
        // typical interior column step.
        let diff = |xa: u32, xb: u32| {
            (0..n)
                .map(|y| {
                    let a = nm.get(xa, y).unwrap();
                    let b = nm.get(xb, y).unwrap();
                    (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()
                })
                .sum::<f32>()
                / n as f32
        };
        let seam = diff(n - 1, 0);
        let interior = diff(1, 2).max(1e-4);
        assert!(
            seam <= interior * 2.5,
            "seam {seam} too large vs interior {interior}"
        );
    }
}
