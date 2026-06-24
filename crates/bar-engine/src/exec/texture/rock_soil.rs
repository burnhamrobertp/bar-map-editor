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
    let color = generate_rock_soil(&input, slope.as_ref(), ctx.params);
    let color = apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
    let color = resize_color_to_tex(color, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([("output".to_string(), PortValue::Color(color))]))
}

/// Slope-driven rock overlay. Alpha encodes rock coverage so this composites
/// only over steep terrain when layered on top of a base texture (e.g. AutoTexture).
/// detail_strength breaks up the flat color with FBM micro-variation.
pub(crate) fn generate_rock_soil(
    heightmap: &Heightmap,
    slope_input: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    let rock_hex = get_string(params, "rock_color", "807870");
    let soil_hex = get_string(params, "soil_color", "8B6914");
    let rock_rgb = parse_hex_color_srgb(rock_hex).unwrap_or([0.50, 0.47, 0.44]);
    let soil_rgb = parse_hex_color_srgb(soil_hex).unwrap_or([0.55, 0.41, 0.08]);
    let threshold = get_float(params, "slope_threshold", 0.4).clamp(0.0, 1.0);
    let blend = get_float(params, "slope_blend", 0.3).max(0.001);
    let ao_strength = get_float(params, "ao_strength", 0.8).clamp(0.0, 1.0);
    let detail_strength = get_float(params, "detail_strength", 0.25).clamp(0.0, 1.0);

    let computed_slope = slope_input.is_none().then(|| compute_slope_map(heightmap));
    let slope_map = slope_input.unwrap_or_else(|| computed_slope.as_ref().unwrap());

    for y in 0..h {
        for x in 0..w {
            let s = slope_map.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            let t = ((s - threshold) / blend).clamp(0.0, 1.0);
            let rock_w = t * t * (3.0 - 2.0 * t);
            let ao = {
                let raw = compute_local_ao(heightmap, x, y);
                1.0 - ao_strength * (1.0 - raw)
            };
            let ux = x as f32 / w as f32;
            let uy = y as f32 / h as f32;
            let noise = micro_fbm(ux, uy, 8.0);
            let detail = 1.0 + detail_strength * (noise * 2.0 - 1.0);
            let base_r = soil_rgb[0] + rock_w * (rock_rgb[0] - soil_rgb[0]);
            let base_g = soil_rgb[1] + rock_w * (rock_rgb[1] - soil_rgb[1]);
            let base_b = soil_rgb[2] + rock_w * (rock_rgb[2] - soil_rgb[2]);
            color.set(
                x,
                y,
                [
                    (base_r * ao * detail).clamp(0.0, 1.0),
                    (base_g * ao * detail).clamp(0.0, 1.0),
                    (base_b * ao * detail).clamp(0.0, 1.0),
                    rock_w, // alpha = rock coverage; transparent on flat terrain
                ],
            );
        }
    }
    color
}
