use std::collections::HashMap;

use bar_compute::NoiseType;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    super::run_noise(NoiseType::Ridged, ctx)
}
