use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_optional_heightmap, scale_by_field};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let slope = get_optional_heightmap(ctx.inputs, "slope");
    let band0 = get_optional_heightmap(ctx.inputs, "band0");
    let band1 = get_optional_heightmap(ctx.inputs, "band1");
    let band2 = get_optional_heightmap(ctx.inputs, "band2");
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = compose_splat_map(
        slope.as_ref(),
        band0.as_ref(),
        band1.as_ref(),
        band2.as_ref(),
        ctx.hm_w,
        ctx.hm_h,
    );
    let hm = scale_by_field(hm, ctrl.as_ref());
    let hm = scale_by_field(hm, mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Compose up to 4 weight channels into a single normalized splat map.
/// The output encodes the dominant channel index (0-3) as a value in [0, 1].
/// For use with Spring/Recoil typemap (8-bit indices) this would be quantized.
pub(crate) fn compose_splat_map(
    slope: Option<&Heightmap>,
    band0: Option<&Heightmap>,
    band1: Option<&Heightmap>,
    band2: Option<&Heightmap>,
    width: u32,
    height: u32,
) -> Heightmap {
    let size = (width as usize) * (height as usize);
    let mut data = vec![0.0f32; size];

    let zero = vec![0.0f32; size];
    let slope_data = slope.map(|h| h.data()).unwrap_or(&zero);
    let b0_data = band0.map(|h| h.data()).unwrap_or(&zero);
    let b1_data = band1.map(|h| h.data()).unwrap_or(&zero);
    let b2_data = band2.map(|h| h.data()).unwrap_or(&zero);

    for (i, pixel) in data.iter_mut().enumerate() {
        let channels = [
            *b0_data.get(i).unwrap_or(&0.0),
            *b1_data.get(i).unwrap_or(&0.0),
            *b2_data.get(i).unwrap_or(&0.0),
            *slope_data.get(i).unwrap_or(&0.0),
        ];

        // Find dominant channel
        let max_idx = channels
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Spring typemap: terrain type index (0-255)
        // Encode as type index normalized to [0,1]
        *pixel = max_idx as f32 / 255.0;
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}
