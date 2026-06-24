use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::get_uint;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Scalar(get_uint(ctx.params, "value", 1) as f32),
    )]))
}
