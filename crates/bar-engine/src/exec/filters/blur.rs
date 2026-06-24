use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{
    apply_modulation,
    get_float,
    get_input_heightmap,
    get_optional_heightmap,
};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let radius = get_float(ctx.params, "radius", 1.0);
    let hm = apply_blur(&input, radius);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Gaussian blur approximation using separable box blur (3 passes).
pub(crate) fn apply_blur(input: &Heightmap, radius: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let r = (radius.round() as usize).clamp(1, 64);

    let mut src: Vec<f32> = input.data().to_vec();
    let mut dst: Vec<f32> = vec![0.0; w * h];

    // 3-pass box blur approximates Gaussian
    for _ in 0..3 {
        // Horizontal pass
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                let mut count = 0.0;
                let x_start = x.saturating_sub(r);
                let x_end = (x + r + 1).min(w);
                for xx in x_start..x_end {
                    sum += src[y * w + xx];
                    count += 1.0;
                }
                dst[y * w + x] = sum / count;
            }
        }
        std::mem::swap(&mut src, &mut dst);

        // Vertical pass
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                let mut count = 0.0;
                let y_start = y.saturating_sub(r);
                let y_end = (y + r + 1).min(h);
                for yy in y_start..y_end {
                    sum += src[yy * w + x];
                    count += 1.0;
                }
                dst[y * w + x] = sum / count;
            }
        }
        std::mem::swap(&mut src, &mut dst);
    }

    Heightmap::frbar_data(w as u32, h as u32, src).unwrap()
}
