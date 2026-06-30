use std::collections::HashMap;

use bar_compute::{thermal_erosion, ThermalErosionParams};
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::ExecCtx;

/// Build `ThermalErosionParams` from node params. Shared by this CPU exec and
/// the GPU path in `hybrid_executor` so the editor and export read the same
/// defaults (a divergent default = a different map on a GPU-less machine).
pub(crate) fn build_thermal_params(params: &HashMap<String, ParamValue>) -> ThermalErosionParams {
    ThermalErosionParams {
        iterations: get_uint(params, "iterations", 100),
        talus_angle: get_float(params, "talus_angle", 0.6),
        erosion_rate: get_float(params, "erosion_rate", 0.5),
    }
}

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let params_e = build_thermal_params(ctx.params);
    let hm = thermal_erosion(&input, &params_e).map_err(|e| EvalError::Compute(e.to_string()))?;
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}
