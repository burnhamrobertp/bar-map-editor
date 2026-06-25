use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap, scale_by_field};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let slope = get_optional_heightmap(ctx.inputs, "slope");
    let density = get_optional_heightmap(ctx.inputs, "density");
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = generate_grass_map(&input, slope.as_ref(), ctx.params);
    let hm = scale_by_field(hm, density.as_ref());
    let hm = scale_by_field(hm, ctrl.as_ref());
    let hm = scale_by_field(hm, mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Smooth band function: returns 1.0 inside [low,high], smoothly falls to 0 outside.
fn smooth_band(value: f32, low: f32, high: f32, falloff: f32) -> f32 {
    if value < low - falloff || value > high + falloff {
        return 0.0;
    }
    if value >= low && value <= high {
        return 1.0;
    }
    if value < low {
        (value - (low - falloff)) / falloff
    } else {
        ((high + falloff) - value) / falloff
    }
}

/// Generate a grass density map based on height and slope constraints.
/// Parameters:
/// - `min_height`: minimum height for grass (default 0.15)
/// - `max_height`: maximum height for grass (default 0.7)
/// - `max_slope`: maximum slope for grass growth (default 0.4)
/// - `density`: overall density multiplier (default 1.0)
pub(crate) fn generate_grass_map(
    heightmap: &Heightmap,
    slope: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> Heightmap {
    let w = heightmap.width();
    let h = heightmap.height();
    let size = (w as usize) * (h as usize);
    let mut data = vec![0.0f32; size];

    let min_height = get_float(params, "min_height", 0.15);
    let max_height = get_float(params, "max_height", 0.7);
    let max_slope = get_float(params, "max_slope", 0.4);
    let density = get_float(params, "density", 1.0);
    let falloff = get_float(params, "falloff", 0.05);

    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            let height_val = heightmap.get(x, y).unwrap_or(0.0);
            let slope_val = slope.and_then(|s| s.get(x, y)).unwrap_or(0.0);

            // Height band with smooth falloff
            let height_factor = smooth_band(height_val, min_height, max_height, falloff);

            // Slope attenuation: grass doesn't grow on steep slopes
            let slope_factor = if slope_val < max_slope {
                1.0
            } else {
                let over = (slope_val - max_slope) / falloff.max(0.01);
                (1.0 - over).max(0.0)
            };

            data[idx] = (height_factor * slope_factor * density).clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}
