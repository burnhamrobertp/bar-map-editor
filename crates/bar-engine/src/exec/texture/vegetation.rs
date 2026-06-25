use std::collections::HashMap;

use bar_data::{ColorBuffer, Heightmap};
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::ExecCtx;
use crate::exec::selectors::shared::compute_slope_map;
use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap, get_string};
use crate::exec::texture::shared::{
    apply_color_modulation,
    compute_local_ao,
    micro_fbm,
    parse_hex_color_srgb,
    resize_color_to_tex,
};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let slope = get_optional_heightmap(ctx.inputs, "slope");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let color = generate_vegetation(&input, slope.as_ref(), ctx.params);
    let color = apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
    let color = resize_color_to_tex(color, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([("output".to_string(), PortValue::Color(color))]))
}

/// Altitude+slope vegetation overlay. Alpha encodes coverage so this composites
/// only over gentle low terrain when layered on top of a base texture.
/// detail_strength breaks up the flat green with FBM micro-variation.
pub(crate) fn generate_vegetation(
    heightmap: &Heightmap,
    slope_input: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    let veg_hex = get_string(params, "vegetation_color", "4A7020");
    let dry_hex = get_string(params, "dry_color", "8B7355");
    let veg_rgb = parse_hex_color_srgb(veg_hex).unwrap_or([0.29, 0.44, 0.13]);
    let dry_rgb = parse_hex_color_srgb(dry_hex).unwrap_or([0.55, 0.45, 0.33]);
    let altitude_max = get_float(params, "altitude_max", 0.6).clamp(0.0, 1.0);
    let slope_cutoff = get_float(params, "slope_cutoff", 0.5).clamp(0.0, 1.0);
    let slope_blend = get_float(params, "slope_blend", 0.2).max(0.001);
    let ao_strength = get_float(params, "ao_strength", 0.6).clamp(0.0, 1.0);
    let detail_strength = get_float(params, "detail_strength", 0.2).clamp(0.0, 1.0);

    let computed_slope = slope_input.is_none().then(|| compute_slope_map(heightmap));
    let slope_map = slope_input.unwrap_or_else(|| computed_slope.as_ref().unwrap());

    const ALT_BLEND: f32 = 0.1;
    for y in 0..h {
        for x in 0..w {
            let elev = heightmap.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            let s = slope_map.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);

            let alt_t = ((elev - altitude_max) / ALT_BLEND).clamp(0.0, 1.0);
            let alt_factor = 1.0 - alt_t * alt_t * (3.0 - 2.0 * alt_t);

            let slp_t = ((s - slope_cutoff) / slope_blend).clamp(0.0, 1.0);
            let slope_factor = 1.0 - slp_t * slp_t * (3.0 - 2.0 * slp_t);

            let veg_weight = alt_factor * slope_factor;
            let ao = {
                let raw = compute_local_ao(heightmap, x, y);
                1.0 - ao_strength * (1.0 - raw)
            };
            let ux = x as f32 / w as f32;
            let uy = y as f32 / h as f32;
            // Slightly higher frequency than rock detail for finer vegetation texture.
            let noise = micro_fbm(ux, uy, 12.0);
            let detail = 1.0 + detail_strength * (noise * 2.0 - 1.0);
            let base_r = dry_rgb[0] + veg_weight * (veg_rgb[0] - dry_rgb[0]);
            let base_g = dry_rgb[1] + veg_weight * (veg_rgb[1] - dry_rgb[1]);
            let base_b = dry_rgb[2] + veg_weight * (veg_rgb[2] - dry_rgb[2]);
            color.set(
                x,
                y,
                [
                    (base_r * ao * detail).clamp(0.0, 1.0),
                    (base_g * ao * detail).clamp(0.0, 1.0),
                    (base_b * ao * detail).clamp(0.0, 1.0),
                    veg_weight,
                ],
            );
        }
    }
    color
}
