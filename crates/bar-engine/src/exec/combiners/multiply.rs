use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    super::run_binop(ctx, |va, vb| va * vb)
}
