use std::collections::HashMap;
use std::f32::consts::PI;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::filters::shared::bilinear_sample;
use crate::exec::shared::{
    apply_modulation,
    get_float,
    get_input_heightmap,
    get_optional_heightmap,
};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let tx = get_float(ctx.params, "translate_x", 0.0);
    let ty = get_float(ctx.params, "translate_y", 0.0);
    let scale = get_float(ctx.params, "scale", 1.0).max(1e-4);
    let angle = get_float(ctx.params, "angle", 0.0);
    let hm = apply_transform(&input, tx, ty, scale, angle);
    let hm = apply_modulation(&input, hm, None, mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Translate, scale, rotate a heightmap via inverse-mapped bilinear sampling.
pub(crate) fn apply_transform(input: &Heightmap, tx: f32, ty: f32, scale: f32, angle_deg: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let angle_rad = angle_deg * PI / 180.0;
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();
    let inv_scale = 1.0 / scale;
    let data_in = input.data();

    let data: Vec<f32> = (0..h)
        .flat_map(|py| {
            (0..w).map(move |px| {
                // Normalize output pixel to [-0.5, 0.5].
                let nx = px as f32 / w as f32 - 0.5;
                let ny = py as f32 / h as f32 - 0.5;
                // Inverse transform: undo translate, undo rotate, undo scale.
                let ux = nx - tx;
                let uy = ny - ty;
                let rx = (ux * cos_a + uy * sin_a) * inv_scale;
                let ry = (-ux * sin_a + uy * cos_a) * inv_scale;
                // Map back to pixel space.
                let sx = (rx + 0.5) * w as f32;
                let sy = (ry + 0.5) * h as f32;
                if sx < 0.0 || sy < 0.0 || sx > w as f32 || sy > h as f32 {
                    return 0.0;
                }
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
    fn transform_identity_roundtrips() {
        let executor = crate::CpuExecutor;
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        let params = HashMap::from([
            ("translate_x".to_string(), ParamValue::Float(0.0)),
            ("translate_y".to_string(), ParamValue::Float(0.0)),
            ("scale".to_string(), ParamValue::Float(1.0)),
            ("angle".to_string(), ParamValue::Float(0.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm.clone()))]);
        let result = executor
            .execute(&NodeType::Transform, &params, &inputs, 4, 4, 4, 4)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Identity transform: output should closely match input.
        let diff: f32 = hm
            .data()
            .iter()
            .zip(out.data().iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 16.0;
        assert!(diff < 0.05, "identity transform mean error = {diff}");
    }

    #[test]
    fn transform_180_rotation_flips_values() {
        let executor = crate::CpuExecutor;
        // Ramp increasing left-to-right.
        let data: Vec<f32> = (0..8).map(|i| i as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 1, data).unwrap();
        let params = HashMap::from([
            ("translate_x".to_string(), ParamValue::Float(0.0)),
            ("translate_y".to_string(), ParamValue::Float(0.0)),
            ("scale".to_string(), ParamValue::Float(1.0)),
            ("angle".to_string(), ParamValue::Float(180.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm.clone()))]);
        let result = executor
            .execute(&NodeType::Transform, &params, &inputs, 8, 1, 8, 1)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // After 180 rotation left pixel should be high, right pixel low.
        assert!(
            out.get(0, 0).unwrap() > out.get(7, 0).unwrap(),
            "left={} should exceed right={}",
            out.get(0, 0).unwrap(),
            out.get(7, 0).unwrap()
        );
    }
}
