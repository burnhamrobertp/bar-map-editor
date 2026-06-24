use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{
    apply_modulation,
    get_float,
    get_input_heightmap,
    get_optional_heightmap,
    get_uint,
};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let layer_count = get_uint(ctx.params, "layer_count", 8).clamp(2, 32);
    let irregularity = get_float(ctx.params, "irregularity", 0.3);
    let hardness = get_float(ctx.params, "hardness", 0.8);
    let noise_scale = get_float(ctx.params, "noise_scale", 0.05);
    let hm = apply_stratify(&input, layer_count, irregularity, hardness, noise_scale);
    let hm = apply_modulation(&input, hm, None, mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Simple 2D value noise in [0, 1].
fn strat_hash(x: i32, y: i32) -> f32 {
    let n = x
        .wrapping_mul(1619)
        .wrapping_add(y.wrapping_mul(31337))
        .wrapping_mul(6364136)
        ^ 0x5851f42d_u32 as i32;
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(n.wrapping_add(15731)).wrapping_add(789221) ^ 1376312589;
    ((n as u32) as f32) / u32::MAX as f32
}

fn value_noise_2d(x: f32, y: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let ux = xf * xf * (3.0 - 2.0 * xf);
    let uy = yf * yf * (3.0 - 2.0 * yf);
    let v00 = strat_hash(xi, yi);
    let v10 = strat_hash(xi + 1, yi);
    let v01 = strat_hash(xi, yi + 1);
    let v11 = strat_hash(xi + 1, yi + 1);
    let v0 = v00 + (v10 - v00) * ux;
    let v1 = v01 + (v11 - v01) * ux;
    v0 + (v1 - v0) * uy
}

/// Procedural horizontal rock strata.
pub(crate) fn apply_stratify(
    input: &Heightmap,
    layer_count: u32,
    irregularity: f32,
    hardness: f32,
    noise_scale: f32,
) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let n = layer_count as f32;

    let data: Vec<f32> = input
        .data()
        .iter()
        .enumerate()
        .map(|(idx, &v)| {
            let px = (idx % w) as f32;
            let py = (idx / w) as f32;
            let perturb = if irregularity > 0.0 {
                let scale = noise_scale * w as f32;
                (value_noise_2d(px / scale, py / scale) - 0.5) * irregularity * (1.0 / n)
            } else {
                0.0
            };
            let vp = (v + perturb).clamp(0.0, 1.0);
            let band = (vp * n).floor().min(n - 1.0);
            let band_h = (band + 0.5) / n;
            v * (1.0 - hardness) + band_h * hardness
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn stratify_quantises_to_bands() {
        let executor = crate::CpuExecutor;
        // Linear ramp 0..1.
        let data: Vec<f32> = (0..8).map(|i| i as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 1, data).unwrap();
        let params = HashMap::from([
            ("layer_count".to_string(), ParamValue::UInt(4)),
            ("irregularity".to_string(), ParamValue::Float(0.0)),
            ("hardness".to_string(), ParamValue::Float(1.0)),
            ("noise_scale".to_string(), ParamValue::Float(0.05)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::Stratify, &params, &inputs, 8, 1, 8, 1)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // With hardness=1 and 4 bands, all values should land on band centres.
        let valid_centres = [0.125f32, 0.375, 0.625, 0.875];
        for x in 0..8u32 {
            let v = out.get(x, 0).unwrap();
            let ok = valid_centres.iter().any(|&c| (v - c).abs() < 0.01);
            assert!(ok, "pixel {x} value {v} is not a band centre");
        }
    }
}
