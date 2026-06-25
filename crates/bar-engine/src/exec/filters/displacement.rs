use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{
    apply_modulation,
    get_float,
    get_input_heightmap,
    get_optional_heightmap,
};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let displacement = get_input_heightmap(ctx.inputs, "displacement")?;
    let strength = get_float(ctx.params, "strength", 0.1);
    let hm = apply_displacement(&input, &displacement, strength);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Displace/warp terrain using another heightmap as the displacement field.
/// Displaces in X direction proportional to displacement map gradient.
pub(crate) fn apply_displacement(input: &Heightmap, displacement: &Heightmap, strength: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let dw = displacement.width();
    let dh = displacement.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    // Strength is in pixels
    let pixel_strength = strength * w as f32;

    for y in 0..h {
        for x in 0..w {
            // Sample displacement map (rescale if dimensions differ)
            let dx_coord = (x as f32 * dw as f32 / w as f32) as u32;
            let dy_coord = (y as f32 * dh as f32 / h as f32) as u32;
            let dx_coord = dx_coord.min(dw - 1);
            let dy_coord = dy_coord.min(dh - 1);

            // Compute displacement gradient (central differences)
            let dx_left = if dx_coord > 0 { dx_coord - 1 } else { 0 };
            let dx_right = (dx_coord + 1).min(dw - 1);
            let dy_top = if dy_coord > 0 { dy_coord - 1 } else { 0 };
            let dy_bot = (dy_coord + 1).min(dh - 1);

            let grad_x = displacement.get(dx_right, dy_coord).unwrap_or(0.0)
                - displacement.get(dx_left, dy_coord).unwrap_or(0.0);
            let grad_y = displacement.get(dx_coord, dy_bot).unwrap_or(0.0)
                - displacement.get(dx_coord, dy_top).unwrap_or(0.0);

            // Displaced source coordinates
            let sx = (x as f32 + grad_x * pixel_strength).clamp(0.0, (w - 1) as f32);
            let sy = (y as f32 + grad_y * pixel_strength).clamp(0.0, (h - 1) as f32);

            // Bilinear interpolation from source
            let x0 = sx as u32;
            let y0 = sy as u32;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();

            let v00 = input.get(x0, y0).unwrap_or(0.0);
            let v10 = input.get(x1, y0).unwrap_or(0.0);
            let v01 = input.get(x0, y1).unwrap_or(0.0);
            let v11 = input.get(x1, y1).unwrap_or(0.0);
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;

            data[(y as usize) * (w as usize) + (x as usize)] = v;
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}
