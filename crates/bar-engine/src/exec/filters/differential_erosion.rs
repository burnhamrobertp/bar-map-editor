use std::collections::HashMap;

use bar_compute::{apply_strata_terracing, differential_erosion};
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");

    let strength = get_float(ctx.params, "strength", 0.5);
    let layers = get_uint(ctx.params, "strata_layers", 6);
    let contrast = get_float(ctx.params, "strata_contrast", 0.6);
    let slope = get_float(ctx.params, "slope_hardening", 0.5);
    let iterations = get_uint(ctx.params, "iterations", 40);
    let terrace = get_float(ctx.params, "terrace", 0.0);

    let mut hm = differential_erosion(&input, strength, layers, contrast, slope, iterations);
    if terrace > 0.0 {
        hm = apply_strata_terracing(&hm, terrace, layers, contrast, slope);
    }
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}
