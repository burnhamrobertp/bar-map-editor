use std::collections::HashMap;

use bar_data::{ColorBuffer, Heightmap};
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_input_color, get_optional_heightmap, get_string};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let base = get_input_color(ctx.inputs, "base")?;
    let overlay = get_input_color(ctx.inputs, "overlay")?;
    let distribution = get_optional_heightmap(ctx.inputs, "distribution");
    let color = generate_texture_overlay(&base, &overlay, distribution.as_ref(), ctx.params);

    Ok(HashMap::from([("output".to_string(), PortValue::Color(color))]))
}

/// Porter-Duff compositor for Color layers. Blends overlay over base using
/// `distribution` heightmap as per-pixel weight (falls back to overlay alpha).
pub(crate) fn generate_texture_overlay(
    base: &ColorBuffer,
    overlay: &ColorBuffer,
    distribution: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = base.width();
    let h = base.height();
    let mut out = ColorBuffer::new(w, h).unwrap();

    let blend_mode = get_string(params, "blend_mode", "over");
    let opacity = get_float(params, "opacity", 1.0).clamp(0.0, 1.0);

    for y in 0..h {
        for x in 0..w {
            let b = base.get(x, y).unwrap_or([0.0; 4]);
            // Sample overlay at the same UV; if sizes differ, nearest neighbour
            let ov_x = ((x as f32 / w as f32) * overlay.width() as f32) as u32;
            let ov_y = ((y as f32 / h as f32) * overlay.height() as f32) as u32;
            let ov = overlay
                .get(
                    ov_x.min(overlay.width() - 1),
                    ov_y.min(overlay.height() - 1),
                )
                .unwrap_or([0.0; 4]);

            let dist = if let Some(dm) = distribution {
                let dm_x = ((x as f32 / w as f32) * dm.width() as f32) as u32;
                let dm_y = ((y as f32 / h as f32) * dm.height() as f32) as u32;
                dm.get(dm_x.min(dm.width() - 1), dm_y.min(dm.height() - 1))
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0)
            } else {
                ov[3].clamp(0.0, 1.0)
            };
            let alpha = (dist * opacity).clamp(0.0, 1.0);

            let (or, og, ob) = (ov[0], ov[1], ov[2]);
            let (br, bg, bb) = (b[0], b[1], b[2]);
            let (r, g, ob_) = match blend_mode {
                "multiply" => (
                    (br * or * alpha + br * (1.0 - alpha)).clamp(0.0, 1.0),
                    (bg * og * alpha + bg * (1.0 - alpha)).clamp(0.0, 1.0),
                    (bb * ob * alpha + bb * (1.0 - alpha)).clamp(0.0, 1.0),
                ),
                "screen" => {
                    let sr = 1.0 - (1.0 - br) * (1.0 - or);
                    let sg = 1.0 - (1.0 - bg) * (1.0 - og);
                    let sb = 1.0 - (1.0 - bb) * (1.0 - ob);
                    (
                        (sr * alpha + br * (1.0 - alpha)).clamp(0.0, 1.0),
                        (sg * alpha + bg * (1.0 - alpha)).clamp(0.0, 1.0),
                        (sb * alpha + bb * (1.0 - alpha)).clamp(0.0, 1.0),
                    )
                }
                "add" => (
                    (br + or * alpha).clamp(0.0, 1.0),
                    (bg + og * alpha).clamp(0.0, 1.0),
                    (bb + ob * alpha).clamp(0.0, 1.0),
                ),
                // "over" and default
                _ => (
                    (or * alpha + br * (1.0 - alpha)).clamp(0.0, 1.0),
                    (og * alpha + bg * (1.0 - alpha)).clamp(0.0, 1.0),
                    (ob * alpha + bb * (1.0 - alpha)).clamp(0.0, 1.0),
                ),
            };
            let out_a = b[3].max(alpha);
            out.set(x, y, [r, g, ob_, out_a]);
        }
    }
    out
}
