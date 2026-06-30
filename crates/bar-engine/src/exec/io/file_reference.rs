use std::collections::HashMap;

use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let path = match ctx.params.get("path") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => String::new(),
    };
    let bundle_path = match ctx.params.get("bundle_path") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => path.clone(),
    };

    Ok(HashMap::from([(
        "file".to_string(),
        PortValue::File(bar_graph::FileRef { path, bundle_path }),
    )]))
}
