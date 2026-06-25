use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap, scale_by_field};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let slope = get_optional_heightmap(ctx.inputs, "slope");
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = generate_specular_map(&input, slope.as_ref(), ctx.params);
    let hm = scale_by_field(hm, ctrl.as_ref());
    let hm = scale_by_field(hm, mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Generate a specular intensity map from height and slope.
/// Parameters:
/// - `rock_specular`: specularity for steep rocky areas (default 0.6)
/// - `flat_specular`: specularity for flat ground (default 0.2)
/// - `water_specular`: specularity for low areas (water/wet, default 0.9)
/// - `water_height`: height threshold below which ground is considered wet (default 0.2)
pub(crate) fn generate_specular_map(
    heightmap: &Heightmap,
    slope: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> Heightmap {
    let w = heightmap.width();
    let h = heightmap.height();
    let size = (w as usize) * (h as usize);
    let mut data = vec![0.0f32; size];

    let rock_specular = get_float(params, "rock_specular", 0.6);
    let flat_specular = get_float(params, "flat_specular", 0.2);
    let water_specular = get_float(params, "water_specular", 0.9);
    let water_height = get_float(params, "water_height", 0.2);
    let snow_specular = get_float(params, "snow_specular", 0.7);
    let snow_height = get_float(params, "snow_height", 0.85);

    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            let height_val = heightmap.get(x, y).unwrap_or(0.0);
            let slope_val = slope.and_then(|s| s.get(x, y)).unwrap_or(0.0);

            // Base specular from slope: steep = shiny rock, flat = dull ground
            let base = flat_specular + (rock_specular - flat_specular) * slope_val;

            // Override for water/wet areas
            let spec = if height_val < water_height {
                let wet_factor = 1.0 - (height_val / water_height);
                base + (water_specular - base) * wet_factor
            } else if height_val > snow_height {
                let snow_factor = (height_val - snow_height) / (1.0 - snow_height);
                base + (snow_specular - base) * snow_factor
            } else {
                base
            };

            data[idx] = spec.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}
