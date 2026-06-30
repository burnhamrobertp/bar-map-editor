use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::shared::get_float;
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Scalar(get_float(ctx.params, "value", 0.5)),
    )]))
}
