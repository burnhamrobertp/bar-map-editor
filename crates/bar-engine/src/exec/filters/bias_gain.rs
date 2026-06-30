use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let bias = get_float(ctx.params, "bias", 0.5);
    let gain = get_float(ctx.params, "gain", 0.5);
    let hm = apply_bias_gain(&input, bias, gain);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Christophe Schlick's bias and gain functions.
/// bias(t, b) = t^(log(b) / log(0.5))
/// gain(t, g) = bias(2t, 1-g)/2 for t < 0.5, else 1 - bias(2-2t, 1-g)/2
pub(crate) fn apply_bias_gain(input: &Heightmap, bias: f32, gain: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    let bias_exp = if bias.abs() < 1e-6 {
        0.0
    } else {
        (bias.clamp(0.001, 0.999)).ln() / (0.5f32).ln()
    };

    for y in 0..h {
        for x in 0..w {
            let t = input.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);

            // Apply bias
            let biased = t.powf(bias_exp);

            // Apply gain
            let gained = if biased < 0.5 {
                let bt = (2.0 * biased).powf((1.0 - gain).clamp(0.001, 0.999).ln() / (0.5f32).ln());
                bt / 2.0
            } else {
                let bt = (2.0 - 2.0 * biased)
                    .powf((1.0 - gain).clamp(0.001, 0.999).ln() / (0.5f32).ln());
                1.0 - bt / 2.0
            };

            data[(y as usize) * (w as usize) + (x as usize)] = gained.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn test_bias_gain() {
        let executor = crate::CpuExecutor;
        // Uniform ramp
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        // Default bias=0.5, gain=0.5 should be roughly identity
        let params = HashMap::from([
            ("bias".to_string(), ParamValue::Float(0.5)),
            ("gain".to_string(), ParamValue::Float(0.5)),
        ]);

        let result = executor
            .execute(&NodeType::BiasGain, &params, &inputs, 4, 4, 4, 4)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                // With bias=gain=0.5, output ~= input
                assert!((hm.get(0, 0).unwrap() - 0.0).abs() < 0.01);
                assert!((hm.get(3, 3).unwrap() - 1.0).abs() < 0.01);
            }
            _ => panic!("Expected heightmap"),
        }
    }
}
