use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let threshold = get_float(ctx.params, "threshold", 0.5);
    let smoothness = get_float(ctx.params, "smoothness", 0.0);
    let hm = if let Some(c) = &ctrl {
        // Control shifts the threshold spatially (WM: higher control = threshold moves up)
        let data: Vec<f32> = input
            .data()
            .iter()
            .zip(c.data())
            .map(|(&v, &cv)| {
                let t = (threshold + cv - 0.5).clamp(0.0, 1.0);
                if smoothness <= 0.001 {
                    if v >= t {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    let s = ((v - t) / smoothness + 0.5).clamp(0.0, 1.0);
                    s * s * (3.0 - 2.0 * s)
                }
            })
            .collect();
        Heightmap::frbar_data(input.width(), input.height(), data).unwrap()
    } else {
        apply_mask_threshold(&input, threshold, smoothness)
    };

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Threshold a heightmap into a binary (or smooth) mask.
/// With smoothness=0: hard binary. With smoothness>0: smooth sigmoid-like transition.
pub(crate) fn apply_mask_threshold(input: &Heightmap, threshold: f32, smoothness: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| {
            if smoothness <= 0.001 {
                if v >= threshold {
                    1.0
                } else {
                    0.0
                }
            } else {
                // Smooth transition using hermite interpolation
                let t = ((v - threshold) / smoothness + 0.5).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
        })
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}
