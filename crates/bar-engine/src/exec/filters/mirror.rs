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

pub(crate) fn apply_mirror(input: &Heightmap, mode: &str) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let src = input.data();
    let mut data = vec![0.0f32; w * h];
    for py in 0..h {
        for px in 0..w {
            // Replace modes (`mirror_*` / `rotate_*`) pick a single
            // source pixel; averaging modes (`average_*`) take the mean
            // of every pixel in the symmetric orbit so both halves
            // contribute to the output.
            data[py * w + px] = match mode {
                "mirror_x" => {
                    let sx = if px < w / 2 { px } else { w - 1 - px };
                    src[py * w + sx]
                }
                "mirror_y" => {
                    let sy = if py < h / 2 { py } else { h - 1 - py };
                    src[sy * w + px]
                }
                "mirror_xy" => {
                    let sx = if px < w / 2 { px } else { w - 1 - px };
                    let sy = if py < h / 2 { py } else { h - 1 - py };
                    src[sy * w + sx]
                }
                "rotate_180" => {
                    if px < w / 2 {
                        src[py * w + px]
                    } else {
                        src[(h - 1 - py) * w + (w - 1 - px)]
                    }
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
