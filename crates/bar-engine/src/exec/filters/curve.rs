use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = apply_curve(&input, ctx.params);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Apply curve/remap: piecewise-linear transfer function defined by control points.
/// Params: points (encoded as pairs in "p0_x", "p0_y", "p1_x", "p1_y", ... or "num_points")
/// Default: S-curve (smoothstep)
pub(crate) fn apply_curve(input: &Heightmap, params: &HashMap<String, ParamValue>) -> Heightmap {
    let w = input.width();
    let h = input.height();

    // Build control points from params (default: smoothstep-like S-curve)
    let num_points = get_uint(params, "num_points", 0) as usize;
    let points: Vec<(f32, f32)> = if num_points >= 2 {
        (0..num_points)
            .map(|i| {
                let px = get_float(
                    params,
                    &format!("p{}_x", i),
                    i as f32 / (num_points - 1) as f32,
                );
                let py = get_float(params, &format!("p{}_y", i), px);
                (px, py)
            })
            .collect()
    } else {
        // Default: smoothstep S-curve
        vec![(0.0, 0.0), (0.25, 0.1), (0.5, 0.5), (0.75, 0.9), (1.0, 1.0)]
    };

    let mut data = vec![0.0f32; (w as usize) * (h as usize)];
    for y in 0..h {
        for x in 0..w {
            let v = input.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            data[(y as usize) * (w as usize) + (x as usize)] = eval_piecewise_linear(&points, v);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Evaluate a piecewise-linear curve at a given x value.
fn eval_piecewise_linear(points: &[(f32, f32)], x: f32) -> f32 {
    if points.is_empty() {
        return x;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[points.len() - 1].0 {
        return points[points.len() - 1].1;
    }
    for i in 1..points.len() {
        if x <= points[i].0 {
            let (x0, y0) = points[i - 1];
            let (x1, y1) = points[i];
            let t = if (x1 - x0).abs() < 1e-8 {
                0.0
            } else {
                (x - x0) / (x1 - x0)
            };
            return y0 + t * (y1 - y0);
        }
    }
    points[points.len() - 1].1
}
