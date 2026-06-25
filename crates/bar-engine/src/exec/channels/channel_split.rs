use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::shared::get_input_color;
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let color = get_input_color(ctx.inputs, "color")?;

    Ok(HashMap::from([
        ("r".to_string(), PortValue::Heightmap(color.channel(0))),
        ("g".to_string(), PortValue::Heightmap(color.channel(1))),
        ("b".to_string(), PortValue::Heightmap(color.channel(2))),
        ("a".to_string(), PortValue::Heightmap(color.channel(3))),
    ]))
}
