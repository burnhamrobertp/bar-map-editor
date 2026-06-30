use std::collections::HashMap;

use bar_data::ColorBuffer;
use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{get_input_heightmap, get_optional_heightmap};
use crate::exec::texture::shared::{
    apply_color_modulation, parse_hex_color_srgb, resize_color_to_tex,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let color = apply_color_ramp(&input, ctx.params);
    let color = apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, None, mask.as_ref());
    let color = resize_color_to_tex(color, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Color(color),
    )]))
}

/// Maps every pixel through the user-defined color-stop gradient.
/// Stops are read from indexed params (pos_i, color_i) up to stop_count.
/// Stops are sorted by position before interpolation so order in params
/// doesn't matter.
pub(crate) fn apply_color_ramp(
    input: &Heightmap,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let stop_count = match params.get("stop_count") {
        Some(ParamValue::UInt(n)) => (*n).clamp(2, 8) as usize,
        _ => 2,
    };

    let mut stops: Vec<(f32, [f32; 3])> = (0..stop_count)
        .map(|i| {
            let pos = match params.get(&format!("pos_{i}")) {
                Some(ParamValue::Float(v)) => v.clamp(0.0, 1.0),
                _ => i as f32 / (stop_count - 1).max(1) as f32,
            };
            let hex = match params.get(&format!("color_{i}")) {
                Some(ParamValue::String(s)) => s.as_str(),
                _ => "808080",
            };
            let rgb = parse_hex_color_srgb(hex).unwrap_or([0.5, 0.5, 0.5]);
            (pos, rgb)
        })
        .collect();

    stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let w = input.width();
    let h = input.height();
    let mut out = ColorBuffer::new(w, h).unwrap();

    for (i, &hv) in input.data().iter().enumerate() {
        let hv = hv.clamp(0.0, 1.0);
        let color = if stops.len() < 2 {
            stops.first().map_or([0.0f32; 3], |s| s.1)
        } else if hv <= stops[0].0 {
            stops[0].1
        } else if hv >= stops[stops.len() - 1].0 {
            stops[stops.len() - 1].1
        } else {
            let hi = stops
                .iter()
                .position(|s| s.0 >= hv)
                .unwrap_or(stops.len() - 1);
            let lo = hi.saturating_sub(1);
            let span = stops[hi].0 - stops[lo].0;
            let t = if span > 1e-6 {
                (hv - stops[lo].0) / span
            } else {
                0.0
            };
            let a = stops[lo].1;
            let b = stops[hi].1;
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        let base = i * 4;
        let data = out.data_mut();
        data[base] = color[0];
        data[base + 1] = color[1];
        data[base + 2] = color[2];
        // alpha stays 1.0 from ColorBuffer::new
    }
    out
}
