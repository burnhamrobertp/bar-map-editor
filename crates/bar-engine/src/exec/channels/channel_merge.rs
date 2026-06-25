use std::collections::HashMap;

use bar_data::ColorBuffer;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_input_heightmap, get_optional_heightmap};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let r = get_input_heightmap(ctx.inputs, "r")?;
    let g = get_input_heightmap(ctx.inputs, "g")?;
    let b = get_input_heightmap(ctx.inputs, "b")?;
    let a = get_optional_heightmap(ctx.inputs, "a");

    let color = ColorBuffer::from_channels(&r, &g, &b, a.as_ref());

    Ok(HashMap::from([("color".to_string(), PortValue::Color(color))]))
}
