use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::get_input_heightmap;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_input_heightmap(ctx.inputs, "mask")?;
    let bg = ctx.inputs.get("background").and_then(|v| match v {
        PortValue::Heightmap(h) => Some(h.clone()),
        _ => None,
    });
    let hm: Heightmap = apply_mask(&input, &mask, bg.as_ref());
    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Apply a mask to blend between input and background.
/// output = input * mask + background * (1 - mask)
pub(crate) fn apply_mask(input: &Heightmap, mask: &Heightmap, background: Option<&Heightmap>) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let val = input.get(x, y).unwrap_or(0.0);
            let m = mask.get(x, y).unwrap_or(1.0);
            let bg = background.and_then(|b| b.get(x, y)).unwrap_or(0.0);
            data[(y as usize) * (w as usize) + (x as usize)] = val * m + bg * (1.0 - m);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}
