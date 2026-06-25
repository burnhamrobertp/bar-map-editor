use std::collections::HashMap;

use bar_data::{ColorBuffer, Heightmap};
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap};
use crate::exec::texture::shared::{apply_color_modulation, resize_color_to_tex};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let strength = get_float(ctx.params, "strength", 1.0);
    let color = generate_normal_map(&input, strength);
    // Neutral normal = flat surface [0.5, 0.5, 1.0, 1.0] in tangent space.
    let color = apply_color_modulation([0.5, 0.5, 1.0, 1.0], color, None, mask.as_ref());
    let color = resize_color_to_tex(color, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Color(color),
    )]))
}

/// Generate a tangent-space normal map from a heightmap.
/// `strength` controls the intensity of the normals (higher = more pronounced bumps).
pub(crate) fn generate_normal_map(heightmap: &Heightmap, strength: f32) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    // Scale factor accounts for the pixel spacing vs height range
    let scale = strength * 2.0;

    for y in 0..h {
        for x in 0..w {
            // Sample neighboring heights using Sobel-like kernel
            let x0 = if x > 0 { x - 1 } else { 0 };
            let x1 = if x < w - 1 { x + 1 } else { w - 1 };
            let y0 = if y > 0 { y - 1 } else { 0 };
            let y1 = if y < h - 1 { y + 1 } else { h - 1 };

            let tl = heightmap.get(x0, y0).unwrap_or(0.0);
            let t = heightmap.get(x, y0).unwrap_or(0.0);
            let tr = heightmap.get(x1, y0).unwrap_or(0.0);
            let l = heightmap.get(x0, y).unwrap_or(0.0);
            let r = heightmap.get(x1, y).unwrap_or(0.0);
            let bl = heightmap.get(x0, y1).unwrap_or(0.0);
            let b = heightmap.get(x, y1).unwrap_or(0.0);
            let br = heightmap.get(x1, y1).unwrap_or(0.0);

            // Sobel filter for dx and dy
            let dx = (tr + 2.0 * r + br) - (tl + 2.0 * l + bl);
            let dy = (bl + 2.0 * b + br) - (tl + 2.0 * t + tr);

            // Construct normal vector (tangent space: Z is up)
            let nx = -dx * scale;
            let ny = -dy * scale;
            let nz = 1.0f32;

            // Normalize
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            let nx = nx / len;
            let ny = ny / len;
            let nz = nz / len;

            // Encode to [0,1] range for storage as RGB
            let r = nx * 0.5 + 0.5;
            let g = ny * 0.5 + 0.5;
            let b = nz * 0.5 + 0.5;

            color.set(x, y, [r, g, b, 1.0]);
        }
    }

    color
}
