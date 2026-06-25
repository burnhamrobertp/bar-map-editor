use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{
    get_bool, get_float, get_optional_heightmap, get_string, scale_by_field,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let hm = generate_gradient(ctx.params, ctx.hm_w, ctx.hm_h);
    let hm = scale_by_field(hm, ctrl.as_ref());
    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Generate a gradient (ramp).
/// Params: direction ("linear_x", "linear_y", "radial", "angular"), invert
pub(crate) fn generate_gradient(
    params: &HashMap<String, ParamValue>,
    width: u32,
    height: u32,
) -> Heightmap {
    let direction = get_string(params, "direction", "linear_y");
    let invert = get_bool(params, "invert", false);
    let center_x = get_float(params, "center_x", 0.5);
    let center_y = get_float(params, "center_y", 0.5);

    let mut data = vec![0.0f32; (width as usize) * (height as usize)];

    for y in 0..height {
        for x in 0..width {
            let nx = x as f32 / (width as f32 - 1.0).max(1.0);
            let ny = y as f32 / (height as f32 - 1.0).max(1.0);

            // Piecewise-linear remap that honours the center param
            // for linear modes: `center` is where the ramp's midpoint
            // (v=0.5) sits. center=0.5 reproduces the simple v=axis
            // gradient; smaller values push the ramp toward the start
            // of the axis, larger values toward the end.
            let remap = |t: f32, center: f32| -> f32 {
                let c = center.clamp(0.001, 0.999);
                if t <= c {
                    0.5 * t / c
                } else {
                    0.5 + 0.5 * (t - c) / (1.0 - c)
                }
            };
            let v = match direction {
                "linear_x" => remap(nx, center_x),
                "radial" => {
                    let dx = nx - center_x;
                    let dy = ny - center_y;
                    let dist = (dx * dx + dy * dy).sqrt() * std::f32::consts::SQRT_2;
                    1.0 - dist.min(1.0)
                }
                "angular" => {
                    let dx = nx - center_x;
                    let dy = ny - center_y;
                    (dy.atan2(dx) / std::f32::consts::TAU + 0.5).fract()
                }
                _ => remap(ny, center_y), // "linear_y" (default)
            };

            let v = if invert { 1.0 - v } else { v };
            data[(y as usize) * (width as usize) + (x as usize)] = v.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType};

    #[test]
    fn test_gradient_generator() {
        let executor = crate::CpuExecutor;
        let params = HashMap::from([(
            "direction".to_string(),
            ParamValue::String("vertical".to_string()),
        )]);
        let result = executor
            .execute(&NodeType::Gradient, &params, &HashMap::new(), 8, 8, 8, 8)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                // Vertical gradient: top row ~0, bottom row ~1
                assert!(hm.get(0, 0).unwrap() < 0.01);
                assert!(hm.get(0, 7).unwrap() > 0.99);
            }
            _ => panic!("Expected heightmap"),
        }
    }
}
