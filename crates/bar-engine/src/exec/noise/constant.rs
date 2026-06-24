use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::get_float;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let value = get_float(ctx.params, "value", 0.5);
    let data = vec![value; (ctx.hm_w as usize) * (ctx.hm_h as usize)];
    let hm = Heightmap::frbar_data(ctx.hm_w, ctx.hm_h, data)
        .map_err(|e| EvalError::Compute(e.to_string()))?;
    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}
