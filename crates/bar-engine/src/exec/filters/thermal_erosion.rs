use std::collections::HashMap;

use bar_compute::{thermal_erosion, ThermalErosionParams};
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let params_e = ThermalErosionParams {
        iterations: get_uint(ctx.params, "iterations", 100),
        talus_angle: get_float(ctx.params, "talus_angle", 0.6),
        erosion_rate: get_float(ctx.params, "erosion_rate", 0.5),
    };
    let hm = thermal_erosion(&input, &params_e).map_err(|e| EvalError::Compute(e.to_string()))?;
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}
