use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{
    apply_modulation,
    get_float,
    get_input_heightmap,
    get_optional_heightmap,
    get_string,
};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let min_val = get_float(ctx.params, "min", 0.0);
    let max_val = get_float(ctx.params, "max", 1.0);
    let mode = get_string(ctx.params, "mode", "clamp");
    let hm = apply_clamp_mode(&input, min_val, max_val, mode);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Range conditioning (WM Clamp/Restrict). `clamp` = hard clip to [min,max];
/// `normalize` = rescale the input's actual min..max to fill [min,max];
/// `soft_clip` = smooth tanh saturation toward the bounds (no hard cut).
pub(crate) fn apply_clamp_mode(input: &Heightmap, min_val: f32, max_val: f32, mode: &str) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let span = (max_val - min_val).abs().max(1e-6);
    let data: Vec<f32> = match mode {
        "normalize" => {
            let lo = input.data().iter().copied().fold(f32::INFINITY, f32::min);
            let hi = input
                .data()
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            let in_span = (hi - lo).max(1e-6);
            input
                .data()
                .iter()
                .map(|&v| min_val + (v - lo) / in_span * span)
                .collect()
        }
        "soft_clip" => input
            .data()
            .iter()
            .map(|&v| {
                let t = (v - min_val) / span;
                let s = 0.5 + 0.5 * ((t - 0.5) * 4.0).tanh();
                min_val + s * span
            })
            .collect(),
        _ => input.data().iter().map(|&v| v.clamp(min_val, max_val)).collect(),
    };
    Heightmap::frbar_data(w, h, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    fn run_node(
        nt: NodeType,
        params: &[(&str, ParamValue)],
        inputs: HashMap<String, PortValue>,
        w: u32,
        h: u32,
    ) -> Heightmap {
        let p: HashMap<String, ParamValue> =
            params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let out = crate::CpuExecutor.execute(&nt, &p, &inputs, w, h, w, h).unwrap();
        match out.get("output").unwrap() {
            PortValue::Heightmap(hm) => hm.clone(),
            _ => panic!("expected heightmap output"),
        }
    }

    #[test]
    fn clamp_hard_mode_clips() {
        let hm = Heightmap::frbar_data(2, 2, vec![0.0, 0.5, 1.0, 0.9]).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let out = run_node(
            NodeType::Clamp,
            &[
                ("min", ParamValue::Float(0.3)),
                ("max", ParamValue::Float(0.7)),
            ],
            inputs,
            2,
            2,
        );
        assert!((out.get(0, 0).unwrap() - 0.3).abs() < 1e-5); // 0.0 -> 0.3
        assert!((out.get(0, 1).unwrap() - 0.7).abs() < 1e-5); // 1.0 -> 0.7
    }

    #[test]
    fn clamp_normalize_stretches_to_full_range() {
        // Input occupies 0.2..0.6; normalize should rescale to fill 0..1.
        let hm = Heightmap::frbar_data(2, 2, vec![0.2, 0.4, 0.4, 0.6]).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let out = run_node(
            NodeType::Clamp,
            &[
                ("mode", ParamValue::String("normalize".into())),
                ("min", ParamValue::Float(0.0)),
                ("max", ParamValue::Float(1.0)),
            ],
            inputs,
            2,
            2,
        );
        let lo = out.data().iter().cloned().fold(f32::INFINITY, f32::min);
        let hi = out.data().iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!((lo - 0.0).abs() < 1e-5 && (hi - 1.0).abs() < 1e-5);
    }

    #[test]
    fn clamp_soft_clip_stays_within_bounds() {
        let data: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        let hm = Heightmap::frbar_data(4, 4, data).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let out = run_node(
            NodeType::Clamp,
            &[
                ("mode", ParamValue::String("soft_clip".into())),
                ("min", ParamValue::Float(0.2)),
                ("max", ParamValue::Float(0.8)),
            ],
            inputs,
            4,
            4,
        );
        for &v in out.data() {
            assert!((0.2 - 1e-4..=0.8 + 1e-4).contains(&v), "soft-clip out of bounds: {v}");
        }
    }
}
