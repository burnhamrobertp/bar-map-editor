use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::filters::shared::bilinear_sample;
use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let warp_x = get_optional_heightmap(ctx.inputs, "warp_x");
    let warp_y = get_optional_heightmap(ctx.inputs, "warp_y");
    let strength = get_float(ctx.params, "strength", 0.1);
    let hm = apply_warp(&input, warp_x.as_ref(), warp_y.as_ref(), strength);

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Domain warp using separate X and Y displacement maps.
/// Each warp map is treated as a signed offset: 0.5 = no displacement.
pub(crate) fn apply_warp(
    input: &Heightmap,
    warp_x: Option<&Heightmap>,
    warp_y: Option<&Heightmap>,
    strength: f32,
) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let data_in = input.data();

    let data: Vec<f32> = (0..h)
        .flat_map(|py| {
            (0..w).map(move |px| {
                let dx = warp_x
                    .and_then(|m| m.get(px as u32, py as u32))
                    .unwrap_or(0.5)
                    - 0.5;
                let dy = warp_y
                    .and_then(|m| m.get(px as u32, py as u32))
                    .unwrap_or(0.5)
                    - 0.5;
                let sx = px as f32 + dx * strength * w as f32;
                let sy = py as f32 + dy * strength * h as f32;
                bilinear_sample(data_in, w, h, sx, sy)
            })
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn warp_no_displacement_is_identity() {
        let executor = crate::CpuExecutor;
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        // Neutral warp maps: all 0.5 means zero displacement.
        let neutral = Heightmap::frbar_data(4, 4, vec![0.5; 16]).unwrap();
        let params = HashMap::from([("strength".to_string(), ParamValue::Float(0.5))]);
        let inputs = HashMap::from([
            ("input".to_string(), PortValue::Heightmap(hm.clone())),
            ("warp_x".to_string(), PortValue::Heightmap(neutral.clone())),
            ("warp_y".to_string(), PortValue::Heightmap(neutral)),
        ]);
        let result = executor
            .execute(&NodeType::Warp, &params, &inputs, 4, 4, 4, 4)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        let diff: f32 = hm
            .data()
            .iter()
            .zip(out.data().iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 16.0;
        assert!(diff < 0.01, "neutral warp should be identity, diff={diff}");
    }

    #[test]
    fn warp_shifts_output() {
        let executor = crate::CpuExecutor;
        // Flat 0 on the left half, flat 1 on the right half.
        let mut data = vec![0.0f32; 8 * 8];
        for y in 0..8usize {
            for x in 4..8usize {
                data[y * 8 + x] = 1.0;
            }
        }
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        // warp_x = 1.0 -> dx = 0.5, so each output pixel samples from
        // input_x + 0.5 * strength * width = px + 4.  The right-half content
        // (bright) therefore appears in the left half of the output.
        let wx = Heightmap::frbar_data(8, 8, vec![1.0; 64]).unwrap();
        let wy = Heightmap::frbar_data(8, 8, vec![0.5; 64]).unwrap();
        let params = HashMap::from([("strength".to_string(), ParamValue::Float(1.0))]);
        let inputs = HashMap::from([
            ("input".to_string(), PortValue::Heightmap(hm)),
            ("warp_x".to_string(), PortValue::Heightmap(wx)),
            ("warp_y".to_string(), PortValue::Heightmap(wy)),
        ]);
        let result = executor
            .execute(&NodeType::Warp, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Left column should now sample from the right half (bright).
        let left_mean: f32 = (0..8).map(|y| out.get(1, y).unwrap()).sum::<f32>() / 8.0;
        assert!(
            left_mean > 0.5,
            "positive warp should bring bright area left, mean={left_mean}"
        );
    }
}
