use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let step_count = get_uint(ctx.params, "step_count", 4).clamp(1, 64);
    let smoothing = get_float(ctx.params, "smoothing", 0.0).clamp(0.0, 1.0);
    let hm = apply_terrace(&input, step_count, smoothing);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

pub(crate) fn apply_terrace(input: &Heightmap, step_count: u32, smoothing: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let steps = step_count.max(1) as f32;
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| {
            let t = v * steps;
            let lo = t.floor();
            let frac = t - lo;
            // Smoothstep within each step band, lerped by `smoothing`.
            let smooth = frac * frac * (3.0 - 2.0 * frac);
            let hard = lo / steps;
            let soft = (lo + smooth) / steps;
            hard + smoothing * (soft - hard)
        })
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}
