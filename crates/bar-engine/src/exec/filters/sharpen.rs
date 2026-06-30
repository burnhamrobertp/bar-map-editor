use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::filters::blur::apply_blur;
use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let radius = get_float(ctx.params, "radius", 1.0).max(0.1);
    let strength = get_float(ctx.params, "strength", 1.0).clamp(0.0, 4.0);
    let hm = apply_sharpen(&input, radius, strength);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

pub(crate) fn apply_sharpen(input: &Heightmap, radius: f32, strength: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let blurred = apply_blur(input, radius);
    let data: Vec<f32> = input
        .data()
        .iter()
        .zip(blurred.data().iter())
        .map(|(&v, &b)| (v + strength * (v - b)).clamp(0.0, 1.0))
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}
