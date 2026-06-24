use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_input_heightmap, get_optional_heightmap, scale_by_field};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let hm = scale_by_field(input, ctrl.as_ref());

    Ok(HashMap::from([("mask".to_string(), PortValue::Mask(hm))]))
}
