use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{get_input_scalar, get_string};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let a = get_input_scalar(ctx.inputs, "a").unwrap_or(0.0);
    let b = get_input_scalar(ctx.inputs, "b").unwrap_or(0.0);
    let op = get_string(ctx.params, "op", "add");

    let out = match op {
        "subtract" => a - b,
        "multiply" => a * b,
        "divide" => {
            if b.abs() > 1e-6 {
                a / b
            } else {
                0.0
            }
        }
        "min" => a.min(b),
        "max" => a.max(b),
        "average" => (a + b) * 0.5,
        "power" => a.powf(b),
        // "add" and any unknown op.
        _ => a + b,
    };

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Scalar(out),
    )]))
}
