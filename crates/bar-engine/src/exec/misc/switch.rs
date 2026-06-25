use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{get_input_heightmap, get_uint};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let selected = get_uint(ctx.params, "selected", 0);
    let port = format!("input_{selected}");
    let hm = get_input_heightmap(ctx.inputs, &port)?;

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}
