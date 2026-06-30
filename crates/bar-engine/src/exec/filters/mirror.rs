use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_input_heightmap, get_optional_heightmap, get_string,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let mode = get_string(ctx.params, "mode", "mirror_x");
    let hm = apply_mirror(&input, mode);
    let hm = apply_modulation(&input, hm, None, mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Average of the values at the listed `(x, y)` source positions. Used
/// by the `average_*` modes to fold all symmetric partners into a
/// single output that preserves information from every quadrant.
fn mean_at_positions(src: &[f32], w: usize, positions: &[(usize, usize)]) -> f32 {
    let mut sum = 0.0;
    for &(x, y) in positions {
        sum += src[y * w + x];
    }
    sum / positions.len() as f32
}

/// Smooth Hermite step: 0 at/below `edge0`, 1 at/above `edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn apply_mirror(input: &Heightmap, mode: &str) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let src = input.data();
    let at = |x: usize, y: usize| src[y * w + x];

    // Feather half-width (pixels) for the single-axis replace modes. An authored
    // half meets its copy at the split line; blending across a small band there
    // removes the hard seam while staying a crisp copy away from it. Weight is 1
    // on the canonical (low-index) side and 0 on the far side, so the result is
    // still exactly symmetric (w(1-x) = 1 - w(x)).
    let feather = (w.min(h) as f32 * 0.025).max(2.0);
    let cx = (w as f32 - 1.0) / 2.0;
    let cy = (h as f32 - 1.0) / 2.0;

    let mut data = vec![0.0f32; w * h];
    for py in 0..h {
        for px in 0..w {
            let (pxf, pyf) = (px as f32, py as f32);
            // Replace modes (`mirror_*` / `rotate_*`) copy one half/quadrant;
            // averaging modes (`average_*`) take the mean of every pixel in the
            // symmetric orbit so both halves contribute to the output.
            data[py * w + px] = match mode {
                "mirror_x" => {
                    let wt = smoothstep(-1.0, 1.0, (cx - pxf) / feather);
                    wt * at(px, py) + (1.0 - wt) * at(w - 1 - px, py)
                }
                "mirror_y" => {
                    let wt = smoothstep(-1.0, 1.0, (cy - pyf) / feather);
                    wt * at(px, py) + (1.0 - wt) * at(px, h - 1 - py)
                }
                "mirror_xy" => {
                    let sx = if px < w / 2 { px } else { w - 1 - px };
                    let sy = if py < h / 2 { py } else { h - 1 - py };
                    src[sy * w + sx]
                }
                "rotate_180" => {
                    let wt = smoothstep(-1.0, 1.0, (cx - pxf) / feather);
                    wt * at(px, py) + (1.0 - wt) * at(w - 1 - px, h - 1 - py)
                }
                "rotate_90_4way" => {
                    // Top-left quadrant is canonical. Other quadrants are mapped
                    // back by 90-degree rotations (assumes a square map).
                    let (sx, sy) = if px < w / 2 && py < h / 2 {
                        (px, py)
                    } else if px >= w / 2 && py < h / 2 {
                        (py, w - 1 - px)
                    } else if px < w / 2 {
                        (h - 1 - py, px)
                    } else {
                        (w - 1 - px, h - 1 - py)
                    };
                    src[sy * w + sx]
                }
                "average_x" => mean_at_positions(src, w, &[(px, py), (w - 1 - px, py)]),
                "average_y" => mean_at_positions(src, w, &[(px, py), (px, h - 1 - py)]),
                "average_xy" => mean_at_positions(
                    src,
                    w,
                    &[
                        (px, py),
                        (w - 1 - px, py),
                        (px, h - 1 - py),
                        (w - 1 - px, h - 1 - py),
                    ],
                ),
                "average_180" => mean_at_positions(src, w, &[(px, py), (w - 1 - px, h - 1 - py)]),
                "average_90_4way" => {
                    // Each output pixel is the mean of its four
                    // 90-degree-rotated partners. Assumes a square map
                    // -- the same caveat as `rotate_90_4way`.
                    mean_at_positions(
                        src,
                        w,
                        &[
                            (px, py),
                            (w - 1 - py, px),
                            (w - 1 - px, h - 1 - py),
                            (py, w - 1 - px),
                        ],
                    )
                }
                _ => src[py * w + px],
            };
        }
    }
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::apply_mirror;
    use bar_data::Heightmap;

    fn asym(n: usize) -> Heightmap {
        // Asymmetric field so a hard rotate_180 would leave a visible seam.
        let mut d = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
                d[y * n + x] = 0.2 + 0.6 * u * v;
            }
        }
        Heightmap::frbar_data(n as u32, n as u32, d).unwrap()
    }

    #[test]
    fn rotate_180_is_symmetric_and_seamless() {
        let n = 128;
        let out = apply_mirror(&asym(n), "rotate_180");
        // Exactly rot180-symmetric.
        let a = out.get(20, 33).unwrap();
        let b = out.get((n - 1 - 20) as u32, (n - 1 - 33) as u32).unwrap();
        assert!((a - b).abs() < 1e-4, "not rot180-symmetric: {a} vs {b}");
        // No hard seam: the step across the centre column is on the order of a
        // normal neighbour step, not the ~0.15 jump a hard copy leaves here.
        let y = n / 4;
        let cx = (n / 2) as u32;
        let seam = (out.get(cx, y as u32).unwrap() - out.get(cx - 1, y as u32).unwrap()).abs();
        // Hard copy leaves ~0.15 here; feathering keeps it small.
        assert!(seam < 0.05, "rotate_180 seam jump too large: {seam}");
    }
}
