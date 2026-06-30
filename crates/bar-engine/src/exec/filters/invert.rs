use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_invert, apply_modulation, get_input_heightmap, get_optional_heightmap,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = apply_invert(&input);
    let hm = apply_modulation(&input, hm, None, mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}
